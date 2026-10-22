//! u32-BE-length + postcard (`cryptography::codec`) frames on one bidirectional QUIC stream, for `tinline/self/5`.

use iroh::endpoint::{RecvStream, SendStream};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::Error;
use node_runtime::frames::{FrameBuf, send_frame};

pub const MAX_FRAME: usize = 16 * 1024 * 1024;

pub struct FrameWriter(pub SendStream);

pub struct FrameReader(FrameBuf);

impl FrameReader {
    pub fn new(recv: RecvStream) -> Self {
        Self(FrameBuf::new(recv))
    }

    /// `None` when the peer finished its side cleanly. Cancel-safe.
    pub async fn recv<T: DeserializeOwned>(&mut self) -> Result<Option<T>, Error> {
        self.0.recv(MAX_FRAME, "").await
    }
}

impl FrameWriter {
    pub async fn send<T: Serialize>(&mut self, msg: &T) -> Result<(), Error> {
        send_frame(&mut self.0, msg, MAX_FRAME, "").await
    }

    pub fn finish(&mut self) {
        let _ = self.0.finish();
    }
}

/// Messages of `tinline/self/3`. Both sides send `Auth` first (dialer, then acceptor).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SelfMsg {
    /// `unlink`: the dialer is only here to say `Unlinked` (the acceptor skips session setup).
    Auth { attestation: proto::SignedAttestation, relay: Option<String>, unlink: bool },
    /// The docs we hold, each with its version vector (`VersionVector::encode`).
    Hello { docs: Vec<(String, Vec<u8>)> },
    /// "Here is my version and the ops of mine you lack"; also the live push. Signed by the
    /// sender's device key (`chat::wire::sign_batch`).
    Sync { doc: String, vv: Vec<u8>, update: Vec<u8>, sig: Vec<u8> },
    /// "You were removed from this account." Only ever sent after the receiver accepted our
    /// `Auth`, i.e. by an own device it does not consider removed.
    Unlinked,
    /// Read cursors `(pair, unix ms)` of conversations: everything incoming at or before is read.
    /// A max-register per pair (34g); unsigned, the link itself is authenticated.
    Read { cursors: Vec<(String, i64)> },
    /// What the contact confirmed holding, `(doc, VersionVector::encode)`: a device that is not
    /// talking to the contact itself still shows two ticks (a max-register per doc, merged).
    Acks { acks: Vec<(String, Vec<u8>)> },
    /// The groups this account is in, as seen by the sender (G2): per group a `codec`-encoded
    /// `group::record::InviteToken` without invite (group, name, owner, DID-signed host
    /// addresses). The receiver adds groups it lacks and dials their hosts (`group/own.rs`).
    Groups { groups: Vec<Vec<u8>> },
    /// Group invitations this account sent or got (G3), as JSON rows
    /// (`group::invitations::InvRow`). The receiver adds rows it lacks and takes an answer
    /// (Joined / Declined / ...) over Pending.
    Invitations { rows: Vec<Vec<u8>> },
    // Appended discriminants; sent only after cap/group-history/v1 negotiation.
    GroupHistoryGet { request: u64, group: String, after: Option<String> },
    GroupHistoryPage { request: u64, group: String, after: Option<String>, days: Vec<(String, Vec<u8>)>, next: Option<String>, deferred: bool },
    GroupWant { request: u64, doc: String, vv: Vec<u8> },
    GroupBatch { request: u64, doc: String, vv: Vec<u8>, update: Vec<u8>, sig: Vec<u8>, more: bool, deferred: bool },
    GroupHistoryChanged { group: String },
    // Appended; only after cap/group-receipts/v1 (and both content/history markers).
    GroupReceiptGet { request: u64, group: String, after: Option<String> },
    GroupReceiptPage { request: u64, group: String, after: Option<String>, proofs: Vec<(String, Vec<u8>)>, next: Option<String> },
}
