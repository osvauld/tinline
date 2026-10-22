//! Image blobs of posts (`docs/design/public-group.md` §5) on the group endpoint.
//!
//! - **Store:** one `transfer::BlobHub` per account runtime (`group-blobs/` next to the group
//!   vault), apart from chat's. Blobs are sealed with the chunked blob cipher under a fresh key per
//!   image, as 1:1 attachments are; the key travels in the signed post (`ImageRef`), so members
//!   and hosts can open them and the store holds only ciphertext. Each group's claim is its own
//!   owner tag (`group/<gid>`), so one group releasing a hash leaves another's alone.
//! - **Transport:** iroh-blobs (`iroh_blobs::ALPN`) as a second protocol on the group endpoint
//!   (U2/U7: nobody learns a member's chat endpoint, and members still dial only hosts). Every
//!   request is gated per (requesting endpoint, hash): served only if a visible post of a group we
//!   hold names the hash (`img/` row, not dropped) and the requester, in that group, is a
//!   current member on a live session with us (we host it) or an active host (a session we
//!   dialed, or a host address we hold). The hash is the ciphertext's BLAKE3, verified while
//!   streaming by iroh-blobs; the fetch also stops at the size the post declared.
//! - **Who fetches what:** a host fetches the image of every visible post (from the author's
//!   sessions first, then other hosts); a member fetches from hosts only, when the auto-download
//!   setting allows (always / Wi-Fi only / on tap, `PictureDownload`, default Wi-Fi only; the app
//!   reports a metered connection with `set_connection_metered`) or the user asked
//!   (`group_fetch_image`). Failed fetches are retried (2 s ... 60 s), then given up until a new
//!   session in the group or a tap.
//! - **Host limits:** a host refuses (never fetches, never serves) an image whose post declares
//!   more than 1 MB, and keeps at most 2 GB of image blobs per group: over that, the blobs no
//!   visible post references go first, then the oldest (`group_core::store::plan_eviction`).
//!   Both limits have test knobs.

use std::collections::BTreeSet;
use std::path::PathBuf;

use iroh_blobs::Hash;
use transfer::{BlobHub, Gate, UploadEnd, UploadProgress, crypt};

use super::*;
use chat_core::store::PictureDownload;
use crate::record::{ImageRef, MAX_IMAGE_BYTES};
use crate::store::{ImgRow, plan_eviction};

/// A host keeps at most this many bytes of image blobs per group.
pub const MAX_GROUP_IMAGE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Waits between fetch attempts of one image; after the last, it is given up (`failed`).
const RETRY_S: [u64; 5] = [2, 5, 15, 30, 60];
/// Image fetches running at once per account.
const MAX_FETCHES: usize = 4;

/// The image runtime of one account (`GroupCore::images`).
pub struct ImagesRt {
    /// The account's directory: blobs in `group-blobs/`, opened pictures in `group-images/`.
    dir: PathBuf,
    hub: tokio::sync::OnceCell<Arc<BlobHub>>,
    /// Images to fetch (not ready, not dropped, not given up), by `(group, hash)`.
    pending: Mutex<HashMap<(String, [u8; 32]), Pend>>,
    /// `pending` was filled from the store once.
    loaded: AtomicBool,
    /// `(largest blob a host takes, bytes a host keeps per group)`.
    pub limits: Mutex<(u64, u64)>,
    /// One budget pass at a time.
    budget: tokio::sync::Mutex<()>,
}

impl Default for ImagesRt {
    fn default() -> Self {
        Self::new(PathBuf::new())
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Pend {
    wanted: bool,
    busy: bool,
    tries: usize,
    next: Option<Instant>,
}

impl ImagesRt {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            hub: Default::default(),
            pending: Default::default(),
            loaded: AtomicBool::new(false),
            limits: Mutex::new((MAX_IMAGE_BYTES, MAX_GROUP_IMAGE_BYTES)),
            budget: Default::default(),
        }
    }

    pub fn hub_now(&self) -> Option<Arc<BlobHub>> {
        self.hub.get().cloned()
    }

    fn opened(&self, hash: &[u8; 32], mime: &str) -> PathBuf {
        let ext = match mime {
            "image/png" => "png",
            "image/webp" => "webp",
            _ => "jpg",
        };
        self.dir
            .join("group-images")
            .join(format!("{}.{ext}", hex::encode(hash)))
    }
}

/// The blob store tag of `gid`'s claim on its images.
fn owner_of(gid: &str) -> String {
    format!("group/{gid}")
}

/// What the UI shows of a post's image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImgState {
    Ready,
    Loading,
    /// Not here and not on its way (tap to load).
    Remote,
    Failed,
}

impl GroupCore {
    /// The serving gate (see the module doc): may the group endpoint `ep` read `hash`?
    pub fn image_may_serve(&self, ep: &[u8; 32], hash: &[u8; 32]) -> bool {
        if self.closed() {
            return false;
        }
        let gids: Vec<String> = self.groups.lock().keys().cloned().collect();
        gids.iter().any(|gid| {
            self.store
                .image(gid, hash)
                .ok()
                .flatten()
                .is_some_and(|r| !r.dropped)
                && self.image_peer_allowed(gid, ep)
        })
    }

    /// `ep` is, in `gid`, a current member on a session we host or an active host.
    fn image_peer_allowed(&self, gid: &str, ep: &[u8; 32]) -> bool {
        let sessions: Vec<Arc<Sess>> = self
            .sessions_of(gid)
            .into_iter()
            .filter(|s| &s.ep == ep)
            .collect();
        let addrs: Vec<HostAddr> = self
            .store
            .hosts(gid)
            .unwrap_or_default()
            .iter()
            .filter_map(|s| s.open().ok())
            .filter(|a| &a.endpoint == ep)
            .collect();
        let groups = self.groups.lock();
        let Some(live) = groups.get(gid) else {
            return false;
        };
        let st = &live.state;
        let by_session = sessions.iter().any(|s| {
            if s.hosting {
                // Membership now (a session opened while joining counts once the join does).
                st.is_member(&s.did) && !st.is_removed(&s.did)
            } else {
                st.is_host(&s.did, &s.dev)
            }
        });
        by_session || addrs.iter().any(|a| st.is_host(&a.did, &a.dev))
    }

    /// `ep` may hold a blob connection with us at all (some group would let it read).
    fn image_peer_known(&self, ep: &[u8; 32]) -> bool {
        let gids: Vec<String> = self.groups.lock().keys().cloned().collect();
        gids.iter().any(|g| self.image_peer_allowed(g, ep))
    }

    /// Auto-download allows fetching now (this device as a member).
    fn image_auto(&self) -> bool {
        match self.store.image_download() {
            0 => true,
            1 => !chat_core::store::metered(),
            _ => false,
        }
    }

    pub fn image_download(&self) -> PictureDownload {
        match self.store.image_download() {
            0 => PictureDownload::Always,
            1 => PictureDownload::Wifi,
            _ => PictureDownload::Tap,
        }
    }

    pub fn set_image_download(&self, p: PictureDownload) -> Result<(), Error> {
        Ok(self.store.set_image_download(match p {
            PictureDownload::Always => 0,
            PictureDownload::Wifi => 1,
            PictureDownload::Tap => 2,
        })?)
    }

    /// What the UI shows of the image `img` of a post in `gid`.
    pub fn image_state(&self, gid: &str, img: &ImageRef) -> ImgState {
        let Some(row) = self.store.image(gid, &img.hash).ok().flatten() else {
            return ImgState::Remote;
        };
        if row.ready {
            return ImgState::Ready;
        }
        if row.failed || row.dropped {
            return ImgState::Failed;
        }
        let pend = self
            .images
            .pending
            .lock()
            .get(&(gid.to_string(), img.hash))
            .copied();
        match pend {
            Some(p) if p.busy || p.wanted || self.am_host(gid) || self.image_auto() => {
                ImgState::Loading
            }
            _ => ImgState::Remote,
        }
    }

    /// Fills `pending` from the store (once per runtime).
    fn images_load(&self) {
        if self.images.loaded.swap(true, Ordering::SeqCst) {
            return;
        }
        let gids: Vec<String> = self.groups.lock().keys().cloned().collect();
        let mut p = self.images.pending.lock();
        for gid in gids {
            for r in self.store.images(&gid).unwrap_or_default() {
                if !r.ready && !r.dropped && !r.failed {
                    p.entry((gid.clone(), r.image.hash)).or_insert(Pend {
                        wanted: r.wanted,
                        ..Default::default()
                    });
                }
            }
        }
    }
}

impl GroupRt {
    /// The account's group blob store, opened on first use.
    pub async fn group_hub(
        self: &Arc<Self>,
        core: &Arc<GroupCore>,
    ) -> Result<Arc<BlobHub>, Error> {
        if core.closed() {
            return Err(Error::Locked);
        }
        let weak = Arc::downgrade(core);
        let dir = core.images.dir.join("group-blobs");
        core.images
            .hub
            .get_or_try_init(|| async move {
                let gate: Gate = Arc::new(move |ep, hash| {
                    weak.upgrade()
                        .is_some_and(|c| c.image_may_serve(ep, hash.as_bytes()))
                });
                let progress: UploadProgress = Arc::new(|_, _, _, _| {});
                let ended: UploadEnd = Arc::new(|_, _, _| {});
                Ok::<_, Error>(Arc::new(BlobHub::open(&dir, gate, progress, ended).await?))
            })
            .await
            .cloned()
    }

    /// An iroh-blobs connection that arrived on the group endpoint.
    pub(super) async fn group_blob_incoming(
        self: &Arc<Self>,
        core: Arc<GroupCore>,
        conn: Connection,
    ) -> Result<(), Error> {
        if !core.image_peer_known(conn.remote_id().as_bytes()) {
            conn.close(0u32.into(), b"not accepted");
            return Ok(());
        }
        let hub = self.group_hub(&core).await?;
        use iroh::protocol::ProtocolHandler;
        hub.protocol()
            .accept(conn)
            .await
            .map_err(|e| Error::Net(e.to_string()))
    }

    /// Posts that show now (`Changed::posts`): note their images and start what should start.
    pub(super) fn group_images_noted(
        self: &Arc<Self>,
        core: &Arc<GroupCore>,
        gid: &str,
        posts: &[PostDiff],
    ) {
        let mut any = false;
        let host = core.am_host(gid);
        let max_blob = core.images.limits.lock().0;
        for i in posts.iter().flat_map(|d| d.put.iter()) {
            let Some(img) = &i.image else { continue };
            match core.store.image(gid, &img.hash) {
                Ok(Some(row)) => {
                    if row.post != i.id {
                        let _ = core.store.index_image_post(gid, &i.id, &img.hash);
                    }
                    continue;
                }
                Ok(None) => {}
                Err(e) => {
                    tracing::warn!("group image row: {e}");
                    continue;
                }
            }
            let refused = host && img.size > max_blob;
            if refused {
                self.log(format!(
                    "group {}: refused an image of {} bytes (limit {max_blob})",
                    crate::short(gid),
                    img.size
                ));
            }
            let row = ImgRow {
                post: i.id.clone(),
                author: i.author.clone(),
                at: i.at,
                image: img.clone(),
                ready: false,
                wanted: false,
                failed: false,
                dropped: refused,
                referenced: true,
            };
            if let Err(e) = core.store.put_image(gid, &row) {
                tracing::warn!("group image row: {e}");
                continue;
            }
            if !refused {
                core.images
                    .pending
                    .lock()
                    .insert((gid.to_string(), img.hash), Pend::default());
                any = true;
            }
        }
        if any {
            self.group_images_pump(core);
        }
    }

    /// A new session in `gid`: images given up on there are tried again, and waiting ones now.
    pub(super) fn group_images_on_session(self: &Arc<Self>, core: &Arc<GroupCore>, gid: &str) {
        core.images_load();
        for mut r in core.store.images(gid).unwrap_or_default() {
            if r.failed && !r.dropped && !r.ready {
                r.failed = false;
                if core.store.put_image(gid, &r).is_ok() {
                    core.images.pending.lock().insert(
                        (gid.to_string(), r.image.hash),
                        Pend {
                            wanted: r.wanted,
                            ..Default::default()
                        },
                    );
                }
            }
        }
        for ((g, _), p) in core.images.pending.lock().iter_mut() {
            if g == gid && !p.busy {
                p.tries = 0;
                p.next = None;
            }
        }
        self.group_images_pump(core);
    }

    /// Starts the fetches that may run now (the dial loop's tick, a new post, a setting).
    pub fn group_images_pump(self: &Arc<Self>, core: &Arc<GroupCore>) {
        if core.closed() {
            return;
        }
        core.images_load();
        let keys: Vec<((String, [u8; 32]), Pend)> = core
            .images
            .pending
            .lock()
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect();
        if keys.is_empty() {
            return;
        }
        let auto = core.image_auto();
        let now = Instant::now();
        let mut running = keys.iter().filter(|(_, p)| p.busy).count();
        let hosts: HashSet<String> = keys
            .iter()
            .map(|((g, _), _)| g.clone())
            .filter(|g| core.am_host(g))
            .collect();
        for ((gid, hash), p) in keys {
            if running >= MAX_FETCHES {
                break;
            }
            if p.busy
                || p.next.is_some_and(|t| t > now)
                || !(p.wanted || auto || hosts.contains(&gid))
            {
                continue;
            }
            {
                let mut pend = core.images.pending.lock();
                match pend.get_mut(&(gid.clone(), hash)) {
                    Some(e) if !e.busy => e.busy = true,
                    _ => continue,
                }
            }
            running += 1;
            let (this, core) = (self.clone(), core.clone());
            self.handle.spawn(async move {
                let res = this.group_image_fetch(&core, &gid, hash).await;
                this.group_image_done(&core, &gid, hash, res);
            });
        }
    }

    /// The outcome of one fetch attempt.
    fn group_image_done(
        self: &Arc<Self>,
        core: &Arc<GroupCore>,
        gid: &str,
        hash: [u8; 32],
        res: Result<(), Error>,
    ) {
        let key = (gid.to_string(), hash);
        let err = match res {
            Ok(()) => {
                core.images.pending.lock().remove(&key);
                return;
            }
            Err(e) => e,
        };
        // A blob larger than its post says can never match (the hash fixes the bytes).
        let hopeless = matches!(err, Error::Protocol(_));
        let give_up = {
            let mut pend = core.images.pending.lock();
            let Some(p) = pend.get_mut(&key) else { return };
            p.busy = false;
            p.tries += 1;
            if hopeless || p.tries > RETRY_S.len() {
                pend.remove(&key);
                true
            } else {
                p.next = Some(Instant::now() + Duration::from_secs(RETRY_S[p.tries - 1]));
                false
            }
        };
        if !give_up {
            return;
        }
        self.log(format!(
            "group {}: image {} given up: {err}",
            crate::short(gid),
            &hex::encode(hash)[..12]
        ));
        if let Ok(Some(mut row)) = core.store.image(gid, &hash) {
            row.failed = true;
            // A host refuses for good what cannot match its post.
            row.dropped |= hopeless && core.am_host(gid);
            core.store.put_image(gid, &row).or_log("saving the failed state of an image");
            self.group_image_event(gid, &row.post);
            let (this, core, gid) = (self.clone(), core.clone(), gid.to_string());
            self.handle.spawn(async move {
                if let Ok(hub) = this.group_hub(&core).await {
                    let _ = hub
                        .release_owned(&owner_of(&gid), &Hash::from_bytes(hash))
                        .await;
                }
            });
        }
    }

    /// One attempt: every source in turn until one has the whole blob.
    async fn group_image_fetch(
        self: &Arc<Self>,
        core: &Arc<GroupCore>,
        gid: &str,
        hash: [u8; 32],
    ) -> Result<(), Error> {
        let row = core.store.image(gid, &hash)?.ok_or(Error::NotFound)?;
        if row.ready || row.dropped {
            return Ok(());
        }
        let hub = self.group_hub(core).await?;
        let h = Hash::from_bytes(hash);
        let total = crypt::cipher_len(row.image.size);
        let owner = owner_of(gid);
        if hub.complete_size(&h).await == Some(total) {
            hub.keep_owned(&owner, &h).await?;
            return self.group_image_ready(core, gid, row).await;
        }
        let ep = self.group_endpoint().ok_or(Error::NotStarted)?;
        let mut err = Error::NotFound;
        for (dev, relay) in self.group_image_sources(core, gid, &row.author) {
            let conn = match addr_for(&dev, relay.as_deref()) {
                Ok(addr) => {
                    match tokio::time::timeout(DIAL_TIMEOUT, ep.connect(addr, iroh_blobs::ALPN))
                        .await
                    {
                        Ok(Ok(c)) => c,
                        Ok(Err(e)) => {
                            err = Error::net(e);
                            continue;
                        }
                        Err(_) => {
                            err = Error::Timeout;
                            continue;
                        }
                    }
                }
                Err(e) => {
                    err = e;
                    continue;
                }
            };
            let res = hub.fetch_owned(&owner, conn.clone(), h, total).await;
            conn.close(0u32.into(), b"done");
            match res {
                Ok(()) => return self.group_image_ready(core, gid, row).await,
                Err(transfer::Error::Protocol(e)) => return Err(Error::Protocol(e)),
                Err(e) => err = e.into(),
            }
        }
        Err(err)
    }

    /// Where to fetch an image of `gid` from: as a host, the author's live sessions with us, then
    /// the other hosts; as a member, the hosts (live sessions first). Never another member (U7).
    fn group_image_sources(
        &self,
        core: &GroupCore,
        gid: &str,
        author: &str,
    ) -> Vec<([u8; 32], Option<String>)> {
        let mut out: Vec<([u8; 32], Option<String>)> = Vec::new();
        let mut add = |d: [u8; 32], r: Option<String>| {
            if !out.iter().any(|(x, _)| *x == d) {
                out.push((d, r));
            }
        };
        let sessions = core.sessions_of(gid);
        for s in sessions.iter().filter(|s| s.hosting && s.did == author) {
            add(s.ep, None);
        }
        let hosts: Vec<HostAddr> = core
            .chat_host_addrs(gid)
            .iter()
            .filter_map(|s| s.open().ok())
            .filter(|a| !(a.did == core.did && a.dev == core.dev))
            .collect();
        for s in sessions.iter().filter(|s| !s.hosting) {
            let relay = hosts
                .iter()
                .find(|a| a.endpoint == s.ep)
                .and_then(|a| a.relay.clone());
            add(s.ep, relay);
        }
        for a in hosts {
            add(a.endpoint, a.relay);
        }
        out
    }

    /// The whole blob is here: mark it, tell the app, and (as a host) keep within the budget.
    async fn group_image_ready(
        self: &Arc<Self>,
        core: &Arc<GroupCore>,
        gid: &str,
        mut row: ImgRow,
    ) -> Result<(), Error> {
        row.ready = true;
        row.failed = false;
        core.store.put_image(gid, &row)?;
        self.group_image_event(gid, &row.post);
        if core.am_host(gid) {
            self.group_images_budget(core, gid).await;
        }
        Ok(())
    }

    fn group_image_event(&self, gid: &str, post: &str) {
        if let Some(ev) = self.group_ev() {
            let (g, p) = (gid.to_string(), post.to_string());
            self.handle.spawn_blocking(move || ev.on_group_image(g, p));
        }
    }

    /// Host: drop image blobs of `gid` until the group's bytes fit the budget, those no visible
    /// post references first, then the oldest. A dropped image is never fetched again here.
    pub async fn group_images_budget(self: &Arc<Self>, core: &Arc<GroupCore>, gid: &str) {
        let _one = core.images.budget.lock().await;
        let budget = core.images.limits.lock().1;
        let rows: Vec<ImgRow> = core
            .store
            .images(gid)
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.ready && !r.dropped)
            .collect();
        let total: u64 = rows.iter().map(|r| crypt::cipher_len(r.image.size)).sum();
        if total <= budget {
            return;
        }
        let shown = self.group_images_shown(core, gid, &rows);
        let held: Vec<([u8; 32], u64, i64, bool)> = rows
            .iter()
            .map(|r| {
                (
                    r.image.hash,
                    crypt::cipher_len(r.image.size),
                    r.at,
                    shown.contains(&r.post),
                )
            })
            .collect();
        let drop: Vec<[u8; 32]> = plan_eviction(&held, budget);
        let Ok(hub) = self.group_hub(core).await else {
            return;
        };
        for hash in drop {
            let Some(mut row) = rows.iter().find(|r| r.image.hash == hash).cloned() else {
                continue;
            };
            let _ = hub
                .release_owned(&owner_of(gid), &Hash::from_bytes(hash))
                .await;
            row.referenced = shown.contains(&row.post);
            row.ready = false;
            row.dropped = true;
            let _ = core.store.put_image(gid, &row);
            let _ = std::fs::remove_file(core.images.opened(&hash, &row.image.mime));
            self.log(format!(
                "group {}: dropped image {} to stay within {budget} bytes",
                crate::short(gid),
                &hex::encode(hash)[..12]
            ));
            self.group_image_event(gid, &row.post);
        }
    }

    /// Which of these rows' posts still show (judged again now, from their day docs).
    fn group_images_shown(&self, core: &GroupCore, gid: &str, rows: &[ImgRow]) -> HashSet<String> {
        let days: BTreeSet<String> = core
            .store
            .days(gid)
            .unwrap_or_default()
            .into_iter()
            .collect();
        let mut want: BTreeSet<String> = BTreeSet::new();
        for r in rows {
            // A post sits in the day doc of its time or a neighbouring one (an hour of tolerance).
            for at in [r.at - crdt::HOUR_MS, r.at, r.at + crdt::HOUR_MS] {
                let d = day_of(at);
                if days.contains(&d) {
                    want.insert(d);
                }
            }
        }
        let mut shown = HashSet::new();
        let mut groups = core.groups.lock();
        let Some(live) = groups.get_mut(gid) else {
            return shown;
        };
        let Live { state, shards, .. } = live;
        for d in want {
            if let Ok(shard) = shards.get(&core.store, gid, core.wid, &d, state) {
                shown.extend(
                    shard
                        .rows
                        .iter()
                        .filter(|r| r.vis == Vis::Visible)
                        .map(|r| r.entry.id.clone()),
                );
            }
        }
        shown
    }

    /// The user asked for the image of `post` (tap to load, or again after it failed).
    pub fn group_image_want(
        self: &Arc<Self>,
        core: &Arc<GroupCore>,
        gid: &str,
        post: &str,
    ) -> Result<(), Error> {
        let mut row = core
            .store
            .image_of_post(gid, post)?
            .ok_or(Error::NotFound)?;
        if row.ready {
            return Ok(());
        }
        if row.dropped {
            return Err(Error::Rejected(
                "this picture is no longer kept here".into(),
            ));
        }
        row.wanted = true;
        row.failed = false;
        core.store.put_image(gid, &row)?;
        core.images_load();
        core.images
            .pending
            .lock()
            .entry((gid.to_string(), row.image.hash))
            .and_modify(|p| {
                p.wanted = true;
                p.tries = 0;
                p.next = None;
            })
            .or_insert(Pend {
                wanted: true,
                ..Default::default()
            });
        self.group_image_event(gid, post);
        self.group_images_pump(core);
        Ok(())
    }

    /// The opened picture of `post` on disk (decrypted once into `group-images/`), if its blob
    /// is here.
    pub async fn group_image_file(
        self: &Arc<Self>,
        core: &Arc<GroupCore>,
        gid: &str,
        post: &str,
    ) -> Result<Option<PathBuf>, Error> {
        let Some(row) = core.store.image_of_post(gid, post)? else {
            return Ok(None);
        };
        if !row.ready {
            return Ok(None);
        }
        let path = core.images.opened(&row.image.hash, &row.image.mime);
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
        hub.export_plain(h, row.image.key, path.clone(), |_| {})
            .await?;
        Ok(Some(path))
    }

    /// Seals `jpeg` into the store under a fresh key, for a post of ours in `gid`; the picture
    /// is also kept opened (we have it already).
    pub async fn group_image_add(
        self: &Arc<Self>,
        core: &Arc<GroupCore>,
        gid: &str,
        p: &crate::images::Prepared,
        mime: &str,
    ) -> Result<ImageRef, Error> {
        let hub = self.group_hub(core).await?;
        let key = crypt::new_key();
        let hash = hub
            .add_bytes_owned(&owner_of(gid), key, p.jpeg.clone())
            .await?;
        let img = ImageRef {
            hash: *hash.as_bytes(),
            key,
            size: p.jpeg.len() as u64,
            w: p.w,
            h: p.h,
            mime: mime.to_string(),
            preview: p.preview.clone(),
        };
        let path = core.images.opened(&img.hash, mime);
        // Our own picture is shown from this file: without it the post would look like one we
        // still have to download. The blob goes again and nothing has been announced.
        let cached = match path.parent() {
            Some(parent) => std::fs::create_dir_all(parent),
            None => Ok(()),
        }
        .and_then(|()| std::fs::write(&path, &p.jpeg));
        if let Err(e) = cached {
            let _ = hub.release_owned(&owner_of(gid), &hash).await;
            return Err(e.into());
        }
        Ok(img)
    }

    /// A post of ours could not be written after its blob went in: let the blob go.
    pub async fn group_image_abandon(
        self: &Arc<Self>,
        core: &Arc<GroupCore>,
        gid: &str,
        img: &ImageRef,
    ) {
        if let Ok(hub) = self.group_hub(core).await {
            let _ = hub
                .release_owned(&owner_of(gid), &Hash::from_bytes(img.hash))
                .await;
        }
        let _ = std::fs::remove_file(core.images.opened(&img.hash, &img.mime));
    }
}
