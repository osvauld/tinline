//! Additive UniFFI surface for recipient-signed message receipts (M3).
//!
//! This is the provenance/status contract for app views. It reports ONLY
//! committed, verified recipient statements for the exact version shown. The
//! legacy `DeliveryState` tick (peer transport Ack) and group `GroupDelivery`
//! (host storage Ack) are unchanged and are NOT recipient evidence; apps must
//! not merge them into `ReceiptLevel`. A signature proves the signer's claim,
//! not human attention.

use super::store::messages::VersionEvidence;
use super::{Kind, doc::Entry};
use crate::Error;
use crate::node::Node;

/// Strongest verified recipient statement for one exact message version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ReceiptLevel {
    /// No committed recipient statement for this version (yet). Not "failed".
    Unconfirmed,
    /// A device of the recipient account signed that it durably holds it.
    Delivered,
    /// A device of the recipient account signed that it was marked read there.
    Read,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ReceiptProofKind {
    Delivered,
    Read,
}

/// One original signed statement backing a level (inspectable provenance).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
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

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RecipientReceipts {
    pub did: String,
    pub level: ReceiptLevel,
    pub proofs: Vec<ReceiptProof>,
}

/// What an outgoing message's status mark should say, decided in core so both
/// apps agree. Only `Delivered`/`Read` come from recipient-signed statements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
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
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ChatReceiptRow {
    pub peer_did: String,
    pub status: MessageReceiptStatus,
}

/// Recipient evidence for the version of a message this device shows. An
/// edited/deleted message is a new version: earlier receipts do not carry over.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
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

pub(crate) fn recipient(did: &str, ev: &VersionEvidence) -> RecipientReceipts {
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

#[uniffi::export]
impl Node {
    /// Signed recipient receipts for one 1:1 message (the version shown here).
    /// Blocking. For our message: the contact's statements. For theirs: the
    /// statements our account issued. Independent of `Message.delivery`.
    pub fn chat_message_receipts(
        &self,
        peer_did: String,
        message_id: String,
    ) -> Result<MessageReceiptStatus, Error> {
        let inner = self.inner.clone();
        self.block_on(async move { inner.chat_message_receipts(&peer_did, &message_id).await })?
    }

    /// The mark for every conversation whose last message is ours (the chat list).
    /// Blocking. Re-read after `on_chat_changed` / `on_delivery_changed` / `on_message_changed`.
    pub fn chat_list_receipts(&self) -> Result<Vec<ChatReceiptRow>, Error> {
        self.inner.chat_list_receipts()
    }

    /// `chat_message_receipts` for every message of one UTC day page, oldest first (as
    /// `chat_day`). Blocking. Use this to render a page; re-read after `on_message_changed`.
    pub fn chat_day_receipts(
        &self,
        peer_did: String,
        day: String,
    ) -> Result<Vec<MessageReceiptStatus>, Error> {
        let inner = self.inner.clone();
        self.block_on(async move { inner.chat_day_receipts(&peer_did, &day).await })?
    }
}
