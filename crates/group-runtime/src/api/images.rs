//! Image posts (public group §5): sealing a picture into the group blob store and the post that
//! names it. The app calls are p2pcore's `group::api::images`.

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
            tracing::warn!("group {}: image row: {e}", crate::short(gid));
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
