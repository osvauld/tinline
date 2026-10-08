//! A task that outlives a switch of account must not write into the next one.

use std::sync::Arc;

use crate::node::{Node, now};
use crate::store::CallRecord;
use crate::{Error, NodeEvents};

struct Quiet;
impl NodeEvents for Quiet {
    fn on_status(&self, _: crate::NodeStatus) {}
    fn on_contacts_changed(&self) {}
    fn on_incoming_call(&self, _: crate::CallInfo) {}
    fn on_call_state(&self, _: String, _: crate::CallState) {}
    fn on_log(&self, _: String) {}
}

fn tmp(tag: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("p2pcore-stale-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    p
}

fn new_contact(who: &identity::Identity, me: &str, device: u8) -> proto::NewContact {
    proto::NewContact {
        did: who.did().to_string(),
        name: "Contact".into(),
        device: [device; 32],
        grant_from_them: proto::issue_grant(who, me, now(), 1000),
        redeemed_nonce: "n".into(),
    }
}

fn record(i: &str) -> CallRecord {
    CallRecord {
        call_id: i.into(),
        peer_did: "did:key:x".into(),
        peer_name: "x".into(),
        incoming: true,
        started_at: 1,
        duration_secs: 0,
        reason: "hangup_local".into(),
        direct: false,
        missed: false,
    }
}

#[test]
fn stale_session_cannot_write_into_next_account() {
    let root = tmp("fence");
    let n = Node::new(root.to_string_lossy().into(), Arc::new(Quiet)).unwrap();
    n.create_identity("a".into(), "pw".into()).unwrap();
    let a = n.profile().unwrap().did;
    let inner = n.inner.clone();
    let epoch_a = inner.me().unwrap().epoch;
    n.begin_new_account().unwrap();
    n.create_identity("b".into(), "pw".into()).unwrap();
    let b = n.profile().unwrap().did;
    let epoch_b = inner.me().unwrap().epoch;
    assert_ne!(epoch_a, epoch_b);

    // The same person is a contact of both accounts, verified in B.
    let (x, _) = identity::generate();
    let save = |epoch: u64, nc: proto::NewContact| {
        let i = inner.clone();
        n.block_on(async move { i.save_contact(epoch, nc, None, true).await }).unwrap()
    };
    save(epoch_b, new_contact(&x, &b, 1)).unwrap();
    n.set_verified(x.did().to_string(), true).unwrap();
    let before = std::fs::read(root.join("accounts").join(b.strip_prefix("did:key:").unwrap()).join("account.redb")).unwrap();

    // Everything a task of A might still do.
    inner.log_call(epoch_a, record("stale"));
    inner.log_refused(epoch_a, "stale2", "did:key:x", "x", "busy", true);
    inner.note_device(epoch_a, x.did(), [9; 32], Some("https://relay.example/".into()));
    inner.renew_grant(epoch_a, x.did(), proto::issue_grant(&x, &b, now(), 100_000));
    assert!(matches!(inner.persist_for(Some(epoch_a)), Err(Error::Locked)));
    assert!(matches!(save(epoch_a, new_contact(&x, &a, 2)), Err(Error::Locked)));
    std::thread::sleep(std::time::Duration::from_millis(300)); // background writes, if any
    assert!(n.recent_calls(10).is_empty());
    let c = &n.contacts()[0];
    assert!(c.verified && c.device != PublicKeyText::of([9; 32]), "B's contact untouched");
    assert_eq!(n.contacts().len(), 1);
    let after = std::fs::read(root.join("accounts").join(b.strip_prefix("did:key:").unwrap()).join("account.redb")).unwrap();
    assert_eq!(before, after, "B's sealed store was not written");

    // B's own session still writes.
    inner.note_device(epoch_b, x.did(), [9; 32], None);
    assert!(!n.contacts()[0].verified, "unfamiliar device resets verification");
    inner.log_call(epoch_b, record("fresh"));
    assert_eq!(n.recent_calls(10).len(), 1);

    // Back to A: a new session, so the old tokens of A stay dead too.
    n.switch_account(a.clone()).unwrap();
    n.unlock("pw".into()).unwrap();
    let epoch_a2 = inner.me().unwrap().epoch;
    assert!(epoch_a2 > epoch_a);
    assert!(n.contacts().is_empty() && n.recent_calls(10).is_empty());
    inner.log_call(epoch_a, record("stale again"));
    inner.log_call(epoch_b, record("b's"));
    assert!(n.recent_calls(10).is_empty());
    inner.log_call(epoch_a2, record("mine"));
    assert_eq!(n.recent_calls(10).len(), 1);

    // Locked: nothing of the last session may write either.
    n.lock();
    inner.log_call(epoch_a2, record("late"));
    assert!(matches!(inner.persist_for(Some(epoch_a2)), Err(Error::Locked)));
    drop(n);
    let _ = std::fs::remove_dir_all(&root);
}

struct PublicKeyText;
impl PublicKeyText {
    fn of(k: [u8; 32]) -> String {
        iroh::PublicKey::from_bytes(&k).map(|k| k.to_string()).unwrap_or_default()
    }
}
