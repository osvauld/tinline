//! Real-code checks for message / call capability tokens (`proto::issue_token`,
//! `proto::verify_token`, `Signed<Grant>::redeem`).
//!
//! The candidate-semantics model that used to live here (issuer-only verification, block
//! notices, legacy migration, renewal) moved to the `design-models` branch on 2026-10-14; see
//! `docs/archive/models.md`.
#[path = "support/fixtures.rs"]
mod fixtures;
use fixtures::*;

const TTL: u64 = 365 * 24 * 3600 * 1000;
const T0: u64 = 1_000_000_000;
const ALICE: u8 = 1;
const CAROL: u8 = 3;

/// The issuer's per-holder cut-off.
struct CutOff(u64);

impl proto::IssuanceStore for CutOff {
    fn spent(&self, _: &str, _: &[u8], _: &str) -> bool {
        false
    }
    fn cut_off_ms(&self, _: &str, _: cryptography::domains::Domain) -> u64 {
        self.0
    }
}

// KIND: invariant (flipped by P6; was `todays_proto_verifier_refuses_a_message_cap`)
// P6 (U13/U15: separate tokens): the REAL `proto::issue_token` mints single-capability tokens; a
// `message` token verifies and carries messaging only, a `call` token calling only; the contact
// exchange carries the message token plus an optional call token (`TokenPolicy`).
#[test]
fn proto_verifier_accepts_single_capability_tokens() {
    let alice = owner(ALICE);
    let carol = did(CAROL);
    let none = proto::Unrecorded;
    let msg = proto::issue_token(&alice, &carol, proto::Cap::Message, T0, TTL);
    let g = proto::verify_token(&msg, alice.did(), &carol, proto::Cap::Message, T0, &none).unwrap();
    assert!(g.allows(proto::Cap::Message) && !g.allows(proto::Cap::Call));
    assert_eq!(
        proto::verify_token(&msg, alice.did(), &carol, proto::Cap::Call, T0, &none),
        Err(proto::Error::BadCapability)
    );
    let call = proto::issue_token(&alice, &carol, proto::Cap::Call, T0, TTL);
    let g = proto::verify_token(&call, alice.did(), &carol, proto::Cap::Call, T0, &none).unwrap();
    assert!(g.allows(proto::Cap::Call) && !g.allows(proto::Cap::Message));
    // Per-DID cut-off: a token issued at or before it is taken back, a later one is not.
    assert_eq!(
        proto::verify_token(&call, alice.did(), &carol, proto::Cap::Call, T0 + 1, &CutOff(T0)),
        Err(proto::Error::Revoked)
    );
    assert!(proto::verify_token(&call, alice.did(), &carol, proto::Cap::Call, T0 + 1, &CutOff(T0 - 1)).is_ok());
}

// ---- 5. linked devices -------------------------------------------------------------------------

