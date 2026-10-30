//! Recipient-signed 1:1 delivered/read receipts on real nodes (roadmap M3 issuance + M4 chat/6
//! exchange): persist -> recipient signs after durable acceptance -> exchange original
//! statements -> sender verifies/commits -> `chat_message_receipts` projection.
//!
//! Real iroh endpoints (needs network, like `readd.rs`). This exercises the core app path only;
//! it is NOT a desktop or Android UI validation.
//!
//! Every test carries `// KIND: acceptance` and the decision it enforces.

use p2pcore::{ReceiptLevel, ReceiptProofKind, ReceiptTick};

use crate::support;
use support::eventually_200ms as eventually;
use support::*;

const TAG: &str = "rcpt";

fn level(p: &Peer, did: &str, id: &str) -> Option<ReceiptLevel> {
    p.node
        .chat_message_receipts(did.to_string(), id.to_string())
        .ok()
        .and_then(|s| s.recipients.first().map(|r| r.level))
}

fn tick(p: &Peer, did: &str, id: &str) -> ReceiptTick {
    p.node
        .chat_message_receipts(did.to_string(), id.to_string())
        .unwrap()
        .tick
}

fn list_tick(p: &Peer, did: &str) -> Option<ReceiptTick> {
    p.node
        .chat_list_receipts()
        .unwrap()
        .into_iter()
        .find(|r| r.peer_did == did)
        .map(|r| r.status.tick)
}

fn version(p: &Peer, did: &str, id: &str) -> String {
    p.node
        .chat_message_receipts(did.to_string(), id.to_string())
        .unwrap()
        .version
}

// KIND: acceptance (user 2026-10-15: one admitted recipient device confirms the account; host/
// transport acks and our own cursor are not recipient evidence; exact versions only)
#[test]
fn signed_delivered_then_read_reach_the_sender_for_the_exact_version() {
    let (a, b) = Peer::pair(TAG, "dr");
    let id = a
        .node
        .send_text(b.did.clone(), "hello".into(), None)
        .unwrap()
        .id;
    // Before any signed statement the mark is never Delivered/Read, whatever the transport ack says.
    assert!(matches!(
        tick(&a, &b.did, &id),
        ReceiptTick::OnlyHere | ReceiptTick::Waiting | ReceiptTick::Delivered
    ));
    eventually(90, "b holds a's message", || holds(&b, &a.did, &id));
    assert_eq!(
        tick(&b, &a.did, &id),
        ReceiptTick::None,
        "incoming messages carry no mark"
    );

    // Recipient: its own delivered statement is committed and projected.
    eventually(30, "b signed delivered", || {
        level(&b, &a.did, &id) == Some(ReceiptLevel::Delivered)
    });
    let mine = b
        .node
        .chat_message_receipts(a.did.clone(), id.clone())
        .unwrap();
    assert_eq!(mine.recipients.len(), 1);
    assert_eq!(mine.recipients[0].did, b.did);
    assert!(
        mine.recipients[0]
            .proofs
            .iter()
            .all(|p| p.signer_did == b.did && p.kind == ReceiptProofKind::Delivered)
    );

    // Sender: the contact's original statement arrives over chat/6 and is verified.
    eventually(60, "a sees b's signed delivered", || {
        level(&a, &b.did, &id) == Some(ReceiptLevel::Delivered)
    });
    let theirs = a
        .node
        .chat_message_receipts(b.did.clone(), id.clone())
        .unwrap();
    assert_eq!(
        theirs.version, mine.version,
        "both sides name the same signed version"
    );
    assert_eq!(theirs.recipients[0].did, b.did);
    assert_eq!(theirs.tick, ReceiptTick::Delivered);
    assert_eq!(list_tick(&a, &b.did), Some(ReceiptTick::Delivered));
    assert_eq!(
        theirs.recipients[0].proofs[0].receipt_id, mine.recipients[0].proofs[0].receipt_id,
        "relayed byte-identical"
    );

    // Our own mark-read is not the contact reading our message.
    a.node.mark_read(b.did.clone()).unwrap();
    assert_eq!(level(&a, &b.did, &id), Some(ReceiptLevel::Delivered));

    b.node.mark_read(a.did.clone()).unwrap();
    assert_eq!(
        level(&b, &a.did, &id),
        Some(ReceiptLevel::Read),
        "read is committed before mark_read returns"
    );
    eventually(60, "a sees b's signed read", || {
        level(&a, &b.did, &id) == Some(ReceiptLevel::Read)
    });
    assert_eq!(tick(&a, &b.did, &id), ReceiptTick::Read);
    assert_eq!(list_tick(&a, &b.did), Some(ReceiptTick::Read));
    let kinds: Vec<_> = a
        .node
        .chat_message_receipts(b.did.clone(), id.clone())
        .unwrap()
        .recipients[0]
        .proofs
        .iter()
        .map(|p| p.kind)
        .collect();
    assert!(
        kinds.contains(&ReceiptProofKind::Delivered) && kinds.contains(&ReceiptProofKind::Read)
    );

    // An edit is a new signed version: nothing carries over; delivery is re-confirmed, read is not.
    let before = version(&a, &b.did, &id);
    a.node
        .edit_message(b.did.clone(), id.clone(), "hello, edited".into())
        .unwrap();
    assert_ne!(version(&a, &b.did, &id), before);
    eventually(90, "a sees the edit delivered", || {
        level(&a, &b.did, &id) == Some(ReceiptLevel::Delivered)
    });
    std::thread::sleep(std::time::Duration::from_secs(2));
    assert_eq!(
        level(&a, &b.did, &id),
        Some(ReceiptLevel::Delivered),
        "the earlier read does not cover the edit"
    );
    assert_eq!(tick(&a, &b.did, &id), ReceiptTick::Delivered);
}

// KIND: acceptance (receipts are never removed; durable across restart; no duplicate statements)
#[test]
fn receipts_survive_restart_and_are_not_reissued() {
    let (a, mut b) = Peer::pair(TAG, "rs");
    let id = a
        .node
        .send_text(b.did.clone(), "persist me".into(), None)
        .unwrap()
        .id;
    eventually(90, "a sees b's signed delivered", || {
        level(&a, &b.did, &id) == Some(ReceiptLevel::Delivered)
    });
    let before = b
        .node
        .chat_message_receipts(a.did.clone(), id.clone())
        .unwrap();
    b.reopen();
    b.node.unlock(PASS.into()).unwrap();
    online(&b.node);
    eventually(30, "b's chat store reopened", || {
        b.node.writer_id_for_test().is_some()
    });
    let after = b
        .node
        .chat_message_receipts(a.did.clone(), id.clone())
        .unwrap();
    assert_eq!(
        after, before,
        "the same original statement, no reissue after restart"
    );
}

// KIND: acceptance (bug on v0.2.6 devices: the phone sent, the laptop got the contact's signed
// read, the phone kept its clock. Signed ledgers now cross the own-device link as the original
// statements; a device that never met the contact shows the contact's own read.)
#[test]
fn an_own_device_that_never_met_the_contact_gets_the_signed_read_at_session_start() {
    let (a, c) = Peer::pair(TAG, "od");
    let id = a
        .node
        .send_text(c.did.clone(), "read on the other side".into(), None)
        .unwrap()
        .id;
    eventually(90, "c holds a's message", || holds(&c, &a.did, &id));
    c.node.mark_read(a.did.clone()).unwrap();
    eventually(60, "a sees c's signed read", || {
        tick(&a, &c.did, &id) == ReceiptTick::Read
    });
    let at_a = a
        .node
        .chat_message_receipts(c.did.clone(), id.clone())
        .unwrap();
    // The contact goes away for good: whatever b learns comes from a, over the own-device link.
    c.node.stop();

    let b = a.link_fresh(TAG, "od-a2", LinkAs::default());
    eventually(90, "b holds the message", || holds(&b, &c.did, &id));
    eventually(60, "b shows the contact's signed read", || {
        tick(&b, &c.did, &id) == ReceiptTick::Read
    });
    assert_eq!(list_tick(&b, &c.did), Some(ReceiptTick::Read));
    let at_b = b
        .node
        .chat_message_receipts(c.did.clone(), id.clone())
        .unwrap();
    assert_eq!(at_b.version, at_a.version, "the same signed version");
    assert_eq!(at_b.recipients[0].did, c.did);
    let ids = |s: &p2pcore::MessageReceiptStatus| {
        let mut v: Vec<String> = s.recipients[0]
            .proofs
            .iter()
            .map(|p| {
                assert_eq!(p.signer_did, c.did, "signed by the contact, not relayed as ours");
                p.receipt_id.clone()
            })
            .collect();
        v.sort();
        v
    };
    assert_eq!(ids(&at_b), ids(&at_a), "the original statements, byte-identical");
    assert!(
        !b.logged("rejected own-device receipts"),
        "nothing refused: {:?}",
        b.refusals()
    );
}
