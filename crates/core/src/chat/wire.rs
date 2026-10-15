//! Frames of `tinline/chat/7`: u32 BE length + postcard (`cryptography::codec`) of `ChatMsg`, on one bidirectional QUIC
//! stream opened by the dialer. Both sides send `Auth` first, then everything is symmetric.
//! Version 3 (P10: one version per writer in each record; P6: `Auth` presents a message token,
//! separate from the call token) carries the strict DID-signed chat docs; `tinline/chat/1` (apps
//! before P9c) and `tinline/chat/2` (before P10/P6) are refused clearly (`crate::version`).

use iroh::endpoint::{RecvStream, SendStream};
use serde::{Deserialize, Serialize};

use crate::Error;

pub const CHAT_ALPN: &[u8] = b"tinline/chat/7";
/// A shard snapshot of a very busy day can be large; blobs carry the rest.
pub const MAX_FRAME: usize = 16 * 1024 * 1024;

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
    /// `ChatHistory`: days older than `before` that the peer holds, newest first.
    HistoryReq { before: String, limit: u32 },
    HistoryResp { days: Vec<HistDay> },
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
                    let msg = cryptography::codec::from_bytes(&self.buf[4..4 + len], MAX_FRAME)
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
        let body = cryptography::codec::to_bytes(msg).map_err(|e| Error::Protocol(e.to_string()))?;
        if body.len() > MAX_FRAME {
            return Err(Error::Protocol("chat frame too large".into()));
        }
        let mut out = (body.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(&body);
        self.0.write_all(&out).await.map_err(Error::net)
    }
}

#[cfg(test)]
mod tests;
