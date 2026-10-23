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

use crate::{NodeCtx, addr_for, now_ms};
use crate::store::{HeldNotice, State};
use crate::Error;
use crate::wire::{Ctrl, DIAL_TIMEOUT, RETRY_SECS};

/// The one reason the blocked side sees, on the notice and on every refusal after it.
pub const BLOCKED: &str = "blocked";

/// A verified block notice someone sent us, as the UI shows it ("<name> blocked you").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockNotice {
    /// The DID that blocked us; the notice is signed by this DID's key.
    pub from_did: String,
    /// Unix seconds, from the signed notice.
    pub signed_at: u64,
    /// Always `"blocked"`.
    pub reason: String,
}

/// A conversation's block state for the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockedContact {
    pub did: String,
    pub name: String,
    /// Unix seconds.
    pub blocked_at: u64,
}


/// Runtime state of blocking that is not persisted.
#[derive(Default)]
pub struct BlockRt {
    /// Notices signed per DID since its last block (for tests: exactly one per block).
    pub signed: Mutex<HashMap<String, u32>>,
    /// DIDs with a delivery task running.
    pub delivering: Mutex<HashSet<String>>,
    /// Test knob: act like a modified app that ignores notices (the backstop must still hold).
    pub ignore: AtomicBool,
}

/// Why an incoming notice was not taken. Nothing changes in any case.
#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Bad signature, wrong addressee, unreadable.
    Invalid,
    /// Not newer than the notice we hold from that DID (a replay, or a stale copy).
    Stale,
    /// An unblock from someone whose block we never held: nothing to lift.
    NothingToLift,
}

/// Signs our next notice to `to`: its `at` is now, but always above the previous one's, so every
/// receiver orders our notices the same way.
pub fn sign_next(id: &identity::Identity, to: &str, blocked: bool, prev: Option<&SignedBlob>) -> SignedBlob {
    let prev_at = prev.and_then(|b| proto::verify_block_notice(b, to).ok()).map_or(0, |n| n.at);
    proto::sign_block_notice(id, to, blocked, now_ms().max(prev_at + 1))
}

/// The intake rule, pure: `blob` must be a notice to `me` signed by the DID it names, and newer
/// than the one we hold from that DID.
pub fn admit(me: &str, blob: &SignedBlob, held: Option<&HeldNotice>) -> Result<Claim, Refusal> {
    let claim = proto::verify_block_notice(blob, me).map_err(|_| Refusal::Invalid)?;
    match held {
        Some(h) if claim.at <= h.at => Err(Refusal::Stale),
        None if !claim.blocked => Err(Refusal::NothingToLift),
        _ => Ok(claim),
    }
}

/// Most notices kept from DIDs that are not contacts (F01: anyone can mint DIDs and send one).
pub const MAX_STRANGER_NOTICES: usize = 256;

/// Bounds `notices` from non-contacts to `MAX_STRANGER_NOTICES`, dropping the oldest by `at`.
/// A contact's notice is never dropped.
pub fn cap_notices(st: &mut State) {
    let contact = |d: &str| st.contacts.iter().any(|c| c.did == d);
    let mut strangers: Vec<(u64, String)> =
        st.notices.iter().filter(|(d, _)| !contact(d)).map(|(d, h)| (h.at, d.clone())).collect();
    let Some(excess) = strangers.len().checked_sub(MAX_STRANGER_NOTICES).filter(|&n| n > 0) else { return };
    strangers.sort_unstable();
    for (_, d) in strangers.into_iter().take(excess) {
        st.notices.remove(&d);
    }
}

// U20, what a block notice's deletion request deletes, is `account_model::block` (the
// own-device merge applies it too).
pub use account_model::block::apply_block_deletion_request;

/// Whether we hold a block from `did` (they blocked us).
pub fn blocked_me(st: &State, did: &str) -> bool {
    st.notices.get(did).is_some_and(|h| h.blocked)
}

/// The name we knew `did` by: the contact's, else the one kept with their notice.
pub fn known_name(st: &State, did: &str) -> Option<String> {
    st.contacts
        .iter()
        .find(|c| c.did == did)
        .map(crate::display_name)
        .or_else(|| st.notices.get(did).map(|h| h.name.clone()))
}

pub fn notice_at(blob: &SignedBlob, to: &str) -> u64 {
    proto::verify_block_notice(blob, to).map_or(0, |n| n.at)
}


impl NodeCtx {
    /// Signs and records an unblock notice if `did` is blocked; whether it was.
    pub fn unblock_inner(&self, did: &str) -> Result<bool, Error> {
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
    pub fn refusal_for(&self, did: &str) -> Option<SignedBlob> {
        let s = self.shared.lock();
        s.state.blocked.contains(did).then(|| s.state.sent_notices.get(did).cloned()).flatten()
    }

    /// Whether `blob` is a valid block notice from `did` to us (verified; nothing stored).
    pub fn is_block_from(&self, did: &str, blob: &SignedBlob) -> bool {
        let Ok(me) = self.me() else { return false };
        proto::verify_block_notice(blob, me.id.did()).is_ok_and(|n| n.blocked && n.from == did)
    }

    /// A notice arrived (delivered, or as the answer to something we tried). Taken if `admit`
    /// says so; a block then applies the deletion request and stops everything we had going with
    /// them. Every own device does the same when the notice syncs over.
    pub fn take_notice(self: &Arc<Self>, blob: &SignedBlob) -> Result<Claim, Refusal> {
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
            if let Some(h) = self.hooks() {
                h.chat_close_sessions(&claim.from);
            }
            self.end_calls_with(&claim.from, BLOCKED);
        }
        self.sink.on_contacts_changed();
        Ok(claim)
    }

    /// An incoming connection that opened with `Notice`: take it and say so. A valid notice we
    /// already hold (or a newer one) is acknowledged too, so the sender stops retrying.
    pub async fn notice_incoming(self: &Arc<Self>, mut ctrl: Ctrl, blob: SignedBlob) -> Result<(), Error> {
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
    pub fn notice_kick(self: &Arc<Self>, did: String) {
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
                    this.persist_logged(me.epoch).await;
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
            other => Err(crate::not_taken("notice", other.as_ref())),
        }
    }

    /// After start: deliver whatever is still owed.
    pub fn notices_started(self: &Arc<Self>) {
        let dids: Vec<String> = self.shared.lock().state.notice_outbox.iter().cloned().collect();
        for did in dids {
            self.notice_kick(did);
        }
    }
}

#[cfg(test)]
mod tests;
