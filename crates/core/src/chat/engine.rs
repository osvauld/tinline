//! The chat runtime: sealed store, shard cache, sessions over `tinline/chat/1`, outbox/retry,
//! blob transfers. Everything hangs off `Inner` (see `node.rs`); the UniFFI methods in `api.rs`
//! are thin wrappers over the `chat_*` functions here.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use iroh::endpoint::{Accepting, Connection};
use iroh_blobs::Hash;
use loro::VersionVector;
use parking_lot::Mutex;
use proto::Msg;
use storage::{Op, Sealed, Store as Db};
use tokio::sync::{OnceCell, Semaphore, mpsc, oneshot};

use super::api::{Attachment, AttachmentKind, Chat, ChatEvents, DayPage, DeliveryState, Message, TransferState};
use super::blobs::{BlobHub, Gate, UploadProgress};
use super::crypt;
use super::doc::*;
use super::store::*;
use super::wire::*;
use crate::Error;
use crate::node::{Inner, Me, addr_for, now, relay_hint, relay_of};

const DIAL_TIMEOUT: Duration = Duration::from_secs(20);
const AUTH_TIMEOUT: Duration = Duration::from_secs(15);
const HELLO_DAYS: i64 = 7;
const MAX_HELLO_SHARDS: usize = 64;
/// A day this old (or older) is compacted into a snapshot blob.
const CLOSE_AFTER_DAYS: i64 = 8;
const UPDATES_BEFORE_SNAPSHOT: u32 = 64;
const MAX_SHARDS_CACHED: usize = 12;

pub struct ChatCore {
    pub store: ChatStore,
    pub dir: PathBuf,
    hub: OnceCell<Arc<BlobHub>>,
    shards: Mutex<HashMap<(String, String), Shard>>,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    dialing: Mutex<HashSet<String>>,
    downloading: Mutex<HashMap<String, (u64, u64)>>,
    pulling: Mutex<HashSet<(String, String)>>,
    dial_sem: Arc<Semaphore>,
    next_session: AtomicU64,
}

pub enum Cmd {
    Push(String),
    Ack(String),
    History(String, u32),
    Close,
}

pub struct Session {
    id: u64,
    pub did: String,
    pub device: [u8; 32],
    dialer: [u8; 32],
    tx: mpsc::UnboundedSender<Cmd>,
    peer_vv: Mutex<HashMap<String, VersionVector>>,
    hist: Mutex<Option<oneshot::Sender<Vec<HistDay>>>>,
}

fn day_valid(day: &str) -> bool {
    day_start(day).is_some()
}

fn decode_vv(b: &[u8]) -> Result<VersionVector, Error> {
    VersionVector::decode(b).map_err(|_| Error::Protocol("bad version vector".into()))
}

fn preview(rec: &MsgRec) -> String {
    if rec.deleted {
        return "Message deleted".into();
    }
    if !rec.text.is_empty() {
        return rec.text.chars().take(120).collect();
    }
    match &rec.file {
        Some(f) if f.kind == "voice" => "Voice message".into(),
        Some(f) => f.name.clone(),
        None => String::new(),
    }
}

fn parse_hash(h: &str) -> Result<Hash, Error> {
    h.parse::<Hash>().map_err(|_| Error::Protocol("bad blob hash".into()))
}

fn parse_key(k: &str) -> Result<[u8; 32], Error> {
    hex::decode(k).ok().and_then(|b| <[u8; 32]>::try_from(b).ok()).ok_or_else(|| Error::Protocol("bad blob key".into()))
}

fn random_msg_id() -> String {
    use rand::RngCore;
    let mut b = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut b);
    hex::encode(b)
}

/// The chat clock. `P2P_CHAT_CLOCK_MS_OFFSET` shifts it (a test knob for the day-shard tests).
fn now_ms() -> i64 {
    static OFFSET: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    let off = *OFFSET.get_or_init(|| std::env::var("P2P_CHAT_CLOCK_MS_OFFSET").ok().and_then(|v| v.parse().ok()).unwrap_or(0));
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0) + off
}

impl Inner {
    // ---- lifecycle -------------------------------------------------------------------------

    /// Opens the sealed chat store; called whenever the data key becomes available.
    pub(crate) fn chat_open(self: &Arc<Self>) {
        let dek = match self.shared.lock().dek.as_ref() {
            Some(d) => **d,
            None => return,
        };
        if self.chat.lock().is_some() {
            return;
        }
        let key = blake3::derive_key("tinline chat store v1", &dek);
        let dir = self.store.dir().to_path_buf();
        let db = match Db::open(dir.join("chat.redb")) {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("chat store: {e}");
                return;
            }
        };
        let core = Arc::new(ChatCore {
            store: ChatStore::new(Sealed::new(db, key)),
            dir,
            hub: OnceCell::new(),
            shards: Default::default(),
            sessions: Default::default(),
            dialing: Default::default(),
            downloading: Default::default(),
            pulling: Default::default(),
            dial_sem: Arc::new(Semaphore::new(4)),
            next_session: AtomicU64::new(1),
        });
        *self.chat.lock() = Some(core);
    }

    pub(crate) async fn chat_close(self: &Arc<Self>) {
        let core = self.chat.lock().take();
        if let Some(core) = core {
            for s in core.sessions.lock().values() {
                let _ = s.tx.send(Cmd::Close);
            }
            if let Some(h) = core.hub.get() {
                h.shutdown().await;
            }
        }
    }

    pub(crate) fn chat_sessions_close(self: &Arc<Self>) {
        if let Some(core) = self.chat.lock().clone() {
            for s in core.sessions.lock().values() {
                let _ = s.tx.send(Cmd::Close);
            }
        }
    }

    pub(crate) fn chat_core(&self) -> Result<Arc<ChatCore>, Error> {
        match self.chat.lock().clone() {
            Some(c) => Ok(c),
            None => match self.me() {
                Err(e) => Err(e),
                Ok(_) => Err(Error::Locked),
            },
        }
    }

    fn ev(&self) -> Option<Arc<dyn ChatEvents>> {
        self.chat_events.lock().clone()
    }

    /// Called once the endpoint is up: reconnect to everyone we owe something, and keep trying.
    pub(crate) fn chat_started(self: &Arc<Self>) {
        let this = self.clone();
        self.handle.spawn(async move {
            // Everyone once at start: catches up what they wrote while we were away.
            let dids: Vec<String> = this.shared.lock().state.contacts.iter().map(|c| c.did.clone()).collect();
            for d in dids {
                this.chat_kick(d);
            }
            let mut tick = tokio::time::interval(Duration::from_secs(30));
            let mut n = 0u32;
            loop {
                tick.tick().await;
                if this.shared.lock().endpoint.is_none() {
                    break;
                }
                this.chat_kick_pending();
                n += 1;
                if n % 120 == 1
                    && let Err(e) = this.chat_compact().await
                {
                    tracing::warn!("chat compaction: {e}");
                }
            }
        });
    }

    /// Dials every contact we have unsent ops or wanted downloads for.
    pub(crate) fn chat_kick_pending(self: &Arc<Self>) {
        let Ok(core) = self.chat_core() else { return };
        let Ok(me) = self.me() else { return };
        let dids: Vec<String> = self.shared.lock().state.contacts.iter().map(|c| c.did.clone()).collect();
        for did in dids {
            let pair = pair_id(me.id.did(), &did);
            if core.store.pending_days(&pair).map(|d| !d.is_empty()).unwrap_or(false) || self.chat_wants(&core, &pair) {
                self.chat_kick(did);
            }
        }
    }

    fn chat_wants(&self, core: &ChatCore, pair: &str) -> bool {
        core.store
            .wanted_blobs()
            .map(|v| v.iter().any(|(_, b)| b.pair == pair))
            .unwrap_or(false)
    }

    fn chat_session(&self, core: &ChatCore, did: &str) -> Option<Arc<Session>> {
        core.sessions.lock().get(did).cloned()
    }

    /// Makes sure there is (or soon will be) a session with `did`, retrying with backoff while
    /// there is something to send.
    pub(crate) fn chat_kick(self: &Arc<Self>, did: String) {
        let Ok(core) = self.chat_core() else { return };
        if self.chat_session(&core, &did).is_some() || !core.dialing.lock().insert(did.clone()) {
            return;
        }
        let this = self.clone();
        self.handle.spawn(async move {
            let mut delay = Duration::from_secs(2);
            loop {
                if this.chat_session(&core, &did).is_some() {
                    break;
                }
                let Ok(me) = this.me() else { break };
                if this.shared.lock().endpoint.is_none() || !this.shared.lock().state.contacts.iter().any(|c| c.did == did) {
                    break;
                }
                let permit = core.dial_sem.clone().acquire_owned().await;
                let res = this.chat_dial(&core, &me, &did).await;
                drop(permit);
                match res {
                    Ok(()) => break,
                    Err(e) => tracing::debug!("chat dial {did}: {e}"),
                }
                let pair = pair_id(me.id.did(), &did);
                let pending = core.store.pending_days(&pair).map(|d| !d.is_empty()).unwrap_or(false) || this.chat_wants(&core, &pair);
                if !pending {
                    break;
                }
                tokio::time::sleep(delay).await;
                delay = (delay * 2).min(Duration::from_secs(60));
            }
            core.dialing.lock().remove(&did);
        });
    }

    // ---- connections -------------------------------------------------------------------------

    fn auth_msg(&self, me: &Me, grant: proto::SignedGrant) -> ChatMsg {
        let relay = self.endpoint().ok().and_then(|ep| relay_of(&ep));
        match proto::call_hello(&me.id, me.attestation.clone(), grant, "chat".into()) {
            Msg::CallHello { attestation, grant, .. } => ChatMsg::Auth { attestation, grant, relay },
            _ => unreachable!(),
        }
    }

    /// Verifies a peer's `Auth` like a call hello: attested device = the connection's key, a
    /// grant we issued them, and they are a contact who is not blocked. Returns their DID.
    fn verify_auth(&self, me: &Me, auth: &ChatMsg, remote: [u8; 32]) -> Result<(String, Option<String>), Error> {
        let ChatMsg::Auth { attestation, grant, relay } = auth else { return Err(Error::Protocol("expected Auth".into())) };
        let msg = Msg::CallHello { call_id: "chat".into(), attestation: attestation.clone(), grant: grant.clone(), relay: relay.clone() };
        let s = self.shared.lock();
        let contacts = &s.state.contacts;
        let who = proto::accept_call_hello(&me.id, &msg, remote, now(), &s.state.revoked, |did| {
            contacts.iter().any(|c| c.did == did) && !s.state.blocked.contains(did)
        })?;
        Ok((who.did, relay_hint(relay)))
    }

    async fn chat_dial(self: &Arc<Self>, core: &Arc<ChatCore>, me: &Arc<Me>, did: &str) -> Result<(), Error> {
        let ep = self.endpoint()?;
        let contact = self
            .shared
            .lock()
            .state
            .contacts
            .iter()
            .find(|c| c.did == did)
            .cloned()
            .ok_or(Error::NotFound)?;
        let mut last = Error::NotFound;
        for device in &contact.devices {
            let addr = match addr_for(device, contact.relay.as_deref()) {
                Ok(a) => a,
                Err(e) => {
                    last = e;
                    continue;
                }
            };
            let conn = match tokio::time::timeout(DIAL_TIMEOUT, ep.connect(addr, CHAT_ALPN)).await {
                Ok(Ok(c)) => c,
                Ok(Err(e)) => {
                    last = Error::net(e);
                    continue;
                }
                Err(_) => {
                    last = Error::Timeout;
                    continue;
                }
            };
            match self.chat_handshake(core, me, conn, true).await {
                Ok(()) => return Ok(()),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    /// Opens (dialer) or accepts (listener) the stream, exchanges `Auth`, registers the session
    /// and starts its task.
    async fn chat_handshake(self: &Arc<Self>, core: &Arc<ChatCore>, me: &Arc<Me>, conn: Connection, dialer: bool) -> Result<(), Error> {
        let remote = *conn.remote_id().as_bytes();
        let (send, recv) = if dialer { conn.open_bi().await } else { conn.accept_bi().await }.map_err(Error::net)?;
        let mut writer = ChatWriter(send);
        let mut reader = ChatReader::new(recv);
        let (did, hint) = if dialer {
            // We know who we are calling: present the grant they gave us.
            let grant = {
                let s = self.shared.lock();
                let c = s.state.contacts.iter().find(|c| c.devices.contains(&remote));
                c.map(|c| c.grant_from_them.clone())
            };
            let grant = grant.ok_or(Error::NotFound)?;
            writer.send(&self.auth_msg(me, grant)).await?;
            let reply = tokio::time::timeout(AUTH_TIMEOUT, reader.recv())
                .await
                .map_err(|_| Error::Timeout)??
                .ok_or_else(|| Error::Protocol("peer closed".into()))?;
            self.verify_auth(me, &reply, remote)?
        } else {
            let first = tokio::time::timeout(AUTH_TIMEOUT, reader.recv())
                .await
                .map_err(|_| Error::Timeout)??
                .ok_or_else(|| Error::Protocol("peer closed before auth".into()))?;
            let (did, hint) = self.verify_auth(me, &first, remote)?;
            let grant = {
                let s = self.shared.lock();
                s.state.contacts.iter().find(|c| c.did == did).map(|c| c.grant_from_them.clone())
            }
            .ok_or(Error::NotFound)?;
            writer.send(&self.auth_msg(me, grant)).await?;
            (did, hint)
        };
        self.note_device(&did, remote, hint);
        let (tx, rx) = mpsc::unbounded_channel();
        let my_dev = me.device;
        let sess = Arc::new(Session {
            id: core.next_session.fetch_add(1, Ordering::Relaxed),
            did: did.clone(),
            device: remote,
            dialer: if dialer { my_dev } else { remote },
            tx,
            peer_vv: Default::default(),
            hist: Default::default(),
        });
        // One session per contact: if both sides dialled at once, the one dialled by the lower
        // device key survives on both ends.
        {
            let mut map = core.sessions.lock();
            if let Some(old) = map.get(&did)
                && old.dialer != sess.dialer
                && old.dialer < sess.dialer
            {
                conn.close(0u32.into(), b"duplicate");
                return Ok(());
            }
            if let Some(old) = map.insert(did.clone(), sess.clone()) {
                let _ = old.tx.send(Cmd::Close);
            }
        }
        self.log(format!("chat session with {did} ({})", if dialer { "dialed" } else { "accepted" }));
        let this = self.clone();
        let core = core.clone();
        self.handle.spawn(async move {
            this.chat_run(core, sess, conn, reader, writer, rx).await;
        });
        Ok(())
    }

    pub(crate) async fn handle_chat_incoming(self: Arc<Self>, accepting: Accepting, chat: bool, permit: tokio::sync::OwnedSemaphorePermit) -> Result<(), Error> {
        let conn = tokio::time::timeout(AUTH_TIMEOUT, accepting).await.map_err(|_| Error::Timeout)?.map_err(Error::net)?;
        let remote = *conn.remote_id().as_bytes();
        if chat {
            let me = self.me()?;
            let core = self.chat_core()?;
            if !self.is_contact_device(&remote) {
                // A new device of a contact is recognised by its Auth; strangers by nothing.
                // Fall through to the handshake, which checks the proof.
            }
            let r = self.chat_handshake(&core, &me, conn.clone(), false).await;
            drop(permit);
            if r.is_err() {
                conn.close(0u32.into(), b"not accepted");
            }
            return r;
        }
        drop(permit);
        // Blobs: only devices we know as a contact's (or in an authenticated session).
        let known = self.is_contact_device(&remote)
            || self.chat_core().map(|c| c.sessions.lock().values().any(|s| s.device == remote)).unwrap_or(false);
        if !known {
            conn.close(0u32.into(), b"not accepted");
            return Ok(());
        }
        let hub = self.chat_hub().await?;
        use iroh::protocol::ProtocolHandler;
        hub.protocol().accept(conn).await.map_err(|e| Error::Net(e.to_string()))
    }

    async fn chat_run(
        self: Arc<Self>,
        core: Arc<ChatCore>,
        sess: Arc<Session>,
        conn: Connection,
        mut reader: ChatReader,
        mut writer: ChatWriter,
        mut cmds: mpsc::UnboundedReceiver<Cmd>,
    ) {
        let reason: String = 'run: {
            match self.chat_hello(&core, &sess) {
                Ok(h) => {
                    if let Err(e) = writer.send(&h).await {
                        break 'run e.to_string();
                    }
                }
                Err(e) => break 'run e.to_string(),
            }
            self.chat_resume_downloads(&core, &sess.did);
            loop {
                tokio::select! {
                    m = reader.recv() => match m {
                        Ok(Some(m)) => {
                            if let Err(e) = self.chat_handle(&core, &sess, m, &mut writer).await {
                                break 'run e.to_string();
                            }
                        }
                        Ok(None) => break 'run "closed".into(),
                        Err(e) => break 'run e.to_string(),
                    },
                    c = cmds.recv() => match c {
                        Some(Cmd::Push(day)) => {
                            if let Err(e) = self.chat_push(&core, &sess, &day, &mut writer).await {
                                break 'run e.to_string();
                            }
                        }
                        Some(Cmd::Ack(day)) => {
                            let me = match self.me() { Ok(m) => m, Err(e) => break 'run e.to_string() };
                            let pair = pair_id(me.id.did(), &sess.did);
                            if let Ok(Some(meta)) = core.store.shard_meta(&pair, &day)
                                && let Err(e) = writer.send(&ChatMsg::Ack { doc: doc_name(&pair, &day), vv: meta.vv }).await
                            {
                                break 'run e.to_string();
                            }
                        }
                        Some(Cmd::History(before, limit)) => {
                            if let Err(e) = writer.send(&ChatMsg::HistoryReq { before, limit }).await {
                                break 'run e.to_string();
                            }
                        }
                        Some(Cmd::Close) | None => break 'run "closed locally".into(),
                    },
                    _ = conn.closed() => break 'run "connection lost".into(),
                }
            }
        };
        self.log(format!("chat session with {} ended: {reason}", sess.did));
        {
            let mut map = core.sessions.lock();
            if map.get(&sess.did).is_some_and(|s| s.id == sess.id) {
                map.remove(&sess.did);
            }
        }
        sess.hist.lock().take();
        let _ = writer.0.finish();
        let c2 = conn.clone();
        tokio::spawn(async move {
            let _ = tokio::time::timeout(Duration::from_millis(300), c2.closed()).await;
            c2.close(0u32.into(), b"bye");
        });
        // Something left unsent: try again.
        if let Ok(me) = self.me() {
            let pair = pair_id(me.id.did(), &sess.did);
            if core.store.pending_days(&pair).map(|d| !d.is_empty()).unwrap_or(false) && self.shared.lock().endpoint.is_some() {
                let this = self.clone();
                let did = sess.did.clone();
                self.handle.spawn(async move {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    this.chat_kick(did);
                });
            }
        }
    }

    // ---- hello / sync -----------------------------------------------------------------------

    fn recent_days(&self) -> Vec<String> {
        let t = now_ms();
        (0..HELLO_DAYS).map(|i| day_of(t - i * DAY_MS)).collect()
    }

    fn chat_hello(&self, core: &ChatCore, sess: &Session) -> Result<ChatMsg, Error> {
        let me = self.me()?;
        let pair = pair_id(me.id.did(), &sess.did);
        let mut days: Vec<String> = self.recent_days();
        for d in core.store.pending_days(&pair)? {
            if !days.contains(&d) {
                days.push(d);
            }
        }
        let mut shards = Vec::new();
        for d in days {
            if let Some(m) = core.store.shard_meta(&pair, &d)? {
                shards.push((d, m.vv));
            }
            if shards.len() >= MAX_HELLO_SHARDS {
                break;
            }
        }
        Ok(ChatMsg::Hello { shards })
    }

    async fn chat_handle(self: &Arc<Self>, core: &Arc<ChatCore>, sess: &Arc<Session>, msg: ChatMsg, writer: &mut ChatWriter) -> Result<(), Error> {
        let me = self.me()?;
        let pair = pair_id(me.id.did(), &sess.did);
        match msg {
            ChatMsg::Auth { .. } => Err(Error::Protocol("unexpected Auth".into())),
            ChatMsg::Hello { shards } => {
                if shards.len() > MAX_HELLO_SHARDS {
                    return Err(Error::Protocol("too many shards".into()));
                }
                let mut theirs: HashMap<String, VersionVector> = HashMap::new();
                for (d, vv) in shards {
                    if !day_valid(&d) {
                        return Err(Error::Protocol("bad day".into()));
                    }
                    theirs.insert(d, decode_vv(&vv)?);
                }
                let mut days: Vec<String> = self.recent_days();
                for d in core.store.pending_days(&pair)? {
                    if !days.contains(&d) {
                        days.push(d);
                    }
                }
                for d in theirs.keys() {
                    if !days.contains(d) {
                        days.push(d.clone());
                    }
                }
                for day in days {
                    let their_vv = theirs.get(&day).cloned().unwrap_or_default();
                    sess.peer_vv.lock().insert(day.clone(), their_vv.clone());
                    self.chat_ack(core, &me, &sess.did, &pair, &day, &their_vv)?;
                    self.chat_send_own(core, &me, sess, &pair, &day, &their_vv, writer, true).await?;
                }
                Ok(())
            }
            ChatMsg::Sync { doc, vv, update, sig } => {
                let their = decode_vv(&vv)?;
                let day = self.chat_doc_day(&me, sess, &doc)?;
                self.chat_receive(core, &me, sess, &doc, &update, &sig, false, writer).await?;
                sess.peer_vv.lock().insert(day, their);
                Ok(())
            }
            ChatMsg::Push { doc, update, sig } => self.chat_receive(core, &me, sess, &doc, &update, &sig, true, writer).await,
            ChatMsg::Ack { doc, vv } => {
                let day = self.chat_doc_day(&me, sess, &doc)?;
                let vv = decode_vv(&vv)?;
                sess.peer_vv.lock().entry(day.clone()).and_modify(|v| merge_vv(v, &vv)).or_insert_with(|| vv.clone());
                self.chat_ack(core, &me, &sess.did, &pair, &day, &vv)
            }
            ChatMsg::HistoryReq { before, limit } => {
                if !day_valid(&before) {
                    return Err(Error::Protocol("bad day".into()));
                }
                let days = self.chat_history_days(core, &me, &pair, &before, limit.min(14)).await?;
                writer.send(&ChatMsg::HistoryResp { days }).await
            }
            ChatMsg::HistoryResp { days } => {
                if let Some(tx) = sess.hist.lock().take() {
                    let _ = tx.send(days);
                }
                Ok(())
            }
        }
    }

    /// `dm/{pair}/{day}` -> day, checking it names this conversation.
    fn chat_doc_day(&self, me: &Me, sess: &Session, doc: &str) -> Result<String, Error> {
        let pair = pair_id(me.id.did(), &sess.did);
        let rest = doc.strip_prefix("dm/").ok_or_else(|| Error::Protocol("bad doc".into()))?;
        let (p, day) = rest.split_once('/').ok_or_else(|| Error::Protocol("bad doc".into()))?;
        if p != pair || !day_valid(day) {
            return Err(Error::Protocol("doc is not part of this conversation".into()));
        }
        Ok(day.to_string())
    }

    /// Loads the shard into the cache (creating an empty one) and returns whether it was there.
    pub(crate) async fn chat_shard(self: &Arc<Self>, core: &Arc<ChatCore>, me: &Me, pair: &str, day: &str) -> Result<(), Error> {
        let key = (pair.to_string(), day.to_string());
        if core.shards.lock().contains_key(&key) {
            return Ok(());
        }
        let meta = core.store.shard_meta(pair, day)?;
        let (snap, ups) = core.store.shard_rows(pair, day)?;
        let base = match (&snap, meta.as_ref().and_then(|m| m.closed.clone())) {
            (None, Some(c)) => {
                let hub = self.chat_hub().await?;
                let hash = parse_hash(&c.hash)?;
                let k = parse_key(&c.key)?;
                let ct = hub.read_cipher(&hash).await?;
                let plain = tokio::task::spawn_blocking(move || crypt::decrypt_bytes(&k, &ct))
                    .await
                    .map_err(|e| Error::Io(e.to_string()))?
                    .map_err(|e| Error::Io(e.to_string()))?;
                Some(plain)
            }
            _ => snap,
        };
        let shard = Shard::load(pair, day, &me.device, base.as_deref(), &ups).map_err(|e| Error::Io(e.to_string()))?;
        let mut cache = core.shards.lock();
        if cache.len() >= MAX_SHARDS_CACHED {
            let today = day_of(now_ms());
            cache.retain(|(_, d), _| *d == today);
        }
        cache.entry(key).or_insert(shard);
        Ok(())
    }

    /// Persists a shard change: the update (or a fresh snapshot every so often), the meta, and
    /// `extra` ops, in one transaction. Call with the shard cache locked.
    fn chat_persist(&self, core: &ChatCore, pair: &str, shard: &Shard, update: &[u8], mut extra: Vec<Op>) -> Result<(), Error> {
        let day = shard.day.as_str();
        let mut meta = core.store.shard_meta(pair, day)?.unwrap_or_default();
        meta.vv = shard.vv().encode();
        meta.n_messages = shard.messages().map(|m| m.len() as u32).unwrap_or(meta.n_messages);
        if meta.n_updates + 1 > UPDATES_BEFORE_SNAPSHOT {
            extra.extend(core.store.put_snap_ops(pair, day, &shard.snapshot())?);
            meta.n_updates = 0;
        } else {
            extra.push(core.store.put_update_op(pair, day, meta.next_seq, update)?);
            meta.next_seq += 1;
            meta.n_updates += 1;
        }
        extra.push(core.store.put_meta_op(pair, day, &meta)?);
        core.store.db.apply(&extra).map_err(io)
    }

    /// Registers the file of a message (a new `blob/` record, the read permission) and returns
    /// the ops, plus the hash if it should be downloaded now.
    fn chat_register_file(&self, core: &ChatCore, pair: &str, day: &str, rec: &MsgRec, incoming: bool) -> Result<(Vec<Op>, Option<String>), Error> {
        let Some(f) = &rec.file else { return Ok((vec![], None)) };
        // Validate before it is trusted as a path component or a key.
        parse_hash(&f.hash)?;
        parse_key(&f.key)?;
        let mut ops = core.store.ref_ops(&f.hash, pair)?;
        if core.store.blob(&f.hash)?.is_some() {
            return Ok((ops, None));
        }
        let wanted = !incoming || (f.size <= core.store.auto_download() && f.size <= crypt::MAX_FILE);
        let info = BlobInfo {
            key: f.key.clone(),
            name: f.name.clone(),
            size: f.size,
            mime: f.mime.clone(),
            kind: f.kind.clone(),
            pair: pair.to_string(),
            ready: !incoming,
            wanted,
            failed: false,
            day: day.to_string(),
            msg: rec.id.clone(),
        };
        ops.push(core.store.put_blob_op(&f.hash, &info)?);
        Ok((ops, (incoming && wanted).then(|| f.hash.clone())))
    }

    /// Conversation row after `rec` was added or changed.
    fn chat_conv_ops(&self, core: &ChatCore, me: &Me, did: &str, pair: &str, recs: &[(&MsgRec, bool)], new_unread: u32) -> Result<(Vec<Op>, ConvMeta), Error> {
        let mut conv = core.store.conv(pair)?.unwrap_or_else(|| ConvMeta { peer_did: did.to_string(), ..Default::default() });
        conv.peer_did = did.to_string();
        conv.unread = conv.unread.saturating_add(new_unread);
        for (rec, _) in recs {
            let key = (rec.at, rec.id.as_str());
            let newer = conv.last.as_ref().is_none_or(|l| (l.at, l.id.as_str()) <= key);
            if newer || conv.last.as_ref().is_some_and(|l| l.id == rec.id) {
                conv.last = Some(LastMsg { id: rec.id.clone(), at: rec.at, preview: preview(rec), outgoing: rec.author == me.id.did() });
            }
        }
        Ok((vec![core.store.conv_op(pair, &conv)?], conv))
    }

    // ---- receiving ---------------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    async fn chat_receive(
        self: &Arc<Self>,
        core: &Arc<ChatCore>,
        me: &Arc<Me>,
        sess: &Arc<Session>,
        doc: &str,
        update: &[u8],
        sig: &[u8],
        live: bool,
        writer: &mut ChatWriter,
    ) -> Result<(), Error> {
        let day = self.chat_doc_day(me, sess, doc)?;
        if !verify_batch(&sess.device, doc, update, sig) {
            return Err(Error::Protocol(Reject::BadSignature.to_string()));
        }
        let pair = pair_id(me.id.did(), &sess.did);
        self.chat_shard(core, me, &pair, &day).await?;
        let mut added_files = Vec::new();
        let mut unchanged: Option<VersionVector> = None;
        let mut need_history = false;
        let (vv_now, notify) = 'blk: {
            let mut shards = core.shards.lock();
            let shard = shards.get_mut(&(pair.clone(), day.clone())).ok_or(Error::NotFound)?;
            let before = shard.vv();
            let ctx = Ctx {
                signer_did: sess.did.clone(),
                signer_peer: peer_id(&sess.device, doc),
                now_ms: now_ms(),
                live,
                history: None,
            };
            let applied = match shard.apply_remote(update, &ctx) {
                Ok(a) => a,
                // Their ops build on ops of ours that we lost (a reinstall): the batch cannot
                // be judged on its own. Ask them for the day as a vouched snapshot instead.
                Err(Reject::Pending) => {
                    need_history = true;
                    unchanged = Some(shard.vv());
                    break 'blk (shard.vv(), (Vec::new(), ConvMeta::default()));
                }
                Err(r) => {
                    self.log(format!("rejected chat batch from {}: {r}", sess.did));
                    return Err(Error::Protocol(format!("rejected batch: {r}")));
                }
            };
            if shard.vv() == before {
                unchanged = Some(shard.vv());
                (shard.vv(), (Vec::new(), ConvMeta::default()))
            } else {
            let msgs = shard.messages().map_err(|e| Error::Io(e.to_string()))?;
            let touched: Vec<&MsgRec> = applied.added.iter().chain(applied.changed.iter()).filter_map(|id| msgs.get(id)).collect();
            let mut ops = Vec::new();
            let mut unread = 0;
            for rec in &touched {
                if applied.added.contains(&rec.id) {
                    ops.push(core.store.mi_op(&pair, &rec.id, &day)?);
                    if rec.author != me.id.did() && !rec.deleted {
                        unread += 1;
                    }
                }
                let (o, dl) = self.chat_register_file(core, &pair, &day, rec, rec.author != me.id.did())?;
                ops.extend(o);
                if let Some(h) = dl {
                    added_files.push(h);
                }
            }
            let flags: Vec<(&MsgRec, bool)> = touched.iter().map(|r| (*r, applied.added.contains(&r.id))).collect();
            let (cops, conv) = self.chat_conv_ops(core, me, &sess.did, &pair, &flags, unread)?;
            ops.extend(cops);
            self.chat_persist(core, &pair, shard, update, ops)?;
            let owned: Vec<(MsgRec, bool)> = flags.iter().map(|(r, a)| ((*r).clone(), *a)).collect();
            (shard.vv(), (owned, conv))
            }
        };
        if need_history {
            self.chat_pull_day(core, me, sess, &day);
            return Ok(());
        }
        if unchanged.is_some() {
            let _ = writer.send(&ChatMsg::Ack { doc: doc.to_string(), vv: vv_now.encode() }).await;
            return Ok(());
        }
        let (recs, conv) = notify;
        if let Some(ev) = self.ev() {
            for (rec, added) in &recs {
                let m = self.chat_api_message(core, me, &sess.did, &pair, &day, rec);
                if *added { ev.on_message_added(m) } else { ev.on_message_changed(m) }
            }
            if let Some(c) = self.chat_api_chat(core, me, &conv) {
                ev.on_chat_changed(c);
            }
        }
        let _ = writer.send(&ChatMsg::Ack { doc: doc.to_string(), vv: vv_now.encode() }).await;
        for h in added_files {
            self.chat_download(sess.did.clone(), h);
        }
        Ok(())
    }

    /// The peer says it holds `their_vv` of a day: our messages up to there are delivered.
    fn chat_ack(&self, core: &ChatCore, me: &Me, did: &str, pair: &str, day: &str, their_vv: &VersionVector) -> Result<(), Error> {
        let Some(meta) = core.store.shard_meta(pair, day)? else { return Ok(()) };
        let name = doc_name(pair, day);
        let my_peer = peer_id(&me.device, &name);
        let mut acked = match core.store.ack(pair, day)? {
            Some(b) => decode_vv(&b).unwrap_or_default(),
            None => VersionVector::default(),
        };
        let old = acked.get(&my_peer).copied().unwrap_or(0);
        merge_vv(&mut acked, their_vv);
        let new = acked.get(&my_peer).copied().unwrap_or(0);
        if new == old {
            return Ok(());
        }
        let mine = decode_vv(&meta.vv).ok().and_then(|v| v.get(&my_peer).copied()).unwrap_or(0);
        let newly: Vec<String> = core
            .store
            .mcs(pair, day)?
            .into_iter()
            .filter(|(_, c)| *c > old && *c <= new)
            .map(|(id, _)| id)
            .collect();
        core.store.db.apply(&[core.store.put_ack_op(pair, day, &acked.encode())?, core.store.out_op(pair, day, mine > new)?]).map_err(io)?;
        if let Some(ev) = self.ev() {
            for id in &newly {
                ev.on_delivery_changed(did.to_string(), id.clone(), DeliveryState::Delivered);
            }
            if !newly.is_empty()
                && let Ok(Some(conv)) = core.store.conv(pair)
                && conv.last.as_ref().is_some_and(|l| newly.contains(&l.id))
                && let Some(c) = self.chat_api_chat(core, me, &conv)
            {
                ev.on_chat_changed(c);
            }
        }
        Ok(())
    }

    // ---- sending sync traffic ------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    async fn chat_send_own(
        self: &Arc<Self>,
        core: &Arc<ChatCore>,
        me: &Arc<Me>,
        sess: &Arc<Session>,
        pair: &str,
        day: &str,
        their_vv: &VersionVector,
        writer: &mut ChatWriter,
        sync: bool,
    ) -> Result<(), Error> {
        let Some(meta) = core.store.shard_meta(pair, day)? else { return Ok(()) };
        let name = doc_name(pair, day);
        let my_peer = peer_id(&me.device, &name);
        let mine = decode_vv(&meta.vv)?.get(&my_peer).copied().unwrap_or(0);
        if their_vv.get(&my_peer).copied().unwrap_or(0) >= mine {
            return Ok(());
        }
        self.chat_shard(core, me, pair, day).await?;
        let out = {
            let shards = core.shards.lock();
            shards.get(&(pair.to_string(), day.to_string())).and_then(|s| s.export_own_since(their_vv).map(|u| (u, s.vv(), s.peer, s.my_counter())))
        };
        let Some((update, vv, peer, counter)) = out else { return Ok(()) };
        let sig = sign_batch(&me.profile.device_secret, &name, &update);
        let msg = if sync {
            ChatMsg::Sync { doc: name, vv: vv.encode(), update, sig }
        } else {
            ChatMsg::Push { doc: name, update, sig }
        };
        writer.send(&msg).await?;
        sess.peer_vv.lock().entry(day.to_string()).or_default().insert(peer, counter);
        Ok(())
    }

    async fn chat_push(self: &Arc<Self>, core: &Arc<ChatCore>, sess: &Arc<Session>, day: &str, writer: &mut ChatWriter) -> Result<(), Error> {
        let me = self.me()?;
        let pair = pair_id(me.id.did(), &sess.did);
        let theirs = sess.peer_vv.lock().get(day).cloned().unwrap_or_default();
        self.chat_send_own(core, &me, sess, &pair, day, &theirs, writer, false).await
    }

    // ---- local writes ----------------------------------------------------------------------------

    fn chat_contact(&self, did: &str) -> Result<(), Error> {
        let s = self.shared.lock();
        if s.state.contacts.iter().any(|c| c.did == did) { Ok(()) } else { Err(Error::NotFound) }
    }

    /// Adds a message of ours to today's shard, persists it, tells the session (or dials).
    pub(crate) async fn chat_send_message(
        self: &Arc<Self>,
        did: &str,
        text: String,
        reply_to: Option<String>,
        file: Option<FileRef>,
    ) -> Result<Message, Error> {
        let me = self.me()?;
        let core = self.chat_core()?;
        self.chat_contact(did)?;
        if text.len() > MAX_TEXT {
            return Err(Error::Protocol("message too long".into()));
        }
        let pair = pair_id(me.id.did(), did);
        let at = now_ms();
        let day = day_of(at);
        self.chat_shard(&core, &me, &pair, &day).await?;
        let rec = MsgRec { id: random_msg_id(), author: me.id.did().to_string(), at, text, edited_at: None, deleted: false, reply_to, file };
        let conv = {
            let mut shards = core.shards.lock();
            let shard = shards.get_mut(&(pair.clone(), day.clone())).ok_or(Error::NotFound)?;
            let update = shard.add_message(&rec).map_err(|e| Error::Io(e.to_string()))?;
            let mut ops = vec![
                core.store.put_mc_op(&pair, &day, &rec.id, shard.my_counter())?,
                core.store.out_op(&pair, &day, true)?,
                core.store.mi_op(&pair, &rec.id, &day)?,
            ];
            let (fo, _) = self.chat_register_file(&core, &pair, &day, &rec, false)?;
            ops.extend(fo);
            let (cops, conv) = self.chat_conv_ops(&core, &me, did, &pair, &[(&rec, true)], 0)?;
            ops.extend(cops);
            self.chat_persist(&core, &pair, shard, &update, ops)?;
            conv
        };
        let m = self.chat_api_message(&core, &me, did, &pair, &day, &rec);
        self.chat_after_local_write(&core, did, &day);
        if let Some(ev) = self.ev() {
            ev.on_message_added(m.clone());
            if let Some(c) = self.chat_api_chat(&core, &me, &conv) {
                ev.on_chat_changed(c);
            }
        }
        Ok(m)
    }

    fn chat_after_local_write(self: &Arc<Self>, core: &Arc<ChatCore>, did: &str, day: &str) {
        match self.chat_session(core, did) {
            Some(s) => {
                let _ = s.tx.send(Cmd::Push(day.to_string()));
            }
            None => self.chat_kick(did.to_string()),
        }
    }

    pub(crate) async fn chat_edit(self: &Arc<Self>, did: &str, id: &str, text: Option<String>) -> Result<Message, Error> {
        let me = self.me()?;
        let core = self.chat_core()?;
        self.chat_contact(did)?;
        let pair = pair_id(me.id.did(), did);
        let day = core.store.mi(&pair, id)?.ok_or(Error::NotFound)?;
        self.chat_shard(&core, &me, &pair, &day).await?;
        let (rec, conv) = {
            let mut shards = core.shards.lock();
            let shard = shards.get_mut(&(pair.clone(), day.clone())).ok_or(Error::NotFound)?;
            let cur = shard.messages().map_err(|e| Error::Io(e.to_string()))?.remove(id).ok_or(Error::NotFound)?;
            if cur.author != me.id.did() {
                return Err(Error::Protocol("only the author can change a message".into()));
            }
            let update = match &text {
                Some(t) => {
                    if t.len() > MAX_TEXT {
                        return Err(Error::Protocol("message too long".into()));
                    }
                    shard.edit(id, t, now_ms())
                }
                None => shard.delete(id, now_ms()),
            }
            .map_err(|e| Error::Io(e.to_string()))?;
            let rec = shard.messages().map_err(|e| Error::Io(e.to_string()))?.remove(id).ok_or(Error::NotFound)?;
            let ops = vec![core.store.out_op(&pair, &day, true)?];
            let (cops, conv) = self.chat_conv_ops(&core, &me, did, &pair, &[(&rec, false)], 0)?;
            let mut ops = ops;
            ops.extend(cops);
            self.chat_persist(&core, &pair, shard, &update, ops)?;
            (rec, conv)
        };
        let m = self.chat_api_message(&core, &me, did, &pair, &day, &rec);
        self.chat_after_local_write(&core, did, &day);
        if let Some(ev) = self.ev() {
            ev.on_message_changed(m.clone());
            if let Some(c) = self.chat_api_chat(&core, &me, &conv) {
                ev.on_chat_changed(c);
            }
        }
        Ok(m)
    }

    // ---- reading -----------------------------------------------------------------------------------

    fn chat_attachment(&self, core: &ChatCore, f: &FileRef, outgoing: bool) -> Attachment {
        let info = core.store.blob(&f.hash).ok().flatten();
        let dl = core.downloading.lock().get(&f.hash).copied();
        let (state, transferred) = match (&info, dl, outgoing) {
            (_, _, true) => (TransferState::Ready, 0),
            (_, Some((done, _)), _) => (TransferState::Downloading, done),
            (Some(i), _, _) if i.ready => (TransferState::Ready, 0),
            (Some(i), _, _) if i.failed => (TransferState::Failed, 0),
            _ => (TransferState::Remote, 0),
        };
        Attachment {
            hash: f.hash.clone(),
            name: f.name.clone(),
            size: f.size,
            mime: f.mime.clone(),
            kind: if f.kind == "voice" { AttachmentKind::Voice } else { AttachmentKind::File },
            duration_ms: f.duration_ms,
            waveform: f.waveform.clone(),
            state,
            transferred,
        }
    }

    pub(crate) fn chat_api_message(&self, core: &ChatCore, me: &Me, did: &str, pair: &str, day: &str, rec: &MsgRec) -> Message {
        let outgoing = rec.author == me.id.did();
        let delivery = if outgoing {
            let my_peer = peer_id(&me.device, &doc_name(pair, day));
            let ack = core.store.ack(pair, day).ok().flatten().and_then(|b| decode_vv(&b).ok()).and_then(|v| v.get(&my_peer).copied()).unwrap_or(0);
            match core.store.mc(pair, day, &rec.id).ok().flatten() {
                Some(c) if c > ack => DeliveryState::Pending,
                _ => DeliveryState::Delivered,
            }
        } else {
            DeliveryState::Delivered
        };
        Message {
            id: rec.id.clone(),
            peer_did: did.to_string(),
            author_did: rec.author.clone(),
            outgoing,
            at: rec.at.max(0) as u64,
            text: rec.text.clone(),
            edited_at: rec.edited_at.map(|t| t.max(0) as u64),
            deleted: rec.deleted,
            reply_to: rec.reply_to.clone(),
            attachment: rec.file.as_ref().map(|f| self.chat_attachment(core, f, outgoing)),
            delivery,
        }
    }

    pub(crate) fn chat_api_chat(&self, core: &ChatCore, me: &Me, conv: &ConvMeta) -> Option<Chat> {
        let (name, ok) = {
            let s = self.shared.lock();
            let c = s.state.contacts.iter().find(|c| c.did == conv.peer_did)?;
            (crate::node::display_name(c), true)
        };
        let _ = ok;
        let pair = pair_id(me.id.did(), &conv.peer_did);
        let (last_outgoing, last_delivery, last_activity, preview) = match &conv.last {
            Some(l) => {
                let delivery = match core.store.mi(&pair, &l.id).ok().flatten() {
                    Some(day) if l.outgoing => {
                        let my_peer = peer_id(&me.device, &doc_name(&pair, &day));
                        let ack = core.store.ack(&pair, &day).ok().flatten().and_then(|b| decode_vv(&b).ok()).and_then(|v| v.get(&my_peer).copied()).unwrap_or(0);
                        match core.store.mc(&pair, &day, &l.id).ok().flatten() {
                            Some(c) if c > ack => DeliveryState::Pending,
                            _ => DeliveryState::Delivered,
                        }
                    }
                    _ => DeliveryState::Delivered,
                };
                (l.outgoing, delivery, l.at.max(0) as u64, l.preview.clone())
            }
            None => (false, DeliveryState::Delivered, 0, String::new()),
        };
        Some(Chat { peer_did: conv.peer_did.clone(), peer_name: name, preview, last_outgoing, last_delivery, last_activity, unread: conv.unread })
    }

    pub(crate) fn chat_list(&self) -> Result<Vec<Chat>, Error> {
        let me = self.me()?;
        let core = self.chat_core()?;
        let dids: Vec<String> = self.shared.lock().state.contacts.iter().map(|c| c.did.clone()).collect();
        let mut out = Vec::new();
        for did in dids {
            let pair = pair_id(me.id.did(), &did);
            let conv = core.store.conv(&pair)?.unwrap_or_else(|| ConvMeta { peer_did: did.clone(), ..Default::default() });
            if let Some(c) = self.chat_api_chat(&core, &me, &conv) {
                out.push(c);
            }
        }
        out.sort_by(|a, b| b.last_activity.cmp(&a.last_activity).then_with(|| a.peer_name.cmp(&b.peer_name)));
        Ok(out)
    }

    pub(crate) async fn chat_day(self: &Arc<Self>, did: &str, day: Option<String>) -> Result<DayPage, Error> {
        let me = self.me()?;
        let core = self.chat_core()?;
        self.chat_contact(did)?;
        let pair = pair_id(me.id.did(), did);
        let days = core.store.shard_days(&pair)?;
        let mut with_msgs = Vec::new();
        for d in &days {
            if core.store.shard_meta(&pair, d)?.is_some_and(|m| m.n_messages > 0) {
                with_msgs.push(d.clone());
            }
        }
        let day = match day {
            Some(d) => {
                if !day_valid(&d) {
                    return Err(Error::Protocol("bad day".into()));
                }
                d
            }
            None => match with_msgs.first() {
                Some(d) => d.clone(),
                None => return Ok(DayPage { day: day_of(now_ms()), messages: vec![], older_day: None }),
            },
        };
        let older_day = with_msgs.iter().find(|d| **d < day).cloned();
        if core.store.shard_meta(&pair, &day)?.is_none() {
            return Ok(DayPage { day, messages: vec![], older_day });
        }
        self.chat_shard(&core, &me, &pair, &day).await?;
        let recs = {
            let shards = core.shards.lock();
            shards.get(&(pair.clone(), day.clone())).map(|s| s.messages()).transpose().map_err(|e| Error::Io(e.to_string()))?.unwrap_or_default()
        };
        let mut recs: Vec<MsgRec> = recs.into_values().collect();
        recs.sort_by(|a, b| a.order().cmp(&b.order()));
        let messages = recs.iter().map(|r| self.chat_api_message(&core, &me, did, &pair, &day, r)).collect();
        Ok(DayPage { day, messages, older_day })
    }

    pub(crate) fn chat_mark_read(&self, did: &str) -> Result<(), Error> {
        let me = self.me()?;
        let core = self.chat_core()?;
        let pair = pair_id(me.id.did(), did);
        if let Some(mut conv) = core.store.conv(&pair)?
            && conv.unread > 0
        {
            conv.unread = 0;
            core.store.db.apply(&[core.store.conv_op(&pair, &conv)?]).map_err(io)?;
            if let (Some(ev), Some(c)) = (self.ev(), self.chat_api_chat(&core, &me, &conv)) {
                ev.on_chat_changed(c);
            }
        }
        Ok(())
    }

    // ---- blobs -----------------------------------------------------------------------------------

    pub(crate) async fn chat_hub(self: &Arc<Self>) -> Result<Arc<BlobHub>, Error> {
        let core = self.chat_core()?;
        let this = Arc::downgrade(self);
        core.hub
            .get_or_try_init(|| async {
                let w1 = this.clone();
                let gate: Gate = Arc::new(move |dev, hash| w1.upgrade().is_some_and(|i| i.chat_may_serve(dev, hash)));
                let w2 = this.clone();
                let progress: UploadProgress = Arc::new(move |dev, hash, done, total| {
                    if let Some(i) = w2.upgrade() {
                        i.chat_upload_progress(dev, hash, done, total);
                    }
                });
                BlobHub::open(&core.dir.join("blobs"), gate, progress).await.map(Arc::new)
            })
            .await
            .cloned()
    }

    fn did_of_device(&self, dev: &[u8; 32]) -> Option<String> {
        if let Some(c) = self.shared.lock().state.contacts.iter().find(|c| c.devices.contains(dev)) {
            return Some(c.did.clone());
        }
        let core = self.chat.lock().clone()?;
        let map = core.sessions.lock();
        map.values().find(|s| s.device == *dev).map(|s| s.did.clone())
    }

    /// May `dev` read the blob? Only a contact in good standing, and only if a message of our
    /// conversation with them names the hash.
    fn chat_may_serve(&self, dev: &[u8; 32], hash: &Hash) -> bool {
        let (Ok(me), Ok(core)) = (self.me(), self.chat_core()) else { return false };
        let Some(did) = self.did_of_device(dev) else { return false };
        {
            let s = self.shared.lock();
            if s.state.blocked.contains(&did) || !s.state.contacts.iter().any(|c| c.did == did) {
                return false;
            }
        }
        core.store.may_read(&hash.to_string(), &pair_id(me.id.did(), &did))
    }

    fn chat_upload_progress(&self, dev: &[u8; 32], hash: &Hash, done: u64, total: u64) {
        if let (Some(ev), Some(did)) = (self.ev(), self.did_of_device(dev)) {
            ev.on_transfer_progress(did, hash.to_string(), done, total, true);
        }
    }

    /// Starts fetching a blob from `did` in the background (idempotent).
    pub(crate) fn chat_download(self: &Arc<Self>, did: String, hash: String) {
        let Ok(core) = self.chat_core() else { return };
        {
            let mut d = core.downloading.lock();
            if d.contains_key(&hash) {
                return;
            }
            d.insert(hash.clone(), (0, 0));
        }
        let this = self.clone();
        self.handle.spawn(async move {
            let res = this.chat_fetch_blob(&core, &did, &hash).await;
            core.downloading.lock().remove(&hash);
            let ok = res.is_ok();
            if let Err(e) = &res {
                this.log(format!("blob download {hash}: {e}"));
            }
            if let Ok(Some(mut info)) = core.store.blob(&hash) {
                info.ready = ok;
                info.failed = !ok;
                if let Ok(op) = core.store.put_blob_op(&hash, &info) {
                    let _ = core.store.db.apply(&[op]);
                }
                this.chat_emit_changed(&core, &did, &info.day, &info.msg);
            }
        });
    }

    fn chat_emit_changed(self: &Arc<Self>, core: &Arc<ChatCore>, did: &str, day: &str, id: &str) {
        let Ok(me) = self.me() else { return };
        let pair = pair_id(me.id.did(), did);
        let rec = core.shards.lock().get(&(pair.clone(), day.to_string())).and_then(|s| s.messages().ok()).and_then(|mut m| m.remove(id));
        let this = self.clone();
        let (core, did, day, id) = (core.clone(), did.to_string(), day.to_string(), id.to_string());
        self.handle.spawn(async move {
            let rec = match rec {
                Some(r) => r,
                None => {
                    let Ok(me) = this.me() else { return };
                    if this.chat_shard(&core, &me, &pair, &day).await.is_err() {
                        return;
                    }
                    let shards = core.shards.lock();
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

    async fn chat_blob_conn(self: &Arc<Self>, core: &Arc<ChatCore>, did: &str) -> Result<Connection, Error> {
        let ep = self.endpoint()?;
        let contact = self.shared.lock().state.contacts.iter().find(|c| c.did == did).cloned().ok_or(Error::NotFound)?;
        let mut devices: Vec<[u8; 32]> = Vec::new();
        if let Some(s) = self.chat_session(core, did) {
            devices.push(s.device);
        }
        for d in &contact.devices {
            if !devices.contains(d) {
                devices.push(*d);
            }
        }
        let mut last = Error::NotFound;
        for d in devices {
            let addr = addr_for(&d, contact.relay.as_deref())?;
            match tokio::time::timeout(DIAL_TIMEOUT, ep.connect(addr, iroh_blobs::ALPN)).await {
                Ok(Ok(c)) => return Ok(c),
                Ok(Err(e)) => last = Error::net(e),
                Err(_) => last = Error::Timeout,
            }
        }
        Err(last)
    }

    async fn chat_fetch_blob(self: &Arc<Self>, core: &Arc<ChatCore>, did: &str, hash_hex: &str) -> Result<(), Error> {
        let info = core.store.blob(hash_hex)?.ok_or(Error::NotFound)?;
        let hash = parse_hash(hash_hex)?;
        let hub = self.chat_hub().await?;
        let total = crypt::cipher_len(info.size);
        if hub.complete_size(&hash).await == Some(total) {
            return Ok(());
        }
        let conn = self.chat_blob_conn(core, did).await?;
        let (c2, did2, h2) = (core.clone(), did.to_string(), hash_hex.to_string());
        let this = self.clone();
        let last = Arc::new(Mutex::new(std::time::Instant::now() - Duration::from_secs(1)));
        hub.fetch(conn, hash, total, move |done, tot| {
            c2.downloading.lock().insert(h2.clone(), (done, tot));
            let mut l = last.lock();
            if l.elapsed() > Duration::from_millis(200) || done == tot {
                *l = std::time::Instant::now();
                if let Some(ev) = this.ev() {
                    ev.on_transfer_progress(did2.clone(), h2.clone(), done, tot, false);
                }
            }
        })
        .await
    }

    /// Test hook: tries to fetch a blob from a contact's device as this node, whatever the
    /// conversations say. Fails when the contact's gate refuses us.
    pub(crate) async fn chat_raw_fetch(self: &Arc<Self>, did: &str, hash_hex: &str, size: u64) -> Result<u64, Error> {
        let core = self.chat_core()?;
        let hash = parse_hash(hash_hex)?;
        let hub = self.chat_hub().await?;
        let conn = self.chat_blob_conn(&core, did).await?;
        hub.fetch(conn, hash, crypt::cipher_len(size), |_, _| {}).await?;
        Ok(size)
    }

    /// After (re)connecting: fetch what we wanted but could not get before.
    fn chat_resume_downloads(self: &Arc<Self>, core: &Arc<ChatCore>, did: &str) {
        let Ok(me) = self.me() else { return };
        let pair = pair_id(me.id.did(), did);
        for (hash, info) in core.store.wanted_blobs().unwrap_or_default() {
            if info.pair == pair {
                self.chat_download(did.to_string(), hash);
            }
        }
    }

    /// User asked for the attachment of a message.
    pub(crate) fn chat_want(self: &Arc<Self>, did: &str, id: &str) -> Result<(), Error> {
        let me = self.me()?;
        let core = self.chat_core()?;
        let pair = pair_id(me.id.did(), did);
        let day = core.store.mi(&pair, id)?.ok_or(Error::NotFound)?;
        let hash = {
            let shards = core.shards.lock();
            shards.get(&(pair.clone(), day.clone())).and_then(|s| s.messages().ok()).and_then(|mut m| m.remove(id)).and_then(|r| r.file).map(|f| f.hash)
        };
        // Not cached: look the hash up through the blob records of this message.
        let hash = match hash {
            Some(h) => h,
            None => core
                .store
                .conv_blobs(&pair)?
                .into_iter()
                .find(|h| core.store.blob(h).ok().flatten().is_some_and(|b| b.msg == id))
                .ok_or(Error::NotFound)?,
        };
        let mut info = core.store.blob(&hash)?.ok_or(Error::NotFound)?;
        if info.ready {
            return Ok(());
        }
        info.wanted = true;
        info.failed = false;
        core.store.db.apply(&[core.store.put_blob_op(&hash, &info)?]).map_err(io)?;
        self.chat_download(did.to_string(), hash);
        self.chat_kick(did.to_string());
        Ok(())
    }

    pub(crate) async fn chat_save(self: &Arc<Self>, did: &str, id: &str, dest: PathBuf) -> Result<(), Error> {
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
    pub(crate) async fn chat_send_file(
        self: &Arc<Self>,
        did: &str,
        path: PathBuf,
        mime: String,
        text: Option<String>,
        voice: Option<(u32, Vec<u8>)>,
    ) -> Result<Message, Error> {
        self.chat_contact(did)?;
        let meta = std::fs::metadata(&path)?;
        if !meta.is_file() {
            return Err(Error::Protocol("not a file".into()));
        }
        if meta.len() > crypt::MAX_FILE {
            return Err(Error::Protocol("files are limited to 2 GB".into()));
        }
        let name: String = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
        let name: String = proto::sanitize_name(&name).chars().take(200).collect();
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
        };
        self.chat_send_message(did, text.unwrap_or_default(), None, Some(file)).await
    }

    // ---- history & compaction ------------------------------------------------------------------------

    async fn chat_snapshot_blob(self: &Arc<Self>, core: &Arc<ChatCore>, me: &Arc<Me>, pair: &str, day: &str) -> Result<HistDay, Error> {
        let meta = core.store.shard_meta(pair, day)?.ok_or(Error::NotFound)?;
        let hub = self.chat_hub().await?;
        if let Some(c) = &meta.closed
            && core.store.shard_rows(pair, day)?.1.is_empty()
            && core.store.shard_rows(pair, day)?.0.is_none()
        {
            let size = hub.complete_size(&parse_hash(&c.hash)?).await.unwrap_or(0);
            return Ok(HistDay { day: day.into(), vv: meta.vv, hash: c.hash.clone(), key: c.key.clone(), size });
        }
        if let Some(s) = core.store.snapcache(pair, day)?
            && s.vv == meta.vv
        {
            return Ok(HistDay { day: day.into(), vv: s.vv, hash: s.hash, key: s.key, size: s.size });
        }
        self.chat_shard(core, me, pair, day).await?;
        let snap = core.shards.lock().get(&(pair.to_string(), day.to_string())).map(|s| s.snapshot()).ok_or(Error::NotFound)?;
        let key = crypt::new_key();
        let hash = hub.add_bytes(key, snap).await?;
        let size = hub.complete_size(&hash).await.unwrap_or(0);
        let hex_hash = hash.to_string();
        let sref = SnapRef { vv: meta.vv.clone(), hash: hex_hash.clone(), key: hex::encode(key), size };
        let mut ops = core.store.ref_ops(&hex_hash, pair)?;
        ops.push(core.store.put_snapcache_op(pair, day, &sref)?);
        let old = core.store.snapcache(pair, day)?;
        core.store.db.apply(&ops).map_err(io)?;
        if let Some(o) = old
            && let Ok(h) = parse_hash(&o.hash)
        {
            let _ = hub.release(&h).await;
        }
        Ok(HistDay { day: day.into(), vv: meta.vv, hash: sref.hash, key: sref.key, size })
    }

    async fn chat_history_days(self: &Arc<Self>, core: &Arc<ChatCore>, me: &Arc<Me>, pair: &str, before: &str, limit: u32) -> Result<Vec<HistDay>, Error> {
        let mut out = Vec::new();
        for d in core.store.shard_days(pair)? {
            if d.as_str() >= before {
                continue;
            }
            if core.store.shard_meta(pair, &d)?.is_none_or(|m| m.n_messages == 0) {
                continue;
            }
            out.push(self.chat_snapshot_blob(core, me, pair, &d).await?);
            if out.len() as u32 >= limit {
                break;
            }
        }
        Ok(out)
    }

    /// Asks the peer for days older than `before` that we do not hold; returns how many arrived.
    pub(crate) async fn chat_fetch_history(self: &Arc<Self>, did: &str, before: String) -> Result<u32, Error> {
        let me = self.me()?;
        let core = self.chat_core()?;
        self.chat_contact(did)?;
        if !day_valid(&before) {
            return Err(Error::Protocol("bad day".into()));
        }
        let ep_ready = self.endpoint().is_ok();
        if !ep_ready {
            return Err(Error::NotStarted);
        }
        let mut sess = self.chat_session(&core, did);
        if sess.is_none() {
            self.chat_dial(&core, &me, did).await?;
            for _ in 0..50 {
                sess = self.chat_session(&core, did);
                if sess.is_some() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
        let sess = sess.ok_or(Error::Timeout)?;
        let (tx, rx) = oneshot::channel();
        *sess.hist.lock() = Some(tx);
        sess.tx.send(Cmd::History(before, 14)).map_err(|_| Error::Net("session closed".into()))?;
        let days = tokio::time::timeout(Duration::from_secs(30), rx).await.map_err(|_| Error::Timeout)?.map_err(|_| Error::Net("session closed".into()))?;
        let pair = pair_id(me.id.did(), did);
        let mut added = 0;
        for d in days.into_iter().take(14) {
            if !day_valid(&d.day) || core.store.shard_meta(&pair, &d.day)?.is_some() {
                continue;
            }
            if self.chat_import_day(&core, &me, &sess, &pair, d).await? {
                added += 1;
            }
        }
        Ok(added)
    }

    /// A peer-vouched snapshot of a day (see `Ctx::history`): validated like a batch, merged
    /// into whatever we hold of that day, stored as the new snapshot. The shard must be loaded.
    fn chat_apply_history(&self, core: &ChatCore, me: &Me, sess: &Session, pair: &str, day: &str, snap: &[u8]) -> Result<bool, Error> {
        let mut shards = core.shards.lock();
        let shard = shards.get_mut(&(pair.to_string(), day.to_string())).ok_or(Error::NotFound)?;
        let before = shard.vv();
        let ctx = Ctx { signer_did: sess.did.clone(), signer_peer: 0, now_ms: now_ms(), live: false, history: Some(me.id.did().to_string()) };
        shard.apply_remote(snap, &ctx).map_err(|r| Error::Protocol(format!("rejected history: {r}")))?;
        if shard.vv() == before {
            return Ok(false);
        }
        let msgs = shard.messages().map_err(|e| Error::Io(e.to_string()))?;
        let mut ops = Vec::new();
        for rec in msgs.values() {
            ops.push(core.store.mi_op(pair, &rec.id, day)?);
            let (o, _) = self.chat_register_file(core, pair, day, rec, rec.author != me.id.did())?;
            ops.extend(o);
        }
        let meta = ShardMeta { vv: shard.vv().encode(), next_seq: 0, n_updates: 0, n_messages: msgs.len() as u32, closed: None };
        ops.extend(core.store.put_snap_ops(pair, day, &shard.snapshot())?);
        ops.push(core.store.put_meta_op(pair, day, &meta)?);
        // The last-message row may need to move.
        let recs: Vec<(&MsgRec, bool)> = msgs.values().map(|r| (r, false)).collect();
        let (cops, _) = self.chat_conv_ops(core, me, &sess.did, pair, &recs, 0)?;
        ops.extend(cops);
        core.store.db.apply(&ops).map_err(io)?;
        Ok(true)
    }

    /// Pulls one day from the peer as a vouched snapshot (in the background: the session task
    /// must stay free to read the answer).
    fn chat_pull_day(self: &Arc<Self>, core: &Arc<ChatCore>, me: &Arc<Me>, sess: &Arc<Session>, day: &str) {
        let key = (pair_id(me.id.did(), &sess.did), day.to_string());
        if !core.pulling.lock().insert(key.clone()) {
            return;
        }
        let (this, core, me, sess) = (self.clone(), core.clone(), me.clone(), sess.clone());
        self.handle.spawn(async move {
            let res: Result<(), Error> = async {
                let upto = day_of(day_start(&key.1).ok_or(Error::NotFound)? + DAY_MS + HOUR_MS);
                let (tx, rx) = oneshot::channel();
                *sess.hist.lock() = Some(tx);
                sess.tx.send(Cmd::History(upto, 1)).map_err(|_| Error::Net("session closed".into()))?;
                let days = tokio::time::timeout(Duration::from_secs(30), rx).await.map_err(|_| Error::Timeout)?.map_err(|_| Error::Net("session closed".into()))?;
                let Some(d) = days.into_iter().find(|d| d.day == key.1) else { return Ok(()) };
                this.chat_import_day(&core, &me, &sess, &key.0, d).await?;
                let _ = sess.tx.send(Cmd::Ack(key.1.clone()));
                Ok(())
            }
            .await;
            if let Err(e) = res {
                this.log(format!("pulling {} from {}: {e}", key.1, sess.did));
            }
            core.pulling.lock().remove(&key);
        });
    }

    /// Fetches the snapshot blob `d` names from the peer and merges it (`chat_apply_history`).
    async fn chat_import_day(self: &Arc<Self>, core: &Arc<ChatCore>, me: &Arc<Me>, sess: &Arc<Session>, pair: &str, d: HistDay) -> Result<bool, Error> {
        if !day_valid(&d.day) || d.size == 0 || d.size > 64 * 1024 * 1024 {
            return Ok(false);
        }
        let (hash, key) = (parse_hash(&d.hash)?, parse_key(&d.key)?);
        let hub = self.chat_hub().await?;
        let conn = self.chat_blob_conn(core, &sess.did).await?;
        hub.fetch(conn, hash, d.size, |_, _| {}).await?;
        let ct = hub.read_cipher(&hash).await?;
        let snap = tokio::task::spawn_blocking(move || crypt::decrypt_bytes(&key, &ct))
            .await
            .map_err(|e| Error::Io(e.to_string()))?
            .map_err(|e| Error::Io(e.to_string()))?;
        let _ = hub.release(&hash).await;
        self.chat_shard(core, me, pair, &d.day).await?;
        self.chat_apply_history(core, me, sess, pair, &d.day, &snap)
    }

    /// Days at least `CLOSE_AFTER_DAYS` old become one encrypted snapshot blob each.
    async fn chat_compact(self: &Arc<Self>) -> Result<(), Error> {
        let me = self.me()?;
        let core = self.chat_core()?;
        let limit = day_of(now_ms() - CLOSE_AFTER_DAYS * DAY_MS);
        let dids: Vec<String> = self.shared.lock().state.contacts.iter().map(|c| c.did.clone()).collect();
        for did in dids {
            let pair = pair_id(me.id.did(), &did);
            for day in core.store.shard_days(&pair)? {
                if day > limit {
                    continue;
                }
                let Some(meta) = core.store.shard_meta(&pair, &day)? else { continue };
                if meta.closed.is_some() && meta.n_updates == 0 && core.store.shard_rows(&pair, &day)?.0.is_none() {
                    continue;
                }
                if core.store.pending_days(&pair)?.contains(&day) {
                    continue;
                }
                self.chat_shard(&core, &me, &pair, &day).await?;
                let snap = core.shards.lock().get(&(pair.clone(), day.clone())).map(|s| s.snapshot()).ok_or(Error::NotFound)?;
                let hub = self.chat_hub().await?;
                let key = crypt::new_key();
                let hash = hub.add_bytes(key, snap).await?;
                let hex_hash = hash.to_string();
                let mut ops = core.store.ref_ops(&hex_hash, &pair)?;
                ops.extend(core.store.drop_rows_ops(&pair, &day));
                let mut m = meta.clone();
                m.n_updates = 0;
                m.next_seq = 0;
                m.closed = Some(ClosedRef { hash: hex_hash, key: hex::encode(key) });
                ops.push(core.store.put_meta_op(&pair, &day, &m)?);
                core.store.db.apply(&ops).map_err(io)?;
                if let Some(old) = meta.closed
                    && let Ok(h) = parse_hash(&old.hash)
                {
                    let _ = hub.release(&h).await;
                }
                core.shards.lock().remove(&(pair.clone(), day.clone()));
            }
        }
        Ok(())
    }

    // ---- contact removal ---------------------------------------------------------------------------------

    /// The conversation and its blobs go away on this device.
    pub(crate) fn chat_purge(self: &Arc<Self>, did: &str) {
        let Ok(core) = self.chat_core() else { return };
        let Ok(me) = self.me() else { return };
        let pair = pair_id(me.id.did(), did);
        if let Some(s) = core.sessions.lock().remove(did) {
            let _ = s.tx.send(Cmd::Close);
        }
        core.shards.lock().retain(|(p, _), _| *p != pair);
        let hashes = core.store.conv_blobs(&pair).unwrap_or_default();
        if let Err(e) = core.store.delete_conversation(&pair, &hashes) {
            tracing::warn!("deleting conversation: {e}");
        }
        let this = self.clone();
        self.handle.spawn(async move {
            if let Ok(hub) = this.chat_hub().await {
                for h in hashes {
                    if let Ok(h) = parse_hash(&h) {
                        let _ = hub.release(&h).await;
                    }
                }
            }
        });
    }
}

pub(crate) fn merge_vv(into: &mut VersionVector, other: &VersionVector) {
    for (p, c) in other.iter() {
        let cur = into.get(p).copied().unwrap_or(0);
        if *c > cur {
            into.insert(*p, *c);
        }
    }
}

