//! The wake-token state of one device and every decision about it (design: `docs/push.md`).
//! Pure logic: the caller does the HTTP (`/v1/register`, `/v1/wake`, `/v1/revoke`), the clock,
//! the randomness and the persistence (the state is serde, for the sealed node state).
//!
//! - Receiving side: [`WakeState::plan_register`] / [`WakeState::apply_register`] get one token
//!   per contact from the relay; the caller hands each to its contact (`WakeTokens`). Revokes
//!   queue in an outbox. [`WakeState::on_wake`] decides what a push means.
//! - Sending side: [`WakeState::hold`] keeps the tokens contacts gave us; [`WakeState::wake_for`]
//!   says whether to wake a device now, [`WakeState::wake_result`] learns from the relay's answer.
//!
//! - Code wake tokens: [`WakeState::plan_code`] / [`WakeState::apply_code`] get one `msg` token
//!   per contact ticket we hand out (keyed by the ticket nonce, expiring with it), so a stranger
//!   holding our code can wake us to deliver a contact request. [`WakeState::on_wake`] answers
//!   such a wake with [`WakeAction::CodeWake`], throttled per tag ([`CODE_WAKE_CAP`] per
//!   [`CODE_WAKE_WINDOW_MS`]).
//!
//! Nothing here carries content: no name, no text. DIDs stay local, as keys of this state.

use serde::{Deserialize, Serialize};
use wake_relay_core::api::{RegisterRequest, RegisterResponse, TagRequest};
use wake_relay_core::hint;
use wake_relay_core::relay::MAX_TAGS_PER_REGISTER;
use wake_relay_core::token::{MAX_TOKEN_CHARS, MAX_TOKEN_LIFE_MS};
pub use wake_relay_core::{Kind, Kinds, Target};

#[cfg(test)]
mod backoff_tests;
#[cfg(test)]
mod code_tests;
#[cfg(test)]
mod tests;

const DAY_MS: u64 = 86_400_000;
/// Life of a token we issue (PROVISIONAL: long enough to rarely renew, short enough that a
/// forgotten contact's token dies by itself). Must stay within `MAX_TOKEN_LIFE_MS`.
pub const TOKEN_LIFE_MS: u64 = 30 * DAY_MS;
const _: () = assert!(TOKEN_LIFE_MS <= MAX_TOKEN_LIFE_MS);
/// Reissue a token this close to its expiry (PROVISIONAL: a week of retries if the contact is offline).
pub const RENEW_BEFORE_MS: u64 = 7 * DAY_MS;
/// At most one `msg` wake per device this often (PROVISIONAL: the relay's rate limiter is only
/// approximate, so this is the real limit).
pub const MSG_WAKE_EVERY_MS: u64 = 30_000;
/// The least time between `msg` wakes of one target that does not answer, after the 1st, 2nd,
/// 3rd, 4th and every later wake: wakes at 0, 30 s, 2 min, 10 min, 30 min from the first, then
/// hourly, about 24 a day (PROVISIONAL, `docs/notifications-plan.md` N2). Reaching the target
/// starts it over ([`WakeState::reached`]); [`MSG_WAKE_EVERY_MS`] stays the floor.
pub const MSG_WAKE_GAPS_MS: [u64; 5] = [
    MSG_WAKE_EVERY_MS,
    90_000,
    8 * 60_000,
    20 * 60_000,
    60 * 60_000,
];

/// The least time after the last `msg` wake of a target before the next, when `count` wakes
/// have gone out since it was last reached ([`MSG_WAKE_GAPS_MS`]).
pub fn msg_wake_gap_ms(count: u32) -> u64 {
    let i = (count as usize)
        .saturating_sub(1)
        .min(MSG_WAKE_GAPS_MS.len() - 1);
    MSG_WAKE_GAPS_MS[i]
}

/// Whether a `msg` wake of a target may go out at `now_ms`: never woken, or the gap of
/// [`msg_wake_gap_ms`] has passed since `last_ms`. Pure; the caller records the wake
/// (`last_ms = now`, `count += 1`) and resets `count` when the target is reached.
pub fn msg_wake_due(last_ms: Option<u64>, count: u32, now_ms: u64) -> bool {
    !last_ms.is_some_and(|t| now_ms < t.saturating_add(msg_wake_gap_ms(count)))
}

/// At most one `call` wake per device this often (PROVISIONAL: the caller redials every ~1 s, a
/// wake should not repeat that fast).
pub const CALL_WAKE_EVERY_MS: u64 = 10_000;

/// A code wake (a stranger holding our contact ticket) is acted on at most [`CODE_WAKE_CAP`]
/// times per tag within this window; more are ignored (PROVISIONAL, `docs/push.md` Open 6: the
/// tag is shared by everyone holding that code, so the cap is for all of them together; a joiner
/// wakes about once per 30 s at most and stops once its request is delivered).
pub const CODE_WAKE_WINDOW_MS: u64 = 10 * 60_000;
/// See [`CODE_WAKE_WINDOW_MS`] (PROVISIONAL).
pub const CODE_WAKE_CAP: usize = 4;

/// A relay token's text (base64url) as the raw bytes a contact ticket carries; `None` if it is
/// not canonical base64url (then it cannot go into a ticket).
pub fn token_bytes(token: &str) -> Option<Vec<u8>> {
    let raw = wake_relay_core::b64::decode(token)?;
    (wake_relay_core::b64::encode(&raw) == token).then_some(raw)
}

/// The token text for raw bytes from a ticket.
pub fn token_text(bytes: &[u8]) -> String {
    wake_relay_core::b64::encode(bytes)
}

/// The User-Agent for relay requests (osvauld.com's bot protection blocks generic agents).
pub fn user_agent(version: &str) -> String {
    format!("Tinline/{version}")
}

/// A token we issued to one contact for this device. Kept so we can take it back.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issued {
    pub tag: [u8; 16],
    pub kinds: Kinds,
    pub token: String,
    pub exp_ms: u64,
}

/// A token one device of a contact gave us.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Held {
    pub token: String,
    pub exp_ms: u64,
    pub kinds: Kinds,
    pub last_wake_ms: Option<u64>,
    pub last_call_wake_ms: Option<u64>,
    /// `msg` wakes since this contact was last reached, for [`msg_wake_due`].
    #[serde(default)]
    pub msg_wakes: u32,
}

/// `Held` with its key; lists rather than tuple-keyed maps so the state stays JSON-friendly.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldEntry {
    pub did: String,
    pub device: [u8; 32],
    pub held: Held,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssuedEntry {
    pub did: String,
    pub issued: Issued,
}

/// A code wake token: issued for one contact ticket of ours (`nonce`, base64url as in the
/// ticket's claim), `msg` only, expiring with the ticket. Kept so we can take it back
/// (`reset_ticket`, a block, push turned off) and to recognise its wakes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeToken {
    pub nonce: String,
    pub tag: [u8; 16],
    pub token: String,
    pub exp_ms: u64,
    /// When its recent wakes were acted on (unix ms), for the throttle.
    #[serde(default)]
    pub wakes: Vec<u64>,
}

/// What a code register request was built for; hand it back with the relay's answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingCode {
    target: Target,
    nonce: String,
    tag: [u8; 16],
    exp_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WakeState {
    /// Opens the hints in our pushes; chosen once, given to the relay with every register.
    pub hint_key: [u8; 32],
    /// Our current platform push token; `None` while always-on or not yet registered.
    pub target: Option<Target>,
    pub issued: Vec<IssuedEntry>,
    pub held: Vec<HeldEntry>,
    /// Wake tokens the relay must be told are revoked, until it answers.
    pub revoke_outbox: Vec<String>,
    /// Code wake tokens of our live contact tickets.
    #[serde(default)]
    pub codes: Vec<CodeToken>,
}

/// What a register request was built for; hand it back with the relay's answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pending {
    target: Target,
    wants: Vec<PendingTag>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PendingTag {
    did: String,
    tag: [u8; 16],
    kinds: Kinds,
    exp_ms: u64,
}

/// What a wake push means for this device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WakeAction {
    /// Do nothing (iOS may still show its generic banner; we start nothing).
    Ignore,
    /// A call wake we refuse. iOS must still report it to CallKit and end it at once.
    IgnoreCall,
    /// A `msg` wake: start the node and dial this contact.
    DialBack { did: String },
    /// A `call` wake: ring for this contact now; the caller dials us, we do not dial.
    Ring { did: String },
    /// A `msg` wake through one of our contact tickets: someone holding our code (not a contact
    /// yet, so nobody to dial) wants to deliver a contact request. Start the node and stay
    /// reachable for the usual window; the request arrives as an ordinary `ContactHello`.
    CodeWake,
}

/// Whether `hint_key` opens `hint_text` (the account it belongs to is the one this push is for).
pub fn hint_opens(hint_key: &[u8; 32], hint_text: &str) -> bool {
    hint::open(hint_key, hint_text).is_ok()
}

/// `kinds` without `kind`; `None` when nothing is left.
fn without(kinds: Kinds, kind: Kind) -> Option<Kinds> {
    let msg = kind != Kind::Msg && kinds.allows(Kind::Msg);
    let call = kind != Kind::Call && kinds.allows(Kind::Call);
    match (msg, call) {
        (true, true) => Some(Kinds::ALL),
        (true, false) => Some(Kinds::MSG),
        (false, true) => Some(Kinds::CALL),
        (false, false) => None,
    }
}

impl WakeState {
    pub fn new(hint_key: [u8; 32]) -> Self {
        Self {
            hint_key,
            target: None,
            issued: Vec::new(),
            held: Vec::new(),
            revoke_outbox: Vec::new(),
            codes: Vec::new(),
        }
    }

    // ---- our own tokens (receiving side) ----

    /// A new or changed push token. Issued tokens are bound to the old one and die with it, so
    /// they are only forgotten (everything is reissued); no revoke is needed.
    pub fn set_target(&mut self, target: Target) {
        if self.target.as_ref() != Some(&target) {
            self.issued.clear();
            self.codes.clear();
            self.target = Some(target);
        }
    }

    /// The user went always-on: take every issued token back, code tokens too.
    pub fn clear_target(&mut self) {
        self.target = None;
        for e in self.issued.drain(..) {
            self.revoke_outbox.push(e.issued.token);
        }
        self.revoke_codes();
    }

    // ---- code wake tokens (receiving side, strangers holding our ticket) ----

    /// The register request for the code wake token of a ticket we are about to sign: `msg`
    /// only, expiring with the ticket (`exp_ms`). `None` without a target, or when the expiry is
    /// past or beyond what the relay seals. `tag` must be fresh random bytes.
    pub fn plan_code(
        &self,
        nonce: &str,
        exp_ms: u64,
        now_ms: u64,
        tag: [u8; 16],
    ) -> Option<(RegisterRequest, PendingCode)> {
        let target = self.target.clone()?;
        if exp_ms <= now_ms || exp_ms > now_ms.saturating_add(MAX_TOKEN_LIFE_MS) {
            return None;
        }
        let req = RegisterRequest {
            target: target.clone(),
            hint_key: self.hint_key,
            tags: vec![TagRequest {
                tag,
                kinds: Kinds::MSG,
                exp_ms,
            }],
        };
        Some((
            req,
            PendingCode {
                target,
                nonce: nonce.to_owned(),
                tag,
                exp_ms,
            },
        ))
    }

    /// Keeps the code token the relay sealed and returns it, for the ticket. `None` if the
    /// answer lacks our tag or the target changed meanwhile (the token would be dead).
    pub fn apply_code(&mut self, pending: PendingCode, resp: RegisterResponse) -> Option<String> {
        if self.target.as_ref() != Some(&pending.target) {
            return None;
        }
        let token = resp
            .tokens
            .into_iter()
            .find(|t| t.tag == pending.tag)?
            .token;
        if let Some(old) = self.codes.iter().position(|c| c.nonce == pending.nonce) {
            let old = self.codes.remove(old);
            self.revoke_outbox.push(old.token);
        }
        self.codes.push(CodeToken {
            nonce: pending.nonce,
            tag: pending.tag,
            token: token.clone(),
            exp_ms: pending.exp_ms,
            wakes: Vec::new(),
        });
        Some(token)
    }

    pub fn code_for(&self, nonce: &str) -> Option<&CodeToken> {
        self.codes.iter().find(|c| c.nonce == nonce)
    }

    /// Every ticket of ours stopped working (`reset_ticket`, a block): take every code token
    /// back at the relay.
    pub fn revoke_codes(&mut self) {
        for c in self.codes.drain(..) {
            self.revoke_outbox.push(c.token);
        }
    }

    /// Takes back the code tokens of tickets that no longer add contacts (`live` says which
    /// nonces still do) and forgets expired ones (the relay refuses those by itself). Whether
    /// anything changed.
    pub fn retire_codes(&mut self, now_ms: u64, live: impl Fn(&str) -> bool) -> bool {
        let before = (self.codes.len(), self.revoke_outbox.len());
        let mut keep = Vec::new();
        for c in self.codes.drain(..) {
            if c.exp_ms <= now_ms {
                continue;
            }
            if live(&c.nonce) {
                keep.push(c);
            } else {
                self.revoke_outbox.push(c.token);
            }
        }
        self.codes = keep;
        (self.codes.len(), self.revoke_outbox.len()) != before
    }

    /// The register request that brings every contact's token up to date, if any needs it:
    /// no token yet, one expiring within [`RENEW_BEFORE_MS`], or other kinds than `contacts` wants.
    /// At most `MAX_TAGS_PER_REGISTER` per request; plan again for the rest. `tags` yields fresh
    /// random tags.
    pub fn plan_register(
        &self,
        contacts: &[(String, Kinds)],
        now_ms: u64,
        mut tags: impl FnMut() -> [u8; 16],
    ) -> Option<(RegisterRequest, Pending)> {
        let target = self.target.clone()?;
        let exp_ms = now_ms + TOKEN_LIFE_MS;
        let mut wants = Vec::new();
        for (did, kinds) in contacts {
            if !kinds.is_valid() {
                continue;
            }
            let fresh = self.issued_for(did).is_some_and(|i| {
                i.kinds == *kinds && i.exp_ms > now_ms.saturating_add(RENEW_BEFORE_MS)
            });
            if !fresh {
                wants.push(PendingTag {
                    did: did.clone(),
                    tag: tags(),
                    kinds: *kinds,
                    exp_ms,
                });
                if wants.len() == MAX_TAGS_PER_REGISTER {
                    break;
                }
            }
        }
        if wants.is_empty() {
            return None;
        }
        let req = RegisterRequest {
            target: target.clone(),
            hint_key: self.hint_key,
            tags: wants
                .iter()
                .map(|w| TagRequest {
                    tag: w.tag,
                    kinds: w.kinds,
                    exp_ms: w.exp_ms,
                })
                .collect(),
        };
        Some((req, Pending { target, wants }))
    }

    /// Stores the tokens the relay sealed and returns what to hand to each contact. Tags we did
    /// not ask for are ignored; an answer for a target that has changed since is dropped. A
    /// replaced token whose kinds differed is revoked (calls turned off must really stop).
    pub fn apply_register(
        &mut self,
        pending: Pending,
        resp: RegisterResponse,
    ) -> Vec<(String, Issued)> {
        if self.target.as_ref() != Some(&pending.target) {
            return Vec::new();
        }
        let mut out = Vec::new();
        for w in &pending.wants {
            let Some(t) = resp.tokens.iter().find(|t| t.tag == w.tag) else {
                continue;
            };
            let issued = Issued {
                tag: w.tag,
                kinds: w.kinds,
                token: t.token.clone(),
                exp_ms: w.exp_ms,
            };
            if let Some(old) = self.issued.iter_mut().find(|e| e.did == w.did) {
                if old.issued.kinds != w.kinds {
                    self.revoke_outbox.push(old.issued.token.clone());
                }
                old.issued = issued.clone();
            } else {
                self.issued.push(IssuedEntry {
                    did: w.did.clone(),
                    issued: issued.clone(),
                });
            }
            out.push((w.did.clone(), issued));
        }
        out
    }

    pub fn issued_for(&self, did: &str) -> Option<&Issued> {
        self.issued.iter().find(|e| e.did == did).map(|e| &e.issued)
    }

    /// The contact is gone: take our token back and drop theirs. A later re-add needs a fresh
    /// token (new tag), which `plan_register` provides.
    pub fn forget_contact(&mut self, did: &str) {
        if let Some(i) = self.issued.iter().position(|e| e.did == did) {
            self.revoke_outbox.push(self.issued.remove(i).issued.token);
        }
        self.held.retain(|e| e.did != did);
    }

    /// Blocking is personal policy; for wakes it is the same as forgetting the contact.
    pub fn block(&mut self, did: &str) {
        self.forget_contact(did);
    }

    /// Tokens to POST to `/v1/revoke` now (kept until [`Self::revoked_ok`]).
    pub fn take_revokes(&self) -> Vec<String> {
        self.revoke_outbox.clone()
    }

    /// The relay answered 204 for this token.
    pub fn revoked_ok(&mut self, token: &str) {
        self.revoke_outbox.retain(|t| t != token);
    }

    // ---- tokens contacts gave us (sending side) ----

    /// Keeps a token a contact's device gave us (one per device; a newer one replaces it and
    /// keeps the throttle). Refuses what is already expired, lives implausibly long or is oversized.
    pub fn hold(
        &mut self,
        did: &str,
        device: [u8; 32],
        token: String,
        exp_ms: u64,
        kinds: Kinds,
        now_ms: u64,
    ) -> bool {
        if token.is_empty()
            || token.len() > MAX_TOKEN_CHARS
            || !kinds.is_valid()
            || exp_ms <= now_ms
            || exp_ms > now_ms.saturating_add(MAX_TOKEN_LIFE_MS)
        {
            return false;
        }
        match self.held_mut(did, &device) {
            Some(h) => {
                h.token = token;
                h.exp_ms = exp_ms;
                h.kinds = kinds;
            }
            None => self.held.push(HeldEntry {
                did: did.to_owned(),
                device,
                held: Held {
                    token,
                    exp_ms,
                    kinds,
                    last_wake_ms: None,
                    last_call_wake_ms: None,
                    msg_wakes: 0,
                },
            }),
        }
        true
    }

    fn held_mut(&mut self, did: &str, device: &[u8; 32]) -> Option<&mut Held> {
        self.held
            .iter_mut()
            .find(|e| e.did == did && e.device == *device)
            .map(|e| &mut e.held)
    }

    /// The token to POST to `/v1/wake` for this device, if we hold a live one that allows `kind`
    /// and we have not woken it too recently (`msg`: [`msg_wake_due`]; `call`:
    /// [`CALL_WAKE_EVERY_MS`]). Records the wake when it returns a token.
    pub fn wake_for(
        &mut self,
        did: &str,
        device: &[u8; 32],
        kind: Kind,
        now_ms: u64,
    ) -> Option<String> {
        let h = self.held_mut(did, device)?;
        if h.exp_ms <= now_ms || !h.kinds.allows(kind) {
            return None;
        }
        match kind {
            Kind::Msg => {
                if !msg_wake_due(h.last_wake_ms, h.msg_wakes, now_ms) {
                    return None;
                }
                h.last_wake_ms = Some(now_ms);
                h.msg_wakes = h.msg_wakes.saturating_add(1);
            }
            Kind::Call => {
                if h.last_call_wake_ms
                    .is_some_and(|t| now_ms < t.saturating_add(CALL_WAKE_EVERY_MS))
                {
                    return None;
                }
                h.last_call_wake_ms = Some(now_ms);
            }
        }
        Some(h.token.clone())
    }

    /// `did` was reached (a dial connected): its `msg` wake schedule starts over, so a later
    /// silence wakes promptly again (the 30 s floor stays). Whether anything changed.
    pub fn reached(&mut self, did: &str) -> bool {
        let mut changed = false;
        for e in self
            .held
            .iter_mut()
            .filter(|e| e.did == did && e.held.msg_wakes != 0)
        {
            e.held.msg_wakes = 0;
            changed = true;
        }
        changed
    }

    /// One device of `did` was reached (an own-device session came up): only its `msg` wake
    /// schedule starts over. Whether anything changed.
    pub fn reached_device(&mut self, did: &str, device: &[u8; 32]) -> bool {
        match self.held_mut(did, device) {
            Some(h) if h.msg_wakes != 0 => {
                h.msg_wakes = 0;
                true
            }
            _ => false,
        }
    }

    /// Drops the token one device of `did` gave us (it said it has none any more). Whether one
    /// was held.
    pub fn drop_held(&mut self, did: &str, device: &[u8; 32]) -> bool {
        let before = self.held.len();
        self.held.retain(|e| !(e.did == did && e.device == *device));
        self.held.len() != before
    }

    /// Learns from the relay's HTTP status for a wake: 410 the token is dead, drop it; 403 the
    /// kind is not allowed, forget that kind; anything else (202, 429, 5xx) changes nothing.
    pub fn wake_result(&mut self, did: &str, device: &[u8; 32], kind: Kind, status: u16) {
        match status {
            410 => self.held.retain(|e| !(e.did == did && e.device == *device)),
            403 => {
                let Some(h) = self.held_mut(did, device) else {
                    return;
                };
                match without(h.kinds, kind) {
                    Some(k) => h.kinds = k,
                    None => self.held.retain(|e| !(e.did == did && e.device == *device)),
                }
            }
            _ => {}
        }
    }

    // ---- being woken ----

    /// Whether our hint key opens `hint_text`: the push is for this account, whatever it means.
    pub fn opens(&self, hint_text: &str) -> bool {
        hint_opens(&self.hint_key, hint_text)
    }

    /// What a push means. The hint names a tag; the tag names the contact we issued that token
    /// to. Undecodable hints and tags we no longer issue (revoked, forgotten) are refused,
    /// blocked contacts too (a call as `IgnoreCall`, for CallKit), stale calls are not rung. A
    /// `msg` wake is a dial-back (decided on; never to a blocked contact). A tag of one of our
    /// ticket's code tokens is a [`WakeAction::CodeWake`] while the code lives and its throttle
    /// allows (the throttle is why this takes `&mut self`).
    pub fn on_wake(
        &mut self,
        hint_text: &str,
        now_ms: u64,
        is_blocked: impl Fn(&str) -> bool,
    ) -> WakeAction {
        let Ok(hint) = hint::open(&self.hint_key, hint_text) else {
            return WakeAction::Ignore;
        };
        let refuse = match hint.kind {
            Kind::Msg => WakeAction::Ignore,
            Kind::Call => WakeAction::IgnoreCall,
        };
        if let Some(c) = self.codes.iter_mut().find(|c| c.tag == hint.tag) {
            // Message only; the sender is unknown, so there is no block to check.
            if hint.kind != Kind::Msg || c.exp_ms <= now_ms {
                return refuse;
            }
            c.wakes
                .retain(|t| now_ms < t.saturating_add(CODE_WAKE_WINDOW_MS) && *t <= now_ms);
            if c.wakes.len() >= CODE_WAKE_CAP {
                return WakeAction::Ignore;
            }
            c.wakes.push(now_ms);
            return WakeAction::CodeWake;
        }
        let Some(e) = self.issued.iter().find(|e| e.issued.tag == hint.tag) else {
            return refuse;
        };
        if is_blocked(&e.did) || hint.is_stale_call(now_ms) || !e.issued.kinds.allows(hint.kind) {
            return refuse;
        }
        let did = e.did.clone();
        match hint.kind {
            Kind::Msg => WakeAction::DialBack { did },
            Kind::Call => WakeAction::Ring { did },
        }
    }
}
