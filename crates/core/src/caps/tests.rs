use super::*;
use crate::store::{ContactDevice, CutOffs, StoredContact};

/// Unix ms.
const T0: u64 = 1_700_000_000_000;
const DAY: u64 = 24 * 3600 * 1000;

fn contact(did: &str, grant: SignedGrant, calls_allowed: bool) -> StoredContact {
    StoredContact {
        did: did.into(),
        name: "n".into(),
        devices: vec![ContactDevice { device: [7; 32], relay: None }],
        device_list: None,
        grant_from_them: grant,
        call_grant_from_them: None,
        calls_allowed,
        added_at: 1,
        alias: None,
        verified: false,
    }
}

fn claims(g: &SignedGrant) -> proto::Grant {
    g.peek().unwrap()
}

// KIND: invariant (P6, U13/U15)
// Separate tokens: a message token opens chat only, a call token rings only; a contact
// without call permission is told "calls not allowed", never the generic refusal.
#[test]
fn each_token_opens_its_own_path() {
    let (me, _) = identity::generate();
    let (them, _) = identity::generate();
    let d = them.did();
    let msg = claims(&proto::issue_token(&me, d, Cap::Message, T0, 100));
    let call = claims(&proto::issue_token(&me, d, Cap::Call, T0, 100));
    let check = |st: &State, d: &str, g: &proto::Grant, cap: Cap| check(st, d, g, cap, T0 + 1);
    let st = State { contacts: vec![contact(d, proto::issue_token(&them, me.did(), Cap::Message, T0, 100), true)], ..Default::default() };
    assert_eq!(check(&st, d, &msg, Cap::Message), Ok(()));
    assert_eq!(check(&st, d, &call, Cap::Call), Ok(()));
    assert_eq!(check(&st, d, &call, Cap::Message), Err(Denied::Refused));
    assert_eq!(check(&st, d, &msg, Cap::Call), Err(Denied::CallsNotAllowed));
    // Message-only contact (group chat request default): even a call token does not ring.
    let mut st2 = st.clone();
    st2.contacts[0].calls_allowed = false;
    assert_eq!(check(&st2, d, &call, Cap::Call), Err(Denied::CallsNotAllowed));
    assert_eq!(check(&st2, d, &msg, Cap::Message), Ok(()));
}

// KIND: invariant (P6, per-DID cut-off, section 14 Q2)
// "Disallow calls" raises the call cut-off: every older call token (seen or not) stops ringing,
// messaging goes on. A block raises both: nothing older opens anything, and a token minted
// above the cut-off does.
#[test]
fn cut_offs_take_tokens_back_per_did() {
    let (me, _) = identity::generate();
    let (them, _) = identity::generate();
    let d = them.did();
    let older_call = claims(&proto::issue_token(&me, d, Cap::Call, T0 - 5, DAY));
    let msg = claims(&proto::issue_token(&me, d, Cap::Message, T0, DAY));
    let mut st = State { contacts: vec![contact(d, proto::issue_token(&them, me.did(), Cap::Message, T0, DAY), true)], ..Default::default() };
    assert_eq!(check(&st, d, &older_call, Cap::Call, T0 + 1), Ok(()));

    st.cut_offs.insert(d.into(), CutOffs { message: 0, call: T0 + 1 });
    st.contacts[0].calls_allowed = false;
    assert_eq!(check(&st, d, &older_call, Cap::Call, T0 + 1), Err(Denied::CallsNotAllowed));
    assert_eq!(check(&st, d, &msg, Cap::Message, T0 + 1), Ok(()));
    // Allow again: only a token above the cut-off rings.
    st.contacts[0].calls_allowed = true;
    assert_eq!(check(&st, d, &older_call, Cap::Call, T0 + 1), Err(Denied::CallsNotAllowed));
    let p = policy_for(&st, d, ContactOrigin::Ticket);
    assert!(p.call && p.not_before_ms == T0 + 1);
    let fresh = claims(&proto::issue_token(&me, d, Cap::Call, (T0).max(p.not_before_ms + 1), DAY));
    assert_eq!(check(&st, d, &fresh, Cap::Call, T0 + 3), Ok(()));

    // Block: both cut-offs; even the fresh call token is refused.
    Inner::revoke_all(&mut st, d);
    let t = now_ms();
    assert!(st.cut_offs[d].message >= t && st.cut_offs[d].call >= t);
    let pre_block = claims(&proto::issue_token(&me, d, Cap::Message, t, DAY));
    assert_eq!(check(&st, d, &pre_block, Cap::Message, t + 1), Err(Denied::Refused));
    let fresh_now = claims(&proto::issue_token(&me, d, Cap::Call, t, DAY));
    assert_eq!(check(&st, d, &fresh_now, Cap::Call, t + 1), Err(Denied::Refused));
    // Unblock does not lower the cut-off; the re-add mints above it.
    let nb = not_before(&st, d);
    let readd = claims(&proto::issue_token(&me, d, Cap::Message, t.max(nb + 1), DAY));
    assert_eq!(check(&st, d, &readd, Cap::Message, nb + 2), Ok(()));
}

// KIND: invariant (P6, U13 defaults)
#[test]
fn origin_decides_a_new_contacts_tokens_and_an_existing_one_keeps_its_permission() {
    let (them, _) = identity::generate();
    let d = them.did();
    let st = State::default();
    assert!(policy_for(&st, d, ContactOrigin::Ticket).call);
    assert!(!policy_for(&st, d, ContactOrigin::GroupRequest).call);
    let st = State { contacts: vec![contact(d, proto::issue_token(&them, "did:me", Cap::Message, T0, 100), false)], ..Default::default() };
    assert!(!policy_for(&st, d, ContactOrigin::Ticket).call, "re-scanning a ticket does not upgrade");
}

// KIND: invariant
// A slot takes only the right kind of token, valid, and expiring later.
#[test]
fn a_slot_takes_only_a_longer_lived_token_of_its_kind() {
    let (them, _) = identity::generate();
    let (me, _) = identity::generate();
    let old = proto::issue_token(&them, me.did(), Cap::Message, T0, 100);
    let newer = proto::issue_token(&them, me.did(), Cap::Message, T0 + 50, 100);
    let call = proto::issue_token(&them, me.did(), Cap::Call, T0 + 50, 100);
    assert!(outlives(Some(&old), &newer, Cap::Message, them.did(), me.did(), T0 + 10));
    assert!(!outlives(Some(&newer), &old, Cap::Message, them.did(), me.did(), T0 + 10));
    assert!(!outlives(Some(&old), &call, Cap::Message, them.did(), me.did(), T0 + 10));
    assert!(outlives(None, &call, Cap::Call, them.did(), me.did(), T0 + 10));
    assert!(!outlives(None, &call, Cap::Call, me.did(), me.did(), T0 + 10), "wrong issuer");
}
