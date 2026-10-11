//! Frames of `tinline/group/1` on the group endpoint (U2): u32 BE length + bincode of `GroupMsg`
//! on one bidirectional stream opened by the dialer. Dialers are members (or hosts acting as
//! members towards another host); the acceptor is always a host (U7).
//!
//! ```text
//! member -> host  Hello { auth, group, vv, invite?, hosts, relay }
//! host -> member  Moved { auth, group, hosts }       (own connection, after a rotation, U12)
//! host -> member  Welcome { auth, vv, hosts }        | Removed { records, vv, update? } | Refused { reason }
//! both            Update { update } / Ack { vv } / Hosts { hosts } / Mail { mail } / MailAck { id }
//! both            CallHostStatus / CallStart / CallEnd / CallHostRequest / CallHostRefusal (GA-I1)
//! ```

use iroh::endpoint::{RecvStream, SendStream};
use serde::{Deserialize, Serialize};

use super::record::{EndpointAuth, SignedHostAddr, SignedInvite};
use crate::Error;

pub const GROUP_ALPN: &[u8] = b"tinline/group/1";
pub const MAX_FRAME: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GroupMsg {
    /// `relay`: the member's home relay, so a host that rotated can reach it again (U12).
    Hello { auth: EndpointAuth, group: String, vv: Vec<u8>, invite: Option<SignedInvite>, hosts: Vec<SignedHostAddr>, relay: Option<String> },
    Welcome { auth: EndpointAuth, vv: Vec<u8>, hosts: Vec<SignedHostAddr> },
    /// The member is removed: the signed records that prove it (genesis, the owner's admin
    /// grants of the flaggers, the flags) and what the host holds, so the member can still upload
    /// posts it wrote before the removal (U11). Nothing else follows from the host.
    /// `update`: what the member lacks of the causal past of the flags that removed it (so its
    /// own replica derives the removal from verified records), never anything after.
    Removed { records: Vec<Vec<u8>>, vv: Vec<u8>, update: Option<Vec<u8>> },
    Refused { reason: String },
    Update { update: Vec<u8> },
    Ack { vv: Vec<u8> },
    Hosts { hosts: Vec<SignedHostAddr> },
    /// Sealed mail between two members (U3, `mail.rs`): member -> host, host -> recipient
    /// member, host <-> host. The host can check the envelope, not read it.
    Mail { mail: Vec<u8> },
    /// Host -> sending member: the host holds that mail (the sender drops it from its outbox).
    MailAck { id: [u8; 16] },
    /// Host -> member, on a connection the host opens to the member's group endpoint after it
    /// rotated its own (U12): "this is me now" (`auth` proves the new endpoint) plus the group's
    /// DID-signed host addresses. The member checks the sender is an active host, keeps the
    /// addresses and dials. Only current members are told; a removed member never is.
    Moved { auth: EndpointAuth, group: String, hosts: Vec<SignedHostAddr> },
    // ---- group calls (GA-I1, G6 §2.2, §2.3, §3.1). Members send these only to hosts; a host
    // verifies each (signature, group, membership, call-host permit) and passes it on: the
    // broadcast ones to every current member session (never to a removed or joining one,
    // §9.3) and to other hosts, the addressed ones towards their one recipient. Every device
    // drops a blob it has seen (signature), so host-to-host relay ends.
    /// A call-host candidate's signed conditions (`proto::gcall_status`), every 30 s.
    CallHostStatus { status: proto::SignedBlob },
    /// A call began (or a backup took it over): the host's signed CallStart and the
    /// DID-signed group addresses of it and its backups (G1: what members dial).
    CallStart { start: proto::SignedBlob, hosts: Vec<SignedHostAddr> },
    /// The call ended (`End{ended}`, signed by its host).
    CallEnd { end: proto::SignedBlob },
    /// Starter -> one call-host candidate (`proto::gcall::HostRequest`).
    CallHostRequest { request: proto::SignedBlob },
    /// Candidate -> starter: declined (`proto::gcall::HostRefusal`). Accepting is a CallStart.
    CallHostRefusal { refusal: proto::SignedBlob },
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
