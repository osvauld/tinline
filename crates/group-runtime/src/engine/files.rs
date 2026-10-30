//! Group files (`docs/design/group-files.md`) on top of the image runtime (`images.rs`): the
//! rows, gate, fetch, retries and budget are the pictures'; this module adds what files need.
//!
//! - **Sending** streams the file from disk through the chunk cipher into the group blob store
//!   (`BlobHub::add_file_owned`); the post signs the resulting hash.
//! - **Hosts keep files (§4, option B):** a host fetches and keeps every file up to
//!   `HOST_KEEP_FILE_MAX` (2 GiB, so every file) by itself, like a picture, within the group
//!   budget. Members fetch from hosts only. A file over the constant (if it is ever lowered) is
//!   not fetched by hosts and so is reachable only while a host holds it for its own user.
//! - **Holders (§6):** a member device that completed a download writes `FileHeld{held: true}`
//!   (in the post's day doc), and `held: false` when it deletes its copy. Not in open groups,
//!   not for a host's kept copy.
//! - **Progress, cancel, delete, open, save.**

use std::path::PathBuf;

use iroh_blobs::Hash;
use transfer::crypt;

use super::images::owner_of;
use super::*;
use crate::store::ImgRow;
use chat_core::store::DEFAULT_AUTO_DOWNLOAD;

/// At most one progress event per file this often.
const PROGRESS_EVERY: Duration = Duration::from_millis(500);

type Key = (String, [u8; 32]);

/// The file side of an account's image runtime (`ImagesRt::files`).
pub struct FilesRt {
    /// Files up to this size auto-download (the 1:1 `auto_download_limit`, set by p2pcore).
    auto_max: AtomicU64,
    /// `(ciphertext bytes so far, total, last event)` of running downloads.
    progress: Mutex<HashMap<Key, (u64, u64, Instant)>>,
    tasks: Mutex<HashMap<Key, tokio::task::AbortHandle>>,
}

impl Default for FilesRt {
    fn default() -> Self {
        Self {
            auto_max: AtomicU64::new(DEFAULT_AUTO_DOWNLOAD),
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

    /// Notes progress; true when an event is due.
    pub(super) fn progress(&self, gid: &str, hash: &[u8; 32], done: u64, total: u64) -> bool {
        let mut p = self.progress.lock();
        let now = Instant::now();
        let e = p
            .entry((gid.to_string(), *hash))
            .or_insert((0, total, now - PROGRESS_EVERY));
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
            Some(&(n, t, _)) if t > 0 => {
                ((n as u128 * size as u128) / t as u128).min(size as u128) as u64
            }
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
}

impl GroupCore {
    /// A file over what hosts keep (`HOST_KEEP_FILE_MAX`; none while it is the 2 GiB maximum).
    pub(super) fn over_keep(&self, row: &ImgRow) -> bool {
        row.file.as_ref().is_some_and(|f| !f.kept_by_hosts())
    }

    /// We host `gid` and do not keep this file, so we fetch it only if our own user asks.
    pub(super) fn host_skips(&self, gid: &str, row: &ImgRow) -> bool {
        self.over_keep(row) && !row.ready && self.am_host(gid)
    }

    /// The DIDs holding the file of post `post` now (group files §6).
    pub fn file_holders_of(&self, gid: &str, post: &str) -> Vec<String> {
        let Ok(Some(row)) = self.store.image_of_post(gid, post) else {
            return Vec::new();
        };
        let Some(file) = row.file else {
            return Vec::new();
        };
        let mut groups = self.groups.lock();
        let Some(live) = groups.get_mut(gid) else {
            return Vec::new();
        };
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
        let shard = shards
            .get(&self.store, gid, self.wid, &day_of(row.at), state)
            .ok()?;
        crate::files::own_statement(
            &shard.rows,
            &row.post,
            &row.image.hash,
            &self.did,
            &self.dev,
        )
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
        let (hash, size) = hub
            .add_file_owned(&owner_of(gid), key, path, |_| {})
            .await?;
        let file = FileRef {
            hash: *hash.as_bytes(),
            key,
            size,
            name,
            mime,
        };
        if !file.well_formed() {
            let _ = hub.release_owned(&owner_of(gid), &hash).await;
            return Err(Error::Rejected(
                "this file cannot be posted (too large, or a bad name)".into(),
            ));
        }
        let blob = file.as_blob();
        let body = Body::File {
            caption,
            reply_to: None,
            file: file.clone(),
        };
        self.group_post_blob(core, gid, blob, Some(file), body)
            .await
    }

    /// Writes our holder announcement for `row` if it changes what we last said. Never in open
    /// groups (no receipts there either), never for a host's kept copy (that is
    /// `kept_by_hosts`), and the author says only `false` (holding is the author's default).
    pub(super) fn group_file_announce(
        self: &Arc<Self>,
        core: &Arc<GroupCore>,
        gid: &str,
        row: &ImgRow,
        held: bool,
    ) {
        if core
            .with_state(gid, |st| {
                st.open || !st.is_member(&core.did) || st.is_removed(&core.did)
            })
            .unwrap_or(true)
        {
            return;
        }
        if held && core.am_host(gid) && !core.over_keep(row) {
            return;
        }
        let said = core.file_own_statement(gid, row);
        let author = row.author == core.did;
        let need = if held {
            said != Some(true) && !(author && said.is_none())
        } else {
            said == Some(true) || (author && said.is_none())
        };
        if !need {
            return;
        }
        let body = Body::FileHeld {
            post: row.post.clone(),
            day: day_of(row.at),
            hash: row.image.hash,
            held,
        };
        match core.write(gid, body) {
            Ok((_, changed)) => self.group_after(core, changed, None),
            Err(e) => tracing::warn!("group {}: file holder note: {e}", crate::short(gid)),
        }
    }

    /// Where the opened copy of a file goes: `group-files/<hash>/<name>` in the account dir.
    fn group_file_dest(core: &GroupCore, row: &ImgRow) -> Option<PathBuf> {
        let f = row.file.as_ref()?;
        Some(
            core.images
                .dir()
                .join("group-files")
                .join(hex::encode(f.hash))
                .join(&f.name),
        )
    }

    /// The opened file of a file post (decrypted once), if its blob is here.
    pub(super) async fn group_file_open(
        self: &Arc<Self>,
        core: &Arc<GroupCore>,
        _gid: &str,
        row: &ImgRow,
    ) -> Result<Option<PathBuf>, Error> {
        let Some(path) = Self::group_file_dest(core, row) else {
            return Ok(None);
        };
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

    /// Decrypts the file of `post` to `dest` (a place the user picked).
    pub async fn group_file_save(
        self: &Arc<Self>,
        core: &Arc<GroupCore>,
        gid: &str,
        post: &str,
        dest: PathBuf,
    ) -> Result<(), Error> {
        let row = core
            .store
            .image_of_post(gid, post)?
            .filter(|r| r.file.is_some())
            .ok_or(Error::NotFound)?;
        if !row.ready {
            return Err(Error::Rejected("the file is not on this device yet".into()));
        }
        let hub = self.group_hub(core).await?;
        hub.export_plain(
            Hash::from_bytes(row.image.hash),
            row.image.key,
            dest,
            |_| {},
        )
        .await?;
        Ok(())
    }

    /// Stops a download of the file of `post` (running, waiting to retry or failed): the
    /// partial bytes are released and the file is `Remote` again. A ready file is left alone.
    pub async fn group_file_cancel(
        self: &Arc<Self>,
        core: &Arc<GroupCore>,
        gid: &str,
        post: &str,
    ) -> Result<(), Error> {
        let mut row = core
            .store
            .image_of_post(gid, post)?
            .filter(|r| r.file.is_some())
            .ok_or(Error::NotFound)?;
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
            let _ = hub
                .release_owned(&owner_of(gid), &Hash::from_bytes(row.image.hash))
                .await;
        }
        self.group_image_event(gid, post);
        Ok(())
    }

    /// Deletes this device's copy of the file of `post` (and its opened copy); if we had said
    /// we hold it, the group learns we no longer do. A host keeps the files it keeps for the
    /// group.
    pub async fn group_file_delete(
        self: &Arc<Self>,
        core: &Arc<GroupCore>,
        gid: &str,
        post: &str,
    ) -> Result<(), Error> {
        let mut row = core
            .store
            .image_of_post(gid, post)?
            .filter(|r| r.file.is_some())
            .ok_or(Error::NotFound)?;
        if core.am_host(gid) && !core.over_keep(&row) {
            return Err(Error::Rejected(
                "this device hosts the group and keeps this file for it".into(),
            ));
        }
        if !row.ready {
            return self.group_file_cancel(core, gid, post).await;
        }
        let hub = self.group_hub(core).await?;
        let _ = hub
            .release_owned(&owner_of(gid), &Hash::from_bytes(row.image.hash))
            .await;
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

#[cfg(test)]
mod tests;
