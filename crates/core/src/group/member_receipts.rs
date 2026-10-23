//! Group member receipts: the runtime is `group_runtime::member_receipts`; this declares its
//! records to UniFFI.

pub use group_runtime::member_receipts::*;

use crate::receipts::api::{ReceiptTick, RecipientReceipts};

/// One account's statements on a post and whether it counts for the mark.
#[uniffi::remote(Record)]
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
#[uniffi::remote(Record)]
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
