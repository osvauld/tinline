//! Code wakes (`docs/push.md` "Code wake tokens" and "Tokens at setup").
//!
//! - Owner side: [`NodeCtx::register_code_token`] + [`apply_code_token`] register a `msg`-only token for a ticket we are
//!   about to sign (`lifecycle.rs` `ticket`), kept in `WakeState::codes` by the ticket's nonce.
//!   [`retire_codes`] (run by every wake round) takes back the tokens of tickets that no longer
//!   add contacts (reset, block) and forgets expired ones.
//! - Joiner side: [`NodeCtx::wake_code`] wakes the owner of a code we hold a queued request for,
//!   with the code's token (`request_out.rs`).
//! - Setup: [`NodeCtx::wake_handover`] is our token for a DID as `ContactHello` /
//!   `ContactWelcome` carry it; [`hold_handover`] keeps the one they gave us.

use std::sync::Arc;
use std::time::{Duration, Instant};

use proto::WakeHandover;
use wake_relay_core::api::RegisterResponse;
use wake_relay_core::{Kind, Kinds};

use super::{random_tag, wake_mut};
use crate::store::State;
use crate::{NodeCtx, now_ms};

/// How long minting a ticket waits for the relay to seal its code token (PROVISIONAL); without an
/// answer the ticket goes out without one, as on a device without push.
pub const CODE_REGISTER_TIMEOUT: Duration = Duration::from_secs(5);
/// How long the contact setup waits for our wake task to register a token for the other side
/// before sending a hello or a Welcome without one (PROVISIONAL; the normal `WakeTokens` round
/// still hands it over later).
pub const HANDOVER_WAIT: Duration = Duration::from_secs(3);

/// Code tokens of tickets that no longer add contacts go to the revoke outbox; expired ones are
/// forgotten. Whether anything changed.
pub(super) fn retire_codes(st: &mut State) -> bool {
    let now = crate::now();
    let live: Vec<String> = match &st.wake {
        Some(w) => w
            .codes
            .iter()
            .map(|c| c.nonce.clone())
            .filter(|n| account_model::tickets::ticket_live(st, n, now))
            .collect(),
        None => return false,
    };
    st.wake
        .as_mut()
        .is_some_and(|w| w.retire_codes(now_ms(), |n| live.iter().any(|l| l == n)))
}

/// Keeps the relay's answer from [`NodeCtx::register_code_token`] and returns the token's raw bytes
/// for the ticket. `None` if the answer is unusable, or the token does not fit a ticket
/// (`proto::MAX_TICKET_WAKE`): it is then taken back (revoke outbox; kick the wake task).
pub fn apply_code_token(
    st: &mut State,
    pending: wake::PendingCode,
    resp: RegisterResponse,
) -> Option<Vec<u8>> {
    let w = st.wake.as_mut()?;
    let token = w.apply_code(pending, resp)?;
    match wake::token_bytes(&token).filter(|b| b.len() <= proto::MAX_TICKET_WAKE) {
        Some(b) => Some(b),
        None => {
            w.codes.retain(|c| c.token != token);
            w.revoke_outbox.push(token);
            None
        }
    }
}

/// Keeps the token `did`'s device gave us in its hello or Welcome. Call once `did` is our contact.
pub fn hold_handover(st: &mut State, did: &str, device: [u8; 32], h: &WakeHandover) -> bool {
    let Some(kinds) = Kinds::from_bits(h.kinds) else {
        return false;
    };
    wake_mut(st).hold(did, device, h.token.clone(), h.exp_ms, kinds, now_ms())
}

/// The handover a `ContactHello` / `ContactWelcome` carries, if any.
pub fn handover_in(msg: &proto::Msg) -> Option<&WakeHandover> {
    match msg {
        proto::Msg::ContactHello { wake, .. } | proto::Msg::ContactWelcome { wake, .. } => {
            wake.as_ref()
        }
        _ => None,
    }
}

/// `msg` with our handover `h` in it (a `ContactHello` or `ContactWelcome`; anything else as is).
pub fn with_handover(mut msg: proto::Msg, h: Option<WakeHandover>) -> proto::Msg {
    if let proto::Msg::ContactHello { wake, .. } | proto::Msg::ContactWelcome { wake, .. } =
        &mut msg
    {
        *wake = h;
    }
    msg
}

impl NodeCtx {
    /// Asks the relay to seal the code wake token for a ticket with nonce `nonce` (base64url, as
    /// in its claim) expiring at `exp_ms`. Apply the answer with [`apply_code_token`] in the same
    /// lock that stores the ticket (a wake round in between would take a token for a ticket it
    /// does not know yet back). `None` without push or when the relay does not answer within
    /// [`CODE_REGISTER_TIMEOUT`].
    pub async fn register_code_token(
        self: &Arc<Self>,
        nonce: &str,
        exp_ms: u64,
    ) -> Option<(wake::PendingCode, RegisterResponse)> {
        let (req, pending) = {
            let s = self.shared.lock();
            s.state
                .wake
                .as_ref()?
                .plan_code(nonce, exp_ms, now_ms(), random_tag())?
        };
        match tokio::time::timeout(CODE_REGISTER_TIMEOUT, self.wake_relay().register(req)).await {
            Ok(Ok(r)) => Some((pending, r)),
            Ok(Err(e)) => {
                tracing::info!("code wake register failed: {e}");
                None
            }
            Err(_) => {
                tracing::info!("code wake register failed: timed out");
                None
            }
        }
    }

    /// Our wake token for `did`, as the contact setup hands it over; `None` while we have none.
    pub fn wake_handover(&self, did: &str) -> Option<WakeHandover> {
        let s = self.shared.lock();
        let w = s.state.wake.as_ref()?;
        w.target.as_ref()?;
        let i = w.issued_for(did)?;
        (i.exp_ms > now_ms()).then(|| WakeHandover {
            token: i.token.clone(),
            exp_ms: i.exp_ms,
            kinds: i.kinds.bits(),
        })
    }

    /// [`Self::wake_handover`], giving our wake task up to `max` to register one first (only with
    /// push on; at once `None` without).
    pub async fn await_wake_handover(
        self: &Arc<Self>,
        did: &str,
        max: Duration,
    ) -> Option<WakeHandover> {
        if self.push_target().is_none() {
            return None;
        }
        if let Some(h) = self.wake_handover(did) {
            return Some(h);
        }
        self.wake_kick();
        let end = Instant::now() + max;
        while Instant::now() < end {
            tokio::time::sleep(Duration::from_millis(100)).await;
            if let Some(h) = self.wake_handover(did) {
                return Some(h);
            }
        }
        None
    }

    /// Wakes the owner of the code our queued request to `did` uses, with the code's token: only
    /// when it carries one the relay has not refused, and at most once per
    /// `wake::MSG_WAKE_EVERY_MS`. `true`: the relay accepted (202); the fast redials follow
    /// (`fast_redial`). Never logs the token.
    pub async fn wake_code(self: &Arc<Self>, did: &str) -> bool {
        let Ok(me) = self.me() else { return false };
        let now = now_ms();
        let token = {
            let mut s = self.shared.lock();
            if s.epoch != me.epoch {
                return false;
            }
            let Some(r) = s.state.sent_requests.get_mut(did) else {
                return false;
            };
            if r.delivered
                || r.wake_dead
                || r.last_wake_ms
                    .is_some_and(|t| now < t.saturating_add(wake::MSG_WAKE_EVERY_MS))
            {
                return false;
            }
            let Some(wake) = r
                .ticket
                .as_deref()
                .and_then(|t| proto::ContactTicket::from_text(t).ok())
                .and_then(|t| t.verify(now / 1000).ok())
                .and_then(|c| c.wake)
            else {
                return false;
            };
            r.last_wake_ms = Some(now);
            wake::token_text(&wake)
        };
        let status = self.wake_relay().wake(token, Kind::Msg).await;
        if status == 410 {
            if let Some(r) = self.shared.lock().state.sent_requests.get_mut(did) {
                r.wake_dead = true;
            }
        }
        tracing::info!(
            "code wake sent for {}: relay status {status} (0: not reached)",
            crate::short(did)
        );
        self.persist_in_background(me.epoch);
        if status == 202 {
            self.push
                .woken
                .lock()
                .insert(did.to_string(), Instant::now());
        }
        status == 202
    }

    /// Whether our queued request to `did` can wake them (its code carries a live token).
    pub fn can_wake_code(&self, did: &str) -> bool {
        let now = crate::now();
        let s = self.shared.lock();
        s.state.sent_requests.get(did).is_some_and(|r| {
            !r.wake_dead
                && r.ticket
                    .as_deref()
                    .and_then(|t| proto::ContactTicket::from_text(t).ok())
                    .and_then(|t| t.verify(now).ok())
                    .is_some_and(|c| c.wake.is_some())
        })
    }

    /// A contact was just added: our current token goes to them by `WakeTokens` too (they may
    /// already hold it from the setup; taking it again is harmless).
    pub fn wake_contact_added(&self, did: &str) {
        let mut s = self.shared.lock();
        let st = &mut s.state;
        if st
            .wake
            .as_ref()
            .is_some_and(|w| w.issued_for(did).is_some())
        {
            st.wake_outbox.entry(did.to_string()).or_default();
        }
    }
}
