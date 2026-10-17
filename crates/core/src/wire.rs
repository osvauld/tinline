//! A control stream carrying `proto::Msg` frames, one per call or contact exchange, and the
//! one-shot deliveries built on it (tokens, block notices, device lists, group letters, welcomes).

use std::time::Duration;

use iroh::EndpointAddr;
use iroh::endpoint::{Connection, RecvStream, SendStream};
use proto::Msg;

use crate::Error;

/// How long a dial may take before the attempt counts as failed.
pub(crate) const DIAL_TIMEOUT: Duration = Duration::from_secs(20);
/// How long a delivery waits for the other side's reply.
pub(crate) const REPLY_TIMEOUT: Duration = Duration::from_secs(10);
/// Pause before each further delivery attempt (seconds; the last repeats).
pub(crate) const RETRY_SECS: [u64; 5] = [2, 5, 15, 30, 60];

/// A chat or own-device session with no stream traffic either way for this long closes
/// cleanly, unless something keeps it open (a watched conversation, pending work, a live call).
/// Decided 2026-10-17: peers do not hold connections open; whoever needs the other redials.
pub(crate) const IDLE_CLOSE: Duration = Duration::from_secs(60);

/// [`IDLE_CLOSE`], or `P2P_IDLE_CLOSE_MS` (a test knob, like `P2P_SELF_SYNC_SECS`).
pub(crate) fn idle_close() -> Duration {
    std::env::var("P2P_IDLE_CLOSE_MS").ok().and_then(|v| v.parse().ok()).map(Duration::from_millis).unwrap_or(IDLE_CLOSE)
}

/// How often a session checks whether it has been idle long enough to close.
pub(crate) fn idle_check() -> Duration {
    (idle_close() / 6).clamp(Duration::from_millis(50), Duration::from_secs(10))
}

/// The pause before retry `round` (0-based) of a schedule like [`RETRY_SECS`].
pub(crate) fn retry_pause(secs: &[u64], round: usize) -> Duration {
    Duration::from_secs(secs[round.min(secs.len() - 1)])
}

/// Dials on the call ALPN (version-checked), within [`DIAL_TIMEOUT`].
pub(crate) async fn dial_call(ep: &iroh::Endpoint, addr: EndpointAddr) -> Result<Connection, Error> {
    tokio::time::timeout(DIAL_TIMEOUT, crate::version::connect(ep, addr, &crate::version::CALL)).await.map_err(|_| Error::Timeout)?
}

/// One delivery: sends `msg` on a fresh stream, waits up to [`REPLY_TIMEOUT`] for the reply and
/// closes the connection. `None`: they finished without replying.
pub(crate) async fn exchange(conn: Connection, msg: &Msg) -> Result<Option<Msg>, Error> {
    let (send, recv) = conn.open_bi().await.map_err(Error::net)?;
    let mut ctrl = Ctrl::new(send, recv);
    ctrl.send(msg).await?;
    let reply = tokio::time::timeout(REPLY_TIMEOUT, ctrl.recv()).await.map_err(|_| Error::Timeout)??;
    ctrl.finish();
    conn.close(0u32.into(), b"bye");
    Ok(reply)
}

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
