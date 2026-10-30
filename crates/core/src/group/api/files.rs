//! File posts (`docs/design/group-files.md`): any file up to 2 GB in a group. Posting streams
//! the file from disk (never all of it in memory); loading, progress, cancel, open, save and
//! deleting our copy mirror pictures and 1:1 attachments. The runtime is `engine::files` on the
//! picture path (`engine::images`).

use std::path::{Path, PathBuf};

use super::*;

/// A file type from the name's extension (the app may know better: `mime` overrides it).
pub fn mime_of(name: &str) -> &'static str {
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "txt" | "log" | "md" => "text/plain",
        "csv" => "text/csv",
        "json" => "application/json",
        "html" | "htm" => "text/html",
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "heic" => "image/heic",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "ogg" | "opus" => "audio/ogg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "odt" => "application/vnd.oasis.opendocument.text",
        "apk" => "application/vnd.android.package-archive",
        "epub" => "application/epub+zip",
        _ => "application/octet-stream",
    }
}

#[uniffi::export]
impl Node {
    /// Posts the file at `path` (any type, up to 2 GB) with an optional caption. `name` is the
    /// name members see (default: the file name of `path`; give it when `path` is a temporary
    /// copy, e.g. from an Android content URI); `mime` its type (default: from the name). The
    /// file is sealed as it is read from disk and the post signs the sealed blob's hash, so every
    /// download is verified chunk by chunk. Same checks as `group_post` (member,
    /// `Rejected("admins only")` in a channel). Blocking (reads and seals the whole file).
    pub fn group_post_file(
        &self,
        group_id: String,
        path: String,
        name: Option<String>,
        mime: Option<String>,
        caption: Option<String>,
    ) -> Result<GroupMessage, Error> {
        let caption = caption.unwrap_or_default();
        if caption.len() > MAX_TEXT {
            return Err(Error::Protocol(
                "message must be at most 20000 bytes".into(),
            ));
        }
        let path = PathBuf::from(path);
        let name = name
            .or_else(|| path.file_name().map(|n| n.to_string_lossy().into_owned()))
            .ok_or_else(|| Error::Protocol("the file has no name".into()))?;
        if !file_name_ok(&name) {
            return Err(Error::Protocol(
                "a file name is one name of at most 255 bytes, without / or \\".into(),
            ));
        }
        let mime = mime
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| mime_of(&name).to_string());
        if !std::fs::metadata(&path)?.is_file() {
            return Err(Error::Protocol("not a file".into()));
        }
        let slot = self.open();
        let core = slot.group.group_core()?;
        slot.group.group_post_check(&core, &group_id)?;
        self.run(|inner| async move {
            inner
                .group
                .group_post_file(&core, &group_id, path, name, mime, caption)
                .await
        })
    }

    /// Starts loading the file of post `message_id` now (tap, or try again after `Failed`),
    /// whatever the auto-download settings say. Progress and the end come as `on_group_image`
    /// (read `GroupMessage.file`).
    pub fn group_fetch_file(&self, group_id: String, message_id: String) -> Result<(), Error> {
        self.group_fetch_image(group_id, message_id)
    }

    /// Stops a download of the file of post `message_id` (running, waiting or failed): the
    /// partial bytes are released and it is `Remote` again; it does not download by itself again
    /// until `group_fetch_file`. A file already here is left alone.
    pub fn group_cancel_file(&self, group_id: String, message_id: String) -> Result<(), Error> {
        let slot = self.open();
        let core = slot.group.group_core()?;
        self.run(|inner| async move {
            inner
                .group
                .group_file_cancel(&core, &group_id, &message_id)
                .await
        })
    }

    /// The file of post `message_id` on this device, opened (decrypted) once under its own
    /// name, or `None` while it is not here. Blocking (decrypts it the first time).
    pub fn group_file_path(&self, group_id: String, message_id: String) -> Option<String> {
        self.group_image_path(group_id, message_id)
    }

    /// Decrypts the file of post `message_id` (it must be `Ready`) to `dest_path`, a place the
    /// user picked. Blocking.
    pub fn group_save_file(
        &self,
        group_id: String,
        message_id: String,
        dest_path: String,
    ) -> Result<(), Error> {
        let slot = self.open();
        let core = slot.group.group_core()?;
        self.run(|inner| async move {
            inner
                .group
                .group_file_save(&core, &group_id, &message_id, PathBuf::from(dest_path))
                .await
        })
    }

    /// Deletes this device's copy of the file of post `message_id` (and its opened copy). If
    /// this account said it holds the file, the group learns it no longer does. A device that
    /// hosts the group keeps the files hosts keep (`Rejected`). It can be loaded again later.
    pub fn group_delete_file_copy(
        &self,
        group_id: String,
        message_id: String,
    ) -> Result<(), Error> {
        let slot = self.open();
        let core = slot.group.group_core()?;
        self.run(|inner| async move {
            inner
                .group
                .group_file_delete(&core, &group_id, &message_id)
                .await
        })
    }
}

/// Test hooks that release builds do not have (Rust only: not exported through uniffi).
#[cfg(any(test, feature = "dev-tools"))]
impl Node {
    /// Whether the complete sealed blob of the file of post `message_id` is in this device's
    /// group blob store (any tag).
    pub fn group_file_blob_here_for_test(&self, group_id: String, message_id: String) -> bool {
        let slot = self.open();
        let Ok(core) = slot.group.group_core() else {
            return false;
        };
        let Ok(Some(row)) = core.store.image_of_post(&group_id, &message_id) else {
            return false;
        };
        let inner = slot.clone();
        self.block_on(async move {
            let Ok(hub) = inner.group.group_hub(&core).await else {
                return false;
            };
            let total = transfer::crypt::cipher_len(row.image.size);
            hub.complete_size(&iroh_blobs::Hash::from_bytes(row.image.hash))
                .await
                == Some(total)
        })
        .unwrap_or(false)
    }

    /// Alters one byte of the sealed blob of the file of post `message_id` as this device would
    /// serve it, keeping the bao tree of the original: a modified app serving a tampered copy.
    /// The blob is re-imported under the same hash from bytes that do not match it.
    pub fn group_file_tamper_for_test(
        &self,
        group_id: String,
        message_id: String,
        at: u64,
    ) -> Result<(), Error> {
        let slot = self.open();
        let core = slot.group.group_core()?;
        let row = core
            .store
            .image_of_post(&group_id, &message_id)?
            .ok_or(Error::NotFound)?;
        let dir = core.images.dir().join("group-blobs").join("data");
        let data = dir.join(format!(
            "{}.data",
            iroh_blobs::Hash::from_bytes(row.image.hash).to_hex()
        ));
        let mut bytes = std::fs::read(&data)?;
        let i = (at as usize).min(bytes.len().saturating_sub(1));
        bytes[i] ^= 0x01;
        std::fs::write(&data, bytes)?;
        Ok(())
    }
}
