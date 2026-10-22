//! Image blobs of posts (public group §5), local to this device:
//!
//! ```text
//! img/{gid}/{hash hex}      ImgRow   an image a visible post of the group names, and what we hold
//! imgp/{gid}/{post id}      hash hex the image of that post
//! set/img_dl                u8       auto-download: 0 always, 1 Wi-Fi only (default), 2 on tap
//! ```
//!
//! The bytes live in the group blob store (p2pcore `group::images`), tagged per group; these rows
//! say which hashes the group references, which we hold, and what a host dropped. The eviction
//! order is [`plan_eviction`], a pure function.

use super::{GroupStore, io, json};
use crate::Error;
use crate::record::ImageRef;
use serde::{Deserialize, Serialize};

/// What this device knows of one image of a group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImgRow {
    /// The first visible post that named it.
    pub post: String,
    pub author: String,
    /// The post's time (author's clock): what "oldest" means for eviction.
    pub at: i64,
    pub image: ImageRef,
    /// The whole blob is in our store.
    pub ready: bool,
    /// The user asked for it (tap to load), whatever the auto-download setting says.
    #[serde(default)]
    pub wanted: bool,
    /// Gave up fetching (every source failed for a while); a new session or a tap tries again.
    #[serde(default)]
    pub failed: bool,
    /// Host side: refused for good (over the host's size limit) or dropped to stay under the
    /// group's budget. Never fetched again here.
    #[serde(default)]
    pub dropped: bool,
    /// A visible post still names it (false once none does: the first to go).
    #[serde(default = "yes")]
    pub referenced: bool,
}

fn yes() -> bool {
    true
}

fn img_key(gid: &str, hash: &[u8; 32]) -> String {
    format!("img/{gid}/{}", hex::encode(hash))
}

/// Which held blobs to drop so the group's bytes fit `budget`: those no visible post references
/// first, then the rest, each oldest first (`at`, then hash). `held` is `(hash, bytes, at,
/// referenced)` of the blobs we hold.
pub fn plan_eviction(held: &[([u8; 32], u64, i64, bool)], budget: u64) -> Vec<[u8; 32]> {
    let mut total: u64 = held.iter().map(|h| h.1).sum();
    if total <= budget {
        return Vec::new();
    }
    let mut order: Vec<&([u8; 32], u64, i64, bool)> = held.iter().collect();
    order.sort_by(|a, b| (a.3, a.2, a.0).cmp(&(b.3, b.2, b.0)));
    let mut out = Vec::new();
    for (hash, bytes, _, _) in order {
        if total <= budget {
            break;
        }
        total = total.saturating_sub(*bytes);
        out.push(*hash);
    }
    out
}

/// The auto-download setting as stored (`set/img_dl`): 0 always, 1 Wi-Fi only, 2 on tap.
pub const IMG_DL_DEFAULT: u8 = 1;

impl GroupStore {
    pub fn image(&self, gid: &str, hash: &[u8; 32]) -> Result<Option<ImgRow>, Error> {
        Ok(self
            .db
            .get(&img_key(gid, hash))
            .map_err(io)?
            .and_then(|b| serde_json::from_slice(&b).ok()))
    }

    /// The image row of a post, if the post has one we noted.
    pub fn image_of_post(&self, gid: &str, post: &str) -> Result<Option<ImgRow>, Error> {
        let Some(h) = self.db.get(&format!("imgp/{gid}/{post}")).map_err(io)? else {
            return Ok(None);
        };
        let Some(hash) = hex::decode(&h)
            .ok()
            .and_then(|b| <[u8; 32]>::try_from(b).ok())
        else {
            return Ok(None);
        };
        self.image(gid, &hash)
    }

    /// Writes the row, and the post index for `row.post`.
    pub fn put_image(&self, gid: &str, row: &ImgRow) -> Result<(), Error> {
        let ops = vec![
            self.db
                .put_op(&img_key(gid, &row.image.hash), &json(row))
                .map_err(io)?,
            self.db
                .put_op(
                    &format!("imgp/{gid}/{}", row.post),
                    hex::encode(row.image.hash).as_bytes(),
                )
                .map_err(io)?,
        ];
        self.db.apply(&ops).map_err(io)
    }

    /// A second post naming a hash we already have a row for: only the index.
    pub fn index_image_post(&self, gid: &str, post: &str, hash: &[u8; 32]) -> Result<(), Error> {
        self.db
            .put(&format!("imgp/{gid}/{post}"), hex::encode(hash).as_bytes())
            .map_err(io)
    }

    /// Every image row of a group.
    pub fn images(&self, gid: &str) -> Result<Vec<ImgRow>, Error> {
        Ok(self
            .db
            .scan(&format!("img/{gid}/"))
            .map_err(io)?
            .into_iter()
            .filter_map(|(_, v)| serde_json::from_slice(&v).ok())
            .collect())
    }

    /// The auto-download setting (`set/img_dl`), default Wi-Fi only.
    pub fn image_download(&self) -> u8 {
        self.db
            .get("set/img_dl")
            .ok()
            .flatten()
            .and_then(|b| b.first().copied())
            .filter(|v| *v <= 2)
            .unwrap_or(IMG_DL_DEFAULT)
    }

    pub fn set_image_download(&self, v: u8) -> Result<(), Error> {
        self.db.put("set/img_dl", &[v.min(2)]).map_err(io)
    }
}

#[cfg(test)]
mod tests;
