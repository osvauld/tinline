//! `SessPolicy::admit` against real signed records in real docs (synthetic identities, no
//! network): the host-side rules for public groups (review findings 2 and 3).

use super::*;

/// One device writing its own replica of the meta doc and of one day doc of posts.
struct Dev {
    id: identity::Identity,
    dev: [u8; 32],
    meta: GroupDoc,
    day: GroupDoc,
    clock: i64,
}

const DAY: &str = "1970-01-01";

impl Dev {
    fn new(gid: &str, seed: u8) -> Self {
        let id = identity::generate().0;
        let dev = [seed; 32];
        let peer = writer_id(id.did(), &dev);
        Self {
            meta: GroupDoc::new(gid, peer),
            day: GroupDoc::new_in(gid, DocId::Day(DAY.into()), peer),
            id,
            dev,
            clock: 1000,
        }
    }

    fn did(&self) -> String {
        self.id.did().to_string()
    }

    /// Writes `body` with `seen` = our meta frontier now; returns the entry and its update.
    fn write(&mut self, body: Body) -> (Entry, Vec<u8>) {
        let seen = self.meta.seen();
        self.write_seen(body, seen)
    }

    /// Writes `body` claiming the frontier `seen` (a modified app may claim any).
    fn write_seen(&mut self, body: Body, seen: Vec<(u64, i32)>) -> (Entry, Vec<u8>) {
        self.clock += 10;
        let rec = Record {
            group: self.meta.group.clone(),
            author: self.did(),
            dev: self.dev,
            at: self.clock,
            seen,
            body,
        };
        let doc = if matches!(rec.body, Body::Post { .. }) {
            &self.day
        } else {
            &self.meta
        };
        doc.write(&self.id, &rec).unwrap()
    }

    fn take(&mut self, from: &Dev) {
        if let Some(u) = from.meta.export_since(&self.meta.vv()) {
            self.meta.apply_remote(&u, |_| Ok(())).unwrap();
        }
    }

    fn state(&self) -> GroupState {
        derive_meta(
            &self.meta.group,
            &self.meta.entries_trusted().unwrap(),
            &self.meta.vv(),
        )
    }

    /// The new entries a meta `update` brings to our replica (what `admit` is asked about).
    fn fresh_meta(&self, update: &[u8]) -> Vec<Entry> {
        let mut seen = Vec::new();
        self.meta
            .checkpoint()
            .apply_remote(update, |f| {
                seen = f.to_vec();
                Ok(())
            })
            .unwrap();
        seen
    }

    /// The post `id` of our day doc, as a host that lacks it would see it arrive.
    fn fresh_post(&self, id: &str) -> Vec<Entry> {
        let mut d = GroupDoc::new_in(&self.meta.group, DocId::Day(DAY.into()), 1);
        let mut seen = Vec::new();
        d.apply_remote(&self.day.export_since(&Default::default()).unwrap(), |f| {
            seen = f.iter().filter(|e| e.id == id).cloned().collect();
            Ok(())
        })
        .unwrap();
        seen
    }
}

/// Owner `o` (the host), a plain member `m`, an admin `a`, and a member `h` whose device hosts.
fn lobby() -> (String, Dev, Dev, Dev, Dev) {
    let mut o = Dev::new("pending", 1);
    let nonce = [7u8; 16];
    let gid = group_id(&o.did(), &nonce);
    o = Dev {
        meta: GroupDoc::new(&gid, writer_id(&o.did(), &o.dev)),
        day: GroupDoc::new_in(&gid, DocId::Day(DAY.into()), writer_id(&o.did(), &o.dev)),
        ..o
    };
    o.write(Body::Genesis {
        name: "lobby".into(),
        nonce,
        owner_name: "o".into(),
    });
    let mut ms = Vec::new();
    for seed in [2u8, 3, 4] {
        let mut m = Dev::new(&gid, seed);
        m.take(&o);
        let inv = issue_invite(&o.id, &gid, 1, MAX_INVITE_TTL_MS);
        m.write(Body::Join {
            invite: inv,
            name: "m".into(),
        });
        o.take(&m);
        ms.push(m);
    }
    let h = ms.pop().unwrap();
    let a = ms.pop().unwrap();
    let m = ms.pop().unwrap();
    o.write(Body::Admin {
        member: a.did(),
        admin: true,
    });
    o.write(Body::HostPermit {
        host: h.did(),
        host_dev: h.dev,
        active: true,
    });
    let mut h = h;
    h.take(&o);
    h.write(Body::HostAccept { active: true });
    o.take(&h);
    let (mut m, mut a) = (m, a);
    m.take(&o);
    a.take(&o);
    h.take(&o);
    let st = o.state();
    assert!(st.is_admin(&a.did()) && st.is_host(&h.did(), &h.dev) && st.is_member(&m.did()));
    (gid, o, m, a, h)
}

fn member_sess(d: &Dev) -> SessPolicy {
    SessPolicy {
        did: d.did(),
        dev: d.dev,
        hosting: true,
        joining: false,
        open_join: false,
    }
}

// KIND: unit (public groups, review finding 2) — while admins-only is on, a host refuses a
// non-admin's post whose (claimed) frontier predates `AdminsOnly{true}`, from that member's
// session and from another non-admin member's; an admin's session and host-to-host sync carry
// it; a post the rules hide anyway (frontier after the switch) is taken; with admins-only off
// everything is taken.
#[test]
fn admins_only_refuses_stale_frontier_posts_at_the_host() {
    let (_gid, mut o, mut m, a, h) = lobby();
    let stale = m.meta.seen();
    o.write(Body::AdminsOnly { active: true });
    m.take(&o);
    let st = o.state();
    assert!(st.admins_only);
    // m claims the frontier from before the switch: it would show everywhere.
    let (e, _) = m.write_seen(
        Body::Post {
            text: "spam".into(),
            reply_to: None,
            image: None,
        },
        stale,
    );
    assert_eq!(post_visible(&st, &e.rec.author, &e.rec.seen), Vis::Visible);
    let fresh = m.fresh_post(&e.id);
    assert_eq!(fresh.len(), 1);
    assert_eq!(
        member_sess(&m).admit(&st, &fresh),
        Err(ADMINS_ONLY.to_string())
    );
    // Another non-admin member carrying it: refused too. An admin, or another host: taken.
    let mut other = member_sess(&m);
    other.did = "did:key:zOther".into();
    assert!(other.admit(&st, &fresh).is_err());
    assert_eq!(member_sess(&a).admit(&st, &fresh), Ok(()));
    assert_eq!(
        member_sess(&h).admit(&st, &fresh),
        Ok(()),
        "host-to-host sync is exempt"
    );
    // A session we dialled (the peer is a host): nothing extra.
    let dialled = SessPolicy {
        hosting: false,
        ..member_sess(&m)
    };
    assert_eq!(dialled.admit(&st, &fresh), Ok(()));
    // An honest post written after the switch is hidden by the rules anyway: taken.
    let (e2, _) = m.write(Body::Post {
        text: "hidden".into(),
        reply_to: None,
        image: None,
    });
    assert_eq!(post_visible(&st, &e2.rec.author, &e2.rec.seen), Vis::Hidden);
    assert_eq!(member_sess(&m).admit(&st, &m.fresh_post(&e2.id)), Ok(()));
    // The owner's and an admin's own posts are always taken.
    let mut a = a;
    a.take(&o);
    let (ea, _) = a.write(Body::Post {
        text: "news".into(),
        reply_to: None,
        image: None,
    });
    assert_eq!(member_sess(&a).admit(&st, &a.fresh_post(&ea.id)), Ok(()));
    let mut by_m = member_sess(&m);
    by_m.did = "did:key:zOther".into();
    assert_eq!(
        by_m.admit(&st, &a.fresh_post(&ea.id)),
        Ok(()),
        "an admin's post from anyone"
    );
    // Admins-only off: the stale post is an ordinary post.
    o.write(Body::AdminsOnly { active: false });
    assert_eq!(member_sess(&m).admit(&o.state(), &fresh), Ok(()));
}

// KIND: unit (public groups, review finding 3) — a member cannot add throwaway identities to a
// closed, once-open group: the host refuses an `OpenJoin` on a member session while the group
// is closed now (even with a frontier that names the old `Open{true}`), and one authored by
// anyone but the session's DID even while open; host-to-host sync carries it.
#[test]
fn open_joins_only_from_their_author_while_open() {
    let (gid, mut o, m, _a, h) = lobby();
    o.write(Body::Open { active: true });
    // x (a throwaway DID m controls) writes its join while open...
    let mut x = Dev::new(&gid, 9);
    x.take(&o);
    let (_, up) = x.write(Body::OpenJoin { name: "x".into() });
    let fresh = o.fresh_meta(&up);
    assert_eq!(fresh.len(), 1);
    let st = o.state();
    assert!(st.open);
    // ...and m uploads it: not its own record.
    assert!(
        member_sess(&m)
            .admit(&st, &fresh)
            .unwrap_err()
            .contains("own author")
    );
    // x's own session (an open joiner) may send it while open.
    let xs = SessPolicy {
        did: x.did(),
        dev: x.dev,
        hosting: true,
        joining: true,
        open_join: true,
    };
    assert_eq!(xs.admit(&st, &fresh), Ok(()));
    // Closed now: refused from any member session, also from its author; hosts still sync it.
    o.write(Body::Open { active: false });
    let st = o.state();
    assert!(!st.open);
    assert_eq!(
        member_sess(&m).admit(&st, &fresh),
        Err(NOT_OPEN.to_string())
    );
    let xm = SessPolicy {
        did: x.did(),
        dev: x.dev,
        hosting: true,
        joining: false,
        open_join: false,
    };
    assert_eq!(xm.admit(&st, &fresh), Err(NOT_OPEN.to_string()));
    assert_eq!(xs.admit(&st, &fresh), Err(NOT_OPEN.to_string()));
    assert_eq!(
        member_sess(&h).admit(&st, &fresh),
        Ok(()),
        "host-to-host sync is exempt"
    );
}
