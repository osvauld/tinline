//! A control stream carrying `proto::Msg` frames, one per call or contact exchange.

use iroh::endpoint::{RecvStream, SendStream};
use proto::Msg;

use crate::Error;

pub struct Ctrl {
    send: SendStream,
    recv: RecvStream,
    buf: Vec<u8>,
}

impl Ctrl {
    pub fn new(send: SendStream, recv: RecvStream) -> Self {
        Self { send, recv, buf: Vec::new() }
    }

    pub async fn send(&mut self, msg: &Msg) -> Result<(), Error> {
        self.send.write_all(&proto::encode_frame(msg)).await.map_err(Error::net)
    }

    /// `None` once the peer finished its side; a half-frame at that point is an error.
    pub async fn recv(&mut self) -> Result<Option<Msg>, Error> {
        let mut chunk = [0u8; 4096];
        loop {
            if let Some((msg, used)) = proto::decode_frame(&self.buf)? {
                self.buf.drain(..used);
                return Ok(Some(msg));
            }
            match self.recv.read(&mut chunk).await.map_err(Error::net)? {
                Some(n) => self.buf.extend_from_slice(&chunk[..n]),
                None if self.buf.is_empty() => return Ok(None),
                None => return Err(Error::Protocol("stream ended mid-frame".into())),
            }
        }
    }

    pub fn finish(&mut self) {
        let _ = self.send.finish();
    }
}
