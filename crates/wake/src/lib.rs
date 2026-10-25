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
//! Nothing here carries content: no name, no text. DIDs stay local, as keys of this state.

use serde::{Deserialize, Serialize};
use wake_relay_core::api::{RegisterRequest, RegisterResponse, TagRequest};
use wake_relay_core::hint;
use wake_relay_core::relay::MAX_TAGS_PER_REGISTER;
use wake_relay_core::token::{MAX_TOKEN_CHARS, MAX_TOKEN_LIFE_MS};
pub use wake_relay_core::{Kind, Kinds, Target};

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
/// At most one `call` wake per device this often (PROVISIONAL: the caller redials every ~1 s, a
/// wake should not repeat that fast).
pub const CALL_WAKE_EVERY_MS: u64 = 10_000;

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
        }
    }

    // ---- our own tokens (receiving side) ----

    /// A new or changed push token. Issued tokens are bound to the old one and die with it, so
    /// they are only forgotten (everything is reissued); no revoke is needed.
    pub fn set_target(&mut self, target: Target) {
        if self.target.as_ref() != Some(&target) {
            self.issued.clear();
            self.target = Some(target);
        }
    }

    /// The user went always-on: take every issued token back.
    pub fn clear_target(&mut self) {
        self.target = None;
        for e in self.issued.drain(..) {
            self.revoke_outbox.push(e.issued.token);
        }
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
    /// and we have not woken it too recently. Records the wake when it returns a token.
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
        let (last, every) = match kind {
            Kind::Msg => (&mut h.last_wake_ms, MSG_WAKE_EVERY_MS),
            Kind::Call => (&mut h.last_call_wake_ms, CALL_WAKE_EVERY_MS),
        };
        if last.is_some_and(|t| now_ms < t.saturating_add(every)) {
            return None;
        }
        *last = Some(now_ms);
        Some(h.token.clone())
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

    /// What a push means. The hint names a tag; the tag names the contact we issued that token
    /// to. Undecodable hints and tags we no longer issue (revoked, forgotten) are refused,
    /// blocked contacts too (a call as `IgnoreCall`, for CallKit), stale calls are not rung. A
    /// `msg` wake is a dial-back (decided on; never to a blocked contact).
    pub fn on_wake(
        &self,
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
