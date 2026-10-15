//! u32-BE-length + postcard (`cryptography::codec`) frames on one bidirectional QUIC stream, for `tinline/self/5`.

use iroh::endpoint::{RecvStream, SendStream};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::Error;

pub const MAX_FRAME: usize = 16 * 1024 * 1024;

pub struct FrameWriter(pub SendStream);

pub struct FrameReader {
    recv: RecvStream,
    buf: Vec<u8>,
}

impl FrameReader {
    pub fn new(recv: RecvStream) -> Self {
        Self { recv, buf: Vec::new() }
    }

    /// `None` when the peer finished its side cleanly. Cancel-safe.
    pub async fn recv<T: DeserializeOwned>(&mut self) -> Result<Option<T>, Error> {
        let mut chunk = vec![0u8; 64 * 1024];
        loop {
            if self.buf.len() >= 4 {
                let len = u32::from_be_bytes(self.buf[..4].try_into().unwrap()) as usize;
                if len > MAX_FRAME {
                    return Err(Error::Protocol("frame too large".into()));
                }
                if self.buf.len() >= 4 + len {
                    let msg = cryptography::codec::from_bytes(&self.buf[4..4 + len], MAX_FRAME).map_err(|_| Error::Protocol("frame does not decode".into()))?;
                    self.buf.drain(..4 + len);
                    return Ok(Some(msg));
                }
            }
            match self.recv.read(&mut chunk).await.map_err(Error::net)? {
                Some(n) => self.buf.extend_from_slice(&chunk[..n]),
                None if self.buf.is_empty() => return Ok(None),
                None => return Err(Error::Protocol("stream ended mid-frame".into())),
            }
        }
    }
}

impl FrameWriter {
    pub async fn send<T: Serialize>(&mut self, msg: &T) -> Result<(), Error> {
        let body = cryptography::codec::to_bytes(msg).map_err(|e| Error::Protocol(e.to_string()))?;
        if body.len() > MAX_FRAME {
            return Err(Error::Protocol("frame too large".into()));
        }
        let mut out = (body.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(&body);
        self.0.write_all(&out).await.map_err(Error::net)
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
