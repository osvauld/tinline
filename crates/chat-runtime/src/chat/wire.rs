//! The framing of `tinline/chat/7` over one iroh bidirectional stream: the messages and the batch
//! signature are `chat_core::wire`, re-exported here.

use iroh::endpoint::{RecvStream, SendStream};

use crate::Error;
use node_runtime::frames::{FrameBuf, send_frame};

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
}

impl ChatWriter {
    pub async fn send(&mut self, msg: &ChatMsg) -> Result<(), Error> {
        send_frame(&mut self.0, msg, MAX_FRAME, "chat ").await
    }
}
