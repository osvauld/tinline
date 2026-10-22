//! Visible, consent-based blocking: the runtime is `node_runtime::block`, re-exported here; this
//! module declares its records to UniFFI and holds the `Node` API (`docs/protocol.md` "Blocking").

use std::sync::atomic::Ordering;

pub use node_runtime::block::*;
use proto::SignedBlockNotice as SignedBlob;

use crate::node::{Inner, Node, now_ms, retire_ticket};
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
        let me = self.inner.me()?;
        let notice = {
            let mut s = self.inner.shared.lock();
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
        self.inner.block.signed.lock().insert(did.clone(), 1);
        // The block holds in memory whether or not it reached the disk: act on it either way.
        let saved = self.inner.persist();
        self.inner.chat_close_sessions(&did);
        self.inner.calls_blocked(&did, &notice);
        self.inner.ctx.notice_kick(did);
        self.inner.sink.on_contacts_changed();
        saved.map_err(Error::from)
    }

    /// Lifts our block: their messages are accepted again, and a signed unblock notice tells
    /// their app (which deleted its contact entry for us) that it no longer has to show
    /// "blocked you". Messaging resumes once they add us again (ticket / QR). No-op if not
    /// blocked.
    pub fn unblock(&self, did: String) -> Result<(), Error> {
        if !self.inner.unblock_inner(&did)? {
            return Ok(());
        }
        let saved = self.inner.persist();
        self.inner.ctx.notice_kick(did.clone());
        self.inner.chat_kick(did);
        self.inner.sink.on_contacts_changed();
        saved.map_err(Error::from)
    }

    /// The block state of the conversation with `did`.
    pub fn block_state(&self, did: String) -> BlockState {
        let s = self.inner.shared.lock();
        let blocked_by_me = s.state.blocked.contains(&did);
        BlockState {
            blocked_by_me,
            blocked_at: s.state.sent_notices.get(&did).filter(|_| blocked_by_me).map(|n| notice_at(n, &did) / 1000),
            blocked_me: s.state.notices.get(&did).filter(|h| h.blocked).map(|h| BlockNotice {
                from_did: did.clone(),
                signed_at: h.at / 1000,
                reason: BLOCKED.into(),
            }),
        }
    }

    /// Everyone we block, newest block first (one list across our devices).
    pub fn blocked_contacts(&self) -> Vec<BlockedContact> {
        let s = self.inner.shared.lock();
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
        if !blocked_me(&self.inner.shared.lock().state, &peer_did) {
            return None;
        }
        self.inner.chat_pending(&peer_did, &message_id).then(|| BLOCKED.to_string())
    }
}

impl Node {
    /// For tests: notices signed for `did` since its last block (retries not counted).
    #[doc(hidden)]
    pub fn block_notices_sent_for_test(&self, did: String) -> u32 {
        self.inner.block.signed.lock().get(&did).copied().unwrap_or(0)
    }

    /// For tests: a block notice to `to` that claims to come from `claimed_from` but is signed
    /// by this node's DID key, as wire JSON. Not in release builds.
    #[cfg(any(test, feature = "dev-tools"))]
    #[doc(hidden)]
    pub fn forge_block_notice_for_test(&self, claimed_from: String, to: String) -> Result<String, Error> {
        let me = self.inner.me()?;
        let n = proto::forge_block_notice_for_test(&me.id, &claimed_from, &to, true, now_ms());
        Ok(serde_json::to_string(&n)?)
    }

    /// For tests: hands a notice (wire JSON) to this node as if it had arrived; whether it was
    /// taken.
    #[doc(hidden)]
    pub fn offer_block_notice_for_test(&self, notice_json: String) -> bool {
        serde_json::from_str::<SignedBlob>(&notice_json).is_ok_and(|n| self.inner.ctx.take_notice(&n).is_ok())
    }

    /// For tests: make this node behave like a modified app that ignores block notices.
    #[doc(hidden)]
    pub fn ignore_block_notices_for_test(&self, ignore: bool) {
        self.inner.block.ignore.store(ignore, Ordering::SeqCst);
    }
}
