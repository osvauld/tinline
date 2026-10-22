//! Blob hub, downloads, uploads and file messages.

use super::*;

impl ChatRt {
    // ---- blobs -----------------------------------------------------------------------------------

    pub async fn chat_hub(self: &Arc<Self>) -> Result<Arc<BlobHub>, Error> {
        let epoch = self.me()?.epoch;
        let core = self.chat_core()?;
        let this = Arc::downgrade(self);
        core.hub
            .get_or_try_init(|| async {
                let w1 = this.clone();
                let gate: Gate = Arc::new(move |dev, hash| w1.upgrade().is_some_and(|i| i.profile_may_serve(dev, hash) || i.chat_may_serve(dev, hash)));
                let w2 = this.clone();
                let progress: UploadProgress = Arc::new(move |dev, hash, done, total| {
                    if let Some(i) = w2.upgrade() {
                        i.chat_upload_progress(dev, hash, done, total);
                    }
                });
                let w3 = this.clone();
                let ended: UploadEnd = Arc::new(move |dev, hash, completed| {
                    if let Some(i) = w3.upgrade() {
                        i.chat_upload_end(dev, hash, completed);
                    }
                });
                let hub = BlobHub::open(&core.dir.join("blobs"), gate, progress, ended).await?;
                if let Some(inner) = this.upgrade() {
                    hub.recover_owned_tags(&inner.profile_blob_tags(epoch)?).await?;
                }
                Ok::<_, Error>(Arc::new(hub))
            })
            .await
            .cloned()
    }

    pub(super) fn did_of_device(&self, dev: &[u8; 32]) -> Option<String> {
        if let Some(c) = self.shared.lock().state.contacts.iter().find(|c| c.has_device(dev)) {
            return Some(c.did.clone());
        }
        let core = self.core.lock().clone()?;
        let map = core.sessions.lock();
        map.values().find(|s| s.device == *dev).map(|s| s.did.clone())
    }

    /// May `dev` read the blob? Only a contact in good standing, and only if a message of our
    /// conversation with them names the hash.
    pub(super) fn chat_may_serve(&self, dev: &[u8; 32], hash: &Hash) -> bool {
        let (Ok(me), Ok(core)) = (self.me(), self.chat_core()) else { return false };
        // Our own devices read whatever any of our conversations references.
        if self.is_own_device(dev) {
            return core.store.referenced(&hash.to_string());
        }
        let Some(did) = self.did_of_device(dev) else { return false };
        {
            let s = self.shared.lock();
            if s.state.blocked.contains(&did) || !s.state.contacts.iter().any(|c| c.did == did) {
                return false;
            }
        }
        core.store.may_read(&hash.to_string(), &pair_id(me.id.did(), &did))
    }

    pub(super) fn chat_upload_progress(&self, dev: &[u8; 32], hash: &Hash, done: u64, total: u64) {
        if let (Some(ev), Some(did)) = (self.ev(), self.did_of_device(dev)) {
            ev.on_transfer_progress(did, hash.to_string(), done, total, true);
        }
    }

    /// The provider finished serving `hash` to `dev`. An aborted transfer (the peer dropped,
    /// reset the stream, or the link died) ends with `(0, 0, outgoing)`, so the sender's UI
    /// leaves "Sending n%"; a completed one needs nothing (progress already reached the total).
    pub(super) fn chat_upload_end(&self, dev: &[u8; 32], hash: &Hash, completed: bool) {
        if completed {
            return;
        }
        if let (Some(ev), Some(did)) = (self.ev(), self.did_of_device(dev)) {
            ev.on_transfer_progress(did, hash.to_string(), 0, 0, true);
        }
    }

    /// Starts fetching a blob from `did` in the background. Idempotent while a fetch is
    /// running; an entry that is only waiting out a retry delay is replaced at once (a new
    /// session or a user tap means "try now"), so nothing stale blocks a new attempt.
    pub fn chat_download(self: &Arc<Self>, did: String, hash: String) {
        let Ok(core) = self.chat_core() else { return };
        let seq = core.next_dl.fetch_add(1, Ordering::Relaxed);
        {
            let mut d = core.downloading.lock();
            let mut seed = (0, 0);
            if let Some(old) = d.get(&hash) {
                if !old.waiting {
                    return;
                }
                if let Some(a) = &old.abort {
                    a.abort();
                }
                seed = (old.done, old.total);
            }
            d.insert(hash.clone(), Dl { done: seed.0, total: seed.1, waiting: false, seq, abort: None });
        }
        let this = self.clone();
        let (c2, h2) = (core.clone(), hash.clone());
        let task = self.handle.spawn(async move {
            let res = this.chat_download_loop(&c2, &did, &h2, seq).await;
            {
                let mut d = c2.downloading.lock();
                if d.get(&h2).is_some_and(|e| e.seq == seq) {
                    d.remove(&h2);
                }
            }
            if let Some(res) = res {
                let ok = res.is_ok();
                if let Ok(Some(mut info)) = c2.store.blob(&h2) {
                    info.ready = ok;
                    info.failed = !ok;
                    if let Ok(op) = c2.store.put_blob_op(&h2, &info) {
                        let _ = c2.store.db.apply(&[op]);
                    }
                    this.chat_emit_changed(&c2, &did, &info.day, &info.msg);
                }
            }
        });
        if let Some(e) = core.downloading.lock().get_mut(&hash)
            && e.seq == seq
        {
            e.abort = Some(task.abort_handle());
        }
    }

    /// Fetch, and retry with backoff. `None` = nothing to record (the user cancelled meanwhile).
    pub(super) async fn chat_download_loop(self: &Arc<Self>, core: &Arc<ChatCore>, did: &str, hash: &str, seq: u64) -> Option<Result<(), Error>> {
        let mut attempt = 0;
        loop {
            let err = match self.chat_fetch_blob(core, did, hash).await {
                Ok(()) => return Some(Ok(())),
                Err(e) => e,
            };
            self.log(format!("blob download {hash}: {err}"));
            let retryable = !matches!(err, Error::NotFound | Error::Protocol(_));
            if !retryable || attempt >= RETRY_BACKOFF.len() {
                return Some(Err(err));
            }
            if !core.store.blob(hash).ok().flatten().is_some_and(|i| i.wanted) {
                return None;
            }
            if let Some(e) = core.downloading.lock().get_mut(hash)
                && e.seq == seq
            {
                e.waiting = true;
            }
            tokio::time::sleep(Duration::from_secs(RETRY_BACKOFF[attempt])).await;
            attempt += 1;
            if let Some(e) = core.downloading.lock().get_mut(hash)
                && e.seq == seq
            {
                e.waiting = false;
            }
        }
    }

    pub(super) fn chat_emit_changed(self: &Arc<Self>, core: &Arc<ChatCore>, did: &str, day: &str, id: &str) {
        let Ok(me) = self.me() else { return };
        let pair = pair_id(me.id.did(), did);
        let rec = core.shards.lock().get(&(pair.clone(), day.to_string())).and_then(|s| s.messages().ok()).and_then(|mut m| m.remove(id));
        let this = self.clone();
        let (core, did, day, id) = (core.clone(), did.to_string(), day.to_string(), id.to_string());
        self.handle.spawn(async move {
            let rec = match rec {
                Some(r) => r,
                None => {
                    let Ok(shards) = this.chat_shard(&core, &pair, &day).await else {
                        return;
                    };
                    match shards.get(&(pair.clone(), day.clone())).and_then(|s| s.messages().ok()).and_then(|mut m| m.remove(&id)) {
                        Some(r) => r,
                        None => return,
                    }
                }
            };
            if let (Some(ev), Ok(me)) = (this.ev(), this.me()) {
                ev.on_message_changed(this.chat_api_message(&core, &me, &did, &pair, &day, &rec));
            }
        });
    }

    /// A connection to someone who may hold the blobs of the conversation with `did`: the
    /// contact's live session device, our own devices we are in sync with right now, the
    /// contact's other devices, then our remaining own devices.
    pub(super) fn chat_blob_cands(self: &Arc<Self>, core: &Arc<ChatCore>, did: &str) -> Vec<([u8; 32], Option<String>)> {
        let mut cands: Vec<([u8; 32], Option<String>)> = Vec::new();
        let mut add = |d: [u8; 32], relay: Option<String>| {
            if !cands.iter().any(|(x, _)| *x == d) {
                cands.push((d, relay));
            }
        };
        let contact = self.shared.lock().state.contacts.iter().find(|c| c.did == did).cloned();
        for s in self.chat_sessions_of(core, did) {
            add(s.device, contact.as_ref().and_then(|c| c.relay_for(&s.device).map(str::to_string)));
        }
        let own = self.own_blob_sources();
        for (d, r, live) in &own {
            if *live {
                add(*d, r.clone());
            }
        }
        if let Some(c) = &contact {
            for cd in &c.devices {
                add(cd.device, c.relay_for(&cd.device).map(str::to_string));
            }
        }
        for (d, r, _) in own {
            add(d, r);
        }
        cands
    }

    pub(super) async fn chat_blob_dial(&self, cand: &([u8; 32], Option<String>)) -> Result<Connection, Error> {
        let ep = self.endpoint()?;
        let addr = addr_for(&cand.0, cand.1.as_deref())?;
        match tokio::time::timeout(DIAL_TIMEOUT, ep.connect(addr, iroh_blobs::ALPN)).await {
            Ok(Ok(c)) => Ok(c),
            Ok(Err(e)) => Err(Error::net(e)),
            Err(_) => Err(Error::Timeout),
        }
    }

    /// The first reachable of the candidates (a contact's blob peers, then our own devices).
    pub(super) async fn chat_blob_conn(self: &Arc<Self>, core: &Arc<ChatCore>, did: &str) -> Result<Connection, Error> {
        let mut last = Error::NotFound;
        for cand in self.chat_blob_cands(core, did) {
            match self.chat_blob_dial(&cand).await {
                Ok(c) => return Ok(c),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    pub(super) async fn chat_fetch_blob(self: &Arc<Self>, core: &Arc<ChatCore>, did: &str, hash_hex: &str) -> Result<(), Error> {
        let info = core.store.blob(hash_hex)?.ok_or(Error::NotFound)?;
        let hash = parse_hash(hash_hex)?;
        let hub = self.chat_hub().await?;
        let total = crypt::cipher_len(info.size);
        if hub.complete_size(&hash).await == Some(total) {
            return Ok(());
        }
        // What an earlier attempt (or an earlier run of the app) already stored.
        let have = hub.local_bytes(&hash).await.min(total);
        if let Some(e) = core.downloading.lock().get_mut(hash_hex) {
            e.done = have;
            e.total = total;
        }
        if have > 0
            && let Some(ev) = self.ev()
        {
            ev.on_transfer_progress(did.to_string(), hash_hex.to_string(), have, total, false);
        }
        // Every candidate in turn (the contact's live device, our own devices, ...): one that
        // is unreachable or lacks the blob must not stop the next from trying.
        let mut err = Error::NotFound;
        for cand in self.chat_blob_cands(core, did) {
            let conn = match self.chat_blob_dial(&cand).await {
                Ok(c) => c,
                Err(e) => {
                    err = e;
                    continue;
                }
            };
            let (c2, did2, h2) = (core.clone(), did.to_string(), hash_hex.to_string());
            let this = self.clone();
            let last = Arc::new(Mutex::new(std::time::Instant::now().checked_sub(Duration::from_secs(1)).unwrap_or_else(std::time::Instant::now)));
            let r = hub
                .fetch(conn, hash, total, move |done, tot| {
                    if let Some(e) = c2.downloading.lock().get_mut(&h2) {
                        e.done = done;
                        e.total = tot;
                    }
                    let mut l = last.lock();
                    if l.elapsed() > Duration::from_millis(200) || done == tot {
                        *l = std::time::Instant::now();
                        if let Some(ev) = this.ev() {
                            ev.on_transfer_progress(did2.clone(), h2.clone(), done, tot, false);
                        }
                    }
                })
                .await;
            match r {
                Ok(()) => return Ok(()),
                Err(e) => err = e.into(),
            }
        }
        Err(err)
    }

    /// Test hook: tries to fetch a blob from a contact's device as this node, whatever the
    /// conversations say. Fails when the contact's gate refuses us.
    pub async fn chat_raw_fetch(self: &Arc<Self>, did: &str, hash_hex: &str, size: u64) -> Result<u64, Error> {
        let core = self.chat_core()?;
        let hash = parse_hash(hash_hex)?;
        let hub = self.chat_hub().await?;
        let conn = self.chat_blob_conn(&core, did).await?;
        hub.fetch(conn, hash, crypt::cipher_len(size), |_, _| {}).await?;
        Ok(size)
    }

    /// After (re)connecting: fetch what we wanted but could not get before.
    pub(super) fn chat_resume_downloads(self: &Arc<Self>, core: &Arc<ChatCore>, did: &str) {
        let Ok(me) = self.me() else { return };
        let pair = pair_id(me.id.did(), did);
        for (hash, info) in core.store.wanted_blobs().unwrap_or_default() {
            if info.pair == pair {
                self.chat_download(did.to_string(), hash);
            }
        }
    }

    /// The blob hash of message `id` in the conversation with `did`.
    pub(super) fn chat_blob_of(&self, core: &Arc<ChatCore>, did: &str, id: &str) -> Result<String, Error> {
        let me = self.me()?;
        let pair = pair_id(me.id.did(), did);
        let day = core.store.mi(&pair, id)?.ok_or(Error::NotFound)?;
        let hash = {
            let shards = core.shards.lock();
            shards.get(&(pair.clone(), day.clone())).and_then(|s| s.messages().ok()).and_then(|mut m| m.remove(id)).and_then(|r| r.file).map(|f| f.hash)
        };
        // Not cached: look the hash up through the blob records of this message.
        match hash {
            Some(h) => Ok(h),
            None => core
                .store
                .conv_blobs(&pair)?
                .into_iter()
                .find(|h| core.store.blob(h).ok().flatten().is_some_and(|b| b.msg == id))
                .ok_or(Error::NotFound),
        }
    }

    /// User asked for the attachment of a message.
    pub fn chat_want(self: &Arc<Self>, did: &str, id: &str) -> Result<(), Error> {
        let core = self.chat_core()?;
        let hash = self.chat_blob_of(&core, did, id)?;
        let mut info = core.store.blob(&hash)?.ok_or(Error::NotFound)?;
        if info.ready {
            return Ok(());
        }
        info.wanted = true;
        info.failed = false;
        core.store.db.apply(&[core.store.put_blob_op(&hash, &info)?])?;
        self.chat_download(did.to_string(), hash);
        self.chat_kick(did.to_string());
        Ok(())
    }

    /// The user gave up on a download: stop the fetch, forget the wish (so a reconnect does not
    /// restart it) and drop our tag so the GC frees the partial bytes. The attachment is
    /// `Remote` again and a later `chat_want` starts from scratch.
    pub async fn chat_cancel(self: &Arc<Self>, did: &str, id: &str) -> Result<(), Error> {
        let core = self.chat_core()?;
        let hash = self.chat_blob_of(&core, did, id)?;
        let mut info = core.store.blob(&hash)?.ok_or(Error::NotFound)?;
        if info.ready {
            return Ok(());
        }
        if let Some(e) = core.downloading.lock().remove(&hash)
            && let Some(a) = e.abort
        {
            a.abort();
        }
        info.wanted = false;
        info.failed = false;
        core.store.db.apply(&[core.store.put_blob_op(&hash, &info)?])?;
        let h = parse_hash(&hash)?;
        self.chat_hub().await?.release(&h).await?;
        if let Some(ev) = self.ev() {
            ev.on_transfer_progress(did.to_string(), hash.clone(), 0, 0, false);
        }
        self.chat_emit_changed(&core, did, &info.day, &info.msg);
        Ok(())
    }

    pub async fn chat_save(self: &Arc<Self>, did: &str, id: &str, dest: PathBuf) -> Result<(), Error> {
        let me = self.me()?;
        let core = self.chat_core()?;
        let pair = pair_id(me.id.did(), did);
        let hash = core
            .store
            .conv_blobs(&pair)?
            .into_iter()
            .find(|h| core.store.blob(h).ok().flatten().is_some_and(|b| b.msg == id))
            .ok_or(Error::NotFound)?;
        let info = core.store.blob(&hash)?.ok_or(Error::NotFound)?;
        let h = parse_hash(&hash)?;
        let hub = self.chat_hub().await?;
        if hub.complete_size(&h).await != Some(crypt::cipher_len(info.size)) {
            return Err(Error::NotFound);
        }
        let key = parse_key(&info.key)?;
        hub.export_plain(h, key, dest, |_| {}).await?;
        Ok(())
    }

    /// Encrypts a local file into the blob store and sends a message that carries it.
    pub async fn chat_send_file(
        self: &Arc<Self>,
        did: &str,
        path: PathBuf,
        mime: String,
        text: Option<String>,
        voice: Option<(u32, Vec<u8>)>,
    ) -> Result<Message, Error> {
        self.chat_contact(did, true)?;
        let meta = std::fs::metadata(&path)?;
        if !meta.is_file() {
            return Err(Error::Protocol("not a file".into()));
        }
        if meta.len() > crypt::MAX_FILE {
            return Err(Error::Protocol("files are limited to 2 GB".into()));
        }
        let name: String = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
        let name: String = proto::sanitize_name(&name).chars().take(200).collect();
        // /5: a picture's size and colour preview travel with it (decoded off the async threads).
        let pic = if voice.is_none() && mime.starts_with("image/") {
            let (p, m) = (path.clone(), mime.clone());
            tokio::task::spawn_blocking(move || crate::chat::photo::describe(&p, &m)).await.ok().flatten()
        } else {
            None
        };
        let hub = self.chat_hub().await?;
        let key = crypt::new_key();
        let (hash, size) = hub.add_file(key, path, |_| {}).await?;
        let (kind, duration_ms, waveform) = match voice {
            Some((d, mut w)) => {
                w.truncate(128);
                ("voice".to_string(), d, w)
            }
            None => ("file".to_string(), 0, vec![]),
        };
        let file = FileRef {
            hash: hash.to_string(),
            key: hex::encode(key),
            name,
            size,
            mime: mime.chars().take(100).collect(),
            kind,
            duration_ms,
            waveform,
            width: pic.as_ref().map_or(0, |p| p.0),
            height: pic.as_ref().map_or(0, |p| p.1),
            preview: pic.map(|p| hex::encode(p.2)).unwrap_or_default(),
        };
        self.chat_send_message(did, text.unwrap_or_default(), None, Some(file)).await
    }
}
