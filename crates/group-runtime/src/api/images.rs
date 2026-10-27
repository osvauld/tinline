//! Image and voice posts (public group §5): sealing a picture or a recording into the group blob
//! store and the post that names it. The app calls are p2pcore's `group::api::images`.

use super::*;
use crate::images::Prepared;
use crate::store::ImgRow;

impl GroupRt {
    /// Seals the prepared picture into the group's blob store and writes the post that names it.
    pub async fn group_post_prepared(
        self: &Arc<Self>,
        core: &Arc<GroupCore>,
        gid: &str,
        p: Prepared,
        mime: &str,
        caption: String,
    ) -> Result<GroupMessage, Error> {
        let img = self.group_image_add(core, gid, &p, mime).await?;
        let body = Body::Post { text: caption, reply_to: None, image: Some(img.clone()) };
        self.group_post_blob(core, gid, img, body).await
    }

    /// Seals a finished recording (Ogg Opus, checked by the caller) into the group's blob store
    /// and writes the voice post that names it.
    pub async fn group_post_voice(
        self: &Arc<Self>,
        core: &Arc<GroupCore>,
        gid: &str,
        ogg: &[u8],
        duration_ms: u32,
        waveform: Vec<u8>,
    ) -> Result<GroupMessage, Error> {
        let voice = self.group_voice_add(core, gid, ogg, duration_ms, waveform).await?;
        let blob = voice.as_blob();
        self.group_post_blob(core, gid, blob, Body::Voice { reply_to: None, voice }).await
    }

    /// Writes `body`, a post naming the blob `blob` that is already sealed into our store; on a
    /// refused write the blob goes again.
    async fn group_post_blob(
        self: &Arc<Self>,
        core: &Arc<GroupCore>,
        gid: &str,
        blob: ImageRef,
        body: Body,
    ) -> Result<GroupMessage, Error> {
        let (entry, changed) = match core.write(gid, body) {
            Ok(x) => x,
            Err(e) => {
                self.group_image_abandon(core, gid, &blob).await;
                return Err(e);
            }
        };
        // Ours and here already: the row says so before `group_after` notes the post.
        let row = ImgRow {
            post: entry.id.clone(),
            author: core.did.clone(),
            at: entry.rec.at,
            image: blob,
            ready: true,
            wanted: false,
            failed: false,
            dropped: false,
            referenced: true,
        };
        if let Err(e) = core.store.put_image(gid, &row) {
            tracing::warn!("group {}: image row: {e}", crate::short(gid));
        }
        let item = changed.new_items.iter().find(|i| i.id == entry.id).cloned();
        self.group_after(core, changed, None);
        // Not shown yet (pending): the post as we wrote it.
        let item = match item {
            Some(i) => i,
            None => core.with_state(gid, |st| crate::state::post_item(&entry, st)).flatten().ok_or(Error::NotFound)?,
        };
        self.group_message(core, gid, &item).ok_or(Error::NotFound)
    }

    /// The checks of `group_post`: a current member, and not a member of a channel.
    pub fn group_post_check(&self, core: &GroupCore, gid: &str) -> Result<(), Error> {
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
