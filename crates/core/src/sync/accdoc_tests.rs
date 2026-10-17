//! Real-Loro account-document tests, not network acceptance tests.
use super::accdoc::{AccDoc, Ctx};
use crate::store::{OwnDevice, State};

fn device(id: &identity::Identity, key: [u8; 32], relay: Option<&str>) -> OwnDevice {
    OwnDevice {
        device: key,
        attestation: proto::attest(id, key, 1),
        label: String::new(),
        removed: false,
        last_seen: 0,
        relay: relay.map(str::to_string),
        links: Vec::new(),
        rotated: false,
    }
}

/// Merges `from`'s working copy into `to` the way a session does: `to` flushes its own state
/// first, imports, reads back. Whether `to` wrote anything new.
fn deliver(from: &AccDoc, to: &AccDoc, st: &mut State, ctx: &Ctx) -> bool {
    let wrote = to.write_from(st, ctx).unwrap();
    if let Some((bytes, _)) = from.export_since(&to.vv()) {
        to.import(&bytes).unwrap();
    }
    to.read_into(st, ctx);
    wrote
}

// KIND: regression (2026-10-17: own-device sessions never idled out; the relay field of a
// device entry flipped between "" and a hint on every merge)
#[test]
fn a_relay_hint_learned_by_a_sibling_converges() {
    let (id, _) = identity::generate();
    let did = id.did().to_string();
    let (ka, kb) = ([1; 32], [2; 32]);
    let hint = "https://relay.example./";
    // A has no hint for itself; B learned A's hint from A's Auth.
    let mut sa = State { registry: vec![device(&id, ka, None), device(&id, kb, None)], ..State::default() };
    let mut sb = State { registry: vec![device(&id, ka, Some(hint)), device(&id, kb, None)], ..State::default() };
    let ctx_a = Ctx { my_did: &did, me_device: ka, now_ms: 0 };
    let ctx_b = Ctx { my_did: &did, me_device: kb, now_ms: 0 };
    let a = AccDoc::new(&ka);
    let b = AccDoc::new(&kb);
    a.write_from(&sa, &ctx_a).unwrap();
    b.write_from(&sb, &ctx_b).unwrap();
    for _ in 0..3 {
        deliver(&b, &a, &mut sa, &ctx_a);
        deliver(&a, &b, &mut sb, &ctx_b);
    }
    // In sync now: another round writes nothing on either side.
    assert!(!deliver(&b, &a, &mut sa, &ctx_a), "A rewrote the account doc");
    assert!(!deliver(&a, &b, &mut sb, &ctx_b), "B rewrote the account doc");
    assert_eq!(a.vv(), b.vv());
    assert_eq!(sb.registry[0].relay.as_deref(), Some(hint));
    assert_eq!(sa.registry[0].relay, None, "a device keeps no hint for itself");
}
