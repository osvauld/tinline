//! Image and voice posts (`docs/design/public-group.md` §5): posting a picture or a voice
//! message, loading one on tap, the opened file, and the auto-download setting (shared by both).
//! The runtime is `engine::images`.

use super::*;
use crate::chat::store::PictureDownload;
use crate::group::images::prepare;

#[uniffi::export]
impl Node {
    /// Posts the picture in the file at `path` (PNG, JPEG or WebP), with an optional caption
    /// (`""` for none). The core scales it to at most 1080 px on its long side and re-encodes it
    /// as JPEG under 300 KB before it leaves this device. Blocking (decodes and encodes).
    pub fn group_post_image(
        &self,
        group_id: String,
        path: String,
        caption: String,
    ) -> Result<GroupMessage, Error> {
        let bytes = std::fs::read(&path)?;
        self.group_post_image_bytes(group_id, bytes, caption)
    }

    /// `group_post_image` for a picture the app holds in memory (Android content URIs).
    pub fn group_post_image_bytes(
        &self,
        group_id: String,
        bytes: Vec<u8>,
        caption: String,
    ) -> Result<GroupMessage, Error> {
        let slot = self.open();
        if caption.len() > MAX_TEXT {
            return Err(Error::Protocol(
                "message must be at most 20000 bytes".into(),
            ));
        }
        let core = slot.group.group_core()?;
        slot.group.group_post_check(&core, &group_id)?;
        let p = prepare(&bytes)?;
        self.run(|inner| async move {
            inner
                .group.group_post_prepared(&core, &group_id, p, "image/jpeg", caption)
                .await
        })
    }

    /// Loads the picture of post `message_id` now (tap to load, or try again after `Failed`),
    /// whatever the auto-download setting says. `on_group_image` reports when it is here.
    pub fn group_fetch_image(&self, group_id: String, message_id: String) -> Result<(), Error> {
        let slot = self.open();
        let core = slot.group.group_core()?;
        Ok(slot.group.group_image_want(&core, &group_id, &message_id)?)
    }

    /// The picture of post `message_id` as a file on this device, once it is here (`None`
    /// before). Blocking (opens it the first time).
    pub fn group_image_path(&self, group_id: String, message_id: String) -> Option<String> {
        let slot = self.open();
        let core = slot.group.group_core().ok()?;
        let inner = slot.clone();
        let path = self
            .block_on(async move { inner.group.group_image_file(&core, &group_id, &message_id).await })
            .ok()?;
        match path {
            Ok(p) => p.map(|p| p.to_string_lossy().into_owned()),
            Err(e) => {
                tracing::warn!("group image: {e}");
                None
            }
        }
    }

    /// Posts the voice message in the file at `path` (Ogg Opus, 16 kHz mono, from the app's
    /// recorder: the 1:1 `send_voice` format). The core checks the file plays and takes its
    /// duration from it; `waveform` is about 64 peaks, 0..=255. Same checks as `group_post`
    /// (member, `Rejected("admins only")` in a channel). The recording is sealed and fetched as a
    /// picture is (`GroupMessage::voice`). Blocking (reads and seals the file).
    pub fn group_post_voice(
        &self,
        group_id: String,
        path: String,
        waveform: Vec<u8>,
    ) -> Result<GroupMessage, Error> {
        let slot = self.open();
        let core = slot.group.group_core()?;
        slot.group.group_post_check(&core, &group_id)?;
        let (ogg, duration_ms) = voice_file(&path)?;
        if waveform.len() > crate::group::record::MAX_VOICE_BARS {
            return Err(Error::Protocol("a waveform has at most 64 bars".into()));
        }
        self.run(|inner| async move {
            inner.group.group_post_voice(&core, &group_id, &ogg, duration_ms, waveform).await
        })
    }

    /// Loads the recording of voice post `message_id` now (tap, or try again after `Failed`);
    /// `on_group_image` reports when its state changes, as for pictures.
    pub fn group_fetch_voice(&self, group_id: String, message_id: String) -> Result<(), Error> {
        self.group_fetch_image(group_id, message_id)
    }

    /// The recording of voice post `message_id` as an Ogg Opus file on this device, once it is
    /// here (`None` before); play it with `VoiceDecoder`. Blocking (opens it the first time).
    pub fn group_voice_path(&self, group_id: String, message_id: String) -> Option<String> {
        self.group_image_path(group_id, message_id)
    }

    /// Settings: when group pictures download by themselves on this device (default `Wifi`:
    /// not on a metered connection, which the app reports with `set_connection_metered`).
    pub fn group_image_auto_download(&self) -> Result<PictureDownload, Error> {
        Ok(self.open().group.group_core()?.image_download())
    }

    pub fn group_set_image_auto_download(&self, mode: PictureDownload) -> Result<(), Error> {
        let slot = self.open();
        let core = slot.group.group_core()?;
        core.set_image_download(mode)?;
        slot.group.group_images_pump(&core);
        Ok(())
    }
}

/// The bytes of a voice file to post and its duration: it must be a voice message the
/// decoder plays (Ogg Opus, mono, within `MAX_VOICE_FILE`) and within the group limits.
fn voice_file(path: &str) -> Result<(Vec<u8>, u32), Error> {
    use crate::group::record::{MAX_VOICE_BYTES, MAX_VOICE_MS};
    let duration_ms = crate::voice::VoiceDecoder::open(path.to_string())?.duration_ms();
    let ogg = std::fs::read(path)?;
    if ogg.is_empty() || ogg.len() as u64 > MAX_VOICE_BYTES {
        return Err(Error::Protocol("voice message too large".into()));
    }
    if !(1..=MAX_VOICE_MS).contains(&duration_ms) {
        return Err(Error::Protocol("voice message too long or empty".into()));
    }
    Ok((ogg, duration_ms))
}

/// Test hooks that release builds do not have (Rust only: not exported through uniffi).
#[cfg(any(test, feature = "dev-tools"))]
impl Node {
    /// `(largest blob a host takes, bytes a host keeps per group)` instead of 1 MB / 2 GB;
    /// `None` goes back to them. Not in release builds.
    pub fn group_set_image_limits_for_test(&self, limits: Option<(u64, u64)>) {
        if let Ok(core) = self.open().group.group_core() {
            *core.images.limits.lock() = limits.unwrap_or((
                crate::group::record::MAX_IMAGE_BYTES,
                super::super::engine::images::MAX_GROUP_IMAGE_BYTES,
            ));
        }
    }

    /// Posts `bytes` as they are (no scaling or re-encoding), declaring `declared_size` bytes:
    /// what a modified app could post (a blob larger than its post says). Not in release builds.
    pub fn group_post_raw_image_for_test(
        &self,
        group_id: String,
        bytes: Vec<u8>,
        declared_size: u64,
    ) -> Result<GroupMessage, Error> {
        let slot = self.open();
        let core = slot.group.group_core()?;
        slot.group.group_post_check(&core, &group_id)?;
        let inner = slot.clone();
        self.block_on(async move {
            let p = crate::group::images::Prepared {
                jpeg: bytes,
                w: 100,
                h: 100,
                preview: Vec::new(),
            };
            let mut img = inner
                .group.group_image_add(&core, &group_id, &p, "image/jpeg")
                .await?;
            img.size = declared_size;
            let (entry, changed) = core.write(
                &group_id,
                Body::Post {
                    text: "raw".into(),
                    reply_to: None,
                    image: Some(img.clone()),
                },
            )?;
            let row = crate::group::store::ImgRow {
                post: entry.id.clone(),
                author: core.did.clone(),
                at: entry.rec.at,
                image: img,
                ready: true,
                wanted: false,
                failed: false,
                dropped: false,
                referenced: true,
            };
            core.store.put_image(&group_id, &row)?;
            let item = changed.new_items.iter().find(|i| i.id == entry.id).cloned();
            inner.group.group_after(&core, changed, None);
            inner
                .group.group_message(&core, &group_id, &item.ok_or(Error::NotFound)?)
                .ok_or(Error::NotFound)
        })?
    }
}
