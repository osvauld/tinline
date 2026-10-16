//! Group member message receipts (receipt roadmap M3/M4, group step 1): who of a group may
//! confirm which post, and what the sender's mark says. Pure functions of the derived meta
//! state, the stored posts and committed receipt statements; the engine glue lives in
//! `engine/member_receipts.rs`, the signed format and ledger in `crate::receipts`.
//!
//! Format: the shared signed `receipts::Receipt` (DID-signed, `Kind::Delivered`/`Read`), one
//! ledger per `(group, UTC day of the post's day doc)`, covering exact `group_version` digests
//! of the stored `SignedRecord` bytes. `metadata` is the issuer's meta-doc frontier when it
//! accepted the post (delivered) or moved its read cursor (read), signed into the receipt.
//!
//! Admission of a statement (every device decides the same from the same records, in every
//! delivery order, like U11 for posts):
//! - **Signed while admitted.** The issuer DID was a member, and not removed, as of the
//!   receipt's own signed frontier (`post_visible(state, issuer, frontier)`). The clock is that
//!   causal frontier, never a wall clock. A frontier naming meta records we lack is *pending*.
//! - **Covers only posts it could have held.** Every covered post is one we hold, visible, not
//!   the issuer's own, and its frontier is inside the receipt frontier (an honest device only
//!   shows a post once it holds the post's frontier).
//! - **No receipts for posts from before joining.** The issuer was a member as of each covered
//!   post's frontier: a late joiner cannot confirm earlier posts.
//!
//! Consequence and limit: a member removed by flags (or who left) can still have a statement
//! accepted that it signed before it saw its removal, also when it arrives late; a statement
//! whose frontier includes the removal is refused, and so is any statement covering a post
//! whose author had already seen the removal (that post's frontier includes it). A removed
//! member that deliberately withholds meta updates can keep signing with a stale frontier for
//! posts concurrent with the removal: the frontier is the signer's claim, exactly as for U11
//! posts. Group core has no per-device admission (membership is per DID; any install holding
//! the account key signs as that DID), so "device admitted" means "its DID admitted".
//!
//! Eligibility for the aggregate (decided 2026-10-16): the members who were members as of the
//! post's frontier (excluding the author) AND are still members now. A member removed or gone
//! after the post stops blocking the mark; its stored statements are kept and listed, never
//! deleted. A late joiner never counts for earlier posts.

use std::collections::{BTreeMap, BTreeSet};

use super::state::{GroupState, Vis, post_visible};
use crate::receipts::api::{ReceiptLevel, ReceiptTick, RecipientReceipts, recipient};
use crate::receipts::doc::Entry;
use crate::receipts::store::messages::VersionEvidence;
use crate::receipts::{Conversation, Kind, Scope};

/// The receipt ledger scope of a group's day doc.
pub(crate) fn scope(gid: &str, day: &str) -> Scope {
    Scope {
        conversation: Conversation::Group(gid.to_string()),
        day: day.to_string(),
    }
}

/// What a receipt decision needs of one stored post.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PostRef {
    pub id: String,
    pub author: String,
    pub seen: Vec<(u64, i32)>,
    pub vis: Vis,
}

/// Why a statement was not admitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// It names meta records or posts we do not hold yet: try again later.
    Pending(&'static str),
    /// It never becomes acceptable.
    Rejected(&'static str),
}

impl Refusal {
    pub(crate) fn message(&self) -> String {
        match self {
            Refusal::Pending(m) => format!("{PENDING_PREFIX}{m}"),
            Refusal::Rejected(m) => (*m).to_string(),
        }
    }
}

/// Prefix of a policy error that only waits for records we lack.
pub(crate) const PENDING_PREFIX: &str = "pending: ";

/// Whether frontier `outer` includes every op named by frontier `inner`.
pub(crate) fn covers(outer: &[(u64, i32)], inner: &[(u64, i32)]) -> bool {
    inner.iter().all(|(peer, end)| {
        outer
            .binary_search_by_key(peer, |(p, _)| *p)
            .is_ok_and(|i| outer[i].1 >= *end)
    })
}

/// Whether this account (`me`, at its meta frontier `frontier`) issues receipts for `post`:
/// someone else's visible post, written while we were a member (a late joiner does not confirm
/// earlier posts), and we are still admitted as of the frontier we would sign (a device that
/// saw its own removal issues nothing: everyone would refuse it).
pub(crate) fn may_issue(
    st: &GroupState,
    me: &str,
    frontier: &[(u64, i32)],
    post: &PostRef,
) -> bool {
    post.author != me
        && post.vis == Vis::Visible
        && post_visible(st, me, &post.seen) == Vis::Visible
        && post_visible(st, me, frontier) == Vis::Visible
}

/// The admission rule of the module doc for one statement.
pub(crate) fn check(
    st: &GroupState,
    posts: &BTreeMap<[u8; 32], PostRef>,
    e: &Entry,
) -> Result<(), Refusal> {
    if !matches!(e.receipt.kind, Kind::Delivered | Kind::Read) {
        return Err(Refusal::Rejected("not a message receipt"));
    }
    let issuer = e.receipt.recipient.as_str();
    let frontier = e.receipt.metadata.as_slice();
    match post_visible(st, issuer, frontier) {
        Vis::Pending => {
            return Err(Refusal::Pending(
                "receipt frontier names meta records we lack",
            ));
        }
        Vis::Hidden => {
            return Err(Refusal::Rejected(
                "issuer not admitted as of its signed frontier",
            ));
        }
        Vis::Visible => {}
    }
    for d in &e.receipt.messages {
        let Some(p) = posts.get(d) else {
            return Err(Refusal::Pending("receipt covers a post we do not hold"));
        };
        if p.author == issuer {
            return Err(Refusal::Rejected("receipt for the issuer's own post"));
        }
        match p.vis {
            Vis::Pending => return Err(Refusal::Pending("receipt covers a pending post")),
            Vis::Hidden => return Err(Refusal::Rejected("receipt covers a hidden post")),
            Vis::Visible => {}
        }
        if !covers(frontier, &p.seen) {
            return Err(Refusal::Rejected(
                "receipt frontier does not include the post's frontier",
            ));
        }
        if post_visible(st, issuer, &p.seen) != Vis::Visible {
            return Err(Refusal::Rejected(
                "issuer was not a member when the post was written",
            ));
        }
    }
    Ok(())
}

/// `check` over a batch: the first refusal, as the ledger's policy error.
pub(crate) fn admit(
    st: &GroupState,
    posts: &BTreeMap<[u8; 32], PostRef>,
    fresh: &[Entry],
) -> Result<(), String> {
    for e in fresh {
        check(st, posts, e).map_err(|r| r.message())?;
    }
    Ok(())
}

/// Members whose receipts the post's mark waits for (module doc, "Eligibility").
pub(crate) fn eligible(st: &GroupState, post: &PostRef) -> BTreeSet<String> {
    st.members
        .keys()
        .filter(|d| **d != post.author && post_visible(st, d, &post.seen) == Vis::Visible)
        .cloned()
        .collect()
}

/// One account's statements on a post and whether it counts for the mark.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct GroupMemberReceipts {
    pub receipts: RecipientReceipts,
    /// The name the member gave the group ("" for someone no longer a member).
    pub name: String,
    /// Counts for the mark (eligible). A non-eligible row holds kept statements of a member
    /// removed or gone since, shown in details only.
    pub eligible: bool,
}

/// What a group post's status says: the sender's mark and the per-member details (UniFFI:
/// `Node::group_receipts` / `group_message_receipts`).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct GroupReceiptStatus {
    pub message_id: String,
    /// Hex `group_version` digest the statements cover.
    pub version: String,
    /// `None` for posts of others; else `Waiting` (one tick), `Delivered` (every eligible
    /// member delivered or read), `Read` (every eligible member read). With nobody eligible
    /// the mark stays `Waiting`: nobody can confirm it.
    pub tick: ReceiptTick,
    /// Eligible members (N of "n/N").
    pub eligible: u32,
    /// Eligible members with a delivered or read statement.
    pub delivered: u32,
    /// Eligible members with a read statement.
    pub read: u32,
    /// Eligible members first (by DID), then other signers with kept statements.
    pub members: Vec<GroupMemberReceipts>,
}

/// Aggregates the committed statements `entries` (of the post's day ledger) for `post`, whose
/// exact version digest is `version`, as seen by `me`.
pub(crate) fn aggregate(
    st: &GroupState,
    me: &str,
    post: &PostRef,
    version: &[u8; 32],
    entries: &[Entry],
) -> GroupReceiptStatus {
    let mut by: BTreeMap<String, VersionEvidence> = BTreeMap::new();
    for e in entries {
        if e.receipt.recipient == post.author || e.receipt.messages.binary_search(version).is_err()
        {
            continue;
        }
        let ev = by
            .entry(e.receipt.recipient.clone())
            .or_insert_with(|| VersionEvidence {
                delivered: vec![],
                read: vec![],
            });
        match e.receipt.kind {
            Kind::Delivered => ev.delivered.push(e.clone()),
            Kind::Read => ev.read.push(e.clone()),
            Kind::Call => {}
        }
    }
    let empty = VersionEvidence {
        delivered: vec![],
        read: vec![],
    };
    let counted = eligible(st, post);
    let mut members = Vec::new();
    let (mut delivered, mut read) = (0u32, 0u32);
    for did in &counted {
        let r = recipient(did, by.get(did).unwrap_or(&empty));
        match r.level {
            ReceiptLevel::Read => {
                read += 1;
                delivered += 1;
            }
            ReceiptLevel::Delivered => delivered += 1,
            ReceiptLevel::Unconfirmed => {}
        }
        let name = st
            .members
            .get(did)
            .map(|m| m.name.clone())
            .unwrap_or_default();
        members.push(GroupMemberReceipts {
            receipts: r,
            name,
            eligible: true,
        });
    }
    for (did, ev) in &by {
        if !counted.contains(did) {
            members.push(GroupMemberReceipts {
                receipts: recipient(did, ev),
                name: st
                    .members
                    .get(did)
                    .map(|m| m.name.clone())
                    .unwrap_or_default(),
                eligible: false,
            });
        }
    }
    let n = counted.len() as u32;
    let tick = if post.author != me {
        ReceiptTick::None
    } else if n > 0 && read == n {
        ReceiptTick::Read
    } else if n > 0 && delivered == n {
        ReceiptTick::Delivered
    } else {
        ReceiptTick::Waiting
    };
    GroupReceiptStatus {
        message_id: post.id.clone(),
        version: hex::encode(version),
        tick,
        eligible: n,
        delivered,
        read,
        members,
    }
}

#[cfg(test)]
mod tests;
