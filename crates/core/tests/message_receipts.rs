//! Recipient-signed 1:1 delivered/read receipts on real nodes (roadmap M3 issuance + M4 chat/6
//! exchange): persist -> recipient signs after durable acceptance -> exchange original
//! statements -> sender verifies/commits -> `chat_message_receipts` projection.
//!
//! Real iroh endpoints (needs network, like `readd.rs`). This exercises the core app path only;
//! it is NOT a desktop or Android UI validation.
//!
//! Every test carries `// KIND: acceptance` and the decision it enforces.

use p2pcore::{ReceiptLevel, ReceiptProofKind};

mod support;
use support::eventually_200ms as eventually;
use support::*;

const TAG: &str = "rcpt";

fn level(p: &Peer, did: &str, id: &str) -> Option<ReceiptLevel> {
    p.node
        .chat_message_receipts(did.to_string(), id.to_string())
        .ok()
        .and_then(|s| s.recipients.first().map(|r| r.level))
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
    eventually(90, "b holds a's message", || holds(&b, &a.did, &id));

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
