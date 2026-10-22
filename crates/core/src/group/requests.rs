//! U3 chat requests between group members. The runtime is `group_runtime::requests`; this
//! declares its records to UniFFI, forwards its events to the app and holds the `Node` calls.

use std::sync::Arc;

pub use group_runtime::requests::*;

use crate::Error;
use crate::group::mail::Letter;
use crate::node::{Node, now_ms};

#[uniffi::remote(Enum)]
pub enum ChatRequestState {
    /// Waiting for the recipient.
    Pending,
    /// Accepted; for an outgoing one, `contact_added` says whether the contact is in place yet.
    Accepted,
    Declined,
    /// The recipient blocks the requester (P5): refused without being shown to them.
    Blocked,
}

#[uniffi::remote(Record)]
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
#[uniffi::export(rust, foreign)]
pub trait GroupRequestEvents: Send + Sync {
    /// A request appeared or changed state.
    fn on_chat_request(&self, request: GroupChatRequest);
}

/// Hands the runtime's events to the app's `GroupRequestEvents`.
pub(crate) struct GroupRequestEventsAdapter(pub(crate) Arc<dyn GroupRequestEvents>);

impl group_runtime::requests::GroupRequestSink for GroupRequestEventsAdapter {
    fn on_chat_request(&self, request: GroupChatRequest) {
        self.0.on_chat_request(request);
    }
}

#[uniffi::export]
impl Node {
    pub fn set_group_request_events(&self, events: Arc<dyn GroupRequestEvents>) {
        *self.inner.group.req_events.lock() = Some(Arc::new(GroupRequestEventsAdapter(events)));
    }

    /// Asks member `did` of `group_id` for a 1:1 chat, through the group's hosts, sealed to them.
    /// Refused locally for ourselves, non-members, someone we block or who blocks us, and existing
    /// contacts. Asking again while a request is pending returns that request.
    pub fn group_chat_request(&self, group_id: String, did: String) -> Result<GroupChatRequest, Error> {
        let inner = self.inner.clone();
        let core = inner.group.group_core()?;
        inner.group.group_member_check(&core, &group_id)?;
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
            return Ok(inner.group.group_req_view(&core, &r));
        }
        let req = new_req_id();
        let my_name = inner.me()?.profile.name.clone();
        let m = inner.group.group_seal(&core, &group_id, &did, Letter::Request { req: req_bytes(&req).expect("16 bytes"), name: my_name })?;
        let row = ReqRow { req, group: group_id, peer: did, peer_name: name, outgoing: true, state: ReqState::Pending, at: now_ms(), ticket: None, added: false, next_try: 0 };
        inner.group.group_req_save(&core, &row);
        inner.group.group_send_mail(&core, &row.group, m);
        Ok(inner.group.group_req_view(&core, &row))
    }

    /// Requests other members sent us (blocked senders' are never listed), newest first.
    pub fn group_chat_requests_incoming(&self) -> Result<Vec<GroupChatRequest>, Error> {
        Ok(self.inner.group.group_req_list(false)?)
    }

    /// Requests we sent, newest first.
    pub fn group_chat_requests_outgoing(&self) -> Result<Vec<GroupChatRequest>, Error> {
        Ok(self.inner.group.group_req_list(true)?)
    }

    /// Accepts an incoming request: our private endpoint goes, sealed, to the requester only, as
    /// a one-time ticket; they add us (message only) and the chat runs directly.
    pub fn group_chat_accept(&self, request_id: String) -> Result<GroupChatRequest, Error> {
        let inner = self.inner.clone();
        self.block_on(async move {
            let core = inner.group.group_core()?;
            let mut row = inner.group.group_req_incoming_pending(&core, &request_id)?;
            let ticket = inner.ctx.request_ticket(&row.peer, REQ_TICKET_TTL).await?;
            let m = inner.group.group_seal(&core, &row.group, &row.peer, Letter::Accept { req: req_bytes(&row.req).ok_or(Error::NotFound)?, ticket })?;
            row.state = ReqState::Accepted;
            inner.group.group_req_save(&core, &row);
            inner.group.group_send_mail(&core, &row.group, m);
            Ok(inner.group.group_req_view(&core, &row))
        })?
    }

    /// Declines an incoming request; the requester sees "declined" (PROVISIONAL).
    pub fn group_chat_decline(&self, request_id: String) -> Result<GroupChatRequest, Error> {
        let inner = self.inner.clone();
        let core = inner.group.group_core()?;
        let mut row = inner.group.group_req_incoming_pending(&core, &request_id)?;
        let m = inner.group.group_seal(&core, &row.group, &row.peer, Letter::Decline { req: req_bytes(&row.req).ok_or(Error::NotFound)?, blocked: false })?;
        row.state = ReqState::Declined;
        inner.group.group_req_save(&core, &row);
        inner.group.group_send_mail(&core, &row.group, m);
        Ok(inner.group.group_req_view(&core, &row))
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
        self.inner.group.group_core().map(|c| c.store.mail(&group_id, None, 0).unwrap_or_default().len() as u32).unwrap_or(0)
    }

    /// Host side: whether any mail this device holds for `group_id` contains the 32 bytes of
    /// `endpoint_id` (a private endpoint must never be readable by the host).
    pub fn group_relayed_mail_contains_for_test(&self, group_id: String, endpoint_id: String) -> bool {
        let Ok(k) = endpoint_id.parse::<iroh::PublicKey>() else { return true };
        let needle = *k.as_bytes();
        let text = endpoint_id.as_bytes().to_vec();
        self.inner.group
            .group_core()
            .map(|c| {
                c.store.mail(&group_id, None, 0).unwrap_or_default().iter().any(|(_, b)| {
                    b.windows(32).any(|w| w == needle) || b.windows(text.len()).any(|w| w == text.as_slice())
                })
            })
            .unwrap_or(true)
    }
}
