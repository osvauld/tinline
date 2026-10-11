//! What a group's record set means: owner, members, admins, hosts, flags, removals and which
//! posts show. A pure function of the set of verified records and their oplog positions, so
//! every device that holds the same records reaches the same state in every delivery order.
//!
//! Rules (decisions in `rotation-open-decisions.md` §0; PROVISIONAL ones marked):
//! - Genesis: written by the owner; `group_id(owner, nonce)` must be the group.
//! - Admins: the owner, plus members the owner made admin (`Admin`, owner only; the latest by
//!   `(at, id)` per member wins). "Admin as of a frontier" counts only the owner's `Admin`
//!   records inside that frontier.
//! - Members: the owner, plus every DID with a `Join` whose invite verifies, names this group
//!   and was issued by someone who was owner/admin as of the join's frontier. `Leave` ends it
//!   (latest of join/leave by `(at, id)`).
//! - Flags (U9/U15): a `Flag` counts if its author was admin as of the flag's frontier; any 3
//!   counted flags on a member remove it (the same admin may flag 3 times). PROVISIONAL: the
//!   owner cannot be flagged out (§11 Q3); a removed admin's earlier flags still count (§11 Q9).
//!   Removal is final for this slice (readmission is a later slice).
//! - Posts (U11 option A, signed frontier): a post shows if its author was a member as of its
//!   frontier and fewer than 3 counted flags on the author are inside its frontier. A removed
//!   member's honest offline post (frontier from before the removal) therefore shows, in every
//!   delivery order; anything written after it saw its own removal does not.
//! - Hosts: device `dev` of `did` hosts when the latest `HostPermit` for it (by an owner/admin
//!   as of the permit's frontier) is active, the device's own latest `HostAccept` is active,
//!   and `did` is a member (not removed).
//! - Call hosts (U21, G6 §2.1): the same rule over the separate `CallHostPermit` /
//!   `CallHostAccept` records. Chat hosting never implies call hosting, nor the reverse.

use std::collections::{BTreeMap, BTreeSet};

use super::doc::Entry;
use super::record::{Body, group_id};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberInfo {
    pub did: String,
    /// The name the member gave when joining (the owner's: in genesis).
    pub name: String,
    pub joined_at: i64,
    pub invited_by: Option<String>,
    /// Every device ID this DID has written with (for "make host: pick one of their devices").
    pub devices: BTreeSet<[u8; 32]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlagInfo {
    pub by: String,
    pub at: i64,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostInfo {
    pub did: String,
    pub dev: [u8; 32],
    pub appointed_by: String,
    pub at: i64,
}

/// A device that may host group calls (or was asked to): `permit` is the id of the admin's
/// latest `CallHostPermit` for it, the `permit_digest` of its `gcall::HostRef`, so a re-issued
/// permit is a new reference and a revoked one stops verifying.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallHostInfo {
    pub did: String,
    pub dev: [u8; 32],
    pub appointed_by: String,
    pub at: i64,
    pub permit: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemKind {
    Post,
    Joined,
    Left,
    Removed,
    AdminGranted,
    AdminRevoked,
    HostAdded,
    HostRemoved,
}

/// One line of the conversation: a post, or a system line derived from an op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub id: String,
    pub kind: ItemKind,
    pub author: String,
    /// Who the line is about (joined/left/removed/admin/host), or the post's author.
    pub subject: String,
    pub at: i64,
    pub text: String,
    pub reply_to: Option<String>,
    /// A post shown although its author is removed now: written before the removal (U11).
    pub before_removal: bool,
    pub peer: u64,
    pub counter: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupState {
    pub name: String,
    pub owner: Option<String>,
    pub members: BTreeMap<String, MemberInfo>,
    pub admins: BTreeSet<String>,
    pub removed: BTreeMap<String, Vec<FlagInfo>>,
    /// Counted flags per member not (yet) removed.
    pub flags: BTreeMap<String, Vec<FlagInfo>>,
    pub left: BTreeSet<String>,
    /// Each member's published encryption key (`Body::EncKey`, latest by `(at, id)`).
    pub enc_keys: BTreeMap<String, [u8; 32]>,
    pub hosts: Vec<HostInfo>,
    /// Devices that accepted hosting but hold no active permit.
    pub host_offers: Vec<(String, [u8; 32])>,
    /// Devices an admin appointed (active permit) that have not accepted (yet), with who
    /// appointed them: the "host this group?" prompt on that device.
    pub host_requests: Vec<HostInfo>,
    /// Devices that host group calls: an active admin `CallHostPermit`, their own active
    /// `CallHostAccept`, and a member DID (U21; separate from `hosts`).
    pub call_hosts: Vec<CallHostInfo>,
    /// Devices an admin allowed to host calls that have not accepted (yet).
    pub call_host_requests: Vec<CallHostInfo>,
    /// Posts and system lines, ordered by `(at, id)`.
    pub items: Vec<Item>,
    /// When something happened that makes hosts rotate their group endpoint (U12): each
    /// removal (its 3rd counted flag's `at`) and each counted host revocation (`at`). Unix ms.
    pub rotation_triggers: Vec<i64>,
}

pub const REMOVAL_FLAGS: usize = 3;

impl GroupState {
    pub fn is_member(&self, did: &str) -> bool {
        self.members.contains_key(did)
    }

    pub fn is_admin(&self, did: &str) -> bool {
        self.admins.contains(did)
    }

    pub fn is_host(&self, did: &str, dev: &[u8; 32]) -> bool {
        self.hosts.iter().any(|h| h.did == did && &h.dev == dev)
    }

    pub fn is_removed(&self, did: &str) -> bool {
        self.removed.contains_key(did)
    }

    pub fn call_host(&self, did: &str, dev: &[u8; 32]) -> Option<&CallHostInfo> {
        self.call_hosts.iter().find(|h| h.did == did && &h.dev == dev)
    }

    pub fn is_call_host(&self, did: &str, dev: &[u8; 32]) -> bool {
        self.call_host(did, dev).is_some()
    }
}

fn order(e: &Entry) -> (i64, &str) {
    (e.rec.at, e.id.as_str())
}

pub fn derive(group: &str, entries: &[Entry]) -> GroupState {
    let mut st = GroupState::default();
    // Genesis: the earliest valid one (all valid ones share owner and nonce).
    let Some(genesis) = entries
        .iter()
        .filter(|e| matches!(&e.rec.body, Body::Genesis { nonce, .. } if group_id(&e.rec.author, nonce) == group))
        .min_by(|a, b| order(a).cmp(&order(b)))
    else {
        return st;
    };
    let owner = genesis.rec.author.clone();
    let mut owner_name = String::new();
    if let Body::Genesis { name, owner_name: on, .. } = &genesis.rec.body {
        st.name = name.clone();
        owner_name = on.clone();
    }
    st.owner = Some(owner.clone());

    // The owner's admin records, ordered.
    let mut admin_recs: Vec<&Entry> = entries.iter().filter(|e| e.rec.author == owner && matches!(e.rec.body, Body::Admin { .. })).collect();
    admin_recs.sort_by(|a, b| order(a).cmp(&order(b)));
    let admin_as_of = |did: &str, seen: Option<&[(u64, i32)]>| -> bool {
        if did == owner {
            return true;
        }
        let mut last = None;
        for e in &admin_recs {
            if let Body::Admin { member, admin } = &e.rec.body
                && member == did
                && seen.is_none_or(|s| e.seen_by(s))
            {
                last = Some(*admin);
            }
        }
        last == Some(true)
    };

    // Counted flags per target.
    let mut flags: BTreeMap<String, Vec<&Entry>> = BTreeMap::new();
    for e in entries {
        if let Body::Flag { member } = &e.rec.body
            && *member != owner
            && admin_as_of(&e.rec.author, Some(&e.rec.seen))
        {
            flags.entry(member.clone()).or_default().push(e);
        }
    }
    for v in flags.values_mut() {
        v.sort_by(|a, b| order(a).cmp(&order(b)));
    }
    let removed_as_of = |did: &str, seen: &[(u64, i32)]| flags.get(did).is_some_and(|f| f.iter().filter(|e| e.seen_by(seen)).count() >= REMOVAL_FLAGS);
    let info = |e: &&Entry| FlagInfo { by: e.rec.author.clone(), at: e.rec.at, id: e.id.clone() };
    for (did, f) in &flags {
        if f.len() >= REMOVAL_FLAGS {
            st.removed.insert(did.clone(), f.iter().map(info).collect());
        } else {
            st.flags.insert(did.clone(), f.iter().map(info).collect());
        }
    }

    // Membership events per DID.
    let valid_join = |e: &Entry| -> Option<String> {
        let Body::Join { invite, .. } = &e.rec.body else { return None };
        let inv = invite.open()?;
        (inv.group == group && admin_as_of(&inv.issuer, Some(&e.rec.seen))).then_some(inv.issuer)
    };
    let mut events: BTreeMap<String, Vec<&Entry>> = BTreeMap::new();
    for e in entries {
        let counts = match &e.rec.body {
            Body::Join { .. } => valid_join(e).is_some(),
            Body::Leave => true,
            _ => false,
        };
        if counts && e.rec.author != owner {
            events.entry(e.rec.author.clone()).or_default().push(e);
        }
    }
    let mut devices: BTreeMap<String, BTreeSet<[u8; 32]>> = BTreeMap::new();
    for e in entries {
        devices.entry(e.rec.author.clone()).or_default().insert(e.rec.dev);
    }
    st.members.insert(
        owner.clone(),
        MemberInfo { did: owner.clone(), name: owner_name, joined_at: genesis.rec.at, invited_by: None, devices: devices.get(&owner).cloned().unwrap_or_default() },
    );
    for evs in events.values_mut() {
        evs.sort_by(|a, b| order(a).cmp(&order(b)));
    }
    for (did, evs) in &events {
        let did = did.clone();
        let last = evs.last().expect("non-empty");
        if st.removed.contains_key(&did) {
            continue;
        }
        match &last.rec.body {
            Body::Join { name, .. } => {
                st.members.insert(
                    did.clone(),
                    MemberInfo { did: did.clone(), name: name.clone(), joined_at: last.rec.at, invited_by: valid_join(last), devices: devices.get(&did).cloned().unwrap_or_default() },
                );
            }
            _ => {
                st.left.insert(did);
            }
        }
    }
    // Member as of a frontier: the owner, or the latest join/leave inside it is a valid join.
    let member_as_of = |did: &str, seen: &[(u64, i32)]| -> bool {
        if did == owner {
            return true;
        }
        // `events` holds only valid joins and leaves, sorted.
        let last = events.get(did).and_then(|evs| evs.iter().rev().find(|e| e.seen_by(seen)));
        last.is_some_and(|e| matches!(e.rec.body, Body::Join { .. }))
    };

    for did in st.members.keys() {
        if admin_as_of(did, None) {
            st.admins.insert(did.clone());
        }
    }

    // Encryption keys of members (U3 mail).
    let mut keys: Vec<&Entry> = entries.iter().filter(|e| matches!(e.rec.body, Body::EncKey { .. }) && st.members.contains_key(&e.rec.author)).collect();
    keys.sort_by(|a, b| order(a).cmp(&order(b)));
    for e in keys {
        if let Body::EncKey { key } = &e.rec.body {
            st.enc_keys.insert(e.rec.author.clone(), *key);
        }
    }

    // Hosts.
    let mut permits: BTreeMap<(String, [u8; 32]), &Entry> = BTreeMap::new();
    let mut accepts: BTreeMap<(String, [u8; 32]), &Entry> = BTreeMap::new();
    for e in entries {
        match &e.rec.body {
            Body::HostPermit { host, host_dev, .. } if admin_as_of(&e.rec.author, Some(&e.rec.seen)) => {
                let k = (host.clone(), *host_dev);
                if permits.get(&k).is_none_or(|p| order(p) < order(e)) {
                    permits.insert(k, e);
                }
            }
            Body::HostAccept { .. } => {
                let k = (e.rec.author.clone(), e.rec.dev);
                if accepts.get(&k).is_none_or(|p| order(p) < order(e)) {
                    accepts.insert(k, e);
                }
            }
            _ => {}
        }
    }
    for ((did, dev), p) in &permits {
        let Body::HostPermit { active: true, .. } = p.rec.body else { continue };
        let accepted = accepts.get(&(did.clone(), *dev)).is_some_and(|a| matches!(a.rec.body, Body::HostAccept { active: true }));
        let info = HostInfo { did: did.clone(), dev: *dev, appointed_by: p.rec.author.clone(), at: p.rec.at };
        if accepted && st.members.contains_key(did) {
            st.hosts.push(info);
        } else if st.members.contains_key(did) && !accepts.get(&(did.clone(), *dev)).is_some_and(|a| order(a) > order(p)) {
            // Not accepted, and not declined after this permit: still asking.
            st.host_requests.push(info);
        }
    }
    for ((did, dev), a) in &accepts {
        if matches!(a.rec.body, Body::HostAccept { active: true })
            && st.members.contains_key(did)
            && !st.hosts.iter().any(|h| &h.did == did && &h.dev == dev)
        {
            st.host_offers.push((did.clone(), *dev));
        }
    }

    // The conversation.
    for e in entries {
        let r = &e.rec;
        let item = |kind: ItemKind, subject: &str, text: String, reply_to: Option<String>, before_removal: bool| Item {
            id: e.id.clone(),
            kind,
            author: r.author.clone(),
            subject: subject.to_string(),
            at: r.at,
            text,
            reply_to,
            before_removal,
            peer: e.peer,
            counter: e.counter,
        };
        match &r.body {
            Body::Post { text, reply_to } => {
                if member_as_of(&r.author, &r.seen) && !removed_as_of(&r.author, &r.seen) {
                    st.items.push(item(ItemKind::Post, &r.author, text.clone(), reply_to.clone(), st.removed.contains_key(&r.author)));
                }
            }
            Body::Join { .. } if valid_join(e).is_some() && r.author != owner => st.items.push(item(ItemKind::Joined, &r.author, String::new(), None, false)),
            Body::Leave if r.author != owner => st.items.push(item(ItemKind::Left, &r.author, String::new(), None, false)),
            Body::Admin { member, admin } if r.author == owner => {
                st.items.push(item(if *admin { ItemKind::AdminGranted } else { ItemKind::AdminRevoked }, member, String::new(), None, false))
            }
            Body::HostPermit { host, active, .. } if admin_as_of(&r.author, Some(&r.seen)) => {
                if !*active {
                    st.rotation_triggers.push(r.at);
                }
                st.items.push(item(if *active { ItemKind::HostAdded } else { ItemKind::HostRemoved }, host, String::new(), None, false))
            }
            _ => {}
        }
    }
    // One "removed" line per removed member, at its 3rd counted flag.
    for (did, f) in &flags {
        if f.len() >= REMOVAL_FLAGS {
            let third = f[REMOVAL_FLAGS - 1];
            st.rotation_triggers.push(third.rec.at);
            st.items.push(Item {
                id: format!("removed-{}", third.id),
                kind: ItemKind::Removed,
                author: third.rec.author.clone(),
                subject: did.clone(),
                at: third.rec.at,
                text: String::new(),
                reply_to: None,
                before_removal: false,
                peer: third.peer,
                counter: third.counter,
            });
        }
    }
    st.items.sort_by(|a, b| (a.at, &a.id).cmp(&(b.at, &b.id)));
    st
}

#[cfg(test)]
mod tests {
    use super::super::doc::GroupDoc;
    use super::super::record::*;
    use super::*;
    use crate::chat::doc::writer_id;

    /// One device of one account, writing into its own replica.
    struct Dev {
        id: identity::Identity,
        dev: [u8; 32],
        doc: GroupDoc,
        clock: i64,
    }

    impl Dev {
        fn new(group: &str, seed: u8) -> Self {
            let id = identity::generate().0;
            let dev = [seed; 32];
            Self { doc: GroupDoc::new(group, writer_id(id.did(), &dev)), id, dev, clock: 1000 }
        }
        fn did(&self) -> String {
            self.id.did().to_string()
        }
        fn write(&mut self, body: Body) -> (Entry, Vec<u8>) {
            self.clock += 10;
            let rec = Record { v: 1, group: self.doc.group.clone(), author: self.did(), dev: self.dev, at: self.clock, seen: self.doc.seen(), body };
            self.doc.write(&self.id, &rec).unwrap()
        }
        fn take(&mut self, update: &[u8]) {
            self.doc.apply_remote(update, |_| Ok(())).unwrap();
        }
        fn state(&self) -> GroupState {
            derive(&self.doc.group, &self.doc.entries().unwrap())
        }
        fn invite(&self, group: &str) -> SignedInvite {
            Invite { group: group.into(), issuer: self.did(), expires: u64::MAX, nonce: [3; 16] }.sign(&self.id)
        }
    }

    fn sync(a: &mut Dev, b: &mut Dev) {
        if let Some(u) = a.doc.export_since(&b.doc.vv()) {
            b.take(&u);
        }
        if let Some(u) = b.doc.export_since(&a.doc.vv()) {
            a.take(&u);
        }
    }

    /// Owner `o` (who wrote genesis) and members, all synced.
    fn group(n: usize) -> (String, Dev, Vec<Dev>) {
        let mut o = Dev::new("pending", 1);
        let nonce = [9u8; 16];
        let gid = group_id(&o.did(), &nonce);
        o.doc = GroupDoc::new(&gid, writer_id(&o.did(), &o.dev));
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
        let posts: Vec<_> = o.state().items.into_iter().filter(|i| i.kind == ItemKind::Post).collect();
        assert_eq!(posts.len(), 1);
        assert_eq!(posts[0].text, "hi");
        // A stranger's post (no join) is inert.
        let mut x = Dev::new(&o.doc.group.clone(), 99);
        sync(&mut o, &mut x);
        x.write(Body::Post { text: "spam".into(), reply_to: None });
        sync(&mut x, &mut o);
        assert!(!o.state().items.iter().any(|i| i.text == "spam"));
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
        r1.take(&offline);
        // Receiver 2: the post first.
        let mut r2 = Dev::new(&o.doc.group.clone(), 51);
        sync(&mut ms[0], &mut r2);
        sync(&mut o, &mut r2);
        for r in [&r1, &r2] {
            let s = r.state();
            assert!(s.is_removed(&m));
            let p = s.items.iter().find(|i| i.text == "written offline").expect("kept");
            assert!(p.before_removal);
        }
        // After m saw its removal, a new post does not show (anywhere).
        sync(&mut o, &mut ms[0]);
        ms[0].write(Body::Post { text: "after".into(), reply_to: None });
        sync(&mut ms[0], &mut r1);
        assert!(!r1.state().items.iter().any(|i| i.text == "after"));
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
        assert!(!s.items.iter().any(|i| i.text == "after leaving"));
        assert!(s.items.iter().any(|i| i.kind == ItemKind::Left));
    }
}
