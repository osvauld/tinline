//! Group files (`docs/design/group-files.md`) on top of the image runtime (`images.rs`): the
//! rows, gate, fetch, retries and budget are the pictures'; this module adds what files need.
//!
//! - **Sending** streams the file from disk through the chunk cipher into the group blob store
//!   (`BlobHub::add_file_owned`); the post signs the resulting hash.
//! - **Host keep or relay (§4, option A by default):** a host keeps a file up to
//!   `keep_max` (`HOST_KEEP_FILE_MAX`) like a picture. A bigger one it never keeps: when a member
//!   asks for it (the gate sees the request), the host pulls it from whoever has it and is
//!   connected (the author's sessions, announced holders, other member sessions, other hosts)
//!   into a short-lived relay cache (`relay/<gid>` tag, verified against the signed hash), and
//!   serves it while the pull runs. Entries go `relay_ttl` after the last request; the cache
//!   holds at most `relay_max` bytes.
//! - **Holders (§6):** a member device that completed a download writes `FileHeld{held: true}`
//!   (in the post's day doc), and `held: false` when it deletes its copy. Not in open groups,
//!   not for a host's kept copy.
//! - **Progress, cancel, delete, open, save.**

use std::path::PathBuf;

use iroh_blobs::Hash;
use transfer::crypt;

use super::images::{CORRUPT, owner_of};
use super::*;
use crate::store::ImgRow;
use chat_core::store::DEFAULT_AUTO_DOWNLOAD;

/// After this long without a request, a relayed file leaves the host's relay cache.
pub const RELAY_TTL: Duration = Duration::from_secs(10 * 60);
/// At most this many ciphertext bytes in a host's relay cache (one 2 GiB file and its tags).
pub const RELAY_CACHE_MAX: u64 = MAX_FILE_BYTES + MAX_FILE_BYTES / 64;
/// At most one progress event per file this often.
const PROGRESS_EVERY: Duration = Duration::from_millis(500);

type Key = (String, [u8; 32]);

#[derive(Debug, Clone, Copy)]
struct Relay {
    /// Ciphertext bytes.
    bytes: u64,
    last: Instant,
    busy: bool,
}

/// The file side of an account's image runtime (`ImagesRt::files`).
pub struct FilesRt {
    /// Files up to this size auto-download (the 1:1 `auto_download_limit`, set by p2pcore).
    auto_max: AtomicU64,
    /// A host keeps files up to this size (`HOST_KEEP_FILE_MAX`; a test knob).
    keep_max: AtomicU64,
    relay_ttl: Mutex<Duration>,
    relay_max: AtomicU64,
    relay: Mutex<HashMap<Key, Relay>>,
    /// `(ciphertext bytes so far, total, last event)` of running downloads.
    progress: Mutex<HashMap<Key, (u64, u64, Instant)>>,
    tasks: Mutex<HashMap<Key, tokio::task::AbortHandle>>,
}

impl Default for FilesRt {
    fn default() -> Self {
        Self {
            auto_max: AtomicU64::new(DEFAULT_AUTO_DOWNLOAD),
            keep_max: AtomicU64::new(HOST_KEEP_FILE_MAX),
            relay_ttl: Mutex::new(RELAY_TTL),
            relay_max: AtomicU64::new(RELAY_CACHE_MAX),
            relay: Default::default(),
            progress: Default::default(),
            tasks: Default::default(),
        }
    }
}

impl FilesRt {
    pub fn auto_max(&self) -> u64 {
        self.auto_max.load(Ordering::Relaxed)
    }

    /// The 1:1 size limit for downloading by itself (p2pcore keeps it in step with the chat
    /// setting).
    pub fn set_auto_max(&self, bytes: u64) {
        self.auto_max.store(bytes, Ordering::Relaxed);
    }

    pub fn keep_max(&self) -> u64 {
        self.keep_max.load(Ordering::Relaxed)
    }

    /// Test knobs: what hosts keep, how long and how much they relay. `None`: the defaults.
    pub fn set_limits_for_test(&self, keep_max: Option<u64>, relay_ttl: Option<Duration>, relay_max: Option<u64>) {
        self.keep_max.store(keep_max.unwrap_or(HOST_KEEP_FILE_MAX), Ordering::Relaxed);
        *self.relay_ttl.lock() = relay_ttl.unwrap_or(RELAY_TTL);
        self.relay_max.store(relay_max.unwrap_or(RELAY_CACHE_MAX), Ordering::Relaxed);
    }

    /// Notes progress; true when an event is due.
    pub(super) fn progress(&self, gid: &str, hash: &[u8; 32], done: u64, total: u64) -> bool {
        let mut p = self.progress.lock();
        let now = Instant::now();
        let e = p.entry((gid.to_string(), *hash)).or_insert((0, total, now - PROGRESS_EVERY));
        e.0 = done;
        e.1 = total;
        if now.duration_since(e.2) >= PROGRESS_EVERY || done == total {
            e.2 = now;
            true
        } else {
            false
        }
    }

    pub(super) fn progress_clear(&self, gid: &str, hash: &[u8; 32]) {
        self.progress.lock().remove(&(gid.to_string(), *hash));
    }

    /// Plaintext bytes of `size` that arrived so far (0 if not running).
    pub fn done_of(&self, gid: &str, hash: &[u8; 32], size: u64) -> u64 {
        match self.progress.lock().get(&(gid.to_string(), *hash)) {
            Some(&(n, t, _)) if t > 0 => ((n as u128 * size as u128) / t as u128).min(size as u128) as u64,
            _ => 0,
        }
    }

    pub(super) fn task_start(&self, gid: &str, hash: [u8; 32], h: tokio::task::AbortHandle) {
        if !h.is_finished() {
            self.tasks.lock().insert((gid.to_string(), hash), h);
        }
    }

    pub(super) fn task_end(&self, gid: &str, hash: &[u8; 32]) {
        self.tasks.lock().remove(&(gid.to_string(), *hash));
    }

    /// Whether the relay cache holds `hash` of `gid` now (tests).
    pub fn relaying(&self, gid: &str, hash: &[u8; 32]) -> bool {
        self.relay.lock().contains_key(&(gid.to_string(), *hash))
    }
}

/// The blob store tag of a host's short-lived relay copies for `gid`.
fn relay_owner(gid: &str) -> String {
    format!("relay/{gid}")
}

impl GroupCore {
    /// A file over what hosts keep.
    pub(super) fn over_keep(&self, row: &ImgRow) -> bool {
        row.file.as_ref().is_some_and(|f| f.size > self.images.files.keep_max())
    }

    /// We host `gid` and relay this file instead of keeping it (unless our user fetched it).
    pub(super) fn relays(&self, gid: &str, row: &ImgRow) -> bool {
        self.over_keep(row) && !row.ready && self.am_host(gid)
    }

    /// The group whose relayed file has `hash`, if a request for it should start a pull here.
    pub(super) fn relay_wanted(&self, hash: &[u8; 32]) -> Option<String> {
        let gids: Vec<String> = self.groups.lock().keys().cloned().collect();
        gids.into_iter().find(|gid| {
            self.store
                .image(gid, hash)
                .ok()
                .flatten()
                .is_some_and(|r| !r.dropped && self.relays(gid, &r))
        })
    }

    /// The DIDs holding the file of post `post` now (group files §6).
    pub fn file_holders_of(&self, gid: &str, post: &str) -> Vec<String> {
        let Ok(Some(row)) = self.store.image_of_post(gid, post) else { return Vec::new() };
        let Some(file) = row.file else { return Vec::new() };
        let mut groups = self.groups.lock();
        let Some(live) = groups.get_mut(gid) else { return Vec::new() };
        let Live { state, shards, .. } = live;
        match shards.get(&self.store, gid, self.wid, &day_of(row.at), state) {
            Ok(shard) => crate::files::holders(&shard.rows, state, post, &row.author, &file),
            Err(_) => Vec::new(),
        }
    }

    /// What this device last announced about holding the file of `row`.
    fn file_own_statement(&self, gid: &str, row: &ImgRow) -> Option<bool> {
        let mut groups = self.groups.lock();
        let live = groups.get_mut(gid)?;
        let Live { state, shards, .. } = live;
        let shard = shards.get(&self.store, gid, self.wid, &day_of(row.at), state).ok()?;
        crate::files::own_statement(&shard.rows, &row.post, &row.image.hash, &self.did, &self.dev)
    }
}

impl GroupRt {
    /// Seals the file at `path` into the group's blob store (streamed from disk) and writes the
    /// file post that names it.
    pub async fn group_post_file(
        self: &Arc<Self>,
        core: &Arc<GroupCore>,
        gid: &str,
        path: PathBuf,
        name: String,
        mime: String,
        caption: String,
    ) -> Result<crate::api::GroupMessage, Error> {
        let len = std::fs::metadata(&path)?.len();
        if len > MAX_FILE_BYTES {
            return Err(Error::Rejected("a file can be at most 2 GB".into()));
        }
        let hub = self.group_hub(core).await?;
        let key = crypt::new_key();
        let (hash, size) = hub.add_file_owned(&owner_of(gid), key, path, |_| {}).await?;
        let file = FileRef { hash: *hash.as_bytes(), key, size, name, mime };
        if !file.well_formed() {
            let _ = hub.release_owned(&owner_of(gid), &hash).await;
            return Err(Error::Rejected("this file cannot be posted (too large, or a bad name)".into()));
        }
        let blob = file.as_blob();
        let body = Body::File { caption, reply_to: None, file: file.clone() };
        self.group_post_blob(core, gid, blob, Some(file), body).await
    }

    /// Writes our holder announcement for `row` if it changes what we last said. Never in open
    /// groups (no receipts there either), never for a host's kept copy (that is
    /// `kept_by_hosts`), and the author says only `false` (holding is the author's default).
    pub(super) fn group_file_announce(self: &Arc<Self>, core: &Arc<GroupCore>, gid: &str, row: &ImgRow, held: bool) {
        if core.with_state(gid, |st| st.open || !st.is_member(&core.did) || st.is_removed(&core.did)).unwrap_or(true) {
            return;
        }
        if held && core.am_host(gid) && !core.over_keep(row) {
            return;
        }
        let said = core.file_own_statement(gid, row);
        let author = row.author == core.did;
        let need = if held { said != Some(true) && !(author && said.is_none()) } else { said == Some(true) || (author && said.is_none()) };
        if !need {
            return;
        }
        let body = Body::FileHeld { post: row.post.clone(), day: day_of(row.at), hash: row.image.hash, held };
        match core.write(gid, body) {
            Ok((_, changed)) => self.group_after(core, changed, None),
            Err(e) => tracing::warn!("group {}: file holder note: {e}", crate::short(gid)),
        }
    }

    /// A member asked us (a host) for a file we relay: pull it into the relay cache unless that
    /// is already running or done (then the request only keeps it longer).
    pub(super) fn group_relay_kick(self: &Arc<Self>, core: &Arc<GroupCore>, gid: &str, hash: [u8; 32]) {
        let Ok(Some(row)) = core.store.image(gid, &hash) else { return };
        let bytes = crypt::cipher_len(row.image.size);
        let key = (gid.to_string(), hash);
        {
            let files = &core.images.files;
            let mut relay = files.relay.lock();
            if let Some(r) = relay.get_mut(&key) {
                r.last = Instant::now();
                return;
            }
            let max = files.relay_max.load(Ordering::Relaxed);
            let mut used: u64 = relay.values().map(|r| r.bytes).sum();
            // Make room: idle entries, oldest first. Their tags go below.
            let mut idle: Vec<(Instant, Key)> = relay.iter().filter(|(_, r)| !r.busy).map(|(k, r)| (r.last, k.clone())).collect();
            idle.sort();
            let mut evict = Vec::new();
            for (_, k) in idle {
                if used + bytes <= max {
                    break;
                }
                if let Some(r) = relay.remove(&k) {
                    used -= r.bytes;
                    evict.push(k);
                }
            }
            if used + bytes > max {
                self.log(format!("group {}: relay cache full, file {} waits", crate::short(gid), &hex::encode(hash)[..12]));
                return;
            }
            relay.insert(key.clone(), Relay { bytes, last: Instant::now(), busy: true });
            if !evict.is_empty() {
                let (this, core) = (self.clone(), core.clone());
                self.handle.spawn(async move {
                    if let Ok(hub) = this.group_hub(&core).await {
                        for (g, h) in evict {
                            let _ = hub.release_owned(&relay_owner(&g), &Hash::from_bytes(h)).await;
                        }
                    }
                });
            }
        }
        let (this, core, gid) = (self.clone(), core.clone(), gid.to_string());
        self.handle.spawn(async move {
            let res = this.group_relay_pull(&core, &gid, &row).await;
            let ok = res.is_ok();
            {
                let mut relay = core.images.files.relay.lock();
                if ok {
                    if let Some(r) = relay.get_mut(&key) {
                        r.busy = false;
                        r.last = Instant::now();
                    }
                } else {
                    relay.remove(&key);
                }
            }
            if let Err(e) = res {
                // Nobody who has it is online (or every copy was bad): the next request tries
                // again.
                this.log(format!("group {}: relay of file {} failed: {e}", crate::short(&gid), &hex::encode(hash)[..12]));
                if let Ok(hub) = this.group_hub(&core).await {
                    let _ = hub.release_owned(&relay_owner(&gid), &Hash::from_bytes(hash)).await;
                }
            }
        });
    }

    /// One relay pull: every source in turn until one sends the whole blob, verified against
    /// the signed hash.
    async fn group_relay_pull(self: &Arc<Self>, core: &Arc<GroupCore>, gid: &str, row: &ImgRow) -> Result<(), Error> {
        let hub = self.group_hub(core).await?;
        let h = Hash::from_bytes(row.image.hash);
        let total = crypt::cipher_len(row.image.size);
        let owner = relay_owner(gid);
        if hub.complete_size(&h).await == Some(total) {
            return hub.keep_owned(&owner, &h).await.map_err(Error::from);
        }
        let ep = self.group_endpoint().ok_or(Error::NotStarted)?;
        let mut err = Error::NotFound;
        for (dev, relay) in self.group_image_sources(core, gid, &row.author, Some(&row.post)) {
            let conn = match addr_for(&dev, relay.as_deref()) {
                Ok(addr) => match tokio::time::timeout(DIAL_TIMEOUT, ep.connect(addr, iroh_blobs::ALPN)).await {
                    Ok(Ok(c)) => c,
                    Ok(Err(e)) => {
                        err = Error::net(e);
                        continue;
                    }
                    Err(_) => {
                        err = Error::Timeout;
                        continue;
                    }
                },
                Err(e) => {
                    err = e;
                    continue;
                }
            };
            let res = hub.fetch_owned(&owner, conn.clone(), h, total).await;
            conn.close(0u32.into(), b"done");
            match res {
                Ok(()) => return Ok(()),
                Err(transfer::Error::Corrupt(e)) => {
                    let _ = hub.release_owned(&owner, &h).await;
                    err = Error::Rejected(format!("{CORRUPT}: {e}"));
                }
                Err(e) => err = e.into(),
            }
        }
        Err(err)
    }

    /// Drops relay copies nobody asked for within the TTL (the dial loop's tick).
    pub(super) fn group_relay_sweep(self: &Arc<Self>, core: &Arc<GroupCore>) {
        let ttl = *core.images.files.relay_ttl.lock();
        let gone: Vec<Key> = {
            let mut relay = core.images.files.relay.lock();
            let gone: Vec<Key> = relay.iter().filter(|(_, r)| !r.busy && r.last.elapsed() >= ttl).map(|(k, _)| k.clone()).collect();
            for k in &gone {
                relay.remove(k);
            }
            gone
        };
        if gone.is_empty() {
            return;
        }
        let (this, core) = (self.clone(), core.clone());
        self.handle.spawn(async move {
            if let Ok(hub) = this.group_hub(&core).await {
                for (g, h) in gone {
                    let _ = hub.release_owned(&relay_owner(&g), &Hash::from_bytes(h)).await;
                }
            }
        });
    }

    /// Where the opened copy of a file goes: `group-files/<hash>/<name>` in the account dir.
    fn group_file_dest(core: &GroupCore, row: &ImgRow) -> Option<PathBuf> {
        let f = row.file.as_ref()?;
        Some(core.images.dir().join("group-files").join(hex::encode(f.hash)).join(&f.name))
    }

    /// The opened file of a file post (decrypted once), if its blob is here.
    pub(super) async fn group_file_open(self: &Arc<Self>, core: &Arc<GroupCore>, _gid: &str, row: &ImgRow) -> Result<Option<PathBuf>, Error> {
        let Some(path) = Self::group_file_dest(core, row) else { return Ok(None) };
        if path.exists() {
            return Ok(Some(path));
        }
        let hub = self.group_hub(core).await?;
        let h = Hash::from_bytes(row.image.hash);
        if hub.complete_size(&h).await != Some(crypt::cipher_len(row.image.size)) {
            return Ok(None);
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        hub.export_plain(h, row.image.key, path.clone(), |_| {}).await?;
        Ok(Some(path))
    }

    /// Decrypts the file of `post` to `dest` (a place the user picked).
    pub async fn group_file_save(self: &Arc<Self>, core: &Arc<GroupCore>, gid: &str, post: &str, dest: PathBuf) -> Result<(), Error> {
        let row = core.store.image_of_post(gid, post)?.filter(|r| r.file.is_some()).ok_or(Error::NotFound)?;
        if !row.ready {
            return Err(Error::Rejected("the file is not on this device yet".into()));
        }
        let hub = self.group_hub(core).await?;
        hub.export_plain(Hash::from_bytes(row.image.hash), row.image.key, dest, |_| {}).await?;
        Ok(())
    }

    /// Stops a download of the file of `post` (running, waiting to retry or failed): the
    /// partial bytes are released and the file is `Remote` again. A ready file is left alone.
    pub async fn group_file_cancel(self: &Arc<Self>, core: &Arc<GroupCore>, gid: &str, post: &str) -> Result<(), Error> {
        let mut row = core.store.image_of_post(gid, post)?.filter(|r| r.file.is_some()).ok_or(Error::NotFound)?;
        if row.ready {
            return Ok(());
        }
        let key = (gid.to_string(), row.image.hash);
        if let Some(t) = core.images.files.tasks.lock().remove(&key) {
            t.abort();
        }
        core.images.pending.lock().remove(&key);
        core.images.files.progress_clear(gid, &row.image.hash);
        row.wanted = false;
        row.failed = false;
        row.deleted = true;
        core.store.put_image(gid, &row)?;
        if let Ok(hub) = self.group_hub(core).await {
            let _ = hub.release_owned(&owner_of(gid), &Hash::from_bytes(row.image.hash)).await;
        }
        self.group_image_event(gid, post);
        Ok(())
    }

    /// Deletes this device's copy of the file of `post` (and its opened copy); if we had said
    /// we hold it, the group learns we no longer do. A host keeps the files it keeps for the
    /// group.
    pub async fn group_file_delete(self: &Arc<Self>, core: &Arc<GroupCore>, gid: &str, post: &str) -> Result<(), Error> {
        let mut row = core.store.image_of_post(gid, post)?.filter(|r| r.file.is_some()).ok_or(Error::NotFound)?;
        if core.am_host(gid) && !core.over_keep(&row) {
            return Err(Error::Rejected("this device hosts the group and keeps this file for it".into()));
        }
        if !row.ready {
            return self.group_file_cancel(core, gid, post).await;
        }
        let hub = self.group_hub(core).await?;
        let _ = hub.release_owned(&owner_of(gid), &Hash::from_bytes(row.image.hash)).await;
        if let Some(dest) = Self::group_file_dest(core, &row)
            && let Some(dir) = dest.parent()
        {
            let _ = std::fs::remove_dir_all(dir);
        }
        row.ready = false;
        row.wanted = false;
        row.failed = false;
        row.deleted = true;
        core.store.put_image(gid, &row)?;
        self.group_file_announce(core, gid, &row, false);
        self.group_image_event(gid, post);
        Ok(())
    }
}
