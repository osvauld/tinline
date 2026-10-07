//! Frames of `tinline/chat/1`: u32 BE length + bincode of `ChatMsg`, on one bidirectional QUIC
//! stream opened by the dialer. Both sides send `Auth` first, then everything is symmetric.

use iroh::endpoint::{RecvStream, SendStream};
use serde::{Deserialize, Serialize};

use crate::Error;

pub const CHAT_ALPN: &[u8] = b"tinline/chat/1";
/// A shard snapshot of a very busy day can be large; blobs carry the rest.
pub const MAX_FRAME: usize = 16 * 1024 * 1024;
const SIG_DOMAIN: &[u8] = b"tinline-chat-batch-v1";

/// One day we can offer history for.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistDay {
    pub day: String,
    pub vv: Vec<u8>,
    /// Ciphertext hash (hex) of an encrypted Loro snapshot of that day.
    pub hash: String,
    /// The snapshot blob's key (hex); sent only inside this authenticated connection.
    pub key: String,
    /// Snapshot blob (ciphertext) size in bytes.
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChatMsg {
    /// The same proof as a call: device attestation + the grant the other side issued to us.
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
    /// `ChatHistory`: days older than `before` that the peer holds, newest first.
    HistoryReq { before: String, limit: u32 },
    HistoryResp { days: Vec<HistDay> },
}

pub fn name(m: &ChatMsg) -> &'static str {
    match m {
        ChatMsg::Auth { .. } => "Auth",
        ChatMsg::Hello { .. } => "Hello",
        ChatMsg::Sync { .. } => "Sync",
        ChatMsg::Push { .. } => "Push",
        ChatMsg::Ack { .. } => "Ack",
        ChatMsg::HistoryReq { .. } => "HistoryReq",
        ChatMsg::HistoryResp { .. } => "HistoryResp",
    }
}

fn sig_input(doc: &str, update: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(SIG_DOMAIN.len() + 4 + doc.len() + update.len());
    v.extend_from_slice(SIG_DOMAIN);
    v.extend_from_slice(&(doc.len() as u32).to_be_bytes());
    v.extend_from_slice(doc.as_bytes());
    v.extend_from_slice(update);
    v
}

/// The author's attested device key signs every update batch.
pub fn sign_batch(device_secret: &[u8; 32], doc: &str, update: &[u8]) -> Vec<u8> {
    cryptography::signature::sign(device_secret, &sig_input(doc, update)).to_vec()
}

pub fn verify_batch(device: &[u8; 32], doc: &str, update: &[u8], sig: &[u8]) -> bool {
    let Ok(sig) = <[u8; 64]>::try_from(sig) else { return false };
    cryptography::signature::verify(device, &sig_input(doc, update), &sig)
}

pub struct ChatWriter(pub SendStream);
pub struct ChatReader {
    recv: RecvStream,
    buf: Vec<u8>,
}

impl ChatReader {
    pub fn new(recv: RecvStream) -> Self {
        Self { recv, buf: Vec::new() }
    }

    /// `None` when the peer finished its side cleanly. Cancel-safe.
    pub async fn recv(&mut self) -> Result<Option<ChatMsg>, Error> {
        let mut chunk = vec![0u8; 64 * 1024];
        loop {
            if self.buf.len() >= 4 {
                let len = u32::from_be_bytes(self.buf[..4].try_into().unwrap()) as usize;
                if len > MAX_FRAME {
                    return Err(Error::Protocol("chat frame too large".into()));
                }
                if self.buf.len() >= 4 + len {
                    let msg = bincode::deserialize(&self.buf[4..4 + len])
                        .map_err(|_| Error::Protocol("chat frame does not decode".into()))?;
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

impl ChatWriter {
    pub async fn send(&mut self, msg: &ChatMsg) -> Result<(), Error> {
        let body = bincode::serialize(msg).map_err(|e| Error::Protocol(e.to_string()))?;
        if body.len() > MAX_FRAME {
            return Err(Error::Protocol("chat frame too large".into()));
        }
        let mut out = (body.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(&body);
        self.0.write_all(&out).await.map_err(Error::net)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_signatures_bind_doc_and_update() {
        let secret = proto::new_device_secret();
        let public = proto::device_public(&secret);
        let sig = sign_batch(&secret, "dm/p/2026-10-07", b"update");
        assert!(verify_batch(&public, "dm/p/2026-10-07", b"update", &sig));
        assert!(!verify_batch(&public, "dm/p/2026-10-08", b"update", &sig));
        assert!(!verify_batch(&public, "dm/p/2026-10-07", b"updatf", &sig));
        let other = proto::device_public(&proto::new_device_secret());
        assert!(!verify_batch(&other, "dm/p/2026-10-07", b"update", &sig));
        assert!(!verify_batch(&public, "x", b"update", &sig[..10]));
    }

    #[test]
    fn frames_encode() {
        let m = ChatMsg::Hello { shards: vec![("2026-10-07".into(), vec![1, 2, 3])] };
        let b = bincode::serialize(&m).unwrap();
        let back: ChatMsg = bincode::deserialize(&b).unwrap();
        assert!(matches!(back, ChatMsg::Hello { shards } if shards[0].1 == vec![1, 2, 3]));
    }
}
