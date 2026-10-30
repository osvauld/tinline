//! Own-device replication of DM receipt ledgers at the ledger level: device A of our account
//! holds the contact's signed statement and relays the original native ops to device B, which
//! never talked to the contact. B admits them under the same rule as the contact's own session.
use super::*;
use crate::status::{ReceiptLevel, recipient};
use crate::testkit;
use loro::{ExportMode, LoroDoc, LoroValue};

const DAY: &str = "2026-10-28";
const VERSION: [u8; 32] = [5; 32];
const LIMITS: crdt::batching::BatchLimits = crdt::batching::BatchLimits {
    bytes: 256 * 1024,
    operations: 64,
    writers: 4096,
};

/// Our account on two installs (A, B), the contact C, and the conversation's scope.
struct World {
    me: identity::Identity,
    contact: identity::Identity,
    a: Arc<MessageReceipts>,
    b: Arc<MessageReceipts>,
    c: Arc<MessageReceipts>,
    scope: Scope,
}

fn world() -> World {
    let me = identity::generate().0;
    let contact = identity::generate().0;
    let scope = Scope {
        conversation: Conversation::Direct(pair_id(me.did(), contact.did())),
        day: DAY.into(),
    };
    World {
        a: MessageReceipts::new(testkit::memory(1), me.did(), [1; 32]),
        b: MessageReceipts::new(testkit::memory(2), me.did(), [2; 32]),
        c: MessageReceipts::new(testkit::memory(3), contact.did(), [3; 32]),
        me,
        contact,
        scope,
    }
}

/// `who` signs `kind` over `VERSION` in `scope` through the real intent -> pump path.
fn issue(who: &MessageReceipts, id: &identity::Identity, scope: &Scope, kind: Kind) {
    let ops = who.intent_ops(kind, scope, [VERSION]).unwrap();
    who.store.apply(&ops).unwrap();
    assert_eq!(who.pump(id).unwrap().len(), 1);
}

/// Everything `from` holds of `scope` that a peer holding `theirs` lacks.
fn export(from: &MessageReceipts, scope: &Scope, theirs: &VersionVector) -> Vec<u8> {
    from.export(scope, theirs, LIMITS).unwrap().unwrap().update
}

fn level(of: &MessageReceipts, scope: &Scope, did: &str) -> ReceiptLevel {
    recipient(did, &of.evidence(scope, &VERSION, |d| d == did).unwrap()).level
}

#[test]
fn own_device_relay_carries_the_contacts_original_statement() {
    let w = world();
    issue(&w.c, &w.contact, &w.scope, Kind::Read);
    // A got it from the contact's session.
    let fresh =
        w.a.import_direct(
            &w.scope,
            w.contact.did(),
            &export(&w.c, &w.scope, &VersionVector::default()),
        )
        .unwrap();
    assert_eq!(fresh.len(), 1);
    assert_eq!(
        w.b.stored_vv(&w.scope).unwrap(),
        None,
        "B never held a statement of this day"
    );
    // A relays its ledger to B over the own-device link: the contact's ops, never re-signed.
    let relayed =
        w.b.import_direct(
            &w.scope,
            w.contact.did(),
            &export(&w.a, &w.scope, &VersionVector::default()),
        )
        .unwrap();
    assert_eq!(relayed.len(), 1);
    assert_eq!(
        relayed[0].id, fresh[0].id,
        "byte-identical original statement"
    );
    assert_eq!(relayed[0].receipt.recipient, w.contact.did());
    assert_eq!(level(&w.b, &w.scope, w.contact.did()), ReceiptLevel::Read);
    assert_eq!(
        w.b.stored_vv(&w.scope).unwrap(),
        Some(w.a.vv(&w.scope).unwrap())
    );
    // Nothing more to send once B holds A's frontier.
    assert!(
        w.a.export(&w.scope, &w.b.vv(&w.scope).unwrap(), LIMITS)
            .unwrap()
            .is_none()
    );
}

#[test]
fn own_statements_replicate_and_count_for_the_account_not_for_the_contact() {
    let w = world();
    issue(&w.a, &w.me, &w.scope, Kind::Read);
    let got =
        w.b.import_direct(
            &w.scope,
            w.contact.did(),
            &export(&w.a, &w.scope, &VersionVector::default()),
        )
        .unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(
        level(&w.b, &w.scope, w.me.did()),
        ReceiptLevel::Read,
        "our account's statement"
    );
    assert_eq!(
        level(&w.b, &w.scope, w.contact.did()),
        ReceiptLevel::Unconfirmed,
        "never the contact's"
    );
    // B's pump sees A's statement as account coverage and signs nothing again.
    let ops = w.b.intent_ops(Kind::Read, &w.scope, [VERSION]).unwrap();
    w.b.store.apply(&ops).unwrap();
    assert!(w.b.pump(&w.me).unwrap().is_empty());
}

#[test]
fn a_statement_from_outside_the_conversation_is_refused() {
    let w = world();
    let stranger = identity::generate().0;
    let theirs = MessageReceipts::new(testkit::memory(4), stranger.did(), [4; 32]);
    issue(&theirs, &stranger, &w.scope, Kind::Read);
    let update = export(&theirs, &w.scope, &VersionVector::default());
    assert!(matches!(
        w.b.import_direct(&w.scope, w.contact.did(), &update),
        Err(ReceiptError::Policy(_))
    ));
    assert_eq!(w.b.stored_vv(&w.scope).unwrap(), None, "nothing stored");
    assert_eq!(
        level(&w.b, &w.scope, w.contact.did()),
        ReceiptLevel::Unconfirmed
    );
}

#[test]
fn a_forged_or_re_signed_contact_statement_is_refused() {
    let w = world();
    let peer = writer_id(w.contact.did(), &[3; 32]);
    let claim = Receipt {
        scope: w.scope.clone(),
        kind: Kind::Read,
        recipient: w.contact.did().into(),
        device: [3; 32],
        messages: vec![VERSION],
        metadata: vec![],
        call: None,
    };
    // Our own key cannot sign as the contact.
    assert!(matches!(
        claim.sign_claim(&w.me),
        Err(ReceiptError::Signature)
    ));
    // A relay that tampers with the contact's signed bytes (here: the signature).
    let mut forged = claim.sign_claim(&w.contact).unwrap().to_bytes();
    *forged.last_mut().unwrap() ^= 1;
    // The same claim re-signed by our key under our DID, but put under the contact's writer.
    let mut ours = claim.clone();
    ours.recipient = w.me.did().into();
    let wrong_writer = ours.sign_claim(&w.me).unwrap().to_bytes();
    for bytes in [forged, wrong_writer] {
        let raw = LoroDoc::new();
        raw.set_peer_id(peer).unwrap();
        raw.get_map("receipts")
            .insert(&receipt_id(&bytes), LoroValue::Binary(bytes.into()))
            .unwrap();
        raw.commit();
        let update = raw
            .export(ExportMode::updates(&VersionVector::default()))
            .unwrap();
        assert!(
            w.b.import_direct(&w.scope, w.contact.did(), &update)
                .is_err()
        );
        assert_eq!(w.b.stored_vv(&w.scope).unwrap(), None, "nothing stored");
    }
    assert_eq!(
        level(&w.b, &w.scope, w.contact.did()),
        ReceiptLevel::Unconfirmed
    );
}

#[test]
fn a_relay_cannot_name_our_own_did_as_the_contact() {
    let w = world();
    issue(&w.a, &w.me, &w.scope, Kind::Read);
    let update = export(&w.a, &w.scope, &VersionVector::default());
    assert!(matches!(
        w.b.import_direct(&w.scope, w.me.did(), &update),
        Err(ReceiptError::Scope)
    ));
    let other = Scope {
        conversation: Conversation::Direct("b".repeat(64)),
        day: DAY.into(),
    };
    assert!(matches!(
        w.b.import_direct(&other, w.contact.did(), &update),
        Err(ReceiptError::Scope)
    ));
}
