//! Frames of `tinline/group/2` on the group endpoint (U2): u32 BE length + bincode of `GroupMsg`
//! on one bidirectional stream opened by the dialer. Dialers are members (or hosts acting as
//! members towards another host); the acceptor is always a host (U7).
//!
//! A group has a `meta` doc and one doc per UTC day of posts (`docs/group-shards.md`); `Hello` and
//! `Welcome` carry the meta version vector and those of the last `HELLO_DAYS` days, `Update` and
//! `Ack` name the doc, and the meta doc always goes before a day doc's update.
//!
//! ```text
//! member -> host  Hello { auth, group, vv, days, invite?, hosts, relay }
//! host -> member  Moved { auth, group, hosts }       (own connection, after a rotation, U12)
//! host -> member  Welcome { auth, vv, days, hosts }  | Removed { records, vv, update? } | Refused { reason }
//! both            Update { doc, update } / Ack { doc, vv } / Hosts { hosts } / Mail { mail } / MailAck { id }
//! both            Days { after, days }   (inventory of every day doc held, in pages; an empty page ends it)
//! member -> host  Code { code, invite?, expires } / RevokeCode { code }  -> CodeAck { code, ok, reason }
//! anyone -> host  Fetch { code }  (first frame, G3)   -> Fetched { token } | Refused { reason }
//! both            CallHostStatus / CallStart / CallEnd / CallHostRequest / CallHostRefusal (GA-I1)
//! ```

use iroh::endpoint::{RecvStream, SendStream};
use serde::{Deserialize, Serialize};

use super::doc::DocId;
use super::record::{EndpointAuth, InviteToken, SignedHostAddr, SignedInvite};
use crate::Error;

pub const GROUP_ALPN: &[u8] = b"tinline/group/2";
pub const MAX_FRAME: usize = 16 * 1024 * 1024;
/// `Hello` and `Welcome` list the day docs of this many recent days (as 1:1 chat does).
pub const HELLO_DAYS: i64 = 7;

/// Most days in one `Days` page.
pub const DAYS_PAGE: usize = 64;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GroupMsg {
    /// `relay`: the member's home relay, so a host that rotated can reach it again (U12).
    /// `vv`: the meta doc's version vector; `days`: those of the day docs of the last `HELLO_DAYS`
    /// days we hold.
    Hello { auth: EndpointAuth, group: String, vv: Vec<u8>, days: Vec<(String, Vec<u8>)>, invite: Option<SignedInvite>, hosts: Vec<SignedHostAddr>, relay: Option<String> },
    Welcome { auth: EndpointAuth, vv: Vec<u8>, days: Vec<(String, Vec<u8>)>, hosts: Vec<SignedHostAddr> },
    /// The member is removed: the signed records that prove it (genesis, the owner's admin
    /// grants of the flaggers, the flags) and what the host holds, so the member can still upload
    /// posts it wrote before the removal (U11). Nothing else follows from the host.
    /// `update`: what the member lacks of the causal past of the flags that removed it (so its
    /// own replica derives the removal from verified records), never anything after.
    Removed { records: Vec<Vec<u8>>, vv: Vec<u8>, update: Option<Vec<u8>> },
    Refused { reason: String },
    /// A Loro export of what the other side lacks of one doc. The meta doc goes before any day
    /// doc's update, in every session and every relay.
    Update { doc: DocId, update: Vec<u8> },
    Ack { doc: DocId, vv: Vec<u8> },
    /// Either side, any session: the day docs we hold, `(day, encoded vv)` ascending, the page
    /// after day `after` (`None`: the first). An empty page ends the inventory. Each side then
    /// sends `Update` for every doc the other lacks or is behind on (meta first, then the days
    /// newest first, a few a second): the older history is that sync running until both
    /// inventories match.
    Days { after: Option<String>, days: Vec<(String, Vec<u8>)> },
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
    /// G3, anyone -> host, as the first frame of a connection (instead of `Hello`): "give me the
    /// invite behind this short code" (`code.rs`). Answered `Fetched` or `Refused`; the dialer
    /// may try again on the same connection, and the host closes it after a few unknown codes.
    Fetch { code: [u8; 12] },
    /// Host -> fetcher: the full invite token (the signed invite, absent for a device code; the
    /// group, its name and owner; the hosts' DID-signed addresses, this host's among them).
    Fetched { token: InviteToken },
    /// G3, member -> host inside a session: keep `invite` (ours, signed by us as an admin; `None`
    /// for a device code, which admits only our own DID) under `code` until `expires` (unix s).
    Code { code: [u8; 12], invite: Option<SignedInvite>, expires: u64 },
    /// G3, member -> host inside a session: forget `code` (its issuer or an admin).
    RevokeCode { code: [u8; 12] },
    /// Host -> member: the answer to `Code` / `RevokeCode`.
    CodeAck { code: [u8; 12], ok: bool, reason: String },
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
