use super::*;
use crate::calldir::device_slot;
use crate::state::{HostInfo, MemberInfo};

const HOST: (&str, [u8; 32]) = ("did:host", [1; 32]);
const OTHER_HOST: (&str, [u8; 32]) = ("did:host2", [2; 32]);
const ALICE: (&str, [u8; 32]) = ("did:alice", [3; 32]);
const BOB: (&str, [u8; 32]) = ("did:bob", [4; 32]);
const GONE: (&str, [u8; 32]) = ("did:gone", [5; 32]);
const STRANGER: (&str, [u8; 32]) = ("did:stranger", [6; 32]);

fn member(did: &str) -> (String, MemberInfo) {
    (did.into(), MemberInfo { did: did.into(), name: did.into(), joined_at: 0, invited_by: None, devices: Default::default() })
}

fn state() -> GroupState {
    let mut st = GroupState::default();
    for d in [HOST.0, OTHER_HOST.0, ALICE.0, BOB.0, GONE.0] {
        st.members.extend([member(d)]);
    }
    st.removed.insert(GONE.0.into(), vec![]);
    for (did, dev) in [HOST, OTHER_HOST] {
        st.hosts.push(HostInfo { did: did.into(), dev, appointed_by: HOST.0.into(), at: 0 });
    }
    st
}

fn ok(st: &GroupState, we_host: bool, who: (&str, [u8; 32]), hosting: bool, route: &Route) -> bool {
    relay_ok(st, we_host, &Peer { did: who.0, dev: &who.1, hosting }, route)
}

#[test]
fn a_member_that_is_not_a_host_sends_only_to_its_hosts() {
    let st = state();
    for route in [Route::All, Route::Did(BOB.0.into())] {
        // Sessions where we are the member (peer.hosting = false: they host us) get it.
        assert!(ok(&st, false, HOST, false, &route));
        // A session we host (a member's own connection to us) never does.
        assert!(!ok(&st, false, ALICE, true, &route));
    }
}

#[test]
fn a_host_relays_to_other_hosts_whatever_the_route() {
    let st = state();
    assert!(ok(&st, true, OTHER_HOST, false, &Route::Did(BOB.0.into())));
    assert!(ok(&st, true, OTHER_HOST, true, &Route::Device(BOB.0.into(), 9)));
}

#[test]
fn a_host_relays_to_hosted_current_members_the_route_names() {
    let st = state();
    assert!(ok(&st, true, ALICE, true, &Route::All));
    assert!(ok(&st, true, ALICE, true, &Route::Did(ALICE.0.into())));
    assert!(!ok(&st, true, ALICE, true, &Route::Did(BOB.0.into())));
    assert!(ok(&st, true, ALICE, true, &Route::Device(ALICE.0.into(), device_slot(&ALICE.1))));
    assert!(!ok(&st, true, ALICE, true, &Route::Device(ALICE.0.into(), device_slot(&ALICE.1) + 1)));
    assert!(!ok(&st, true, ALICE, true, &Route::Device(BOB.0.into(), device_slot(&ALICE.1))));
}

#[test]
fn a_host_never_relays_to_removed_joining_or_unhosted_peers() {
    let st = state();
    for route in [Route::All, Route::Did(GONE.0.into()), Route::Did(STRANGER.0.into())] {
        // Removed member (still connected), and a joining peer that is not a member yet.
        assert!(!ok(&st, true, GONE, true, &route));
        assert!(!ok(&st, true, STRANGER, true, &route));
    }
    // A member session we do not host (a peer that hosts us but is not an appointed host).
    assert!(!ok(&st, true, ALICE, false, &Route::All));
}
