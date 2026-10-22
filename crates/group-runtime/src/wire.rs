//! Frames of `tinline/group/5` on the group endpoint (U2): u32 BE length + postcard (`cryptography::codec`) of `GroupMsg`
//! on one bidirectional stream opened by the dialer. Dialers are members (or hosts acting as
//! members towards another host); the acceptor is always a host (U7).
//!
//! A group has a `meta` doc and one doc per UTC day of posts (`docs/group-shards.md`); `Hello` and
//! `Welcome` carry the meta version vector and those of the last `HELLO_DAYS` days, `Update` and
//! `Ack` name the doc, and the meta doc always goes before a day doc's update.
//!
//! ```text
//! member -> host  Hello { auth, group, vv, days, invite?, hosts, relay, presence }
//! host -> member  Moved { auth, group, hosts }       (own connection, after a rotation)
//! host -> member  Welcome { auth, vv, days, hosts }  | Removed { records, vv, update? } | Refused { reason }
//! both            Update { doc, update } / Ack { doc, vv } / Hosts { hosts } / Mail { mail } / MailAck { id }
//! both            Days { after, days }   (inventory of every day doc held, in pages; an empty page ends it)
//! member -> host  Code { code, kind, invite?, expires } / RevokeCode { code }  -> CodeAck { code, ok, reason }
//! anyone -> host  Fetch { code }  (first frame, G3)   -> Fetched { token } | Refused { reason }
//! both            CallHostStatus / CallStart / CallEnd / CallHostRequest / CallHostRefusal (GA-I1)
//! both            ReceiptHello { days } / Receipts { day, update } / ReceiptAck { day, vv }  (member receipts; none in an open group)
//! host -> both    Presence { presence }  (open groups: a host's signed online list; hosts relay other hosts')
//! ```
//!
//! group/4 = group/3 plus the member receipt frames; group/5 = group/4 plus open links (`Code`
//! carries a `CodeKind`, `InviteToken` an `open` flag; `docs/design/public-group.md` §2), the
//! online list (`Hello.presence`, `Presence`, §3) and no receipts in open groups (§4). No
//! compatibility with older peers: the ALPN differs, so an old peer fails at connect instead of
//! mid-session.

use iroh::endpoint::{RecvStream, SendStream};
use serde::{Deserialize, Serialize};

use super::doc::DocId;
use super::code::CodeKind;
use super::record::{EndpointAuth, InviteToken, SignedHostAddr, SignedInvite, SignedPresence};
use crate::Error;
use node_runtime::frames::{FrameBuf, send_frame};

pub const GROUP_ALPN: &[u8] = b"tinline/group/5";
pub const MAX_FRAME: usize = 16 * 1024 * 1024;
/// `Hello` and `Welcome` list the day docs of this many recent days (as 1:1 chat does).
pub const HELLO_DAYS: i64 = 7;

/// Most days in one `Days` page.
pub const DAYS_PAGE: usize = 64;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GroupMsg {
    /// `relay`: the member's home relay, so a host that rotated can reach it again (`docs/rotation.md`).
    /// `vv`: the meta doc's version vector; `days`: those of the day docs of the last `HELLO_DAYS`
    /// days we hold. `presence`: the account shows as online in open groups (public group §3;
    /// a host never lists a member whose session said `false`).
    Hello { auth: EndpointAuth, group: String, vv: Vec<u8>, days: Vec<(String, Vec<u8>)>, invite: Option<SignedInvite>, hosts: Vec<SignedHostAddr>, relay: Option<String>, presence: bool },
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
    /// rotated its own (`docs/rotation.md`): "this is me now" (`auth` proves the new endpoint) plus the group's
    /// DID-signed host addresses. The member checks the sender is an active host, keeps the
    /// addresses and dials. Only current members are told; a removed member never is.
    Moved { auth: EndpointAuth, group: String, hosts: Vec<SignedHostAddr> },
    /// G3, anyone -> host, as the first frame of a connection (instead of `Hello`): "give me the
    /// invite behind this short code" (`code.rs`). Answered `Fetched` or `Refused`; the dialer
    /// may try again on the same connection, and the host closes it after a few unknown codes.
    Fetch { code: [u8; 12] },
    /// Host -> fetcher: the full invite token (the signed invite, absent for a device code and an
    /// open link, which sets `open`; the group, its name and owner; the hosts' DID-signed
    /// addresses, this host's among them).
    Fetched { token: InviteToken },
    /// G3, member -> host inside a session: keep `code` until `expires` (unix ms). `kind`
    /// `Invite`: `invite` is ours, signed by us as an admin; `Device`: no invite, admits only our
    /// own DID; `Open` (admins, group open): no invite, never expires, dropped when it closes.
    Code { code: [u8; 12], kind: CodeKind, invite: Option<SignedInvite>, expires: u64 },
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
    CallHostStatus {
        status: proto::gcall_status::SignedStatus,
    },
    /// A call began (or a backup took it over): the host's signed CallStart and the
    /// DID-signed group addresses of it and its backups (G1: what members dial).
    CallStart {
        start: proto::gcall::SignedStart,
        hosts: Vec<SignedHostAddr>,
    },
    /// The call ended (`End{ended}`, signed by its host).
    CallEnd {
        end: proto::gcall::SignedEnd,
    },
    /// Starter -> one call-host candidate (`proto::gcall::HostRequest`).
    CallHostRequest {
        request: proto::gcall::SignedHostRequest,
    },
    /// Candidate -> starter: declined (`proto::gcall::HostRefusal`). Accepting is a CallStart.
    CallHostRefusal {
        refusal: proto::gcall::SignedHostRefusal,
    },
    // ---- member message receipts (`engine::member_receipts`, as chat/6 does for 1:1) --------
    /// Both, after the session's catch-up: our receipt-ledger version vector per day (recent
    /// days we hold, plus days with statements of ours no host acknowledged yet). The other
    /// side answers with `Receipts` for what we lack.
    ReceiptHello { days: Vec<(String, Vec<u8>)> },
    /// Original native receipt-ledger ops of one day (bounded; every statement keeps its
    /// signer's exact signed bytes and writer, whoever relays it). Pushed live too.
    Receipts { day: String, update: Vec<u8> },
    /// What we hold of that day's ledger after the import (authoritative for the sender; a
    /// refused frame is sent again on the next trigger).
    ReceiptAck { day: String, vv: Vec<u8> },
    // ---- presence (public group §3, open groups only; `engine::presence`) ---------------------
    /// A host's DID-signed list of the members it sees online (at most `MAX_PRESENCE`), every
    /// 15 s and after a session starts or ends, to its member sessions and the other hosts. A host
    /// relays other hosts' frames as they are (newest `seq` per host device; an older or equal
    /// one is dropped, which ends relay loops). Accepted only from an active host. Never stored.
    Presence { presence: SignedPresence },
}

pub struct Writer(pub SendStream);
pub struct Reader(FrameBuf);

impl Reader {
    pub fn new(recv: RecvStream) -> Self {
        Self(FrameBuf::new(recv))
    }

    /// `None` when the peer finished its side cleanly. Cancel-safe.
    pub async fn recv(&mut self) -> Result<Option<GroupMsg>, Error> {
        self.0.recv(MAX_FRAME, "group ").await
    }
}

impl Writer {
    pub async fn send(&mut self, msg: &GroupMsg) -> Result<(), Error> {
        send_frame(&mut self.0, msg, MAX_FRAME, "group ").await
    }
}
