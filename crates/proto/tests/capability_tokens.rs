//! Real-code checks for message / call capability tokens (`proto::issue_token`,
//! `proto::verify_token`, `proto::verify_grant`, legacy v1 "call" grants).
//!
//! The candidate-semantics model that used to live here (issuer-only verification, block
//! notices, legacy migration, renewal) moved to the `design-models` branch on 2026-10-14; see
//! `docs/archive/models.md`.
use std::collections::HashSet;

use identity::Identity;

#[path = "support/fixtures.rs"]
mod fixtures;
use fixtures::*;

const PROTO_GRANT_DOMAIN: &[u8] = b"osvauld/p2p/grant/v1\0";
const TTL: u64 = 365 * 24 * 3600;
const T0: u64 = 1_000_000;
const ALICE: u8 = 1;
const CAROL: u8 = 3;

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

