//! `derive_meta`: the pure fold of the meta doc's records into a [`GroupState`]. One step per
//! rule; every step reads only `entries` (and the state earlier steps built), so the result
//! does not depend on the order the records arrived in.

use std::collections::{BTreeMap, BTreeSet};

use loro::VersionVector;

use super::*;

fn order(e: &Entry) -> (i64, &str) {
    (e.rec.at, e.id.as_str())
}

/// What every step shares: the group, its records, its owner and the owner's admin records.
struct Ctx<'a> {
    group: &'a str,
    entries: &'a [Entry],
    owner: String,
    /// The owner's admin records, ordered.
    admin_recs: Vec<&'a Entry>,
    /// `Open` / `AdminsOnly` records by an admin as of their own frontier, ordered.
    open_recs: Vec<Setting>,
    admins_only_recs: Vec<Setting>,
}

impl<'a> Ctx<'a> {
    fn new(group: &'a str, entries: &'a [Entry], owner: String) -> Self {
        let mut admin_recs: Vec<&Entry> = entries.iter().filter(|e| e.rec.author == owner && matches!(e.rec.body, Body::Admin { .. })).collect();
        admin_recs.sort_by(|a, b| order(a).cmp(&order(b)));
        let mut cx = Ctx { group, entries, owner, admin_recs, open_recs: Vec::new(), admins_only_recs: Vec::new() };
        let settings = |pick: fn(&Body) -> Option<bool>| {
            let mut v: Vec<&Entry> = entries.iter().filter(|e| pick(&e.rec.body).is_some() && cx.admin_as_of(&e.rec.author, Some(&e.rec.seen))).collect();
            v.sort_by(|a, b| order(a).cmp(&order(b)));
            v.into_iter().map(|e| Setting::of(e, pick(&e.rec.body) == Some(true))).collect::<Vec<_>>()
        };
        let open_recs = settings(|b| match b {
            Body::Open { active } => Some(*active),
            _ => None,
        });
        let admins_only_recs = settings(|b| match b {
            Body::AdminsOnly { active } => Some(*active),
            _ => None,
        });
        cx.open_recs = open_recs;
        cx.admins_only_recs = admins_only_recs;
        cx
    }

    /// Whether the group is open as of `seen` (`None`: now).
    fn open_as_of(&self, seen: Option<&[(u64, i32)]>) -> bool {
        in_force(&self.open_recs, seen)
    }

    /// Whether `e` is a join that counts: an invite join (`valid_join`), or an `OpenJoin`
    /// written while the group was open as of its frontier.
    fn joins(&self, e: &Entry) -> bool {
        match &e.rec.body {
            Body::Join { .. } => self.valid_join(e).is_some(),
            Body::OpenJoin { .. } => self.open_as_of(Some(&e.rec.seen)),
            _ => false,
        }
    }

    /// Whether `did` is an admin as of `seen` (`None`: now).
    fn admin_as_of(&self, did: &str, seen: Option<&[(u64, i32)]>) -> bool {
        if did == self.owner {
            return true;
        }
        let mut last = None;
        for e in &self.admin_recs {
            if let Body::Admin { member, admin } = &e.rec.body
                && member == did
                && seen.is_none_or(|s| e.seen_by(s))
            {
                last = Some(*admin);
            }
        }
        last == Some(true)
    }

    /// The issuer of a join's invite, if the join is valid.
    fn valid_join(&self, e: &Entry) -> Option<String> {
        let Body::Join { invite, .. } = &e.rec.body else {
            return None;
        };
        // A join in the doc stays valid after its invite expires: only signature, group and the
        // issuer's admin role as of the join count here (`redeem_invite` is for admission).
        let inv = invite.open().ok()?;
        (inv.claim.group == self.group && self.admin_as_of(&inv.iss, Some(&e.rec.seen)))
            .then_some(inv.iss)
    }
}

/// The state of the meta doc `entries` (which hold every record but posts); `have` is its
/// version vector.
pub fn derive_meta(group: &str, entries: &[Entry], have: &VersionVector) -> GroupState {
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
    st.pos.owner = Some(owner.clone());
    st.pos.have = have.iter().map(|(p, c)| (*p, *c)).collect();

    let cx = Ctx::new(group, entries, owner);
    let flags = derive_flags(&cx, &mut st);
    derive_members(&cx, &mut st, genesis, owner_name);
    derive_admins(&mut st, &cx);
    derive_settings(&cx, &mut st);
    derive_enc_keys(&cx, &mut st);
    derive_hosts(&cx, &mut st);
    derive_call_hosts(&cx, &mut st);
    derive_items(&cx, &mut st, &flags);
    st
}

/// Counted flags per target (by an admin as of the flag, never on the owner); the targets with
/// `REMOVAL_FLAGS` are removed. Returns the flags for the "removed" lines.
fn derive_flags<'a>(cx: &Ctx<'a>, st: &mut GroupState) -> BTreeMap<String, Vec<&'a Entry>> {
    let mut flags: BTreeMap<String, Vec<&Entry>> = BTreeMap::new();
    for e in cx.entries {
        if let Body::Flag { member } = &e.rec.body
            && *member != cx.owner
            && cx.admin_as_of(&e.rec.author, Some(&e.rec.seen))
        {
            flags.entry(member.clone()).or_default().push(e);
        }
    }
    for v in flags.values_mut() {
        v.sort_by(|a, b| order(a).cmp(&order(b)));
    }
    st.pos.flags = flags.iter().map(|(d, f)| (d.clone(), f.iter().map(|e| (e.peer, e.counter)).collect())).collect();
    let info = |e: &&Entry| FlagInfo { by: e.rec.author.clone(), at: e.rec.at, id: e.id.clone() };
    for (did, f) in &flags {
        if f.len() >= REMOVAL_FLAGS {
            st.removed.insert(did.clone(), f.iter().map(info).collect());
        } else {
            st.flags.insert(did.clone(), f.iter().map(info).collect());
        }
    }
    flags
}

/// The owner, and every other DID whose last valid join/leave is a join and who is not removed.
fn derive_members(cx: &Ctx, st: &mut GroupState, genesis: &Entry, owner_name: String) {
    let owner = &cx.owner;
    let mut events: BTreeMap<String, Vec<&Entry>> = BTreeMap::new();
    for e in cx.entries {
        let counts = match &e.rec.body {
            Body::Join { .. } | Body::OpenJoin { .. } => cx.joins(e),
            Body::Leave => true,
            _ => false,
        };
        if counts && e.rec.author != *owner {
            events.entry(e.rec.author.clone()).or_default().push(e);
        }
    }
    let mut devices: BTreeMap<String, BTreeSet<[u8; 32]>> = BTreeMap::new();
    for e in cx.entries {
        devices.entry(e.rec.author.clone()).or_default().insert(e.rec.dev);
    }
    st.members.insert(
        owner.clone(),
        MemberInfo { did: owner.clone(), name: owner_name, joined_at: genesis.rec.at, invited_by: None, devices: devices.get(owner).cloned().unwrap_or_default() },
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
            Body::Join { name, .. } | Body::OpenJoin { name } => {
                st.members.insert(
                    did.clone(),
                    MemberInfo { did: did.clone(), name: name.clone(), joined_at: last.rec.at, invited_by: cx.valid_join(last), devices: devices.get(&did).cloned().unwrap_or_default() },
                );
            }
            _ => {
                st.left.insert(did);
            }
        }
    }
    // Positions of the valid joins and leaves (`events` holds only those, sorted).
    st.pos.events = events.iter().map(|(d, evs)| (d.clone(), evs.iter().map(|e| (e.peer, e.counter, matches!(e.rec.body, Body::Join { .. } | Body::OpenJoin { .. }))).collect())).collect();
}

fn derive_admins(st: &mut GroupState, cx: &Ctx) {
    for did in st.members.keys() {
        if cx.admin_as_of(did, None) {
            st.admins.insert(did.clone());
        }
    }
}

/// `Open` / `AdminsOnly` now, and what `post_visible` needs to judge a post's author as of
/// its frontier: the admins-only records and the owner's admin records.
fn derive_settings(cx: &Ctx, st: &mut GroupState) {
    st.open = cx.open_as_of(None);
    st.pos.admins_only = cx.admins_only_recs.clone();
    st.admins_only = in_force(&st.pos.admins_only, None);
    for e in &cx.admin_recs {
        if let Body::Admin { member, admin } = &e.rec.body {
            st.pos.admin_events.entry(member.clone()).or_default().push((e.peer, e.counter, *admin));
        }
    }
}

/// Encryption keys of members (U3 mail).
fn derive_enc_keys(cx: &Ctx, st: &mut GroupState) {
    let mut keys: Vec<&Entry> = cx.entries.iter().filter(|e| matches!(e.rec.body, Body::EncKey { .. }) && st.members.contains_key(&e.rec.author)).collect();
    keys.sort_by(|a, b| order(a).cmp(&order(b)));
    for e in keys {
        if let Body::EncKey { key } = &e.rec.body {
            st.enc_keys.insert(e.rec.author.clone(), *key);
        }
    }
}

fn derive_hosts(cx: &Ctx, st: &mut GroupState) {
    let mut permits: BTreeMap<(String, [u8; 32]), &Entry> = BTreeMap::new();
    let mut accepts: BTreeMap<(String, [u8; 32]), &Entry> = BTreeMap::new();
    for e in cx.entries {
        match &e.rec.body {
            Body::HostPermit { host, host_dev, .. } if cx.admin_as_of(&e.rec.author, Some(&e.rec.seen)) => {
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
}

/// Call hosts (U21): the same rule over their own permit / acceptance records.
fn derive_call_hosts(cx: &Ctx, st: &mut GroupState) {
    let mut cpermits: BTreeMap<(String, [u8; 32]), &Entry> = BTreeMap::new();
    let mut caccepts: BTreeMap<(String, [u8; 32]), &Entry> = BTreeMap::new();
    for e in cx.entries {
        match &e.rec.body {
            Body::CallHostPermit { host, host_dev, .. } if cx.admin_as_of(&e.rec.author, Some(&e.rec.seen)) => {
                let k = (host.clone(), *host_dev);
                if cpermits.get(&k).is_none_or(|p| order(p) < order(e)) {
                    cpermits.insert(k, e);
                }
            }
            Body::CallHostAccept { .. } => {
                let k = (e.rec.author.clone(), e.rec.dev);
                if caccepts.get(&k).is_none_or(|p| order(p) < order(e)) {
                    caccepts.insert(k, e);
                }
            }
            _ => {}
        }
    }
    for ((did, dev), p) in &cpermits {
        let Body::CallHostPermit { active: true, .. } = p.rec.body else { continue };
        if !st.members.contains_key(did) {
            continue;
        }
        let acc = caccepts.get(&(did.clone(), *dev));
        let info = CallHostInfo { did: did.clone(), dev: *dev, appointed_by: p.rec.author.clone(), at: p.rec.at, permit: p.id.clone() };
        if acc.is_some_and(|a| matches!(a.rec.body, Body::CallHostAccept { active: true })) {
            st.call_hosts.push(info);
        } else if !acc.is_some_and(|a| order(a) > order(p)) {
            st.call_host_requests.push(info);
        }
    }
}

/// The conversation: system lines, and the rotation triggers (counted host revocations and
/// removals).
fn derive_items(cx: &Ctx, st: &mut GroupState, flags: &BTreeMap<String, Vec<&Entry>>) {
    for e in cx.entries {
        let r = &e.rec;
        let item = |kind: ItemKind, subject: &str| Item {
            id: e.id.clone(),
            kind,
            author: r.author.clone(),
            subject: subject.to_string(),
            at: r.at,
            text: String::new(),
            reply_to: None,
            before_removal: false,
            peer: e.peer,
            counter: e.counter,
        };
        match &r.body {
            Body::Join { .. } | Body::OpenJoin { .. } if cx.joins(e) && r.author != cx.owner => st.items.push(item(ItemKind::Joined, &r.author)),
            Body::Leave if r.author != cx.owner => st.items.push(item(ItemKind::Left, &r.author)),
            Body::Admin { member, admin } if r.author == cx.owner => {
                st.items.push(item(if *admin { ItemKind::AdminGranted } else { ItemKind::AdminRevoked }, member))
            }
            Body::HostPermit { host, active, .. } if cx.admin_as_of(&r.author, Some(&r.seen)) => {
                st.items.push(item(if *active { ItemKind::HostAdded } else { ItemKind::HostRemoved }, host))
            }
            _ => {}
        }
    }
    // One "removed" line per removed member, at its 3rd counted flag.
    for (did, f) in flags {
        if f.len() >= REMOVAL_FLAGS {
            let third = f[REMOVAL_FLAGS - 1];
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
}
