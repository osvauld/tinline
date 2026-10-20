use super::super::doc::{DocId, GroupDoc};
use super::super::record::*;
use super::*;
use crdt::writer_id;
use proto::Signed;

/// One device of one account, writing into its own replica: the meta doc and the day doc
/// of 1970-01-01 (where its posts go; the clock stays in that day).
struct Dev {
    id: identity::Identity,
    dev: [u8; 32],
    doc: GroupDoc,
    posts: GroupDoc,
    clock: i64,
}

impl Dev {
    fn new(group: &str, seed: u8) -> Self {
        let id = identity::generate().0;
        let dev = [seed; 32];
        let peer = writer_id(id.did(), &dev);
        Self { doc: GroupDoc::new(group, peer), posts: GroupDoc::new_in(group, DocId::Day("1970-01-01".into()), peer), id, dev, clock: 1000 }
    }
    fn did(&self) -> String {
        self.id.did().to_string()
    }
    /// A post goes into the day doc, every other record into meta; `seen` is the meta vv.
    fn write(&mut self, body: Body) -> (Entry, Vec<u8>) {
        self.clock += 10;
        let rec = Record {
            group: self.doc.group.clone(),
            author: self.did(),
            dev: self.dev,
            at: self.clock,
            seen: self.doc.seen(),
            body,
        };
        let doc = if matches!(rec.body, Body::Post { .. }) {
            &self.posts
        } else {
            &self.doc
        };
        doc.write(&self.id, &rec).unwrap()
    }
    /// A meta update.
    fn take(&mut self, update: &[u8]) {
        self.doc.apply_remote(update, |_| Ok(())).unwrap();
    }
    /// An update of the day doc.
    fn take_posts(&mut self, update: &[u8]) {
        self.posts.apply_remote(update, |_| Ok(())).unwrap();
    }
    fn state(&self) -> GroupState {
        derive_meta(&self.doc.group, &self.doc.entries().unwrap(), &self.doc.vv())
    }
    /// Every post held with its verdict.
    fn judged(&self) -> Vec<(Entry, Vis)> {
        let st = self.state();
        self.posts.entries().unwrap().into_iter().map(|e| (e.clone(), post_visible(&st, &e.rec.author, &e.rec.seen))).collect()
    }
    /// The posts that show.
    fn shown(&self) -> Vec<Item> {
        let st = self.state();
        self.judged().iter().filter(|(_, v)| *v == Vis::Visible).filter_map(|(e, _)| post_item(e, &st)).collect()
    }
    fn invite(&self, group: &str) -> SignedInvite {
        Signed::sign(
            &Invite {
                iss: self.did(),
                sub: proto::Subject::Anyone,
                iat_ms: 1,
                exp_ms: 1 + crate::record::MAX_INVITE_TTL_MS,
                nonce: [3; 16],
                claim: GroupJoin { group: group.into() },
            },
            &self.id,
        )
        .unwrap()
    }
}

fn sync(a: &mut Dev, b: &mut Dev) {
    // Meta first, like the wire does.
    if let Some(u) = a.doc.export_since(&b.doc.vv()) {
        b.take(&u);
    }
    if let Some(u) = b.doc.export_since(&a.doc.vv()) {
        a.take(&u);
    }
    if let Some(u) = a.posts.export_since(&b.posts.vv()) {
        b.take_posts(&u);
    }
    if let Some(u) = b.posts.export_since(&a.posts.vv()) {
        a.take_posts(&u);
    }
}

/// Owner `o` (who wrote genesis) and members, all synced.
fn group(n: usize) -> (String, Dev, Vec<Dev>) {
    let mut o = Dev::new("pending", 1);
    let nonce = [9u8; 16];
    let gid = group_id(&o.did(), &nonce);
    o.doc = GroupDoc::new(&gid, writer_id(&o.did(), &o.dev));
    o.posts = GroupDoc::new_in(&gid, DocId::Day("1970-01-01".into()), writer_id(&o.did(), &o.dev));
    o.write(Body::Genesis { name: "hike".into(), nonce, owner_name: "o".into() });
    let mut ms = Vec::new();
    for i in 0..n {
        let mut m = Dev::new(&gid, 10 + i as u8);
        sync(&mut o, &mut m);
        let inv = o.invite(&gid);
        m.write(Body::Join { invite: inv, name: "m".into() });
        sync(&mut o, &mut m);
        ms.push(m);
    }
    for m in ms.iter_mut() {
        sync(&mut o, m);
    }
    (gid, o, ms)
}

// KIND: unit (G1 stage 1)
#[test]
fn genesis_join_and_posts_derive_members() {
    let (_g, mut o, mut ms) = group(2);
    let s = o.state();
    assert_eq!(s.name, "hike");
    assert_eq!(s.owner.as_deref(), Some(o.did().as_str()));
    assert_eq!(s.members.len(), 3);
    assert!(s.is_admin(&o.did()) && !s.is_admin(&ms[0].did()));
    assert_eq!(s.members[&ms[0].did()].invited_by.as_deref(), Some(o.did().as_str()));
    ms[0].write(Body::Post { text: "hi".into(), reply_to: None });
    sync(&mut ms[0], &mut o);
    let posts = o.shown();
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].text, "hi");
    // A stranger's post (no join) is inert.
    let mut x = Dev::new(&o.doc.group.clone(), 99);
    sync(&mut o, &mut x);
    x.write(Body::Post { text: "spam".into(), reply_to: None });
    sync(&mut x, &mut o);
    assert!(!o.shown().iter().any(|i| i.text == "spam"));
    assert!(o.judged().iter().any(|(e, v)| matches!(&e.rec.body, Body::Post { text, .. } if text == "spam") && *v == Vis::Hidden));
    // A join with an invite from a non-admin is inert.
    let bad = ms[1].invite(&o.doc.group.clone());
    x.write(Body::Join { invite: bad, name: "x".into() });
    sync(&mut x, &mut o);
    assert!(!o.state().is_member(&x.did()));
}

// KIND: unit (G1 stage 1) — U9/U15: any 3 admin flags remove (the same admin may flag 3
// times); non-admin flags are inert; flags never remove the owner (PROVISIONAL).
#[test]
fn three_admin_flags_remove_and_non_admin_flags_are_inert() {
    let (_g, mut o, mut ms) = group(3);
    let (m0, m1) = (ms[0].did(), ms[1].did());
    // Non-admin m1 flags m0 three times: nothing.
    for _ in 0..3 {
        ms[1].write(Body::Flag { member: m0.clone() });
    }
    sync(&mut ms[1], &mut o);
    assert!(o.state().is_member(&m0));
    // Owner makes m1 admin; m1's earlier flags still don't count (not admin as of them).
    o.write(Body::Admin { member: m1.clone(), admin: true });
    sync(&mut o, &mut ms[1]);
    assert!(o.state().is_admin(&m1));
    assert!(o.state().is_member(&m0));
    // Two flags by the new admin, one by the owner: removed.
    ms[1].write(Body::Flag { member: m0.clone() });
    ms[1].write(Body::Flag { member: m0.clone() });
    sync(&mut ms[1], &mut o);
    assert_eq!(o.state().flags[&m0].len(), 2);
    o.write(Body::Flag { member: m0.clone() });
    let s = o.state();
    assert!(s.is_removed(&m0) && !s.is_member(&m0));
    assert_eq!(s.removed[&m0].len(), 3);
    assert!(s.items.iter().any(|i| i.kind == ItemKind::Removed && i.subject == m0));
    // Flags on the owner never count.
    for _ in 0..3 {
        ms[1].write(Body::Flag { member: o.did() });
    }
    sync(&mut ms[1], &mut o);
    assert!(o.state().is_member(&o.did()) && !o.state().is_removed(&o.did()));
    let _ = &mut ms[2];
}

// KIND: unit (G1 stage 1) — U11 option A: a removed member's post shows iff its signed
// frontier predates the removal, in every delivery order.
#[test]
fn signed_frontier_decides_posts_around_a_removal_in_every_order() {
    let (_g, mut o, mut ms) = group(1);
    let m = ms[0].did();
    // m writes offline (it has not seen any flag).
    let (_, offline) = ms[0].write(Body::Post { text: "written offline".into(), reply_to: None });
    // Owner flags m three times.
    let mut ups = Vec::new();
    for _ in 0..3 {
        ups.push(o.write(Body::Flag { member: m.clone() }).1);
    }
    // Receiver 1: flags first, then the offline post.
    let mut r1 = Dev::new(&o.doc.group.clone(), 50);
    sync(&mut o, &mut r1);
    r1.take_posts(&offline);
    // Receiver 2: the post first.
    let mut r2 = Dev::new(&o.doc.group.clone(), 51);
    sync(&mut ms[0], &mut r2);
    sync(&mut o, &mut r2);
    for r in [&r1, &r2] {
        let s = r.state();
        assert!(s.is_removed(&m));
        let p = r.shown().into_iter().find(|i| i.text == "written offline").expect("kept");
        assert!(p.before_removal);
    }
    // After m saw its removal, a new post does not show (anywhere).
    sync(&mut o, &mut ms[0]);
    ms[0].write(Body::Post { text: "after".into(), reply_to: None });
    sync(&mut ms[0], &mut r1);
    assert!(!r1.shown().iter().any(|i| i.text == "after"));
    assert_eq!(r1.state(), {
        sync(&mut r1, &mut r2);
        r2.state()
    });
}

// KIND: unit (G1 stage 1) — hosts need an admin permit AND the device's own acceptance.
#[test]
fn hosts_need_permit_and_acceptance() {
    let (_g, mut o, mut ms) = group(2);
    let (m0, dev0) = (ms[0].did(), ms[0].dev);
    // Owner hosts itself.
    o.write(Body::HostPermit { host: o.did(), host_dev: o.dev, active: true });
    o.write(Body::HostAccept { active: true });
    assert!(o.state().is_host(&o.did(), &o.dev));
    // Permit for m0 without acceptance: not a host.
    o.write(Body::HostPermit { host: m0.clone(), host_dev: dev0, active: true });
    sync(&mut o, &mut ms[0]);
    assert!(!ms[0].state().is_host(&m0, &dev0));
    // The appointed device sees the request until it answers (G2: accept/decline prompt).
    assert!(ms[0].state().host_requests.iter().any(|h| h.did == m0 && h.dev == dev0 && h.appointed_by == o.did()));
    ms[0].write(Body::HostAccept { active: true });
    assert!(ms[0].state().host_requests.is_empty());
    sync(&mut ms[0], &mut o);
    assert!(o.state().is_host(&m0, &dev0));
    // A non-admin's permit is inert; an admin can revoke.
    let (m1, dev1) = (ms[1].did(), ms[1].dev);
    sync(&mut o, &mut ms[1]);
    ms[1].write(Body::HostPermit { host: m1.clone(), host_dev: dev1, active: true });
    ms[1].write(Body::HostAccept { active: true });
    sync(&mut ms[1], &mut o);
    assert!(!o.state().is_host(&m1, &dev1));
    assert!(o.state().host_offers.contains(&(m1.clone(), dev1)));
    o.write(Body::HostPermit { host: m0.clone(), host_dev: dev0, active: false });
    assert!(!o.state().is_host(&m0, &dev0));
    assert_eq!(o.state().hosts.len(), 1);
    // A device that declines (HostAccept false after the permit) is no longer asked.
    o.write(Body::HostPermit { host: m1.clone(), host_dev: dev1, active: true });
    assert!(!o.state().host_requests.iter().any(|h| h.did == m1), "m1 had offered: it hosts now");
    let (m2, dev2) = (ms[2 - 1].did(), [77u8; 32]);
    o.write(Body::HostPermit { host: m2.clone(), host_dev: dev2, active: true });
    assert!(o.state().host_requests.iter().any(|h| h.did == m2 && h.dev == dev2));
}

// KIND: unit (GA-I1) — U21 / G6 §2.1: call hosting needs its own admin permit AND the
// device's own call-host acceptance; chat hosting implies nothing about calls, and the
// reverse; non-admin permits are inert; a revocation or a removal ends it; a re-issued
// permit is a new reference.
#[test]
fn call_hosts_need_their_own_permit_and_acceptance() {
    let (_g, mut o, mut ms) = group(3);
    let (m0, dev0) = (ms[0].did(), ms[0].dev);
    let (m1, dev1) = (ms[1].did(), ms[1].dev);
    // The owner hosts chat: not a call host.
    o.write(Body::HostPermit { host: o.did(), host_dev: o.dev, active: true });
    o.write(Body::HostAccept { active: true });
    assert!(o.state().is_host(&o.did(), &o.dev) && !o.state().is_call_host(&o.did(), &o.dev));
    // A call permit without the device's acceptance: asked, not hosting.
    o.write(Body::CallHostPermit { host: m0.clone(), host_dev: dev0, active: true });
    sync(&mut o, &mut ms[0]);
    let s = ms[0].state();
    assert!(!s.is_call_host(&m0, &dev0));
    assert!(s.call_host_requests.iter().any(|h| h.did == m0 && h.dev == dev0 && h.appointed_by == o.did()));
    // A chat-host acceptance is not a call-host acceptance.
    ms[0].write(Body::HostAccept { active: true });
    assert!(!ms[0].state().is_call_host(&m0, &dev0));
    ms[0].write(Body::CallHostAccept { active: true });
    sync(&mut ms[0], &mut o);
    let s = o.state();
    assert!(s.is_call_host(&m0, &dev0), "permit + call acceptance");
    assert!(!s.is_host(&m0, &dev0), "call hosting does not imply chat hosting");
    assert!(s.call_host_requests.is_empty());
    let first = s.call_host(&m0, &dev0).unwrap().permit.clone();
    assert!(s.entries_have(&o.doc, &first));
    // A chat-host permit + acceptance does not make a call host.
    o.write(Body::HostPermit { host: m1.clone(), host_dev: dev1, active: true });
    sync(&mut o, &mut ms[1]);
    ms[1].write(Body::HostAccept { active: true });
    ms[1].write(Body::CallHostAccept { active: true });
    sync(&mut ms[1], &mut o);
    assert!(o.state().is_host(&m1, &dev1) && !o.state().is_call_host(&m1, &dev1));
    // A non-admin's call permit is inert.
    ms[1].write(Body::CallHostPermit { host: m1.clone(), host_dev: dev1, active: true });
    sync(&mut ms[1], &mut o);
    assert!(!o.state().is_call_host(&m1, &dev1));
    // Revoked: gone; re-issued: back under a new permit id.
    o.write(Body::CallHostPermit { host: m0.clone(), host_dev: dev0, active: false });
    assert!(!o.state().is_call_host(&m0, &dev0));
    o.write(Body::CallHostPermit { host: m0.clone(), host_dev: dev0, active: true });
    let again = o.state().call_host(&m0, &dev0).unwrap().permit.clone();
    assert_ne!(again, first);
    // The device withdraws: gone.
    sync(&mut o, &mut ms[0]);
    ms[0].write(Body::CallHostAccept { active: false });
    sync(&mut ms[0], &mut o);
    assert!(!o.state().is_call_host(&m0, &dev0));
    ms[0].write(Body::CallHostAccept { active: true });
    sync(&mut ms[0], &mut o);
    assert!(o.state().is_call_host(&m0, &dev0));
    // Removed by 3 flags: no longer a call host.
    for _ in 0..3 {
        o.write(Body::Flag { member: m0.clone() });
    }
    assert!(!o.state().is_call_host(&m0, &dev0));
    let _ = &mut ms[2];
}

impl GroupState {
    fn entries_have(&self, doc: &GroupDoc, id: &str) -> bool {
        doc.entries().unwrap().iter().any(|e| e.id == id && matches!(e.rec.body, Body::CallHostPermit { .. }))
    }
}

// KIND: unit (G1 stage 1) — leaving ends membership; a left member's later post is inert.
#[test]
fn leaving_ends_membership() {
    let (_g, mut o, mut ms) = group(1);
    let m = ms[0].did();
    ms[0].write(Body::Leave);
    ms[0].write(Body::Post { text: "after leaving".into(), reply_to: None });
    sync(&mut ms[0], &mut o);
    let s = o.state();
    assert!(!s.is_member(&m) && s.left.contains(&m));
    assert!(!o.shown().iter().any(|i| i.text == "after leaving"));
    assert!(s.items.iter().any(|i| i.kind == ItemKind::Left));
}

// KIND: unit (group shards) — a post that names meta records we do not hold yet is pending,
// not refused; it shows once the meta doc has advanced (any arrival order), and a pending
// post by someone who turns out removed stays hidden.
#[test]
fn a_post_before_its_meta_is_pending_then_shown() {
    let (g, mut o, mut ms) = group(1);
    // A late joiner n: its join and a first post.
    let mut n = Dev::new(&g, 40);
    sync(&mut o, &mut n);
    let inv = o.invite(&g);
    n.write(Body::Join { invite: inv, name: "n".into() });
    n.write(Body::Post { text: "hello".into(), reply_to: None });
    // r gets the post (day doc) first, without the join (meta): pending, hidden.
    let mut r = Dev::new(&g, 60);
    sync(&mut o, &mut r);
    r.take_posts(&n.posts.export_since(&r.posts.vv()).unwrap());
    assert_eq!(r.judged().iter().map(|(_, v)| *v).collect::<Vec<_>>(), vec![Vis::Pending]);
    assert!(r.shown().is_empty());
    // Meta arrives: the same post now shows.
    r.take(&n.doc.export_since(&r.doc.vv()).unwrap());
    assert_eq!(r.judged().iter().map(|(_, v)| *v).collect::<Vec<_>>(), vec![Vis::Visible]);
    assert_eq!(r.shown()[0].text, "hello");
    // Hidden is a verdict, not a wait: a stranger's post is hidden even with all meta held.
    let mut x = Dev::new(&g, 99);
    sync(&mut o, &mut x);
    x.write(Body::Post { text: "spam".into(), reply_to: None });
    sync(&mut x, &mut r);
    assert!(r.judged().iter().any(|(e, v)| e.rec.author == x.did() && *v == Vis::Hidden));
    // No genesis yet at all (a joiner's first sync order): everything pending.
    let mut j = Dev::new(&g, 70);
    j.take_posts(&n.posts.export_since(&j.posts.vv()).unwrap());
    assert_eq!(j.judged()[0].1, Vis::Pending);
    let _ = &mut ms[0];
}
