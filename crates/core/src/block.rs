//! Visible, consent-based blocking (U5 revised, U19, U20, U23; `docs/protocol.md` "Blocking").
//!
//! Blocking keeps the contact and its history, signs ONE notice with the DID key
//! (`proto::sign_block_notice`) and delivers it to the blocked contact (retried until taken).
//! Two layers enforce it:
//! - the blocked person's app honours the notice: it deletes its contact entry for us (the
//!   deletion request every block carries, U20), keeps the chat history read-only, and stops
//!   sending, calling and retrying (`blocked_me`);
//! - every device of ours refuses whatever the blocked DID still sends (a modified build) by
//!   answering with that same signed notice, which reads as the reason `"blocked"`.
//!
//! The notice and the block list sync to our own devices through the account doc; they hand out
//! the notice as it is and never sign their own. Unblock signs a newer notice with
//! `blocked: false`, delivered the same way. Group membership is not touched (U5).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::Mutex;
use proto::{BlockNotice as Claim, Msg, SignedBlockNotice as SignedBlob};

use crate::node::{Inner, Node, addr_for, now_ms};
use crate::store::{HeldNotice, State};
use crate::Error;
use crate::wire::{Ctrl, DIAL_TIMEOUT, RETRY_SECS};

/// The one reason the blocked side sees, on the notice and on every refusal after it.
pub const BLOCKED: &str = "blocked";

/// A verified block notice someone sent us, as the UI shows it ("<name> blocked you").
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BlockNotice {
    /// The DID that blocked us; the notice is signed by this DID's key.
    pub from_did: String,
    /// Unix seconds, from the signed notice.
    pub signed_at: u64,
    /// Always `"blocked"`.
    pub reason: String,
}

/// A conversation's block state for the UI.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
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
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BlockedContact {
    pub did: String,
    pub name: String,
    /// Unix seconds.
    pub blocked_at: u64,
}

/// Runtime state of blocking that is not persisted.
#[derive(Default)]
pub(crate) struct BlockRt {
    /// Notices signed per DID since its last block (for tests: exactly one per block).
    signed: Mutex<HashMap<String, u32>>,
    /// DIDs with a delivery task running.
    delivering: Mutex<HashSet<String>>,
    /// Test knob: act like a modified app that ignores notices (the backstop must still hold).
    ignore: AtomicBool,
}

/// Why an incoming notice was not taken. Nothing changes in any case.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// Bad signature, wrong addressee, unreadable.
    Invalid,
    /// Not newer than the notice we hold from that DID (a replay, or a stale copy).
    Stale,
    /// An unblock from someone whose block we never held: nothing to lift.
    NothingToLift,
}

/// Signs our next notice to `to`: its `at` is now, but always above the previous one's, so every
/// receiver orders our notices the same way.
pub(crate) fn sign_next(id: &identity::Identity, to: &str, blocked: bool, prev: Option<&SignedBlob>) -> SignedBlob {
    let prev_at = prev.and_then(|b| proto::verify_block_notice(b, to).ok()).map_or(0, |n| n.at);
    proto::sign_block_notice(id, to, blocked, now_ms().max(prev_at + 1))
}

/// The intake rule, pure: `blob` must be a notice to `me` signed by the DID it names, and newer
/// than the one we hold from that DID.
pub(crate) fn admit(me: &str, blob: &SignedBlob, held: Option<&HeldNotice>) -> Result<Claim, Refusal> {
    let claim = proto::verify_block_notice(blob, me).map_err(|_| Refusal::Invalid)?;
    match held {
        Some(h) if claim.at <= h.at => Err(Refusal::Stale),
        None if !claim.blocked => Err(Refusal::NothingToLift),
        _ => Ok(claim),
    }
}

/// Most notices kept from DIDs that are not contacts (F01: anyone can mint DIDs and send one).
pub(crate) const MAX_STRANGER_NOTICES: usize = 256;

/// Bounds `notices` from non-contacts to `MAX_STRANGER_NOTICES`, dropping the oldest by `at`.
/// A contact's notice is never dropped.
pub(crate) fn cap_notices(st: &mut State) {
    let contact = |d: &str| st.contacts.iter().any(|c| c.did == d);
    let mut strangers: Vec<(u64, String)> =
        st.notices.iter().filter(|(d, _)| !contact(d)).map(|(d, h)| (h.at, d.clone())).collect();
    let Some(excess) = strangers.len().checked_sub(MAX_STRANGER_NOTICES).filter(|&n| n > 0) else { return };
    strangers.sort_unstable();
    for (_, d) in strangers.into_iter().take(excess) {
        st.notices.remove(&d);
    }
}

/// U20, the deletion request every block notice carries: the blocked person's app deletes its
/// contact entry for the blocker (address, grant, device list) unless it was added after the
/// notice was signed. The chat history is kept, read-only (`chat_contact`). The one place that
/// decides what a deletion request deletes. Returns whether a contact went.
pub(crate) fn apply_block_deletion_request(st: &mut State, from: &str, at_ms: u64) -> bool {
    let n = st.contacts.len();
    st.contacts.retain(|c| !(c.did == from && c.added_at.saturating_mul(1000) <= at_ms));
    st.contacts.len() != n
}

/// Whether we hold a block from `did` (they blocked us).
pub(crate) fn blocked_me(st: &State, did: &str) -> bool {
    st.notices.get(did).is_some_and(|h| h.blocked)
}

/// The name we knew `did` by: the contact's, else the one kept with their notice.
pub(crate) fn known_name(st: &State, did: &str) -> Option<String> {
    st.contacts
        .iter()
        .find(|c| c.did == did)
        .map(crate::node::display_name)
        .or_else(|| st.notices.get(did).map(|h| h.name.clone()))
}

fn notice_at(blob: &SignedBlob, to: &str) -> u64 {
    proto::verify_block_notice(blob, to).map_or(0, |n| n.at)
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
            Inner::revoke_all(&mut s.state, &did);
            // A ticket they may have seen is of no further use (nor any older one still live).
            crate::node::retire_ticket(&mut s.state, false);
            n
        };
        self.inner.block.signed.lock().insert(did.clone(), 1);
        // The block holds in memory whether or not it reached the disk: act on it either way.
        let saved = self.inner.persist();
        self.inner.chat_close_sessions(&did);
        self.inner.calls_blocked(&did, &notice);
        self.inner.notice_kick(did);
        self.inner.events.on_contacts_changed();
        saved
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
        self.inner.notice_kick(did.clone());
        self.inner.chat_kick(did);
        self.inner.events.on_contacts_changed();
        saved
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
        serde_json::from_str::<SignedBlob>(&notice_json).is_ok_and(|n| self.inner.take_notice(&n).is_ok())
    }

    /// For tests: make this node behave like a modified app that ignores block notices.
    #[doc(hidden)]
    pub fn ignore_block_notices_for_test(&self, ignore: bool) {
        self.inner.block.ignore.store(ignore, Ordering::SeqCst);
    }
}

impl Inner {
    /// Signs and records an unblock notice if `did` is blocked; whether it was.
    pub(crate) fn unblock_inner(&self, did: &str) -> Result<bool, Error> {
        let me = self.me()?;
        let mut s = self.shared.lock();
        if !s.state.blocked.contains(did) {
            return Ok(false);
        }
        let n = sign_next(&me.id, did, false, s.state.sent_notices.get(did));
        s.state.sent_notices.insert(did.to_string(), n);
        s.state.blocked.remove(did);
        s.state.notice_outbox.insert(did.to_string());
        Ok(true)
    }

    /// The notice we answer `did` with, if we block them.
    pub(crate) fn refusal_for(&self, did: &str) -> Option<SignedBlob> {
        let s = self.shared.lock();
        s.state.blocked.contains(did).then(|| s.state.sent_notices.get(did).cloned()).flatten()
    }

    /// Whether `blob` is a valid block notice from `did` to us (verified; nothing stored).
    pub(crate) fn is_block_from(&self, did: &str, blob: &SignedBlob) -> bool {
        let Ok(me) = self.me() else { return false };
        proto::verify_block_notice(blob, me.id.did()).is_ok_and(|n| n.blocked && n.from == did)
    }

    /// A notice arrived (delivered, or as the answer to something we tried). Taken if `admit`
    /// says so; a block then applies the deletion request and stops everything we had going with
    /// them. Every own device does the same when the notice syncs over.
    pub(crate) fn take_notice(self: &Arc<Self>, blob: &SignedBlob) -> Result<Claim, Refusal> {
        if self.block.ignore.load(Ordering::SeqCst) {
            return Err(Refusal::Invalid);
        }
        let me = self.me().map_err(|_| Refusal::Invalid)?;
        let claim = {
            let mut s = self.shared.lock();
            if s.epoch != me.epoch {
                return Err(Refusal::Invalid);
            }
            let held = s.state.notices.get(&proto::verify_block_notice(blob, me.id.did()).map_err(|_| Refusal::Invalid)?.from);
            let claim = admit(me.id.did(), blob, held)?;
            let name = known_name(&s.state, &claim.from).unwrap_or_default();
            s.state.notices.insert(claim.from.clone(), HeldNotice { notice: blob.clone(), name, blocked: claim.blocked, at: claim.at });
            if claim.blocked {
                apply_block_deletion_request(&mut s.state, &claim.from, claim.at);
            }
            cap_notices(&mut s.state);
            claim
        };
        self.log(format!("{} {} us", claim.from, if claim.blocked { "blocked" } else { "unblocked" }));
        self.persist_in_background(me.epoch);
        if claim.blocked {
            self.chat_close_sessions(&claim.from);
            self.end_calls_with(&claim.from, BLOCKED);
        }
        self.events.on_contacts_changed();
        Ok(claim)
    }

    /// An incoming connection that opened with `Notice`: take it and say so. A valid notice we
    /// already hold (or a newer one) is acknowledged too, so the sender stops retrying.
    pub(crate) async fn notice_incoming(self: &Arc<Self>, mut ctrl: Ctrl, blob: SignedBlob) -> Result<(), Error> {
        let reply = match self.take_notice(&blob) {
            Ok(_) | Err(Refusal::Stale) | Err(Refusal::NothingToLift) => Msg::NoticeTaken,
            Err(Refusal::Invalid) => Msg::Reject { reason: "not accepted".into() },
        };
        ctrl.send(&reply).await?;
        ctrl.finish();
        Ok(())
    }

    /// Delivers our pending notice to `did` (all their devices; one taking it is enough, their
    /// devices sync it), retrying with backoff while the endpoint is up. One task per DID; it
    /// re-reads the notice each round, so an unblock right after a block is delivered too.
    pub(crate) fn notice_kick(self: &Arc<Self>, did: String) {
        if !self.block.delivering.lock().insert(did.clone()) {
            return;
        }
        let this = self.clone();
        self.handle.spawn(async move {
            let mut round = 0usize;
            loop {
                let Ok(me) = this.me() else { break };
                let Ok(ep) = this.endpoint() else { break };
                let (notice, devices) = {
                    let s = this.shared.lock();
                    if !s.state.notice_outbox.contains(&did) {
                        break;
                    }
                    let devices = s.state.contacts.iter().find(|c| c.did == did).map(|c| c.devices.clone()).unwrap_or_default();
                    match s.state.sent_notices.get(&did) {
                        Some(n) => (n.clone(), devices),
                        None => break,
                    }
                };
                let mut taken = false;
                for d in &devices {
                    match this.deliver_notice(&ep, d.device, d.relay.as_deref(), &notice).await {
                        Ok(()) => {
                            taken = true;
                            break;
                        }
                        Err(e) => tracing::debug!("notice to {did}: {e}"),
                    }
                }
                if taken {
                    let done = {
                        let mut s = this.shared.lock();
                        let same = s.state.sent_notices.get(&did) == Some(&notice);
                        if same {
                            s.state.notice_outbox.remove(&did);
                        }
                        same
                    };
                    let _ = this.persist_async(me.epoch).await;
                    this.log(format!("notice delivered to {did}"));
                    if done {
                        break;
                    }
                    round = 0;
                    continue;
                }
                if devices.is_empty() {
                    break;
                }
                tokio::time::sleep(crate::wire::retry_pause(&RETRY_SECS, round)).await;
                round += 1;
            }
            this.block.delivering.lock().remove(&did);
        });
    }

    async fn deliver_notice(&self, ep: &iroh::Endpoint, device: [u8; 32], relay: Option<&str>, notice: &SignedBlob) -> Result<(), Error> {
        let addr = addr_for(&device, relay)?;
        let conn = tokio::time::timeout(DIAL_TIMEOUT, ep.connect(addr, proto::ALPN))
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(Error::net)?;
        let reply = crate::wire::exchange(conn, &Msg::Notice { notice: notice.clone() }).await?;
        match reply {
            Some(Msg::NoticeTaken) => Ok(()),
            other => Err(Error::Protocol(format!("notice not taken: {other:?}"))),
        }
    }

    /// After start: deliver whatever is still owed.
    pub(crate) fn notices_started(self: &Arc<Self>) {
        let dids: Vec<String> = self.shared.lock().state.notice_outbox.iter().cloned().collect();
        for did in dids {
            self.notice_kick(did);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{ContactDevice, StoredContact};

    fn held(blob: &SignedBlob, to: &str) -> HeldNotice {
        let n = proto::verify_block_notice(blob, to).unwrap();
        HeldNotice { notice: blob.clone(), name: "a".into(), blocked: n.blocked, at: n.at }
    }

    #[test]
    fn intake_takes_newer_notices_only() {
        let (a, _) = identity::generate();
        let (b, _) = identity::generate();
        let (x, _) = identity::generate();
        let block = proto::sign_block_notice(&a, b.did(), true, 100);
        // Fresh block: taken.
        assert!(admit(b.did(), &block, None).is_ok_and(|c| c.blocked && c.from == a.did()));
        // The same notice again (a replay, or every later refusal): stale, nothing changes.
        assert_eq!(admit(b.did(), &block, Some(&held(&block, b.did()))), Err(Refusal::Stale));
        // Unblock, newer: taken. The old block replayed after it: stale.
        let unblock = sign_next(&a, b.did(), false, Some(&block));
        let h = held(&unblock, b.did());
        assert!(h.at > 100 && !h.blocked);
        assert!(admit(b.did(), &unblock, Some(&held(&block, b.did()))).is_ok());
        assert_eq!(admit(b.did(), &block, Some(&h)), Err(Refusal::Stale));
        // An unblock from someone whose block we never held lifts nothing.
        assert_eq!(admit(b.did(), &unblock, None), Err(Refusal::NothingToLift));
        // Forged (X signs as A), or addressed to X: invalid.
        let forged = proto::forge_block_notice_for_test(&x, a.did(), b.did(), true, 200);
        assert_eq!(admit(b.did(), &forged, None), Err(Refusal::Invalid));
        assert_eq!(admit(x.did(), &block, None), Err(Refusal::Invalid));
    }

    #[test]
    fn the_deletion_request_drops_only_an_older_contact_entry() {
        let (a, _) = identity::generate();
        let (b, _) = identity::generate();
        let grant = proto::issue_grant(&a, b.did(), 1, 10);
        let contact = |did: &str, added_at: u64| StoredContact {
            did: did.into(),
            name: "n".into(),
            devices: vec![ContactDevice { device: [7; 32], relay: None }],
            device_list: None,
            grant_from_them: grant.clone(),
            call_grant_from_them: None,
            calls_allowed: true,
            added_at,
            alias: None,
            verified: true,
        };
        let mut st = State { contacts: vec![contact(a.did(), 50), contact("did:other", 50)], ..Default::default() };
        assert!(apply_block_deletion_request(&mut st, a.did(), 60_000));
        assert_eq!(st.contacts.len(), 1);
        assert_eq!(st.contacts[0].did, "did:other");
        // Re-added after the notice was signed: kept.
        let mut st = State { contacts: vec![contact(a.did(), 61)], ..Default::default() };
        assert!(!apply_block_deletion_request(&mut st, a.did(), 60_000));
    }

    #[test]
    fn stranger_notices_are_capped_oldest_first_and_contacts_kept() {
        let (a, _) = identity::generate();
        let blob = proto::sign_block_notice(&a, "did:me", true, 1);
        let h = |at: u64| HeldNotice { notice: blob.clone(), name: String::new(), blocked: true, at };
        let mut st = State::default();
        st.contacts.push(StoredContact {
            did: "did:friend".into(),
            name: "f".into(),
            devices: vec![],
            device_list: None,
            grant_from_them: proto::issue_grant(&a, "did:me", 1, 10),
            call_grant_from_them: None,
            calls_allowed: true,
            added_at: 0,
            alias: None,
            verified: true,
        });
        st.notices.insert("did:friend".into(), h(0)); // oldest of all, but a contact
        for i in 0..MAX_STRANGER_NOTICES as u64 + 10 {
            st.notices.insert(format!("did:x{i}"), h(i + 1));
            cap_notices(&mut st);
        }
        assert_eq!(st.notices.len(), MAX_STRANGER_NOTICES + 1);
        assert!(st.notices.contains_key("did:friend"));
        assert!((0..10).all(|i| !st.notices.contains_key(&format!("did:x{i}"))));
        assert!(st.notices.contains_key(&format!("did:x{}", MAX_STRANGER_NOTICES + 9)));
    }
}
