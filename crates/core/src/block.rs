//! Visible, consent-based blocking: the runtime is `node_runtime::block`, re-exported here; this
//! module declares its records to UniFFI and holds the `Node` API (`docs/protocol.md` "Blocking").

use std::sync::atomic::Ordering;

pub use node_runtime::block::*;
use proto::SignedBlockNotice as SignedBlob;

use crate::node::{Node, now_ms, retire_ticket};
use crate::Error;

/// A verified block notice someone sent us, as the UI shows it ("<name> blocked you").
#[uniffi::remote(Record)]
pub struct BlockNotice {
    /// The DID that blocked us; the notice is signed by this DID's key.
    pub from_did: String,
    /// Unix seconds, from the signed notice.
    pub signed_at: u64,
    /// Always `"blocked"`.
    pub reason: String,
}

/// A conversation's block state for the UI.
#[uniffi::remote(Record)]
pub struct BlockState {
    /// We block them: "Blocked" under the name, Unblock instead of the composer.
    pub blocked_by_me: bool,
    /// Unix seconds of our block, when `blocked_by_me`.
    pub blocked_at: Option<u64>,
    /// They block us: "<name> blocked you", no composer, call button crossed out; our pending
    /// messages to them read "Not delivered: <name> blocked you".
    pub blocked_me: Option<BlockNotice>,
}

/// One conversation's block and call state (`Node::peer_blocks`).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PeerBlocks {
    pub did: String,
    /// As `Node::block_state`.
    pub state: BlockState,
    /// We let them call us, as `Node::calls_allowed`.
    pub calls_allowed: bool,
    /// We hold a live call token from them, as `Node::can_call`.
    pub can_call: bool,
}

/// One row of Settings › Blocked people.
#[uniffi::remote(Record)]
pub struct BlockedContact {
    pub did: String,
    pub name: String,
    /// Unix seconds.
    pub blocked_at: u64,
}


#[uniffi::export]
impl Node {
    /// Blocks a contact (U19: the contact action; there is no separate Remove). The contact and
    /// the chat history are kept and shown as Blocked. Both their tokens (message and call) are
    /// taken back for good (P6): after an unblock they re-add us and get fresh ones. One DID-signed notice goes to them
    /// (retried until their app takes it); it also asks their app to delete its contact entry
    /// for us (U20). From now on all our devices refuse their calls, chats, history requests and
    /// ticket redemptions with the reason `"blocked"`. Any call with them ends. Does not rotate
    /// any endpoint and does not change group membership. Blocking a blocked contact is a no-op.
    pub fn block(&self, did: String) -> Result<(), Error> {
        let slot = self.open();
        let me = slot.me()?;
        let notice = {
            let mut s = slot.shared.lock();
            if !s.state.contacts.iter().any(|c| c.did == did) {
                return Err(Error::NotFound);
            }
            if s.state.blocked.contains(&did) {
                return Ok(());
            }
            let n = sign_next(&me.id, &did, true, s.state.sent_notices.get(&did));
            s.state.sent_notices.insert(did.clone(), n.clone());
            s.state.blocked.insert(did.clone());
            s.state.notice_outbox.insert(did.clone());
            // Takes back both their tokens (P6): an unblock does not revive them.
            node_runtime::NodeCtx::revoke_all(&mut s.state, &did);
            // A ticket they may have seen is of no further use (nor any older one still live).
            retire_ticket(&mut s.state, false);
            n
        };
        slot.block.signed.lock().insert(did.clone(), 1);
        // The block holds in memory whether or not it reached the disk: act on it either way.
        let saved = slot.persist();
        slot.chat.chat_close_sessions(&did);
        slot.calls_blocked(&did, &notice);
        slot.ctx.notice_kick(did);
        // Their wake token is taken back at the relay.
        slot.ctx.wake_kick();
        slot.sink.on_contacts_changed();
        saved.map_err(Error::from)
    }

    /// Lifts our block: their messages are accepted again, and a signed unblock notice tells
    /// their app (which deleted its contact entry for us) that it no longer has to show
    /// "blocked you". Messaging resumes once they add us again (ticket / QR). No-op if not
    /// blocked.
    pub fn unblock(&self, did: String) -> Result<(), Error> {
        let slot = self.open();
        if !slot.unblock_inner(&did)? {
            return Ok(());
        }
        let saved = slot.persist();
        slot.ctx.notice_kick(did.clone());
        slot.ctx.wake_kick();
        slot.chat.chat_kick(did);
        slot.sink.on_contacts_changed();
        saved.map_err(Error::from)
    }

    /// The block state of the conversation with `did`.
    pub fn block_state(&self, did: String) -> BlockState {
        block_state_in(&self.open().shared.lock().state, &did)
    }

    /// `block_state`, `calls_allowed` and `can_call` for each of `dids`, in that order, in one
    /// call: the apps' block refresh at start-up asks this for every contact and kept chat, and
    /// one crossing per DID per question grows with the contact list.
    pub fn peer_blocks(&self, dids: Vec<String>) -> Vec<PeerBlocks> {
        let slot = self.open();
        let me = slot.me().ok();
        // Read under the lock; the call tokens' signatures are checked after it is released.
        let rows: Vec<_> = {
            let s = slot.shared.lock();
            let st = &s.state;
            dids.into_iter()
                .map(|did| {
                    let c = st.contacts.iter().find(|c| c.did == did);
                    let grant = c.and_then(|c| c.call_grant_from_them.clone()).filter(|_| !blocked_me(st, &did));
                    (block_state_in(st, &did), c.is_some_and(|c| c.calls_allowed), grant, did)
                })
                .collect()
        };
        let now = now_ms();
        rows.into_iter()
            .map(|(state, calls_allowed, grant, did)| PeerBlocks {
                can_call: me.as_ref().zip(grant).is_some_and(|(me, g)| crate::caps::call_token_ok(&g, &did, me.id.did(), now)),
                did,
                state,
                calls_allowed,
            })
            .collect()
    }

    /// Everyone we block, newest block first (one list across our devices).
    pub fn blocked_contacts(&self) -> Vec<BlockedContact> {
        let slot = self.open();
        let s = slot.shared.lock();
        let mut v: Vec<BlockedContact> = s
            .state
            .blocked
            .iter()
            .map(|did| BlockedContact {
                did: did.clone(),
                name: known_name(&s.state, did).unwrap_or_default(),
                blocked_at: s.state.sent_notices.get(did).map_or(0, |n| notice_at(n, did) / 1000),
            })
            .collect();
        v.sort_by(|a, b| b.blocked_at.cmp(&a.blocked_at).then_with(|| a.did.cmp(&b.did)));
        v
    }

    /// Why our message `message_id` to `peer_did` is not delivered: `Some("blocked")` while they
    /// block us and it has not reached them (UI: "Not delivered: <name> blocked you"); `None`
    /// when delivered, still pending for another reason, or not ours.
    pub fn not_delivered_reason(&self, peer_did: String, message_id: String) -> Option<String> {
        let slot = self.open();
        if !blocked_me(&slot.shared.lock().state, &peer_did) {
            return None;
        }
        slot.chat.chat_pending(&peer_did, &message_id).then(|| BLOCKED.to_string())
    }
}

impl Node {
    /// For tests: notices signed for `did` since its last block (retries not counted).
    #[doc(hidden)]
    pub fn block_notices_sent_for_test(&self, did: String) -> u32 {
        self.open().block.signed.lock().get(&did).copied().unwrap_or(0)
    }

    /// For tests: a block notice to `to` that claims to come from `claimed_from` but is signed
    /// by this node's DID key, as wire JSON. Not in release builds.
    #[cfg(any(test, feature = "dev-tools"))]
    #[doc(hidden)]
    pub fn forge_block_notice_for_test(&self, claimed_from: String, to: String) -> Result<String, Error> {
        let me = self.open().me()?;
        let n = proto::forge_block_notice_for_test(&me.id, &claimed_from, &to, true, now_ms());
        Ok(serde_json::to_string(&n)?)
    }

    /// For tests: hands a notice (wire JSON) to this node as if it had arrived; whether it was
    /// taken.
    #[doc(hidden)]
    pub fn offer_block_notice_for_test(&self, notice_json: String) -> bool {
        serde_json::from_str::<SignedBlob>(&notice_json).is_ok_and(|n| self.open().ctx.take_notice(&n).is_ok())
    }

    /// For tests: make this node behave like a modified app that ignores block notices.
    #[doc(hidden)]
    pub fn ignore_block_notices_for_test(&self, ignore: bool) {
        self.open().block.ignore.store(ignore, Ordering::SeqCst);
    }
}

fn block_state_in(st: &node_runtime::store::State, did: &str) -> BlockState {
    let blocked_by_me = st.blocked.contains(did);
    BlockState {
        blocked_by_me,
        blocked_at: st.sent_notices.get(did).filter(|_| blocked_by_me).map(|n| notice_at(n, did) / 1000),
        blocked_me: st.notices.get(did).filter(|h| h.blocked).map(|h| BlockNotice {
            from_did: did.to_string(),
            signed_at: h.at / 1000,
            reason: BLOCKED.into(),
        }),
    }
}
