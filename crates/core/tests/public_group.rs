//! Public groups (`docs/design/public-group.md`): the permanent open link, open-join admission
//! at hosts, admins-only posting and the online list (presence), on real nodes over real iroh
//! endpoints (needs network, like `group_g3.rs`; the shared harness is in `support/`). Receipts
//! off (test 8) is a session-level simulation in p2pcore's lib tests
//! (`group_receipts_sim::public_group_sends_no_receipts`).
//!
//! Every test carries `// KIND: acceptance (public group N)`, N the spec's acceptance test.
#![allow(dead_code)]

use p2pcore::{GroupDelivery, GroupItemKind};

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

// ---- host-side rules (review findings 2-4) ----------------------------------------------------

fn role(p: &Peer, g: &str) -> Option<p2pcore::GroupRole> {
    p.node.group_detail(g.into()).ok().map(|d| d.my_role)
}

fn rejected(r: Result<String, p2pcore::Error>) -> String {
    match r {
        Err(p2pcore::Error::Rejected(r)) => r,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

// KIND: acceptance (public group, review finding 2) — a member (a modified app) posts in an
// admins-only group claiming a frontier from before the switch: the post would show on every
// device by the rules, so the host refuses it and nobody else ever gets it; the owner's posts
// still go through.
#[test]
fn stale_frontier_post_refused_in_admins_only() {
    let o = peer("sf-o");
    let a = peer("sf-a");
    let b = peer("sf-b");
    let g = group_with(&o, &[&a, &b]);
    let stale = a.node.group_meta_frontier_for_test(g.clone()).unwrap();
    o.node.group_set_admins_only(g.clone(), true).unwrap();
    for p in [&a, &b] {
        eventually(WAIT, "admins only everywhere", || {
            p.node.group_detail(g.clone()).is_ok_and(|d| d.admins_only)
        });
    }
    a.node
        .group_post_with_seen_for_test(g.clone(), "sneaky".into(), stale)
        .unwrap();
    assert!(
        !within(8, || has_post(&o, &g, &a.did, "sneaky") || has_post(&b, &g, &a.did, "sneaky")),
        "the host refuses a non-admin's post with a stale frontier"
    );
    o.node.group_post(g.clone(), "news".into(), None).unwrap();
    eventually(WAIT, "b sees the owner's post", || {
        has_post(&b, &g, &o.did, "news")
    });
}

// KIND: acceptance (public group, follow-up Fix B) — an honest member posts while offline, the
// owner turns admins-only on meanwhile, the member comes back: the hosts would refuse its post,
// so the member marks it failed ("Not posted: only admins post here"), keeps it visible to
// itself and never uploads it; nobody else gets it, and the member keeps syncing (it gets a
// later admin post) on a session the host keeps open (at most one refusal, no closes).
#[test]
fn offline_post_refused_by_admins_only_fails_and_sync_goes_on() {
    let o = peer("af-o");
    let a = peer("af-a");
    let b = peer("af-b");
    let g = group_with(&o, &[&a, &b]);
    a.node.stop();
    std::thread::sleep(std::time::Duration::from_secs(1));
    let m = a
        .node
        .group_post(g.clone(), "written offline".into(), None)
        .unwrap();
    assert_eq!(m.delivery, GroupDelivery::Waiting);
    assert_eq!(a.node.group_post_failure(g.clone(), m.id.clone()).unwrap(), None);
    o.node.group_set_admins_only(g.clone(), true).unwrap();
    online(&a.node);
    eventually(WAIT, "a's post is marked failed", || {
        a.node.group_post_failure(g.clone(), m.id.clone()).unwrap().as_deref()
            == Some("Not posted: only admins post here")
    });
    let shown = a.node.group_messages(g.clone(), None, 50).unwrap().messages.into_iter().find(|x| x.id == m.id).unwrap();
    assert_eq!(shown.delivery, GroupDelivery::Failed, "never Waiting or Delivered");
    assert_eq!(shown.failure.as_deref(), Some("Not posted: only admins post here"));
    assert!(has_post(&a, &g, &a.did, "written offline"), "the author still sees it");
    o.node
        .group_post(g.clone(), "after the switch".into(), None)
        .unwrap();
    eventually(WAIT, "a keeps syncing", || {
        has_post(&a, &g, &o.did, "after the switch")
    });
    eventually(WAIT, "b gets the admin post", || {
        has_post(&b, &g, &o.did, "after the switch")
    });
    assert!(
        !within(5, || has_post(&o, &g, &a.did, "written offline")
            || has_post(&b, &g, &a.did, "written offline")),
        "the failed post is never uploaded"
    );
    assert!(connected_to(&a, &g, &o.did), "the session stays up");
    let refusals = o
        .logs
        .lock()
        .unwrap()
        .iter()
        .filter(|l| l.contains("refused an update from") && l.contains(a.did.as_str()))
        .count();
    assert!(refusals <= 1, "no repeated refusals ({refusals})");
    assert_eq!(
        a.node.group_post_failure(g.clone(), m.id.clone()).unwrap().as_deref(),
        Some("Not posted: only admins post here"),
        "still failed"
    );
}

// KIND: acceptance (public group, review finding 3) — a member cannot add identities: an
// `OpenJoin` of a throwaway DID it uploads is refused by the host, whether the group is closed
// (with a frontier naming the old `Open{true}`, so the record itself would count) or open.
#[test]
fn member_cannot_add_open_joins_for_others() {
    let o = peer("fj-o");
    let a = peer("fj-a");
    let b = peer("fj-b");
    let g = group_with(&o, &[&a, &b]);
    open_up(&o, &g, &[&o, &a, &b]);
    let stale = a.node.group_meta_frontier_for_test(g.clone()).unwrap();
    o.node.group_set_open(g.clone(), false).unwrap();
    eventually(WAIT, "a sees it closed", || !is_open(&a, &g));
    let x = a.node.group_forge_open_join_for_test(g.clone(), stale).unwrap();
    assert!(
        members(&a, &g).contains(&x),
        "on a's own replica the record counts (its frontier says open)"
    );
    // Open again: b relays another throwaway's join written while open.
    open_up(&o, &g, &[&o, &b]);
    let now = b.node.group_meta_frontier_for_test(g.clone()).unwrap();
    let y = b.node.group_forge_open_join_for_test(g.clone(), now).unwrap();
    assert!(
        !within(8, || {
            let m = members(&o, &g);
            m.contains(&x) || m.contains(&y)
        }),
        "the host takes an open join only from its author while open"
    );
}

// KIND: acceptance (public group, review finding 4) — open links: asking again gives the same
// link (registered again at the host); another admin's close drops every open code at the host,
// so a reopen does not revive the old link and the admin gets a new one; a link stops working
// when its issuer is no longer an admin.
#[test]
fn open_links_die_on_close_and_with_the_issuer_role() {
    let o = peer("oc-o");
    let a = peer("oc-a");
    let b = peer("oc-b");
    let c = peer("oc-c");
    let g = group_with(&o, &[&a]);
    o.node
        .group_set_admin(g.clone(), a.did.clone(), true)
        .unwrap();
    eventually(WAIT, "a is an admin", || {
        role(&a, &g) == Some(p2pcore::GroupRole::Admin)
    });
    let first = a.node.group_open_link(g.clone()).unwrap();
    assert_eq!(a.node.group_open_link(g.clone()).unwrap(), first, "the same link again");
    // The owner (not a) closes and reopens.
    o.node.group_set_open(g.clone(), false).unwrap();
    eventually(WAIT, "a sees it closed", || !is_open(&a, &g));
    open_up(&o, &g, &[&o, &a]);
    let r = rejected(b.node.group_join(first.clone()));
    assert!(r.contains("unknown or expired"), "{r}");
    let second = a.node.group_open_link(g.clone()).unwrap();
    assert_ne!(second, first, "a new open period, a new link");
    assert_eq!(join(&b, second.clone()), g);
    // a is no longer an admin: its link stops working.
    o.node
        .group_set_admin(g.clone(), a.did.clone(), false)
        .unwrap();
    eventually(WAIT, "a is a member again", || {
        role(&a, &g) == Some(p2pcore::GroupRole::Member)
    });
    let r = rejected(c.node.group_join(second));
    assert!(r.contains("withdrawn"), "{r}");
    assert!(!members(&o, &g).contains(&c.did));
}

// ---- presence (§3) ---------------------------------------------------------------------------

/// Presence for tests: a frame every second (the dial tick makes it about two), online for 4 s
/// after a host last saw someone (instead of 15 s / 60 s).
const FAST: (u64, u64) = (1_000, 4_000);

fn timing(ps: &[&Peer], t: (u64, u64)) {
    for p in ps {
        p.node.group_set_presence_timing_for_test(Some(t));
    }
}

/// Sorted DIDs `p` sees online in `g`.
fn online_dids(p: &Peer, g: &str) -> Vec<String> {
    sorted(
        p.node
            .group_online(g.into())
            .unwrap()
            .into_iter()
            .map(|m| m.did)
            .collect(),
    )
}

fn sees_online(p: &Peer, g: &str, did: &str) -> bool {
    online_dids(p, g).iter().any(|d| d == did)
}

/// Opens `g` (owner `o`) and waits until every peer holds the record.
fn open_up(o: &Peer, g: &str, ps: &[&Peer]) {
    o.node.group_set_open(g.into(), true).unwrap();
    for p in ps {
        eventually(WAIT, "everyone sees it open", || is_open(p, g));
    }
}

/// `o` appoints `h` (a member) as a second host and `h` accepts; everyone sees two hosts.
fn second_host(o: &Peer, h: &Peer, g: &str, ps: &[&Peer]) {
    o.node
        .group_appoint_host(g.into(), h.did.clone(), h.node.group_device_id().unwrap())
        .unwrap();
    eventually(WAIT, "h sees the permit", || {
        all(h, g)
            .iter()
            .any(|x| x.kind == GroupItemKind::HostAdded && x.subject_did == h.did)
    });
    h.node.group_accept_host(g.into(), true).unwrap();
    for p in ps {
        eventually(WAIT, "two hosts everywhere", || {
            p.node.group_detail(g.into()).unwrap().hosts.len() == 2
        });
    }
}

/// `p` has a live session with host `host`.
fn connected_to(p: &Peer, g: &str, host: &str) -> bool {
    p.node
        .group_detail(g.into())
        .is_ok_and(|d| d.hosts.iter().any(|h| h.did == host && h.online))
}

// KIND: acceptance (public group 5) — two hosts, each with a member that dials only it: both
// members (and both hosts) see all four online, the host they do not use relaying the other
// host's signed list; a member that goes away drops out once the window passes. An own post in
// the open group still turns Delivered from the host's ack (receipts are off, §4).
#[test]
fn presence_union_across_two_hosts() {
    let o = peer("pu-o");
    let h = peer("pu-h");
    let a = peer("pu-a");
    let b = peer("pu-b");
    let everyone = [&o, &h, &a, &b];
    timing(&everyone, FAST);
    let g = group_with(&o, &[&h, &a, &b]);
    second_host(&o, &h, &g, &everyone);
    open_up(&o, &g, &everyone);
    a.node.group_dial_only_for_test(Some(o.did.clone()));
    b.node.group_dial_only_for_test(Some(h.did.clone()));
    // A dial already in flight when the knob is set may still land: setting it again closes it.
    eventually(WAIT, "a uses only o, b only h", || {
        a.node.group_dial_only_for_test(Some(o.did.clone()));
        b.node.group_dial_only_for_test(Some(h.did.clone()));
        connected_to(&a, &g, &o.did)
            && connected_to(&b, &g, &h.did)
            && !connected_to(&a, &g, &h.did)
            && !connected_to(&b, &g, &o.did)
    });
    let all4 = sorted(everyone.iter().map(|p| p.did.clone()).collect());
    for p in everyone {
        eventually(20, "all four online", || online_dids(p, &g) == all4);
    }
    let me = a.node.group_online(g.clone()).unwrap();
    let owner = me.iter().find(|m| m.did == o.did).unwrap();
    assert!(owner.is_admin && !owner.name.is_empty());
    assert!(me.iter().any(|m| m.did == b.did && !m.is_admin));
    // Receipts are off, own-post delivery is not.
    a.node
        .group_post(g.clone(), "on time".into(), None)
        .unwrap();
    eventually(WAIT, "a's post is delivered", || {
        all(&a, &g)
            .iter()
            .any(|x| x.text == "on time" && x.delivery == GroupDelivery::Delivered)
    });
    // b goes away: gone everywhere once the window has passed (spec: within 75 s at 60 s).
    b.node.stop();
    for p in [&a, &o, &h] {
        eventually(75, "b dropped out", || !sees_online(p, &g, &b.did));
    }
    assert!(sees_online(&a, &g, &h.did), "the others stay");
}

// KIND: acceptance (public group 6) — a member that hides is never listed by any host (not
// even to itself), though it is connected and posts; showing it again lists it, hiding again
// takes it off.
#[test]
fn hidden_member_not_listed() {
    let o = peer("hm-o");
    let a = peer("hm-a");
    let z = peer("hm-z");
    timing(&[&o, &a, &z], FAST);
    assert!(
        z.node.group_presence_visible().unwrap(),
        "visible by default"
    );
    z.node.group_set_presence_visible(false).unwrap();
    assert!(!z.node.group_presence_visible().unwrap());
    let g = o.node.group_create("Lobby".into()).unwrap();
    let link = o.node.group_open_link(g.clone()).unwrap();
    assert_eq!(join(&a, link.clone()), g);
    assert_eq!(join(&z, link), g);
    z.node
        .group_post(g.clone(), "lurking".into(), None)
        .unwrap();
    for p in [&o, &a] {
        eventually(WAIT, "z's post arrived", || {
            has_post(p, &g, &z.did, "lurking")
        });
    }
    eventually(WAIT, "z is connected", || connected_to(&z, &g, &o.did));
    for p in [&o, &a] {
        eventually(20, "o and a online", || {
            sees_online(p, &g, &o.did) && sees_online(p, &g, &a.did)
        });
    }
    assert!(
        !within(6, || [&o, &a, &z]
            .iter()
            .any(|p| sees_online(p, &g, &z.did))),
        "a hidden member is never listed"
    );
    z.node.group_set_presence_visible(true).unwrap();
    eventually(WAIT, "shown again", || sees_online(&a, &g, &z.did));
    z.node.group_set_presence_visible(false).unwrap();
    for p in [&o, &a] {
        eventually(20, "hidden again", || !sees_online(p, &g, &z.did));
    }
}

// KIND: acceptance (public group 7) — a presence frame signed by a member that is not a host is
// ignored (by the host, which does not relay it either, and by the other members); the same
// frame signed by the host is taken.
#[test]
fn presence_only_from_active_hosts() {
    let o = peer("pa-o");
    let m = peer("pa-m");
    let n = peer("pa-n");
    let z = peer("pa-z");
    // Slow own frames, so the host's forged-by-test frame below is not replaced at once.
    timing(&[&o, &m, &n, &z], (10_000, 60_000));
    z.node.group_set_presence_visible(false).unwrap();
    let g = group_with(&o, &[&m, &n, &z]);
    open_up(&o, &g, &[&o, &m, &n, &z]);
    for p in [&m, &n, &z] {
        eventually(WAIT, "connected", || connected_to(p, &g, &o.did));
    }
    eventually(30, "n sees m", || sees_online(&n, &g, &m.did));
    m.node
        .group_send_presence_for_test(g.clone(), vec![z.did.clone()])
        .unwrap();
    assert!(
        !within(5, || [&o, &n].iter().any(|p| sees_online(p, &g, &z.did))),
        "a non-host's frame is ignored"
    );
    o.node
        .group_send_presence_for_test(g.clone(), vec![z.did.clone()])
        .unwrap();
    eventually(10, "the host's frame is taken", || {
        sees_online(&n, &g, &z.did)
    });
}

// ---- images (public group §5) -------------------------------------------------------------------

/// A noisy picture (so its JPEG stays large), `w` x `h`, as PNG bytes.
fn noisy_png(w: u32, h: u32, seed: u32) -> Vec<u8> {
    let mut s = seed | 1;
    let img = image::RgbImage::from_fn(w, h, |_, _| {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        image::Rgb([s as u8, (s >> 8) as u8, (s >> 16) as u8])
    });
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img).write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

fn image_of(p: &Peer, g: &str, id: &str) -> Option<p2pcore::GroupImage> {
    all(p, g).into_iter().find(|m| m.id == id).and_then(|m| m.image)
}

fn image_state(p: &Peer, g: &str, id: &str) -> Option<p2pcore::GroupImageState> {
    image_of(p, g, id).map(|i| i.state)
}

fn image_bytes(p: &Peer, g: &str, id: &str) -> Option<Vec<u8>> {
    p.node.group_image_path(g.into(), id.into()).map(|f| std::fs::read(f).unwrap())
}

// KIND: acceptance (public group 9) — a member's picture reaches another member through the host:
// the sender scales it (<= 1080 px, < 300 KB JPEG), the host fetches it from the sender's session,
// the other member (tap to load) fetches it from the host and gets the same bytes (the ciphertext
// hash is verified while streaming, the blob cipher when it is opened). A host over its per-group
// budget drops the oldest picture; one over the host's size limit is refused (the host never takes
// it, so no member gets it); a blob larger than its post says is refused when the stream passes
// the declared size.
#[test]
fn image_post_roundtrip_via_host() {
    use p2pcore::{GroupImageState as St, PictureDownload};
    let o = peer("im-o");
    let a = peer("im-a");
    let b = peer("im-b");
    let g = group_with(&o, &[&a, &b]);
    assert_eq!(a.node.group_image_auto_download().unwrap(), PictureDownload::Wifi, "the default");
    b.node.group_set_image_auto_download(PictureDownload::Tap).unwrap();

    let sent = a.node.group_post_image_bytes(g.clone(), noisy_png(1600, 1200, 7), "".into()).unwrap();
    assert_eq!(sent.text, "", "a picture needs no caption");
    let img = sent.image.clone().expect("the post carries its picture");
    assert_eq!((img.width, img.height), (1080, 810));
    assert!(img.size < 300 * 1024 && img.mime == "image/jpeg", "{img:?}");
    assert_eq!(img.state, St::Ready);
    let mine = image_bytes(&a, &g, &sent.id).expect("the sender has it at once");
    assert_eq!(image::load_from_memory(&mine).unwrap().width(), 1080);

    eventually(WAIT, "the host fetches it from a", || image_state(&o, &g, &sent.id) == Some(St::Ready));
    assert_eq!(image_bytes(&o, &g, &sent.id).unwrap(), mine);
    eventually(WAIT, "b sees the post", || image_of(&b, &g, &sent.id).is_some());
    assert_eq!(image_state(&b, &g, &sent.id), Some(St::Remote), "on tap only: it waits");
    assert!(b.node.group_image_path(g.clone(), sent.id.clone()).is_none());
    b.node.group_fetch_image(g.clone(), sent.id.clone()).unwrap();
    eventually(WAIT, "b loads it from the host", || image_state(&b, &g, &sent.id) == Some(St::Ready));
    assert_eq!(image_bytes(&b, &g, &sent.id).unwrap(), mine, "the same picture");

    // Over the host's budget: the oldest picture goes, the newest stays.
    b.node.group_set_image_auto_download(PictureDownload::Always).unwrap();
    let budget = img.size + img.size / 2;
    o.node.group_set_image_limits_for_test(Some((1024 * 1024, budget)));
    let second = a.node.group_post_image_bytes(g.clone(), noisy_png(1200, 900, 11), "second".into()).unwrap();
    eventually(WAIT, "b gets the second one from the host", || image_state(&b, &g, &second.id) == Some(St::Ready));
    eventually(WAIT, "the host dropped the first", || image_state(&o, &g, &sent.id) == Some(St::Failed));
    assert_eq!(image_state(&o, &g, &second.id), Some(St::Ready));
    assert!(o.node.group_image_path(g.clone(), sent.id.clone()).is_none());
    assert_eq!(image_state(&b, &g, &sent.id), Some(St::Ready), "b keeps its copy");

    // Over the host's size limit: refused, never fetched, so b never gets it.
    o.node.group_set_image_limits_for_test(Some((40_000, u64::MAX)));
    let big = a.node.group_post_image_bytes(g.clone(), noisy_png(1080, 800, 13), "big".into()).unwrap();
    assert!(big.image.as_ref().unwrap().size > 40_000);
    eventually(WAIT, "the host refuses it", || image_state(&o, &g, &big.id) == Some(St::Failed));
    eventually(WAIT, "b sees the post", || image_of(&b, &g, &big.id).is_some());
    assert!(!within(6, || b.node.group_image_path(g.clone(), big.id.clone()).is_some()), "b never gets it");

    // A blob larger than its post says: the host stops at the declared size and refuses it.
    o.node.group_set_image_limits_for_test(None);
    let liar = a.node.group_post_raw_image_for_test(g.clone(), vec![7u8; 200_000], 1_000).unwrap();
    eventually(WAIT, "the host refuses the oversized blob", || image_state(&o, &g, &liar.id) == Some(St::Failed));
    assert!(o.node.group_image_path(g.clone(), liar.id.clone()).is_none());
}
