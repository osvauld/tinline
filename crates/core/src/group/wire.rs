//! Frames of `tinline/group/1` on the group endpoint (U2): u32 BE length + bincode of `GroupMsg`
//! on one bidirectional stream opened by the dialer. Dialers are members (or hosts acting as
//! members towards another host); the acceptor is always a host (U7).
//!
//! ```text
//! member -> host  Hello { auth, group, vv, invite?, hosts }
//! host -> member  Welcome { auth, vv, hosts }        | Removed { records, vv } | Refused { reason }
//! both            Update { update } / Ack { vv } / Hosts { hosts }
//! ```

use iroh::endpoint::{RecvStream, SendStream};
use serde::{Deserialize, Serialize};

use super::record::{EndpointAuth, SignedHostAddr, SignedInvite};
use crate::Error;

pub const GROUP_ALPN: &[u8] = b"tinline/group/1";
pub const MAX_FRAME: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GroupMsg {
    Hello { auth: EndpointAuth, group: String, vv: Vec<u8>, invite: Option<SignedInvite>, hosts: Vec<SignedHostAddr> },
    Welcome { auth: EndpointAuth, vv: Vec<u8>, hosts: Vec<SignedHostAddr> },
    /// The member is removed: the signed records that prove it (genesis, the owner's admin
    /// grants of the flaggers, the flags) and what the host holds, so the member can still upload
    /// posts it wrote before the removal (U11). Nothing else follows from the host.
    Removed { records: Vec<Vec<u8>>, vv: Vec<u8> },
    Refused { reason: String },
    Update { update: Vec<u8> },
    Ack { vv: Vec<u8> },
    Hosts { hosts: Vec<SignedHostAddr> },
}

pub struct Writer(pub SendStream);
pub struct Reader {
    recv: RecvStream,
    buf: Vec<u8>,
}

impl Reader {
    pub fn new(recv: RecvStream) -> Self {
        Self { recv, buf: Vec::new() }
    }

    /// `None` when the peer finished its side cleanly. Cancel-safe.
    pub async fn recv(&mut self) -> Result<Option<GroupMsg>, Error> {
        let mut chunk = vec![0u8; 64 * 1024];
        loop {
            if self.buf.len() >= 4 {
                let len = u32::from_be_bytes(self.buf[..4].try_into().unwrap()) as usize;
                if len > MAX_FRAME {
                    return Err(Error::Protocol("group frame too large".into()));
                }
                if self.buf.len() >= 4 + len {
                    let msg = bincode::deserialize(&self.buf[4..4 + len]).map_err(|_| Error::Protocol("group frame does not decode".into()))?;
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

impl Writer {
    pub async fn send(&mut self, msg: &GroupMsg) -> Result<(), Error> {
        let body = bincode::serialize(msg).map_err(|e| Error::Protocol(e.to_string()))?;
        if body.len() > MAX_FRAME {
            return Err(Error::Protocol("group frame too large".into()));
        }
        let mut out = (body.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(&body);
        self.0.write_all(&out).await.map_err(Error::net)
    }
}
