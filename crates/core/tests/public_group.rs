//! Public groups (`docs/design/public-group.md`): the permanent open link, open-join admission
//! at hosts, and admins-only posting, on real nodes over real iroh endpoints (needs network, like
//! `group_g3.rs`; the shared harness is in `support/`).
//!
//! Every test carries `// KIND: acceptance (public group N)`, N the spec's acceptance test.
#![allow(dead_code)]

mod support;
use support::group::*;
use support::*;

const S: Suite = Suite("public");

fn peer(name: &str) -> Peer {
    S.peer(name)
}

fn is_open(p: &Peer, g: &str) -> bool {
    p.node.group_detail(g.into()).is_ok_and(|d| d.open)
}

// KIND: acceptance (public group 1) — the open link (no invite, no expiry) peeks as open, a
// non-member joins with it (its `Hello` carries no invite; the host welcomes it because the group
// is open, and its `OpenJoin` makes it a member everywhere), and asking again gives the same link.
#[test]
fn open_link_joins_without_invite() {
    let o = peer("ol-o");
    let a = peer("ol-a");
    let b = peer("ol-b");
    let g = o.node.group_create("Lobby".into()).unwrap();
    // A plain member cannot make one.
    join(&a, o.node.group_invite(g.clone()).unwrap());
    eventually(WAIT, "a is a member", || members(&o, &g).contains(&a.did));
    match a.node.group_open_link(g.clone()) {
        Err(p2pcore::Error::Rejected(r)) => assert!(r.contains("admins"), "{r}"),
        other => panic!("expected a refusal, got {other:?}"),
    }
    let link = o.node.group_open_link(g.clone()).unwrap();
    assert!(
        link.starts_with("tinline://g/") && link.len() <= 90,
        "{link}"
    );
    assert!(is_open(&o, &g));
    assert_eq!(
        o.node.group_open_link(g.clone()).unwrap(),
        link,
        "the same link again"
    );
    let peek = b
        .node
        .group_peek_invite(link.clone())
        .expect("peek fetches from the host");
    assert_eq!(peek.group_id, g);
    assert!(peek.open);
    assert!(peek.issuer_did.is_none() && peek.expires.is_none());
    assert_eq!(join(&b, link), g);
    let want = sorted(vec![o.did.clone(), a.did.clone(), b.did.clone()]);
    for p in [&o, &a, &b] {
        eventually(WAIT, "everyone sees the open joiner", || {
            members(p, &g) == want
        });
    }
    assert!(
        b.node
            .groups()
            .unwrap()
            .iter()
            .any(|s| s.id == g && s.open && !s.admins_only)
    );
    b.node.group_post(g.clone(), "hi all".into(), None).unwrap();
    eventually(WAIT, "a sees b's post", || {
        has_post(&a, &g, &b.did, "hi all")
    });
}

// KIND: acceptance (public group 2) — after `Open{false}` the open link is dead (refused at
// fetch) and a joiner holding a token fetched while open is refused at the host's `Hello`;
// someone who joined while open stays a member and keeps syncing.
#[test]
fn closing_refuses_new_open_joins_keeps_members() {
    let o = peer("cl-o");
    let a = peer("cl-a");
    let b = peer("cl-b");
    let g = o.node.group_create("Lobby".into()).unwrap();
    let link = o.node.group_open_link(g.clone()).unwrap();
    assert_eq!(join(&a, link.clone()), g);
    eventually(WAIT, "a is a member", || members(&o, &g).contains(&a.did));
    let stale = b
        .node
        .group_fetch_token_for_test(link.clone())
        .expect("fetched while open");
    o.node.group_set_open(g.clone(), false).unwrap();
    assert!(!is_open(&o, &g));
    match b.node.group_join(link.clone()) {
        Err(p2pcore::Error::Rejected(r)) => assert!(
            r.contains("unknown or expired") || r.contains("not open"),
            "{r}"
        ),
        other => panic!("expected the link refused, got {other:?}"),
    }
    match b.node.group_join_token_for_test(stale) {
        Err(p2pcore::Error::Rejected(r)) => assert!(r.contains("not a member"), "{r}"),
        other => panic!("expected the host to refuse the open joiner, got {other:?}"),
    }
    assert!(b.node.groups().unwrap().is_empty());
    assert!(!members(&o, &g).contains(&b.did));
    // The member that joined while open still syncs both ways.
    o.node
        .group_post(g.clone(), "after closing".into(), None)
        .unwrap();
    eventually(WAIT, "a sees it", || {
        has_post(&a, &g, &o.did, "after closing")
    });
    a.node
        .group_post(g.clone(), "still here".into(), None)
        .unwrap();
    eventually(WAIT, "o sees it", || has_post(&o, &g, &a.did, "still here"));
    // Opening again makes a new link that works.
    let again = o.node.group_open_link(g.clone()).unwrap();
    assert_ne!(again, link);
    assert_eq!(join(&b, again), g);
}

// KIND: acceptance (public group 4) — in an admins-only group a member's `group_post` returns
// `Rejected("admins only")` and writes nothing; the owner still posts; only admins change the
// settings; turning it off lets the member post again.
#[test]
fn admins_only_rejects_local_post() {
    let o = peer("ao-o");
    let a = peer("ao-a");
    let g = group_with(&o, &[&a]);
    match a.node.group_set_admins_only(g.clone(), true) {
        Err(p2pcore::Error::Rejected(r)) => assert!(r.contains("admins"), "{r}"),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert!(matches!(
        a.node.group_set_open(g.clone(), true),
        Err(p2pcore::Error::Rejected(_))
    ));
    o.node.group_set_admins_only(g.clone(), true).unwrap();
    eventually(WAIT, "a sees admins only", || {
        a.node.group_detail(g.clone()).is_ok_and(|d| d.admins_only)
    });
    match a.node.group_post(g.clone(), "may I?".into(), None) {
        Err(p2pcore::Error::Rejected(r)) => assert_eq!(r, "admins only"),
        other => panic!("expected Rejected(admins only), got {other:?}"),
    }
    assert!(!has_post(&a, &g, &a.did, "may I?"));
    o.node
        .group_post(g.clone(), "announcement".into(), None)
        .unwrap();
    eventually(WAIT, "a sees the owner's post", || {
        has_post(&a, &g, &o.did, "announcement")
    });
    o.node.group_set_admins_only(g.clone(), false).unwrap();
    eventually(WAIT, "a sees it off", || {
        a.node.group_detail(g.clone()).is_ok_and(|d| !d.admins_only)
    });
    a.node.group_post(g.clone(), "thanks".into(), None).unwrap();
    eventually(WAIT, "o sees a's post", || {
        has_post(&o, &g, &a.did, "thanks")
    });
}
