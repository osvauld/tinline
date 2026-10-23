//! U3: 1:1 chat requests between members of a group, through the group host, sealed end to end
//! (`mail.rs`). The host learns that one member wrote to another, never what.
//!
//! - `group_chat_request(group, did)`: we ask; the request waits at the hosts until the
//!   recipient's device comes by (PROVISIONAL: kept 7 days).
//! - The recipient sees it (`group_chat_requests_incoming`, `GroupRequestSink`), and accepts or
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

pub use group_core::requests::*;

use super::engine::GroupCore;
use super::mail::{self, Letter, SignedMail};
use crate::Error;
use crate::GroupRt;
use node_runtime::{OrLog, OrLogFlat, now, now_ms};

/// How long the one-time ticket in an acceptance stays good (PROVISIONAL).
pub const REQ_TICKET_TTL: u64 = 7 * 24 * 3600;
/// Retry period for adding an accepted contact that failed (offline peer).
const ADD_RETRY_MS: u64 = 20_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatRequestState {
    /// Waiting for the recipient.
    Pending,
    /// Accepted; for an outgoing one, `contact_added` says whether the contact is in place yet.
    Accepted,
    Declined,
    /// The recipient blocks the requester (P5): refused without being shown to them.
    Blocked,
}

#[derive(Debug, Clone, PartialEq)]
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

/// Where the runtime reports to; p2pcore forwards it to the app's callback (`Node::set_group_request_events`).
/// Called on core threads: implementations must not block.
pub trait GroupRequestSink: Send + Sync {
    /// A request appeared or changed state.
    fn on_chat_request(&self, request: GroupChatRequest);
}

pub fn new_req_id() -> String {
    hex::encode(cryptography::rng::bytes::<16>())
}

pub fn req_bytes(id: &str) -> Option<[u8; 16]> {
    hex::decode(id).ok()?.try_into().ok()
}

impl GroupRt {
    pub fn group_req_view(&self, core: &GroupCore, r: &ReqRow) -> GroupChatRequest {
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

    pub fn group_req_save(&self, core: &GroupCore, r: &ReqRow) {
        if let Err(e) = core.store.put_req(r) {
            tracing::warn!("chat request: {e}");
        }
        if let Some(ev) = self.req_events.lock().clone() {
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
    pub fn group_seal(&self, core: &Arc<GroupCore>, gid: &str, to: &str, letter: Letter) -> Result<SignedMail, Error> {
        let key = core
            .groups
            .lock()
            .get(gid)
            .and_then(|l| l.state.enc_keys.get(to).copied())
            .ok_or_else(|| Error::Rejected("their app has not published a key in this group yet".into()))?;
        Ok(mail::seal(&core.account, gid, to, &key, letter, now_ms())?)
    }

    /// Mail addressed to us arrived (from a host, or handed over by our own host side).
    pub fn group_mail_for_me(self: &Arc<Self>, core: &Arc<GroupCore>, gid: &str, sm: &SignedMail) {
        let Some((m, letter)) = mail::open_for(&core.account, gid, sm) else { return };
        if proto::IssuanceStore::spent(&core.store, &m.from, &m.id, &core.did) || core.store.mark_seen(&m.id).is_err() {
            return;
        }
        let member = core.groups.lock().get(gid).is_some_and(|l| l.state.is_member(&m.from));
        match letter {
            Letter::Request { req, name } => {
                if !member {
                    return;
                }
                let id = hex::encode(req);
                // A read error must not look like "no such request": saving a fresh Pending row
                // would overwrite one the user already answered. Refuse; the sender can ask again.
                match core.store.req(&id) {
                    Ok(None) => {}
                    Ok(Some(_)) => return,
                    Err(e) => {
                        tracing::warn!("chat request not stored (reading requests failed): {e}");
                        return;
                    }
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
                let Some(mut row) = core.store.req(&hex::encode(req)).or_log_flat("reading a chat request") else { return };
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
                let Some(mut row) = core.store.req(&hex::encode(req)).or_log_flat("reading a chat request") else { return };
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
        // A failed write loses the retry stamp: this attempt still runs, the next tick sees the old one.
        core.store.put_req(&row).or_log("saving the chat request retry time");
        let this = self.clone();
        self.handle.spawn(async move {
            match this.ctx.clone().add_contact_from(ticket, node_runtime::caps::ContactOrigin::GroupRequest).await {
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
    pub fn group_req_tick(self: &Arc<Self>, core: &Arc<GroupCore>) {
        let now = now_ms();
        for r in core.store.reqs().unwrap_or_default() {
            if r.outgoing && r.state == ReqState::Accepted && !r.added && r.ticket.is_some() && r.next_try <= now {
                self.group_req_add(core.clone(), r);
            }
        }
    }

    pub fn group_req_list(self: &Arc<Self>, outgoing: bool) -> Result<Vec<GroupChatRequest>, Error> {
        let core = self.group_core()?;
        let mut rows: Vec<ReqRow> = core.store.reqs()?.into_iter().filter(|r| r.outgoing == outgoing).collect();
        rows.sort_by_key(|r| std::cmp::Reverse(r.at));
        Ok(rows.iter().map(|r| self.group_req_view(&core, r)).collect())
    }

    pub fn group_req_incoming_pending(&self, core: &GroupCore, id: &str) -> Result<ReqRow, Error> {
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
