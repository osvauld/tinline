//! U3: 1:1 chat requests between members of a group, through the group host, sealed end to end
//! (`mail.rs`). The host learns that one member wrote to another, never what.
//!
//! - `group_chat_request(group, did)`: we ask; the request waits at the hosts until the
//!   recipient's device comes by (PROVISIONAL: kept 7 days).
//! - The recipient sees it (`group_chat_requests_incoming`, `GroupRequestEvents`), and accepts or
//!   declines. Accepting mints a one-time contact ticket of the recipient's PRIVATE endpoint
//!   (`State::req_tickets`) and seals it to the requester: the private endpoint is revealed only
//!   then, and only to the requester. The requester's app adds the recipient from it with
//!   `ContactOrigin::GroupRequest` (P6: message token only, no calls); the recipient's node takes
//!   that hello with the same origin. From then on the two chat directly.
//! - Declined: the requester sees "declined" (PROVISIONAL, from the design).
//! - A request from a DID the recipient blocks is answered automatically with `Decline{blocked}`
//!   and never shown (P5: refusal with a notice); the requester sees `Blocked`.
//! - We refuse locally to ask someone we block, someone who blocks us, or an existing contact.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use storage::Op;

use super::engine::GroupCore;
use super::store::{GroupStore, io, json};
use super::mail::{self, Letter, SignedMail};
use crate::Error;
use crate::node::{Inner, Node, now, now_ms};

/// How long the one-time ticket in an acceptance stays good (PROVISIONAL).
pub const REQ_TICKET_TTL: u64 = 7 * 24 * 3600;
/// Retry period for adding an accepted contact that failed (offline peer).
const ADD_RETRY_MS: u64 = 20_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ReqState {
    Pending,
    Accepted,
    Declined,
    Blocked,
}

/// One request as this device holds it (group store `req/{id}`; local, PROVISIONAL: not synced
/// to own devices).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ReqRow {
    pub req: String,
    pub group: String,
    pub peer: String,
    pub peer_name: String,
    pub outgoing: bool,
    pub state: ReqState,
    pub at: u64,
    /// Outgoing, accepted: the ticket to add them from, until added.
    #[serde(default)]
    pub ticket: Option<String>,
    #[serde(default)]
    pub added: bool,
    #[serde(default)]
    pub next_try: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ChatRequestState {
    /// Waiting for the recipient.
    Pending,
    /// Accepted; for an outgoing one, `contact_added` says whether the contact is in place yet.
    Accepted,
    Declined,
    /// The recipient blocks the requester (P5): refused without being shown to them.
    Blocked,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct GroupChatRequest {
    pub id: String,
    pub group_id: String,
    pub group_name: String,
    pub peer_did: String,
    pub peer_name: String,
    /// We asked (`true`) or were asked.
    pub outgoing: bool,
    pub state: ChatRequestState,
    /// Unix ms.
    pub at: u64,
    /// Outgoing and accepted: the peer is a contact now (message only until "allow calls").
    pub contact_added: bool,
}

/// Registered with `Node::set_group_request_events`. Called on core threads: do not block.
#[uniffi::export(with_foreign)]
pub trait GroupRequestEvents: Send + Sync {
    /// A request appeared or changed state.
    fn on_chat_request(&self, request: GroupChatRequest);
}

fn new_req_id() -> String {
    hex::encode(rand::random::<[u8; 16]>())
}

fn req_bytes(id: &str) -> Option<[u8; 16]> {
    hex::decode(id).ok()?.try_into().ok()
}

impl Inner {
    fn group_req_view(&self, core: &GroupCore, r: &ReqRow) -> GroupChatRequest {
        let group_name = core.groups.lock().get(&r.group).map(|l| l.state.name.clone()).unwrap_or_default();
        GroupChatRequest {
            id: r.req.clone(),
            group_id: r.group.clone(),
            group_name,
            peer_did: r.peer.clone(),
            peer_name: r.peer_name.clone(),
            outgoing: r.outgoing,
            state: match r.state {
                ReqState::Pending => ChatRequestState::Pending,
                ReqState::Accepted => ChatRequestState::Accepted,
                ReqState::Declined => ChatRequestState::Declined,
                ReqState::Blocked => ChatRequestState::Blocked,
            },
            at: r.at,
            contact_added: r.added,
        }
    }

    fn group_req_save(&self, core: &GroupCore, r: &ReqRow) {
        if let Err(e) = core.store.put_req(r) {
            tracing::warn!("chat request: {e}");
        }
        if let Some(ev) = self.group.req_events.lock().clone() {
            let v = self.group_req_view(core, r);
            self.handle.spawn_blocking(move || ev.on_chat_request(v));
        }
    }

    /// Seals `letter` to `to` (with the key `to` published in the group) and sends it.
    fn group_letter(self: &Arc<Self>, core: &Arc<GroupCore>, gid: &str, to: &str, letter: Letter) -> Result<(), Error> {
        let m = self.group_seal(core, gid, to, letter)?;
        self.group_send_mail(core, gid, m);
        Ok(())
    }

    /// Seals `letter` to `to` without sending it. GF: callers record their own row first and
    /// send after, so an answer that comes back fast (an automatic `Decline{blocked}`) never
    /// finds the row missing (it would be dropped, its mail ID already seen).
    fn group_seal(&self, core: &Arc<GroupCore>, gid: &str, to: &str, letter: Letter) -> Result<SignedMail, Error> {
        let key = core
            .groups
            .lock()
            .get(gid)
            .and_then(|l| l.state.enc_keys.get(to).copied())
            .ok_or_else(|| Error::Rejected("their app has not published a key in this group yet".into()))?;
        mail::seal(&core.account, gid, to, &key, letter, now_ms())
    }

    /// Mail addressed to us arrived (from a host, or handed over by our own host side).
    pub(crate) fn group_mail_for_me(self: &Arc<Self>, core: &Arc<GroupCore>, gid: &str, sm: &SignedMail) {
        let Some((m, letter)) = mail::open_for(&core.account, gid, sm) else { return };
        if !core.store.first_seen(&mail::id_hex(&m.id)).unwrap_or(false) {
            return;
        }
        let member = core.groups.lock().get(gid).is_some_and(|l| l.state.is_member(&m.from));
        match letter {
            Letter::Request { req, name } => {
                if !member {
                    return;
                }
                let id = hex::encode(req);
                if core.store.req(&id).ok().flatten().is_some() {
                    return;
                }
                if self.shared.lock().state.blocked.contains(&m.from) {
                    // P5: refused with a notice; never shown here.
                    self.log(format!("group {gid}: refused a chat request from blocked {}", m.from));
                    let _ = self.group_letter(core, gid, &m.from, Letter::Decline { req, blocked: true });
                    return;
                }
                let mut peer_name: String = name.chars().take(100).collect();
                if peer_name.trim().is_empty() {
                    peer_name = core.groups.lock().get(gid).and_then(|l| l.state.members.get(&m.from).map(|x| x.name.clone())).unwrap_or_default();
                }
                let row = ReqRow { req: id, group: gid.to_string(), peer: m.from.clone(), peer_name, outgoing: false, state: ReqState::Pending, at: m.at.min(now_ms()), ticket: None, added: false, next_try: 0 };
                self.group_req_save(core, &row);
            }
            Letter::Accept { req, ticket } => {
                let Some(mut row) = core.store.req(&hex::encode(req)).ok().flatten() else { return };
                if !row.outgoing || row.peer != m.from || row.group != gid || row.state != ReqState::Pending {
                    return;
                }
                let ok = proto::ContactTicket::from_text(&ticket).ok().and_then(|t| t.verify(now()).ok()).is_some_and(|c| c.iss == m.from);
                if !ok {
                    self.log(format!("group {gid}: an acceptance carried a ticket that is not theirs"));
                    return;
                }
                row.state = ReqState::Accepted;
                row.ticket = Some(ticket);
                self.group_req_save(core, &row);
                self.group_req_add(core.clone(), row);
            }
            Letter::Decline { req, blocked } => {
                let Some(mut row) = core.store.req(&hex::encode(req)).ok().flatten() else { return };
                if !row.outgoing || row.peer != m.from || row.state != ReqState::Pending {
                    return;
                }
                row.state = if blocked { ReqState::Blocked } else { ReqState::Declined };
                self.group_req_save(core, &row);
            }
        }
    }

    /// Adds the contact of an accepted outgoing request (once; retried by `group_req_tick`).
    fn group_req_add(self: &Arc<Self>, core: Arc<GroupCore>, mut row: ReqRow) {
        let Some(ticket) = row.ticket.clone() else { return };
        row.next_try = now_ms() + ADD_RETRY_MS;
        let _ = core.store.put_req(&row);
        let this = self.clone();
        self.handle.spawn(async move {
            match this.clone().add_contact_from(ticket, crate::caps::ContactOrigin::GroupRequest).await {
                Ok(c) => {
                    this.log(format!("chat request accepted: {} is a contact", c.name));
                    row.added = true;
                    row.ticket = None;
                    this.group_req_save(&core, &row);
                }
                Err(e) => this.log(format!("chat request: adding {} failed: {e}", row.peer)),
            }
        });
    }

    /// Retries adds that failed (called by the group dial loop).
    pub(crate) fn group_req_tick(self: &Arc<Self>, core: &Arc<GroupCore>) {
        let now = now_ms();
        for r in core.store.reqs().unwrap_or_default() {
            if r.outgoing && r.state == ReqState::Accepted && !r.added && r.ticket.is_some() && r.next_try <= now {
                self.group_req_add(core.clone(), r);
            }
        }
    }

    fn group_req_list(self: &Arc<Self>, outgoing: bool) -> Result<Vec<GroupChatRequest>, Error> {
        let core = self.group_core()?;
        let mut rows: Vec<ReqRow> = core.store.reqs()?.into_iter().filter(|r| r.outgoing == outgoing).collect();
        rows.sort_by(|a, b| b.at.cmp(&a.at));
        Ok(rows.iter().map(|r| self.group_req_view(&core, r)).collect())
    }

    fn group_req_incoming_pending(&self, core: &GroupCore, id: &str) -> Result<ReqRow, Error> {
        let row = core.store.req(id)?.ok_or(Error::NotFound)?;
        if row.outgoing {
            return Err(Error::Rejected("that is our own request".into()));
        }
        if row.state != ReqState::Pending {
            return Err(Error::Rejected("already answered".into()));
        }
        Ok(row)
    }
}

#[uniffi::export]
impl Node {
    pub fn set_group_request_events(&self, events: Arc<dyn GroupRequestEvents>) {
        *self.inner.group.req_events.lock() = Some(events);
    }

    /// Asks member `did` of `group_id` for a 1:1 chat, through the group's hosts, sealed to them.
    /// Refused locally for ourselves, non-members, someone we block or who blocks us, and existing
    /// contacts. Asking again while a request is pending returns that request.
    pub fn group_chat_request(&self, group_id: String, did: String) -> Result<GroupChatRequest, Error> {
        let inner = self.inner.clone();
        let core = inner.group_core()?;
        inner.group_member_check(&core, &group_id)?;
        if did == core.did {
            return Err(Error::Rejected("that is you".into()));
        }
        let (member, name) = {
            let groups = core.groups.lock();
            let l = groups.get(&group_id).ok_or(Error::NotFound)?;
            (l.state.is_member(&did), l.state.members.get(&did).map(|m| m.name.clone()).unwrap_or_default())
        };
        if !member {
            return Err(Error::Rejected("not a member of this group".into()));
        }
        let skip = inner.group.skip_request_checks.load(std::sync::atomic::Ordering::SeqCst);
        if !skip {
            let s = inner.shared.lock();
            if s.state.blocked.contains(&did) {
                return Err(Error::Rejected("you block them".into()));
            }
            if crate::block::blocked_me(&s.state, &did) {
                return Err(Error::Rejected(crate::BLOCKED.into()));
            }
            if s.state.contacts.iter().any(|c| c.did == did) {
                return Err(Error::Rejected("already a contact".into()));
            }
        }
        if let Some(r) = core.store.reqs()?.into_iter().find(|r| r.outgoing && r.peer == did && r.group == group_id && r.state == ReqState::Pending) {
            return Ok(inner.group_req_view(&core, &r));
        }
        let req = new_req_id();
        let my_name = inner.me()?.profile.name.clone();
        let m = inner.group_seal(&core, &group_id, &did, Letter::Request { req: req_bytes(&req).expect("16 bytes"), name: my_name })?;
        let row = ReqRow { req, group: group_id, peer: did, peer_name: name, outgoing: true, state: ReqState::Pending, at: now_ms(), ticket: None, added: false, next_try: 0 };
        inner.group_req_save(&core, &row);
        inner.group_send_mail(&core, &row.group, m);
        Ok(inner.group_req_view(&core, &row))
    }

    /// Requests other members sent us (blocked senders' are never listed), newest first.
    pub fn group_chat_requests_incoming(&self) -> Result<Vec<GroupChatRequest>, Error> {
        self.inner.group_req_list(false)
    }

    /// Requests we sent, newest first.
    pub fn group_chat_requests_outgoing(&self) -> Result<Vec<GroupChatRequest>, Error> {
        self.inner.group_req_list(true)
    }

    /// Accepts an incoming request: our private endpoint goes, sealed, to the requester only, as
    /// a one-time ticket; they add us (message only) and the chat runs directly.
    pub fn group_chat_accept(&self, request_id: String) -> Result<GroupChatRequest, Error> {
        let inner = self.inner.clone();
        self.block_on(async move {
            let core = inner.group_core()?;
            let mut row = inner.group_req_incoming_pending(&core, &request_id)?;
            let ticket = inner.request_ticket(&row.peer, REQ_TICKET_TTL).await?;
            let m = inner.group_seal(&core, &row.group, &row.peer, Letter::Accept { req: req_bytes(&row.req).ok_or(Error::NotFound)?, ticket })?;
            row.state = ReqState::Accepted;
            inner.group_req_save(&core, &row);
            inner.group_send_mail(&core, &row.group, m);
            Ok(inner.group_req_view(&core, &row))
        })?
    }

    /// Declines an incoming request; the requester sees "declined" (PROVISIONAL).
    pub fn group_chat_decline(&self, request_id: String) -> Result<GroupChatRequest, Error> {
        let inner = self.inner.clone();
        let core = inner.group_core()?;
        let mut row = inner.group_req_incoming_pending(&core, &request_id)?;
        let m = inner.group_seal(&core, &row.group, &row.peer, Letter::Decline { req: req_bytes(&row.req).ok_or(Error::NotFound)?, blocked: false })?;
        row.state = ReqState::Declined;
        inner.group_req_save(&core, &row);
        inner.group_send_mail(&core, &row.group, m);
        Ok(inner.group_req_view(&core, &row))
    }
}

impl GroupStore {
    // ---- requests and mail (U3) --------------------------------------------------------------

    pub(crate) fn put_req(&self, r: &super::requests::ReqRow) -> Result<(), Error> {
        self.db.put(&format!("req/{}", r.req), &json(r)).map_err(io)
    }

    pub(crate) fn req(&self, id: &str) -> Result<Option<super::requests::ReqRow>, Error> {
        Ok(self.db.get(&format!("req/{id}")).map_err(io)?.and_then(|v| serde_json::from_slice(&v).ok()))
    }

    pub(crate) fn reqs(&self) -> Result<Vec<super::requests::ReqRow>, Error> {
        Ok(self.db.scan("req/").map_err(io)?.into_iter().filter_map(|(_, v)| serde_json::from_slice(&v).ok()).collect())
    }

    pub fn put_out(&self, gid: &str, id: &str, mail: &[u8]) -> Result<(), Error> {
        self.db.put(&format!("out/{gid}/{id}"), mail).map_err(io)
    }

    pub fn outbox(&self, gid: &str) -> Result<Vec<Vec<u8>>, Error> {
        Ok(self.db.scan(&format!("out/{gid}/")).map_err(io)?.into_iter().map(|(_, v)| v).collect())
    }

    pub fn drop_out(&self, gid: &str, id: &str) -> Result<(), Error> {
        self.db.apply(&[Op::Delete(format!("out/{gid}/{id}"))]).map_err(io)
    }

    /// Host side: keeps mail for `to` until `expires`; false if it was already here.
    pub fn put_mail(&self, gid: &str, to: &str, id: &str, expires: u64, mail: &[u8]) -> Result<bool, Error> {
        let k = format!("mail/{gid}/{to}/{id}");
        if self.db.get(&k).map_err(io)?.is_some() {
            return Ok(false);
        }
        self.db.put(&k, &bincode::serialize(&(expires, mail.to_vec())).expect("serialises")).map_err(io)?;
        Ok(true)
    }

    /// Host side: unexpired mail of `gid` (for `to`, or all), `(to, bytes)`; expired rows go.
    pub fn mail(&self, gid: &str, to: Option<&str>, now_ms: u64) -> Result<Vec<(String, Vec<u8>)>, Error> {
        let p = match to {
            Some(t) => format!("mail/{gid}/{t}/"),
            None => format!("mail/{gid}/"),
        };
        let mut out = Vec::new();
        let mut dead = Vec::new();
        for (k, v) in self.db.scan(&p).map_err(io)? {
            let Ok((exp, b)) = bincode::deserialize::<(u64, Vec<u8>)>(&v) else { continue };
            if exp < now_ms {
                dead.push(Op::Delete(k));
                continue;
            }
            let rest = k.strip_prefix(&format!("mail/{gid}/")).unwrap_or_default();
            let to = rest.rsplit_once('/').map(|(t, _)| t.to_string()).unwrap_or_default();
            out.push((to, b));
        }
        if !dead.is_empty() {
            let _ = self.db.apply(&dead);
        }
        Ok(out)
    }

    /// Recipient side: true the first time `id` is seen.
    pub fn first_seen(&self, id: &str) -> Result<bool, Error> {
        let k = format!("seen/{id}");
        if self.db.get(&k).map_err(io)?.is_some() {
            return Ok(false);
        }
        self.db.put(&k, &[1]).map_err(io)?;
        Ok(true)
    }

}

/// Test hooks (not for apps).
#[uniffi::export]
impl Node {
    /// Behave like a modified app that skips its own chat-request checks (block, contact).
    pub fn group_skip_request_checks_for_test(&self, skip: bool) {
        self.inner.group.skip_request_checks.store(skip, std::sync::atomic::Ordering::SeqCst);
    }

    /// Host side: how many sealed mails this device holds for `group_id`.
    pub fn group_relayed_mail_count_for_test(&self, group_id: String) -> u32 {
        self.inner.group_core().map(|c| c.store.mail(&group_id, None, 0).unwrap_or_default().len() as u32).unwrap_or(0)
    }

    /// Host side: whether any mail this device holds for `group_id` contains the 32 bytes of
    /// `endpoint_id` (a private endpoint must never be readable by the host).
    pub fn group_relayed_mail_contains_for_test(&self, group_id: String, endpoint_id: String) -> bool {
        let Ok(k) = endpoint_id.parse::<iroh::PublicKey>() else { return true };
        let needle = *k.as_bytes();
        let text = endpoint_id.as_bytes().to_vec();
        self.inner
            .group_core()
            .map(|c| {
                c.store.mail(&group_id, None, 0).unwrap_or_default().iter().any(|(_, b)| {
                    b.windows(32).any(|w| w == needle) || b.windows(text.len()).any(|w| w == text.as_slice())
                })
            })
            .unwrap_or(true)
    }
}
