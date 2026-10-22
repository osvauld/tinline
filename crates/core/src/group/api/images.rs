//! Image posts (`docs/design/public-group.md` §5): posting a picture, loading one on tap, the
//! opened file, and the auto-download setting. The runtime is `engine::images`.

use super::*;
use crate::chat::store::PictureDownload;
use crate::group::images::{Prepared, prepare};
use crate::group::store::ImgRow;

impl Inner {
    /// Seals the prepared picture into the group's blob store and writes the post that names it.
    async fn group_post_prepared(
        self: &Arc<Self>,
        core: &Arc<GroupCore>,
        gid: &str,
        p: Prepared,
        mime: &str,
        caption: String,
    ) -> Result<GroupMessage, Error> {
        let img = self.group_image_add(core, gid, &p, mime).await?;
        let (entry, changed) = match core.write(
            gid,
            Body::Post {
                text: caption,
                reply_to: None,
                image: Some(img.clone()),
            },
        ) {
            Ok(x) => x,
            Err(e) => {
                self.group_image_abandon(core, gid, &img).await;
                return Err(e);
            }
        };
        // Ours and here already: the row says so before `group_after` notes the post.
        let row = ImgRow {
            post: entry.id.clone(),
            author: core.did.clone(),
            at: entry.rec.at,
            image: img.clone(),
            ready: true,
            wanted: false,
            failed: false,
            dropped: false,
            referenced: true,
        };
        if let Err(e) = core.store.put_image(gid, &row) {
            tracing::warn!("group image row: {e}");
        }
        let item = changed.new_items.iter().find(|i| i.id == entry.id).cloned();
        self.group_after(core, changed, None);
        let item = item.unwrap_or(Item {
            id: entry.id.clone(),
            kind: ItemKind::Post,
            author: core.did.clone(),
            subject: core.did.clone(),
            at: entry.rec.at,
            text: String::new(),
            reply_to: None,
            image: Some(img),
            before_removal: false,
            peer: entry.peer,
            counter: entry.counter,
        });
        self.group_message(core, gid, &item).ok_or(Error::NotFound)
    }

    /// The checks of `group_post`: a current member, and not a member of a channel.
    fn group_post_check(&self, core: &GroupCore, gid: &str) -> Result<(), Error> {
        self.group_member_check(core, gid)?;
        if core
            .with_state(gid, |st| st.admins_only && !st.is_admin(&core.did))
            .unwrap_or(false)
        {
            return Err(Error::Rejected("admins only".into()));
        }
        Ok(())
    }
}

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
        if caption.len() > MAX_TEXT {
            return Err(Error::Protocol(
                "message must be at most 20000 bytes".into(),
            ));
        }
        let core = self.inner.group_core()?;
        self.inner.group_post_check(&core, &group_id)?;
        let p = prepare(&bytes)?;
        let inner = self.inner.clone();
        self.block_on(async move {
            inner
                .group_post_prepared(&core, &group_id, p, "image/jpeg", caption)
                .await
        })?
    }

    /// Loads the picture of post `message_id` now (tap to load, or try again after `Failed`),
    /// whatever the auto-download setting says. `on_group_image` reports when it is here.
    pub fn group_fetch_image(&self, group_id: String, message_id: String) -> Result<(), Error> {
        let core = self.inner.group_core()?;
        self.inner.group_image_want(&core, &group_id, &message_id)
    }

    /// The picture of post `message_id` as a file on this device, once it is here (`None`
    /// before). Blocking (opens it the first time).
    pub fn group_image_path(&self, group_id: String, message_id: String) -> Option<String> {
        let core = self.inner.group_core().ok()?;
        let inner = self.inner.clone();
        let path = self
            .block_on(async move { inner.group_image_file(&core, &group_id, &message_id).await })
            .ok()?;
        match path {
            Ok(p) => p.map(|p| p.to_string_lossy().into_owned()),
            Err(e) => {
                tracing::warn!("group image: {e}");
                None
            }
        }
    }

    /// Settings: when group pictures download by themselves on this device (default `Wifi`:
    /// not on a metered connection, which the app reports with `set_connection_metered`).
    pub fn group_image_auto_download(&self) -> Result<PictureDownload, Error> {
        Ok(self.inner.group_core()?.image_download())
    }

    pub fn group_set_image_auto_download(&self, mode: PictureDownload) -> Result<(), Error> {
        let core = self.inner.group_core()?;
        core.set_image_download(mode)?;
        self.inner.group_images_pump(&core);
        Ok(())
    }
}

/// Test hooks that release builds do not have (Rust only: not exported through uniffi).
#[cfg(any(test, feature = "dev-tools"))]
impl Node {
    /// `(largest blob a host takes, bytes a host keeps per group)` instead of 1 MB / 2 GB;
    /// `None` goes back to them. Not in release builds.
    pub fn group_set_image_limits_for_test(&self, limits: Option<(u64, u64)>) {
        if let Ok(core) = self.inner.group_core() {
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
        let core = self.inner.group_core()?;
        self.inner.group_post_check(&core, &group_id)?;
        let inner = self.inner.clone();
        self.block_on(async move {
            let p = Prepared {
                jpeg: bytes,
                w: 100,
                h: 100,
                preview: Vec::new(),
            };
            let mut img = inner
                .group_image_add(&core, &group_id, &p, "image/jpeg")
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
            let row = ImgRow {
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
            inner.group_after(&core, changed, None);
            inner
                .group_message(&core, &group_id, &item.ok_or(Error::NotFound)?)
                .ok_or(Error::NotFound)
        })?
    }
}
