//! Frames of `tinline/chat/8`: u32 BE length + postcard (`cryptography::codec`) of `ChatMsg`, on one bidirectional QUIC
//! stream opened by the dialer. Both sides send `Auth` first, then everything is symmetric.
//! Version 3 (P10: one version per writer in each record; P6: `Auth` presents a message token,
//! separate from the call token) carries the strict DID-signed chat docs; `tinline/chat/1` (apps
//! before P9c) and `tinline/chat/2` (before P10/P6) are refused clearly (`p2pcore::version`).
//! Version 8 (docs/chat.md section 10): `Days` inventories replace the history request.

use serde::{Deserialize, Serialize};

pub const CHAT_ALPN: &[u8] = b"tinline/chat/8";
/// A shard snapshot of a very busy day can be large; blobs carry the rest.
pub const MAX_FRAME: usize = 16 * 1024 * 1024;

/// Days per `ChatMsg::Days` page.
pub const DAYS_PAGE: usize = 64;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChatMsg {
    /// Device attestation + the message token the other side issued to us (P6).
    Auth { attestation: proto::SignedAttestation, grant: proto::SignedGrant, relay: Option<String> },
    /// `ChatHello`: the days we hold (the last seven, and any with unacknowledged ops), each
    /// with its version vector. Doubles as an acknowledgement of everything in those vvs.
    Hello { shards: Vec<(String, Vec<u8>)> },
    /// `ChatSync`: "here is my version and the ops of mine you lack". Signed.
    Sync { doc: String, vv: Vec<u8>, update: Vec<u8>, sig: Vec<u8> },
    /// `ChatPush`: live, after a local write. Signed.
    Push { doc: String, update: Vec<u8>, sig: Vec<u8> },
    /// `ChatAck`: "I now hold up to here".
    Ack { doc: String, vv: Vec<u8> },
    /// Our inventory: every day of the conversation we hold with its version vector, ascending,
    /// in pages of at most `DAYS_PAGE` after the day `after`; an empty page ends it. The peer
    /// then sends us, newest first, every op of a day we lack (docs/chat.md section 10).
    Days { after: Option<String>, days: Vec<(String, Vec<u8>)> },
    /// Instead of `Auth`, from a contact that blocks us: its DID-signed block notice
    /// (`proto::sign_block_notice`); the connection ends after it.
    Notice {
        notice: proto::SignedBlockNotice,
    },
    /// chat/6: recipient-signed delivered/read receipt exchange (`docs/chat.md` "Receipts").
    /// The receipt-ledger version vector we hold per day of this conversation.
    ReceiptHello { days: Vec<(String, Vec<u8>)> },
    /// Native receipt-ledger ops of one day the peer lacks: original DID-signed statements,
    /// never re-signed by a relay. Admitted only from this conversation's two DIDs.
    Receipts { day: String, update: Vec<u8> },
    /// "I now hold this receipt-ledger version of `day`".
    ReceiptAck { day: String, vv: Vec<u8> },
}

use cryptography::domains::CHAT_BATCH;
use proto::signed::{SecretKey, Signer, sign_detached, verify_detached};

/// The author's attested device key signs every update batch (detached, over `[doc, update]`).
pub fn sign_batch(device_secret: &[u8; 32], doc: &str, update: &[u8]) -> Vec<u8> {
    sign_detached(
        &SecretKey(device_secret),
        CHAT_BATCH,
        &[doc.as_bytes(), update],
    )
    .to_vec()
}

pub fn verify_batch(device: &[u8; 32], doc: &str, update: &[u8], sig: &[u8]) -> bool {
    let Ok(sig) = <[u8; 64]>::try_from(sig) else {
        return false;
    };
    verify_detached(
        Signer::Key(device),
        CHAT_BATCH,
        &[doc.as_bytes(), update],
        &sig,
    )
    .is_ok()
}

#[cfg(test)]
mod tests;
