//! u32-BE-length + postcard (`cryptography::codec`) frames on a QUIC stream, shared by the chat,
//! group and own-device sync wires. Each wire passes its own size limit and an error-message
//! label (`"chat "`, `"group "`, `""`), so its errors read as before.

use iroh::endpoint::{RecvStream, SendStream};
use serde::{Serialize, de::DeserializeOwned};

use crate::Error;

/// The receiving half: buffers stream bytes and yields whole frames.
pub struct FrameBuf {
    recv: RecvStream,
    buf: Vec<u8>,
}

impl FrameBuf {
    pub fn new(recv: RecvStream) -> Self {
        Self { recv, buf: Vec::new() }
    }

    /// `None` when the peer finished its side cleanly. Cancel-safe.
    pub async fn recv<T: DeserializeOwned>(&mut self, limit: usize, label: &str) -> Result<Option<T>, Error> {
        let mut chunk = vec![0u8; 64 * 1024];
        loop {
            if let Some(head) = self.buf.first_chunk::<4>() {
                let len = u32::from_be_bytes(*head) as usize;
                if len > limit {
                    return Err(Error::Protocol(format!("{label}frame too large")));
                }
                if self.buf.len() >= 4 + len {
                    let msg = cryptography::codec::from_bytes(&self.buf[4..4 + len], limit).map_err(|_| Error::Protocol(format!("{label}frame does not decode")))?;
                    self.buf.drain(..4 + len);
                    return Ok(Some(msg));
                }
            }
            match self.recv.read(&mut chunk).await.map_err(Error::net)? {
                Some(n) => self.buf.extend_from_slice(&chunk[..n]),
                None if self.buf.is_empty() => return Ok(None),
                None => return Err(Error::eof("stream ended mid-frame")),
            }
        }
    }
}

/// Writes one frame (length prefix + body in a single `write_all`).
pub async fn send_frame<T: Serialize>(stream: &mut SendStream, msg: &T, limit: usize, label: &str) -> Result<(), Error> {
    let body = cryptography::codec::to_bytes(msg)?;
    if body.len() > limit {
        return Err(Error::Protocol(format!("{label}frame too large")));
    }
    let mut out = (body.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(&body);
    stream.write_all(&out).await.map_err(Error::net)
}
