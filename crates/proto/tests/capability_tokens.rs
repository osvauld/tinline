//! Packet W2-C model: SEPARATE MESSAGE / CALL CAPABILITY TOKENS (user decision U13,
//! `docs/design/rotation-open-decisions.md` §0). Test-only candidate semantics, NOT a production
//! wire format or policy decision.
//!
//! What U13 (with U5 revised, U3, U10, U12 detail) fixes and this model encodes as given:
//! - Two capabilities, `message` and `call`. A contact may hold message only.
//! - The issuer (our DID) is the only verifier, so revoking is local: the issuer refuses.
//! - Defaults: ticket/QR contact -> message+call; group chat-request contact (U3) -> message only;
//!   "allow calls" sends the call token; existing `call` grants migrate to message+call.
//! - Blocking revokes both and sends one DID-signed visible notice (U5 revised); the same reason
//!   is returned on every later attempt (§8b.1); refusal is immediate (U12 detail).
//! - Every signature is verified (U10); real Ed25519 via `identity`.
//!
//! Today (verified in source): `proto::Grant` has one cap, `CAP_CALL = "call"`
//! (`crates/proto/src/lib.rs:34`, issued `:542`, checked `:570`); core issues it with a 1-year
//! TTL (`crates/core/src/node.rs:33`) and renews it when the callee answers (`node.rs:2753`). The
//! legacy-grant tests below call the REAL `proto::issue_grant` / `proto::verify_grant`.
//!
//! Model choices that are NOT decisions (flagged where used):
//! - Token shape: separate single-cap tokens vs one token with a cap set (`Shape`, gate
//!   G-CAPSHAPE — a new gate id proposed by this packet).
//! - Verification is a deny-list (revoked ids), not an allow-list of issued ids, so a sibling
//!   install honours tokens it has not yet seen issued. `issued` is bookkeeping for revocation.
//! - Downgrading a migrated contact's legacy grant: by presented id vs per-DID cut-off
//!   (`LegacyRevocation`, G-REVOCATION).
//! - Message-token renewal: never vs on activity (`MsgRenewal`, G-RENEWAL).
//! - A block keeps the contact entry (unblock semantics are open, §8b.5).
//! - Every install holds the shared mnemonic: nothing here revokes a hostile linked device.
//!
//! Fake `u64` ticks, deterministic fixture identities, in-memory only.
use std::collections::{BTreeMap, BTreeSet, HashSet};

use identity::Identity;
use serde::{Deserialize, Serialize};

#[path = "support/fixtures.rs"]
mod fixtures;
use fixtures::*;

const CAP_DOMAIN: &[u8] = b"test-only/w2c-cap-token/v1\0";
const NOTICE_DOMAIN: &[u8] = b"test-only/w2c-block-notice/v1\0";
/// Copied from `crates/proto/src/lib.rs:26` (private there) to hand-sign proto grants.
const PROTO_GRANT_DOMAIN: &[u8] = b"osvauld/p2p/grant/v1\0";
const MESSAGE: &str = "message";
const CALL: &str = "call";
/// Mirrors `GRANT_TTL` (`crates/core/src/node.rs:33`).
const TTL: u64 = 365 * 24 * 3600;
const T0: u64 = 1_000_000;

const ALICE: u8 = 1;
const CAROL: u8 = 3;
const DAVE: u8 = 4;
const MALLORY: u8 = 9;

// ---- tokens ------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct CapClaims {
    v: u8,
    id: String,
    iss: String,
    holder: String,
    caps: Vec<String>,
    iat: u64,
    exp: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CapToken {
    payload: Vec<u8>,
    sig: Vec<u8>,
}

fn sign_token(id: &Identity, c: &CapClaims) -> CapToken {
    let payload = serde_json::to_vec(c).unwrap();
    let sig = id.sign(&[CAP_DOMAIN, &payload].concat()).to_vec();
    CapToken { payload, sig }
}

/// The signer is whoever the payload claims as `iss`; callers then check it is themselves.
fn open_token(t: &CapToken) -> Option<CapClaims> {
    let c: CapClaims = serde_json::from_slice(&t.payload).ok()?;
    let pk = identity::public_key_from_did(&c.iss)?;
    let sig: [u8; 64] = t.sig.as_slice().try_into().ok()?;
    identity::verify(&pk, &[CAP_DOMAIN, &t.payload].concat(), &sig).then_some(c)
}

fn claims(t: &CapToken) -> CapClaims {
    open_token(t).expect("a token we built verifies")
}

// ---- the visible block notice (U5 revised) -----------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct NoticeClaims {
    owner: String,
    blocked: String,
    issued_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BlockNotice {
    claims: NoticeClaims,
    sig: Vec<u8>,
}

fn verify_notice(n: &BlockNotice, owner_did: &str, me_did: &str) -> bool {
    let Some(pk) = identity::public_key_from_did(owner_did) else {
        return false;
    };
    let Ok(sig) = <[u8; 64]>::try_from(n.sig.as_slice()) else {
        return false;
    };
    n.claims.owner == owner_did
        && n.claims.blocked == me_did
        && identity::verify(
            &pk,
            &[NOTICE_DOMAIN, &serde_json::to_vec(&n.claims).unwrap()].concat(),
            &sig,
        )
}

// ---- issuer state ------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Origin {
    /// Added by ticket/QR.
    Ticket,
    /// Accepted a chat request through a group host (U3).
    GroupRequest,
    /// A contact from before U13 that holds a legacy `call` grant.
    Migrated,
}

/// G-CAPSHAPE candidates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    /// One token per capability.
    Separate,
    /// One token carrying the whole capability set.
    CapSet,
}

/// G-REVOCATION candidates for legacy (pre-U13) `call` grants on a downgrade or block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LegacyRevocation {
    /// Revoke the ids of legacy grants we have seen presented (today core keeps no record of the
    /// grants it issued: `crates/core/src/store.rs:166`).
    ById,
    /// Refuse every legacy grant held by that DID from now on.
    PerDid,
}

/// G-RENEWAL candidates for message tokens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MsgRenewal {
    Never,
    /// An accepted message within `window` of expiry earns a fresh message token.
    OnActivity {
        window: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Out {
    Token(CapToken),
    Notice(BlockNotice),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Refusal {
    BadSignature,
    WrongIssuer,
    WrongHolder,
    Expired,
    Revoked,
    NotContact,
    CapMissing,
    Blocked(BlockNotice),
}

impl Refusal {
    /// What the presenter is shown. Only a missing call cap and a block are explained; anything
    /// else stays the generic refusal (today's `REFUSED`, `crates/core/src/node.rs:67`).
    fn shown(&self) -> &'static str {
        match self {
            Refusal::CapMissing => "calls not allowed",
            Refusal::Blocked(_) => "blocked",
            _ => "not accepted",
        }
    }
}

/// What a credential-holder presents with a call or a chat session.
#[derive(Clone, Debug)]
enum Cred {
    Token(CapToken),
    Legacy(proto::SignedGrant),
}

/// One install's durable state. `outbox`, `rings` and `delivered` are effects, not synced.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
struct State {
    contacts: BTreeMap<String, Origin>,
    issued: BTreeMap<String, CapClaims>,
    revoked: BTreeSet<String>,
    legacy_seen: BTreeMap<String, String>,
    legacy_dead: BTreeSet<String>,
    blocked: BTreeMap<String, BlockNotice>,
    outbox: Vec<(String, Out)>,
    rings: Vec<String>,
    delivered: Vec<String>,
    next_id: u64,
}

struct Install {
    id: Identity,
    tag: &'static str,
    shape: Shape,
    legacy: LegacyRevocation,
    renewal: MsgRenewal,
    st: State,
}

impl Install {
    fn new(seed: u8, tag: &'static str) -> Self {
        Install {
            id: owner(seed),
            tag,
            shape: Shape::Separate,
            legacy: LegacyRevocation::PerDid,
            renewal: MsgRenewal::Never,
            st: State::default(),
        }
    }

    fn did(&self) -> String {
        self.id.did().to_string()
    }

    fn issue(&mut self, holder: &str, caps: &[&str], now: u64) -> CapToken {
        self.st.next_id += 1;
        let c = CapClaims {
            v: 1,
            id: format!("{}-{}", self.tag, self.st.next_id),
            iss: self.did(),
            holder: holder.to_string(),
            caps: caps.iter().map(|c| c.to_string()).collect(),
            iat: now,
            exp: now + TTL,
        };
        let t = sign_token(&self.id, &c);
        self.st.issued.insert(c.id.clone(), c);
        self.st
            .outbox
            .push((holder.to_string(), Out::Token(t.clone())));
        t
    }

    fn live_ids(&self, holder: &str, cap: &str) -> Vec<String> {
        self.st
            .issued
            .values()
            .filter(|c| c.holder == holder && c.caps.iter().any(|x| x == cap))
            .filter(|c| !self.st.revoked.contains(&c.id))
            .map(|c| c.id.clone())
            .collect()
    }

    fn add_contact(&mut self, holder: &str, origin: Origin, now: u64) -> Vec<CapToken> {
        self.st.contacts.insert(holder.to_string(), origin);
        match (origin, self.shape) {
            (Origin::Ticket, Shape::Separate) => vec![
                self.issue(holder, &[MESSAGE], now),
                self.issue(holder, &[CALL], now),
            ],
            (Origin::Ticket, Shape::CapSet) => vec![self.issue(holder, &[MESSAGE, CALL], now)],
            (Origin::GroupRequest, _) => vec![self.issue(holder, &[MESSAGE], now)],
            (Origin::Migrated, _) => vec![],
        }
    }

    fn allow_calls(&mut self, holder: &str, now: u64) -> Result<Vec<CapToken>, Refusal> {
        if let Some(n) = self.st.blocked.get(holder) {
            return Err(Refusal::Blocked(n.clone()));
        }
        if !self.st.contacts.contains_key(holder) {
            return Err(Refusal::NotContact);
        }
        if !self.live_ids(holder, CALL).is_empty() {
            return Ok(vec![]);
        }
        Ok(match self.shape {
            Shape::Separate => vec![self.issue(holder, &[CALL], now)],
            Shape::CapSet => {
                for id in self.live_ids(holder, MESSAGE) {
                    self.st.revoked.insert(id);
                }
                vec![self.issue(holder, &[MESSAGE, CALL], now)]
            }
        })
    }

    fn kill_legacy(&mut self, holder: &str) {
        match self.legacy {
            LegacyRevocation::ById => {
                let ids: Vec<String> = self
                    .st
                    .legacy_seen
                    .iter()
                    .filter(|(_, h)| h.as_str() == holder)
                    .map(|(id, _)| id.clone())
                    .collect();
                self.st.revoked.extend(ids);
            }
            LegacyRevocation::PerDid => {
                self.st.legacy_dead.insert(holder.to_string());
            }
        }
    }

    /// "Disallow calls": the contact keeps messaging.
    fn disallow_calls(&mut self, holder: &str, now: u64) -> Vec<CapToken> {
        for id in self.live_ids(holder, CALL) {
            self.st.revoked.insert(id);
        }
        self.kill_legacy(holder);
        if self.live_ids(holder, MESSAGE).is_empty() {
            vec![self.issue(holder, &[MESSAGE], now)]
        } else {
            vec![]
        }
    }

    fn block(&mut self, holder: &str, now: u64) -> BlockNotice {
        let ids: Vec<String> = self
            .st
            .issued
            .values()
            .filter(|c| c.holder == holder)
            .map(|c| c.id.clone())
            .collect();
        self.st.revoked.extend(ids);
        self.kill_legacy(holder);
        let claims = NoticeClaims {
            owner: self.did(),
            blocked: holder.to_string(),
            issued_at: now,
        };
        let sig = self
            .id
            .sign(&[NOTICE_DOMAIN, &serde_json::to_vec(&claims).unwrap()].concat())
            .to_vec();
        let n = BlockNotice { claims, sig };
        self.st.blocked.insert(holder.to_string(), n.clone());
        self.st
            .outbox
            .push((holder.to_string(), Out::Notice(n.clone())));
        n
    }

    /// Verifies `cred` for `cap` from the transport-authenticated `presenter`. On refusal the
    /// state is untouched. On success: a call rings, a message is delivered, and the message
    /// renewal policy may issue a fresh message token (returned and queued).
    fn admit(
        &mut self,
        presenter: &str,
        cred: &Cred,
        cap: &str,
        now: u64,
    ) -> Result<Vec<CapToken>, Refusal> {
        if let Some(n) = self.st.blocked.get(presenter) {
            return Err(Refusal::Blocked(n.clone()));
        }
        if !self.st.contacts.contains_key(presenter) {
            return Err(Refusal::NotContact);
        }
        let me = self.did();
        let (exp, legacy_id) = match cred {
            Cred::Token(t) => {
                let c = open_token(t).ok_or(Refusal::BadSignature)?;
                if c.iss != me {
                    return Err(Refusal::WrongIssuer);
                }
                if c.holder != presenter {
                    return Err(Refusal::WrongHolder);
                }
                if self.st.revoked.contains(&c.id) {
                    return Err(Refusal::Revoked);
                }
                if now >= c.exp {
                    return Err(Refusal::Expired);
                }
                if !c.caps.iter().any(|x| x == cap) {
                    return Err(Refusal::CapMissing);
                }
                (Some(c.exp), None)
            }
            Cred::Legacy(g) => {
                let revoked: HashSet<String> = self.st.revoked.iter().cloned().collect();
                let grant =
                    proto::verify_grant(g, &me, presenter, now, &revoked).map_err(|e| match e {
                        proto::Error::WrongIssuer => Refusal::WrongIssuer,
                        proto::Error::WrongHolder => Refusal::WrongHolder,
                        proto::Error::Expired => Refusal::Expired,
                        proto::Error::Revoked => Refusal::Revoked,
                        _ => Refusal::BadSignature,
                    })?;
                if self.st.legacy_dead.contains(presenter) {
                    return Err(Refusal::Revoked);
                }
                // U13 migration: a legacy `call` grant means message+call.
                (None, Some(grant.id))
            }
        };
        if let Some(id) = legacy_id {
            self.st.legacy_seen.insert(id, presenter.to_string());
        }
        let mut renewed = vec![];
        if cap == CALL {
            self.st.rings.push(presenter.to_string());
        } else {
            self.st.delivered.push(presenter.to_string());
            if let (MsgRenewal::OnActivity { window }, Some(exp)) = (self.renewal, exp)
                && exp.saturating_sub(now) <= window
            {
                renewed.push(self.issue(presenter, &[MESSAGE], now));
            }
        }
        Ok(renewed)
    }

    /// Today's rule, kept for the call token: answering a ringing call sends the caller a fresh
    /// call token (`crates/core/src/node.rs:2753`).
    fn answer(&mut self, caller: &str, now: u64) -> Option<CapToken> {
        if self.st.rings.last().map(String::as_str) != Some(caller)
            || self.st.blocked.contains_key(caller)
        {
            return None;
        }
        let caps: &[&str] = match self.shape {
            Shape::Separate => &[CALL],
            Shape::CapSet => &[MESSAGE, CALL],
        };
        Some(self.issue(caller, caps, now))
    }

    /// Own-device sync of the durable sets: union, never shrinks (like `revoked` in the account
    /// doc today, `crates/core/src/sync/accdoc.rs:241`, `:400`).
    fn sync_from(&mut self, other: &State) {
        for (d, o) in &other.contacts {
            self.st.contacts.entry(d.clone()).or_insert(*o);
        }
        for (id, c) in &other.issued {
            self.st
                .issued
                .entry(id.clone())
                .or_insert_with(|| c.clone());
        }
        self.st.revoked.extend(other.revoked.iter().cloned());
        for (id, h) in &other.legacy_seen {
            self.st
                .legacy_seen
                .entry(id.clone())
                .or_insert_with(|| h.clone());
        }
        self.st
            .legacy_dead
            .extend(other.legacy_dead.iter().cloned());
        for (d, n) in &other.blocked {
            self.st
                .blocked
                .entry(d.clone())
                .or_insert_with(|| n.clone());
        }
    }
}

/// The newest unrevoked token in the outbox for `holder` that carries `cap` (what the holder
/// would present).
fn latest(st: &State, holder: &str, cap: &str) -> CapToken {
    st.outbox
        .iter()
        .rev()
        .filter(|(to, _)| to == holder)
        .find_map(|(_, o)| match o {
            Out::Token(t) => {
                let c = claims(t);
                (c.caps.iter().any(|x| x == cap) && !st.revoked.contains(&c.id)).then(|| t.clone())
            }
            Out::Notice(_) => None,
        })
        .expect("holder has a live token for that cap")
}

fn tok(t: &CapToken) -> Cred {
    Cred::Token(t.clone())
}

/// Asserts `admit` refuses with `want` and leaves the whole state unchanged.
fn refused(i: &mut Install, presenter: &str, cred: &Cred, cap: &str, now: u64, want: Refusal) {
    let before = i.st.clone();
    assert_eq!(i.admit(presenter, cred, cap, now), Err(want));
    assert_eq!(i.st, before, "a refusal must not change state");
}

fn hand_signed_proto_grant(iss: &Identity, holder: &str, cap: &str) -> proto::SignedGrant {
    let g = proto::Grant {
        v: 1,
        id: "hand-signed".into(),
        iss: iss.did().to_string(),
        holder: holder.to_string(),
        cap: cap.to_string(),
        iat: T0,
        exp: T0 + TTL,
    };
    let payload = serde_json::to_vec(&g).unwrap();
    proto::SignedBlob {
        signature: base64_encode(&iss.sign(&[PROTO_GRANT_DOMAIN, &payload].concat())),
        payload: base64_encode(&payload),
    }
}

// ---- 1. issuer-only verification ---------------------------------------------------------------

// KIND: invariant
// Under both token shapes a ticket contact's tokens are signed by our DID for that holder, and
// the issuer admits the holder for exactly the caps the tokens carry; a call rings, a message is
// delivered. Nobody but the issuer is asked (there is no third-party verifier in the model).
#[test]
fn issued_tokens_admit_their_holder_for_their_caps_under_both_shapes() {
    for shape in [Shape::Separate, Shape::CapSet] {
        let mut a = Install::new(ALICE, "a1");
        a.shape = shape;
        let carol = did(CAROL);
        let toks = a.add_contact(&carol, Origin::Ticket, T0);
        assert_eq!(toks.len(), if shape == Shape::Separate { 2 } else { 1 });
        for t in &toks {
            let c = claims(t);
            assert_eq!(
                (c.iss.as_str(), c.holder.as_str()),
                (a.did().as_str(), carol.as_str())
            );
            assert_eq!(c.exp - c.iat, TTL);
        }
        let m = latest(&a.st, &carol, MESSAGE);
        let k = latest(&a.st, &carol, CALL);
        assert_eq!(a.admit(&carol, &tok(&m), MESSAGE, T0 + 1), Ok(vec![]));
        assert_eq!(a.admit(&carol, &tok(&k), CALL, T0 + 2), Ok(vec![]));
        assert_eq!(a.st.delivered, vec![carol.clone()]);
        assert_eq!(a.st.rings, vec![carol.clone()]);
    }
}

// KIND: invariant
// Forged, foreign-issuer, wrong-holder, wrong-cap, expired, revoked and non-contact credentials
// are all refused, each with the whole issuer state unchanged.
#[test]
fn bad_credentials_are_refused_with_state_unchanged() {
    let mut a = Install::new(ALICE, "a1");
    let (carol, dave, mallory) = (did(CAROL), did(DAVE), did(MALLORY));
    a.add_contact(&carol, Origin::Ticket, T0);
    a.add_contact(&dave, Origin::GroupRequest, T0);
    let c_msg = latest(&a.st, &carol, MESSAGE);
    let c_call = latest(&a.st, &carol, CALL);
    let d_msg = latest(&a.st, &dave, MESSAGE);

    // Forged: Carol stretches her token's expiry under the old signature.
    let mut forged_claims = claims(&c_call);
    forged_claims.exp += 10 * TTL;
    let forged = CapToken {
        payload: serde_json::to_vec(&forged_claims).unwrap(),
        sig: c_call.sig.clone(),
    };
    refused(
        &mut a,
        &carol,
        &tok(&forged),
        CALL,
        T0 + 1,
        Refusal::BadSignature,
    );
    // Foreign issuer: Mallory signs a call token for Dave under her own DID.
    let mal = Install::new(MALLORY, "m1");
    let foreign = sign_token(
        &mal.id,
        &CapClaims {
            caps: vec![CALL.into()],
            iss: mallory.clone(),
            holder: dave.clone(),
            ..claims(&d_msg)
        },
    );
    refused(
        &mut a,
        &dave,
        &tok(&foreign),
        CALL,
        T0 + 1,
        Refusal::WrongIssuer,
    );
    // Wrong holder: Dave presents Carol's call token.
    refused(
        &mut a,
        &dave,
        &tok(&c_call),
        CALL,
        T0 + 1,
        Refusal::WrongHolder,
    );
    // Wrong cap: Dave's message token for a call.
    refused(
        &mut a,
        &dave,
        &tok(&d_msg),
        CALL,
        T0 + 1,
        Refusal::CapMissing,
    );
    // Expired.
    refused(
        &mut a,
        &carol,
        &tok(&c_msg),
        MESSAGE,
        T0 + TTL,
        Refusal::Expired,
    );
    // Not a contact (even with a token of ours for someone).
    refused(
        &mut a,
        &mallory,
        &tok(&c_msg),
        MESSAGE,
        T0 + 1,
        Refusal::NotContact,
    );
    // Revoked.
    a.disallow_calls(&carol, T0 + 2);
    refused(
        &mut a,
        &carol,
        &tok(&c_call),
        CALL,
        T0 + 3,
        Refusal::Revoked,
    );
    assert!(a.st.rings.is_empty() && a.st.delivered.is_empty());
}

// ---- 2. defaults, upgrade, downgrade -----------------------------------------------------------

// KIND: invariant
// U13 defaults: a ticket/QR contact gets message+call; a group chat-request contact (U3) gets
// message only.
#[test]
fn defaults_ticket_gets_message_and_call_group_request_gets_message_only() {
    let mut a = Install::new(ALICE, "a1");
    let (carol, dave) = (did(CAROL), did(DAVE));
    a.add_contact(&carol, Origin::Ticket, T0);
    a.add_contact(&dave, Origin::GroupRequest, T0);
    let caps_of = |st: &State, h: &str| -> BTreeSet<String> {
        st.issued
            .values()
            .filter(|c| c.holder == h)
            .flat_map(|c| c.caps.clone())
            .collect()
    };
    assert_eq!(
        caps_of(&a.st, &carol),
        [MESSAGE, CALL].iter().map(|s| s.to_string()).collect()
    );
    assert_eq!(
        caps_of(&a.st, &dave),
        [MESSAGE].iter().map(|s| s.to_string()).collect()
    );
}

// KIND: invariant
// A message-only contact: messages are accepted; a call is refused with a visible reason
// ("calls not allowed", not the generic refusal) and never rings. State unchanged by the refusal.
#[test]
fn message_only_contact_messages_but_call_is_refused_visibly_without_ring() {
    let mut a = Install::new(ALICE, "a1");
    let dave = did(DAVE);
    a.add_contact(&dave, Origin::GroupRequest, T0);
    let m = latest(&a.st, &dave, MESSAGE);
    assert_eq!(a.admit(&dave, &tok(&m), MESSAGE, T0 + 1), Ok(vec![]));
    let before = a.st.clone();
    let err = a.admit(&dave, &tok(&m), CALL, T0 + 2).unwrap_err();
    assert_eq!(err, Refusal::CapMissing);
    assert_eq!(err.shown(), "calls not allowed");
    assert_eq!(a.st, before);
    assert!(a.st.rings.is_empty());
}

// KIND: invariant
// "Allow calls" sends a call token and the next call rings; "disallow calls" revokes it and the
// next call is refused, while messaging keeps working (with whatever message token is live).
// Holds under both shapes.
#[test]
fn allow_calls_upgrades_and_disallow_downgrades_under_both_shapes() {
    for shape in [Shape::Separate, Shape::CapSet] {
        let mut a = Install::new(ALICE, "a1");
        a.shape = shape;
        let dave = did(DAVE);
        a.add_contact(&dave, Origin::GroupRequest, T0);
        let sent = a.allow_calls(&dave, T0 + 10).unwrap();
        assert_eq!(sent.len(), 1);
        assert!(claims(&sent[0]).caps.iter().any(|c| c == CALL));
        assert_eq!(a.admit(&dave, &tok(&sent[0]), CALL, T0 + 11), Ok(vec![]));
        assert_eq!(a.st.rings, vec![dave.clone()]);

        a.disallow_calls(&dave, T0 + 20);
        refused(
            &mut a,
            &dave,
            &tok(&sent[0]),
            CALL,
            T0 + 21,
            Refusal::Revoked,
        );
        let m = latest(&a.st, &dave, MESSAGE);
        assert_eq!(a.admit(&dave, &tok(&m), MESSAGE, T0 + 22), Ok(vec![]));
        assert_eq!(a.st.rings, vec![dave.clone()], "no second ring");
    }
}

// KIND: candidate-policy G-CAPSHAPE
// Option "separate tokens": a downgrade is one revocation and sends nothing; the message token
// is untouched, so a holder that never hears of the downgrade keeps messaging.
#[test]
fn shape_separate_downgrade_is_one_revocation_and_leaves_messaging_alone() {
    let mut a = Install::new(ALICE, "a1");
    let carol = did(CAROL);
    a.add_contact(&carol, Origin::Ticket, T0);
    let m = latest(&a.st, &carol, MESSAGE);
    let (revoked_before, outbox_before) = (a.st.revoked.len(), a.st.outbox.len());
    let sent = a.disallow_calls(&carol, T0 + 5);
    assert!(sent.is_empty());
    assert_eq!(a.st.revoked.len(), revoked_before + 1);
    assert_eq!(a.st.outbox.len(), outbox_before);
    assert_eq!(a.admit(&carol, &tok(&m), MESSAGE, T0 + 6), Ok(vec![]));
}

// KIND: candidate-policy G-CAPSHAPE
// Option "one token with a cap set": a downgrade revokes the message+call token and must send a
// new message-only token. Until that token reaches the holder, its old token is refused for
// MESSAGES too: a downgrade briefly cuts messaging (offline holder: until delivery).
#[test]
fn shape_cap_set_downgrade_reissues_and_cuts_messaging_until_delivery() {
    let mut a = Install::new(ALICE, "a1");
    a.shape = Shape::CapSet;
    let carol = did(CAROL);
    let old = a.add_contact(&carol, Origin::Ticket, T0).remove(0);
    let sent = a.disallow_calls(&carol, T0 + 5);
    assert_eq!(sent.len(), 1);
    assert_eq!(claims(&sent[0]).caps, vec![MESSAGE.to_string()]);
    refused(
        &mut a,
        &carol,
        &tok(&old),
        MESSAGE,
        T0 + 6,
        Refusal::Revoked,
    );
    assert_eq!(a.admit(&carol, &tok(&sent[0]), MESSAGE, T0 + 7), Ok(vec![]));
}

// ---- 3. block ----------------------------------------------------------------------------------

// KIND: invariant
// Blocking revokes every token of that holder and queues exactly one DID-signed visible notice
// and no token. Refusal is immediate (the same tick) for message and call, with the notice as
// reason; "allow calls" cannot re-arm a blocked contact. State unchanged by every refusal.
#[test]
fn block_revokes_both_sends_a_signed_notice_and_refuses_at_once() {
    let mut a = Install::new(ALICE, "a1");
    let carol = did(CAROL);
    a.add_contact(&carol, Origin::Ticket, T0);
    let m = latest(&a.st, &carol, MESSAGE);
    let k = latest(&a.st, &carol, CALL);
    let outbox_before = a.st.outbox.len();
    let notice = a.block(&carol, T0 + 10);

    assert!(verify_notice(&notice, &a.did(), &carol));
    assert!(!verify_notice(&notice, &did(MALLORY), &carol));
    let new_out: Vec<_> = a.st.outbox[outbox_before..].to_vec();
    assert_eq!(new_out, vec![(carol.clone(), Out::Notice(notice.clone()))]);
    for c in a.st.issued.values().filter(|c| c.holder == carol) {
        assert!(a.st.revoked.contains(&c.id));
    }
    refused(
        &mut a,
        &carol,
        &tok(&m),
        MESSAGE,
        T0 + 10,
        Refusal::Blocked(notice.clone()),
    );
    refused(
        &mut a,
        &carol,
        &tok(&k),
        CALL,
        T0 + 10,
        Refusal::Blocked(notice.clone()),
    );
    let before = a.st.clone();
    assert_eq!(
        a.allow_calls(&carol, T0 + 11),
        Err(Refusal::Blocked(notice.clone()))
    );
    assert_eq!(a.st, before);
    assert!(a.st.rings.is_empty() && a.st.delivered.is_empty());
}

// KIND: invariant
// Every later attempt gets the same notice (§8b.1) and nothing new is queued for the blocked DID;
// a call from the blocked DID is never answered/renewed; other contacts are unaffected.
#[test]
fn blocked_holder_gets_the_same_notice_every_time_and_others_are_unaffected() {
    let mut a = Install::new(ALICE, "a1");
    let (carol, dave) = (did(CAROL), did(DAVE));
    a.add_contact(&carol, Origin::Ticket, T0);
    a.add_contact(&dave, Origin::Ticket, T0);
    let k = latest(&a.st, &carol, CALL);
    let n = a.block(&carol, T0 + 1);
    let outbox_after_block = a.st.outbox.clone();
    for t in [T0 + 2, T0 + 1000, T0 + TTL + 5] {
        for cap in [MESSAGE, CALL] {
            assert_eq!(
                a.admit(&carol, &tok(&k), cap, t),
                Err(Refusal::Blocked(n.clone()))
            );
        }
    }
    assert_eq!(a.answer(&carol, T0 + 3), None);
    assert_eq!(a.st.outbox, outbox_after_block);
    let dk = latest(&a.st, &dave, CALL);
    assert_eq!(a.admit(&dave, &tok(&dk), CALL, T0 + 4), Ok(vec![]));
}

// ---- 4. migration of today's `call` grant ------------------------------------------------------

// KIND: invariant
// U13 migration: a REAL legacy grant (`proto::issue_grant`, cap "call", 1-year TTL) is admitted
// for both message and call. Its forged / wrong-holder / expired forms are refused by the REAL
// `proto::verify_grant`, state unchanged. A block refuses it too.
#[test]
fn legacy_call_grant_migrates_to_message_and_call() {
    let mut a = Install::new(ALICE, "a1");
    let (carol, dave) = (did(CAROL), did(DAVE));
    a.add_contact(&carol, Origin::Migrated, T0);
    a.add_contact(&dave, Origin::Migrated, T0);
    let g = proto::issue_grant(&a.id, &carol, T0, TTL);
    let real = proto::verify_grant(&g, &a.did(), &carol, T0, &HashSet::new()).unwrap();
    assert_eq!((real.cap.as_str(), real.exp - real.iat), ("call", TTL));

    assert_eq!(
        a.admit(&carol, &Cred::Legacy(g.clone()), MESSAGE, T0 + 1),
        Ok(vec![])
    );
    assert_eq!(
        a.admit(&carol, &Cred::Legacy(g.clone()), CALL, T0 + 2),
        Ok(vec![])
    );

    let mut forged = g.clone();
    forged.signature = proto::issue_grant(&a.id, &carol, T0, TTL).signature;
    refused(
        &mut a,
        &carol,
        &Cred::Legacy(forged),
        CALL,
        T0 + 3,
        Refusal::BadSignature,
    );
    refused(
        &mut a,
        &dave,
        &Cred::Legacy(g.clone()),
        CALL,
        T0 + 3,
        Refusal::WrongHolder,
    );
    refused(
        &mut a,
        &carol,
        &Cred::Legacy(g.clone()),
        MESSAGE,
        T0 + TTL,
        Refusal::Expired,
    );
    let n = a.block(&carol, T0 + 4);
    refused(
        &mut a,
        &carol,
        &Cred::Legacy(g),
        MESSAGE,
        T0 + 5,
        Refusal::Blocked(n),
    );
}

// KIND: candidate-policy G-REVOCATION
// Option "revoke legacy grants by presented id". FAILS the downgrade: today every answered call
// issues a fresh grant with a new id (`node.rs:2753`), and the holder may keep older ones (each
// valid for a year). Revoking the ids we happen to have seen leaves an unseen older grant that
// still rings. (And a contact that presented nothing before the downgrade loses nothing.)
#[test]
fn legacy_downgrade_by_presented_id_leaves_unseen_grants_valid() {
    let mut a = Install::new(ALICE, "a1");
    a.legacy = LegacyRevocation::ById;
    let carol = did(CAROL);
    a.add_contact(&carol, Origin::Migrated, T0);
    let g1 = proto::issue_grant(&a.id, &carol, T0, TTL);
    let g2 = proto::issue_grant(&a.id, &carol, T0 + 100, TTL); // renewed on an answered call
    assert_eq!(
        a.admit(&carol, &Cred::Legacy(g2.clone()), CALL, T0 + 200),
        Ok(vec![])
    );
    a.disallow_calls(&carol, T0 + 300);
    refused(
        &mut a,
        &carol,
        &Cred::Legacy(g2),
        CALL,
        T0 + 301,
        Refusal::Revoked,
    );
    // The hole: the older, never-presented grant still rings.
    assert_eq!(
        a.admit(&carol, &Cred::Legacy(g1), CALL, T0 + 302),
        Ok(vec![])
    );
    assert_eq!(a.st.rings.len(), 2);
}

// KIND: candidate-policy G-REVOCATION
// Option "per-DID legacy cut-off": after a downgrade every legacy grant of that DID is refused,
// seen or not, and the contact keeps messaging with the message-only token sent at downgrade.
#[test]
fn legacy_downgrade_by_per_did_cutoff_refuses_every_legacy_grant() {
    let mut a = Install::new(ALICE, "a1");
    a.legacy = LegacyRevocation::PerDid;
    let carol = did(CAROL);
    a.add_contact(&carol, Origin::Migrated, T0);
    let g1 = proto::issue_grant(&a.id, &carol, T0, TTL);
    let g2 = proto::issue_grant(&a.id, &carol, T0 + 100, TTL);
    let sent = a.disallow_calls(&carol, T0 + 300);
    assert_eq!(sent.len(), 1);
    assert_eq!(claims(&sent[0]).caps, vec![MESSAGE.to_string()]);
    for g in [g1, g2] {
        refused(
            &mut a,
            &carol,
            &Cred::Legacy(g.clone()),
            CALL,
            T0 + 301,
            Refusal::Revoked,
        );
        refused(
            &mut a,
            &carol,
            &Cred::Legacy(g),
            MESSAGE,
            T0 + 301,
            Refusal::Revoked,
        );
    }
    assert_eq!(
        a.admit(&carol, &tok(&sent[0]), MESSAGE, T0 + 302),
        Ok(vec![])
    );
    refused(
        &mut a,
        &carol,
        &tok(&sent[0]),
        CALL,
        T0 + 303,
        Refusal::CapMissing,
    );
}

// KIND: invariant (flipped by P6; was `todays_proto_verifier_refuses_a_message_cap`)
// Before P6 the proto verifier hard-coded the one cap "call", so a DID-signed `message` grant was
// refused with `BadCapability` and add-contact could not carry a message-only grant. P6 (U13/U15:
// separate tokens): the REAL `proto::issue_token` mints single-capability version-2 tokens; a
// `message` token verifies and carries messaging only, a `call` token calling only; the contact
// exchange carries the message token plus an optional call token (`TokenPolicy`). A hand-signed
// version-1 grant with cap "message" never existed and stays refused; a version-1 "call" grant is
// the legacy grant and carries both (migration, U13).
#[test]
fn proto_verifier_accepts_single_capability_tokens() {
    let alice = owner(ALICE);
    let carol = did(CAROL);
    let none = HashSet::new();
    let msg = proto::issue_token(&alice, &carol, proto::Cap::Message, T0, TTL);
    let g = proto::verify_grant(&msg, alice.did(), &carol, T0, &none).unwrap();
    assert!(g.allows(proto::Cap::Message) && !g.allows(proto::Cap::Call));
    assert_eq!(
        proto::verify_token(&msg, alice.did(), &carol, proto::Cap::Call, 0, T0, &none),
        Err(proto::Error::BadCapability)
    );
    let call = proto::issue_token(&alice, &carol, proto::Cap::Call, T0, TTL);
    let g = proto::verify_grant(&call, alice.did(), &carol, T0, &none).unwrap();
    assert!(g.allows(proto::Cap::Call) && !g.allows(proto::Cap::Message));
    // Legacy v1 "call" grant: both; v1 "message" (never issued): refused.
    let legacy = hand_signed_proto_grant(&alice, &carol, "call");
    let g = proto::verify_grant(&legacy, alice.did(), &carol, T0, &none).unwrap();
    assert!(g.allows(proto::Cap::Call) && g.allows(proto::Cap::Message));
    let v1_msg = hand_signed_proto_grant(&alice, &carol, "message");
    assert_eq!(
        proto::verify_grant(&v1_msg, alice.did(), &carol, T0, &none),
        Err(proto::Error::BadCapability)
    );
    // Per-DID cut-off: a token issued at or before it is taken back, a later one is not.
    assert_eq!(
        proto::verify_token(&legacy, alice.did(), &carol, proto::Cap::Call, T0, T0 + 1, &none),
        Err(proto::Error::Revoked)
    );
    assert!(proto::verify_token(&legacy, alice.did(), &carol, proto::Cap::Message, T0 - 1, T0 + 1, &none).is_ok());
}

// ---- 5. linked devices -------------------------------------------------------------------------

// KIND: invariant
// A token issued on one own install is honoured by a sibling install of the same DID that has
// not yet synced the issuance (verification checks the DID signature, not a local issue record).
#[test]
fn sibling_install_honours_a_token_it_never_saw_issued() {
    let mut a1 = Install::new(ALICE, "a1");
    let mut a2 = Install::new(ALICE, "a2");
    let dave = did(DAVE);
    a1.add_contact(&dave, Origin::GroupRequest, T0);
    a2.sync_from(&a1.st.clone());
    let call = a1.allow_calls(&dave, T0 + 10).unwrap().remove(0);
    assert!(!a2.st.issued.contains_key(&claims(&call).id));
    assert_eq!(a2.admit(&dave, &tok(&call), CALL, T0 + 11), Ok(vec![]));
    assert_eq!(a2.st.rings, vec![dave]);
}

// KIND: limitation
// Sync window: a downgrade or a block made on one install is not enforced by a sibling until the
// revocation set syncs; meanwhile the sibling rings and delivers. EXPECTED-CORRECT (U12 detail:
// "refusal is immediate"): every own install refuses from the moment of the block. After sync
// (union, never shrinks) the sibling refuses with the same signed notice. Shared mnemonic: this
// is not protection against a hostile linked install.
#[test]
fn sibling_admits_revoked_and_blocked_holders_until_sync() {
    let mut a1 = Install::new(ALICE, "a1");
    let mut a2 = Install::new(ALICE, "a2");
    let (carol, dave) = (did(CAROL), did(DAVE));
    a1.add_contact(&carol, Origin::Ticket, T0);
    a1.add_contact(&dave, Origin::GroupRequest, T0);
    let d_call = a1.allow_calls(&dave, T0 + 1).unwrap().remove(0);
    a2.sync_from(&a1.st.clone());
    let c_call = latest(&a1.st, &carol, CALL);
    let c_msg = latest(&a1.st, &carol, MESSAGE);

    a1.disallow_calls(&dave, T0 + 10);
    let n = a1.block(&carol, T0 + 10);
    // a1 refuses at once.
    refused(
        &mut a1,
        &dave,
        &tok(&d_call),
        CALL,
        T0 + 11,
        Refusal::Revoked,
    );
    refused(
        &mut a1,
        &carol,
        &tok(&c_call),
        CALL,
        T0 + 11,
        Refusal::Blocked(n.clone()),
    );
    // a2, not yet synced, rings and delivers: the limitation.
    assert_eq!(a2.admit(&dave, &tok(&d_call), CALL, T0 + 11), Ok(vec![]));
    assert_eq!(a2.admit(&carol, &tok(&c_call), CALL, T0 + 11), Ok(vec![]));
    assert_eq!(a2.admit(&carol, &tok(&c_msg), MESSAGE, T0 + 11), Ok(vec![]));
    assert_eq!(a2.st.rings, vec![dave.clone(), carol.clone()]);
    // After sync a2 refuses, with a1's notice; a sync back changes nothing on a1.
    a2.sync_from(&a1.st.clone());
    refused(
        &mut a2,
        &dave,
        &tok(&d_call),
        CALL,
        T0 + 12,
        Refusal::Revoked,
    );
    refused(
        &mut a2,
        &carol,
        &tok(&c_msg),
        MESSAGE,
        T0 + 12,
        Refusal::Blocked(n),
    );
    let a1_before = a1.st.clone();
    a1.sync_from(&a2.st.clone());
    assert_eq!(a1.st.revoked, a1_before.revoked);
    assert_eq!(a1.st.blocked, a1_before.blocked);
}

// ---- 6. renewal --------------------------------------------------------------------------------

// KIND: candidate-policy G-RENEWAL
// Option "renew message tokens on activity": an accepted message within the window of expiry
// earns a fresh message-only token, so an active contact never lapses; the renewal never adds a
// call cap. Outside the window nothing is issued.
#[test]
fn message_token_renews_on_activity_near_expiry() {
    let mut a = Install::new(ALICE, "a1");
    a.renewal = MsgRenewal::OnActivity { window: TTL / 4 };
    let dave = did(DAVE);
    let m0 = a.add_contact(&dave, Origin::GroupRequest, T0).remove(0);
    assert_eq!(a.admit(&dave, &tok(&m0), MESSAGE, T0 + 10), Ok(vec![]));
    let now = T0 + TTL - 100;
    let renewed = a.admit(&dave, &tok(&m0), MESSAGE, now).unwrap();
    assert_eq!(renewed.len(), 1);
    let c = claims(&renewed[0]);
    assert_eq!(
        (c.caps.clone(), c.iat, c.exp),
        (vec![MESSAGE.to_string()], now, now + TTL)
    );
    refused(
        &mut a,
        &dave,
        &tok(&m0),
        MESSAGE,
        T0 + TTL + 50,
        Refusal::Expired,
    );
    assert_eq!(
        a.admit(&dave, &tok(&renewed[0]), MESSAGE, T0 + TTL + 50),
        Ok(vec![])
    );
    refused(
        &mut a,
        &dave,
        &tok(&renewed[0]),
        CALL,
        T0 + TTL + 51,
        Refusal::CapMissing,
    );
}

// KIND: candidate-policy G-RENEWAL
// Option "fixed TTL, no renewal": activity does not extend a message token; at expiry the
// contact is refused (state unchanged) until we re-issue explicitly — an open UX question.
#[test]
fn message_token_with_fixed_ttl_expires_despite_activity() {
    let mut a = Install::new(ALICE, "a1");
    a.renewal = MsgRenewal::Never;
    let dave = did(DAVE);
    let m0 = a.add_contact(&dave, Origin::GroupRequest, T0).remove(0);
    for t in [T0 + 10, T0 + TTL / 2, T0 + TTL - 1] {
        assert_eq!(a.admit(&dave, &tok(&m0), MESSAGE, t), Ok(vec![]));
    }
    refused(
        &mut a,
        &dave,
        &tok(&m0),
        MESSAGE,
        T0 + TTL,
        Refusal::Expired,
    );
}

// KIND: candidate-policy G-RENEWAL
// Option "keep today's rule for the call token": answering a ringing call sends the caller a
// fresh call token (as `node.rs:2753` does for today's grant), so a contact who keeps calling and
// being answered never lapses. No answer, no renewal; a message-only contact never gets a call
// token this way (it never rings).
#[test]
fn call_token_renews_when_a_call_is_answered() {
    let mut a = Install::new(ALICE, "a1");
    let (carol, dave) = (did(CAROL), did(DAVE));
    a.add_contact(&carol, Origin::Ticket, T0);
    a.add_contact(&dave, Origin::GroupRequest, T0);
    let k0 = latest(&a.st, &carol, CALL);
    let near = T0 + TTL - 100;
    assert_eq!(a.admit(&carol, &tok(&k0), CALL, near), Ok(vec![]));
    let k1 = a.answer(&carol, near + 1).expect("answered: renewed");
    assert_eq!(claims(&k1).caps, vec![CALL.to_string()]);
    assert_eq!(claims(&k1).exp, near + 1 + TTL);
    refused(&mut a, &carol, &tok(&k0), CALL, T0 + TTL, Refusal::Expired);
    assert_eq!(a.admit(&carol, &tok(&k1), CALL, T0 + TTL), Ok(vec![]));

    let dm = latest(&a.st, &dave, MESSAGE);
    let _ = a.admit(&dave, &tok(&dm), CALL, T0 + 5);
    assert_eq!(
        a.answer(&dave, T0 + 6),
        None,
        "a message-only contact never rang"
    );
    assert!(a.live_ids(&dave, CALL).is_empty());
}
