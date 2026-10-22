//! What an app shows for a message's recipient-signed receipts (M3): the strongest verified
//! level per recipient, the original statements behind it and the status mark. Plain records;
//! p2pcore declares them to UniFFI (`receipts::api`).
//!
//! Reports ONLY committed, verified recipient statements for the exact version shown. The
//! legacy `DeliveryState` tick (peer transport Ack) and group `GroupDelivery` (host storage Ack)
//! are NOT recipient evidence. A signature proves the signer's claim, not human attention.

use crate::store::messages::VersionEvidence;
use crate::{Kind, doc::Entry};

/// Strongest verified recipient statement for one exact message version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiptLevel {
    /// No committed recipient statement for this version (yet). Not "failed".
    Unconfirmed,
    /// A device of the recipient account signed that it durably holds it.
    Delivered,
    /// A device of the recipient account signed that it was marked read there.
    Read,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiptProofKind {
    Delivered,
    Read,
}

/// One original signed statement backing a level (inspectable provenance).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptProof {
    pub kind: ReceiptProofKind,
    /// The DID whose key signed the statement.
    pub signer_did: String,
    /// The signing install's scoped device ID (hex), not a network address.
    pub signer_device: String,
    /// Content address of the original signed bytes (hex).
    pub receipt_id: String,
    /// UTC day of the receipt scope (the message's day shard).
    pub scope_day: String,
    /// How many exact versions that one signed statement covers.
    pub covered_versions: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipientReceipts {
    pub did: String,
    pub level: ReceiptLevel,
    pub proofs: Vec<ReceiptProof>,
}

/// What an outgoing message's status mark should say, decided in core so both
/// apps agree. Only `Delivered`/`Read` come from recipient-signed statements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiptTick {
    /// No mark: incoming, deleted, or a note to yourself.
    None,
    /// Still only on this device (it has not gone out yet).
    OnlyHere,
    /// Sent; the recipient's device has not confirmed this version yet. Must
    /// not look delivered, whatever the unsigned transport ack says.
    Waiting,
    /// A device of the recipient signed that it holds this version.
    Delivered,
    /// A device of the recipient signed that it marked this version read.
    Read,
}

/// The chat list's mark for a conversation whose last message is ours.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatReceiptRow {
    pub peer_did: String,
    pub status: MessageReceiptStatus,
}

/// Recipient evidence for the version of a message this device shows. An
/// edited/deleted message is a new version: earlier receipts do not carry over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageReceiptStatus {
    pub message_id: String,
    /// Hex digest of the exact signed version the statements must cover.
    pub version: String,
    /// The mark to draw for this message (see `ReceiptTick`).
    pub tick: ReceiptTick,
    /// One row per recipient account; empty for "You" (notes to self).
    pub recipients: Vec<RecipientReceipts>,
}

fn proof(e: &Entry, day: &str) -> ReceiptProof {
    ReceiptProof {
        kind: if e.receipt.kind == Kind::Read {
            ReceiptProofKind::Read
        } else {
            ReceiptProofKind::Delivered
        },
        signer_did: e.receipt.recipient.clone(),
        signer_device: hex::encode(e.receipt.device),
        receipt_id: e.id.clone(),
        scope_day: day.to_string(),
        covered_versions: e.receipt.messages.len() as u32,
    }
}

pub fn recipient(did: &str, ev: &VersionEvidence) -> RecipientReceipts {
    let level = if !ev.read.is_empty() {
        ReceiptLevel::Read
    } else if !ev.delivered.is_empty() {
        ReceiptLevel::Delivered
    } else {
        ReceiptLevel::Unconfirmed
    };
    let proofs = ev
        .delivered
        .iter()
        .chain(&ev.read)
        .map(|e| proof(e, &e.receipt.scope.day))
        .collect();
    RecipientReceipts {
        did: did.to_string(),
        level,
        proofs,
    }
}

