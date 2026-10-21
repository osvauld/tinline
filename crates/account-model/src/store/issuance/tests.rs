use super::*;
use crate::store::{CutOffs, ReqTicket};

const NOW: u64 = 1_800_000_000;

fn spent(st: &State, nonce: &[u8], holder: &str) -> bool {
    Tickets { st, now: NOW }.spent("did:key:me", nonce, holder)
}

// A one-time request ticket is spent for every DID but its subject, once it expired, and once
// used (dropped from `req_tickets`); a live reusable code adds anyone; anything else is spent.
#[test]
fn tickets_spend_by_subject_expiry_and_liveness() {
    let (req, code, dead) = ([1u8; 6], [2u8; 6], [3u8; 6]);
    let n = |b: &[u8; 6]| URL_SAFE_NO_PAD.encode(b);
    let mut st = State::default();
    st.req_tickets.insert(n(&req), ReqTicket { did: "did:key:zB".into(), exp: NOW });
    st.live_tickets.insert(n(&code), NOW + 1);
    assert!(!spent(&st, &req, "did:key:zB"));
    assert!(spent(&st, &req, "did:key:zC"), "wrong holder");
    assert!(!spent(&st, &code, "did:key:zB") && !spent(&st, &code, "did:key:zC"), "reusable code");
    assert!(spent(&st, &dead, "did:key:zB"), "not a ticket we hand out");
    let mut later = st.clone();
    later.req_tickets.get_mut(&n(&req)).unwrap().exp = NOW - 1;
    later.live_tickets.insert(n(&code), NOW);
    assert!(spent(&later, &req, "did:key:zB"), "expired request ticket");
    assert!(spent(&later, &code, "did:key:zB"), "expired code");
    st.req_tickets.clear();
    assert!(spent(&st, &req, "did:key:zB"), "used request ticket");
}

// The cut-offs synced through the account doc decide per capability; tokens are never spent.
#[test]
fn token_cut_offs_are_per_capability() {
    let mut st = State::default();
    st.cut_offs.insert("did:key:zB".into(), CutOffs { message: 5, call: 9 });
    let m = TokenCutOffs { st: &st, cap: Cap::Message };
    let c = TokenCutOffs { st: &st, cap: Cap::Call };
    assert_eq!((m.cut_off_ms("did:key:zB", b"x"), c.cut_off_ms("did:key:zB", b"x")), (5, 9));
    assert_eq!(c.cut_off_ms("did:key:zC", b"x"), 0);
    assert!(!m.spent("did:key:me", &[0; 16], "did:key:zB"));
}
