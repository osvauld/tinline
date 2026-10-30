//! The framing of `tinline/chat/8` over one iroh bidirectional stream: the messages and the batch
//! signature are `chat_core::wire`, re-exported here.

use iroh::endpoint::{RecvStream, SendStream};

use crate::Error;
use node_runtime::frames::{FIRST_FRAME, FrameBuf, send_frame};

pub use chat_core::wire::*;

pub struct ChatWriter(pub SendStream);
pub struct ChatReader(FrameBuf);

impl ChatReader {
    pub fn new(recv: RecvStream) -> Self {
        Self(FrameBuf::new(recv))
    }

    /// `None` when the peer finished its side cleanly. Cancel-safe.
    pub async fn recv(&mut self) -> Result<Option<ChatMsg>, Error> {
        self.0.recv(MAX_FRAME, "chat ").await
    }

    /// The session's first frame (`Auth`, or a `Notice`), before the peer is authenticated:
    /// capped at `FIRST_FRAME` rather than `MAX_FRAME`.
    pub async fn recv_first(&mut self) -> Result<Option<ChatMsg>, Error> {
        self.0.recv(FIRST_FRAME, "chat ").await
    }
}

impl ChatWriter {
    pub async fn send(&mut self, msg: &ChatMsg) -> Result<(), Error> {
        send_frame(&mut self.0, msg, MAX_FRAME, "chat ").await
    }
}
