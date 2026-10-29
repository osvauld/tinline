//! u32-BE-length + postcard (`cryptography::codec`) frames on a QUIC stream, shared by the chat,
//! group and own-device sync wires. Each wire passes its own size limit and an error-message
//! label (`"chat "`, `"group "`, `""`), so its errors read as before.

use iroh::endpoint::{RecvStream, SendStream};
use serde::{Serialize, de::DeserializeOwned};

use crate::Error;

/// The most a session's first frame may claim, before the peer has proved anything: the chat
/// `Auth` and the own-device `Auth` (an attestation, a token, a relay URL: a few hundred bytes)
/// fit many times over. The same bound as the control stream's `proto::MAX_FRAME`. A frame
/// header above it ends the connection before the body is read, so a stranger cannot make a
/// device buffer its wire's full frame size (16 MiB) per slot.
pub const FIRST_FRAME: usize = proto::MAX_FRAME;

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

#[cfg(test)]
mod tests {
    use iroh::Endpoint;
    use iroh::address_lookup::memory::MemoryLookup;
    use iroh::endpoint::presets;

    use super::*;

    const ALPN: &[u8] = b"tinline/test-frames/1";

    /// A frame header over the limit ends the read at once: the body is never waited for or
    /// buffered (a stranger cannot make us hold `limit` bytes it never sends).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn frames_header_over_limit_is_refused_before_the_body() {
        let lookup = MemoryLookup::new();
        let bind = || async {
            let ep = Endpoint::builder(presets::Minimal).alpns(vec![ALPN.to_vec()]).address_lookup(lookup.clone()).bind().await.unwrap();
            lookup.add_endpoint_info(ep.addr());
            ep
        };
        let (a, b) = (bind().await, bind().await);
        let b_addr = b.addr();
        let server = tokio::spawn(async move {
            let conn = b.accept().await.unwrap().await.unwrap();
            let (_send, recv) = conn.accept_bi().await.unwrap();
            let mut fb = FrameBuf::new(recv);
            let r = tokio::time::timeout(std::time::Duration::from_secs(5), fb.recv::<u8>(FIRST_FRAME, "t ")).await;
            (r, conn)
        });
        let conn = a.connect(b_addr, ALPN).await.unwrap();
        let (mut send, _recv) = conn.open_bi().await.unwrap();
        // Only the header: a claimed body one byte over the cap, never sent.
        send.write_all(&((FIRST_FRAME as u32 + 1).to_be_bytes())).await.unwrap();
        let (r, _c) = server.await.unwrap();
        let err = r.expect("refused at once, not after a timeout").expect_err("over the limit");
        assert!(err.to_string().contains("frame too large"), "{err}");
    }

    #[test]
    fn frames_first_frame_is_the_control_bound() {
        assert_eq!(FIRST_FRAME, proto::MAX_FRAME);
        assert!(FIRST_FRAME >= 16 * 1024, "an Auth must fit");
    }
}
