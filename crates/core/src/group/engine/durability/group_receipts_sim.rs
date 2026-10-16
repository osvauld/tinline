//! Group member receipts, step 1: deterministic multi-node simulation over real in-process
//! `GroupCore`s (sealed stores, real record/receipt signatures, the real acceptance paths).
//! Frames are exchanged by direct calls; loss, reordering and relays are scripted. This is
//! NOT the group wire or a network test (no receipts frame exists yet).
use super::*;
use crate::chat::doc::day_start;
use crate::group::doc::batching::BatchLimits;
use crate::group::engine::member_receipts::controller;
use crate::group::member_receipts::{GroupReceiptStatus, PENDING_PREFIX, scope};
use crate::receipts::api::{ReceiptLevel, ReceiptTick};
use crate::receipts::store::messages::group_version;
use crate::receipts::{Kind, Receipt, ReceiptError, receipt_id};
use loro::{ExportMode, LoroDoc, LoroValue};

const DAY: &str = "2026-10-16";
const LIMITS: BatchLimits = BatchLimits {
    bytes: crate::receipts::MAX_UPDATE_BYTES,
    operations: 4096,
    writers: 4096,
};

/// One device of one account, on its own sealed store.
fn device(id: &identity::Identity, dev: [u8; 32], store: GroupStore) -> GroupCore {
    let mut core = receiver();
    core.store = store;
    core.account = id.clone();
    core.did = id.did().into();
    core.dev = dev;
    core.wid = writer_id(&core.did, &dev);
    core.receipts = controller(&core.store, &core.did, dev);
    core
}

/// Rebuilds a device's groups from its store, as `group_core` does at start.
fn load_groups(core: &GroupCore) {
    for (gid, meta) in core.store.groups().unwrap() {
        let (snap, ups) = core.store.meta_rows(&gid).unwrap();
        let doc = GroupDoc::load(&gid, core.wid, snap.as_deref(), &ups).unwrap();
        let entries = doc.entries_trusted().unwrap();
        let state = derive_meta(&gid, &entries, &doc.vv());
        core.groups.lock().insert(
            gid,
            Live {
                doc,
                entries,
                state,
                meta,
                shards: Default::default(),
            },
        );
    }
}

struct Sim {
    gid: String,
    /// `nodes[0]` is the owner's only device.
    nodes: Vec<GroupCore>,
    clock: i64,
}

impl Sim {
    fn new(members: usize) -> Self {
        Self::with_owner_store(members, GroupStore::in_memory("group receipts sim"))
    }

    fn with_owner_store(members: usize, store: GroupStore) -> Self {
        let owner = device(&identity::generate().0, [1; 32], store);
        let nonce = [9; 16];
        let gid = group_id(&owner.did, &nonce);
        owner.groups.lock().insert(
            gid.clone(),
            Live::empty(&gid, owner.wid, GroupMeta::default()),
        );
        let mut sim = Sim {
            gid,
            nodes: vec![owner],
            clock: day_start(DAY).unwrap() + 3_600_000,
        };
        let at = sim.tick();
        sim.nodes[0]
            .write_at(
                &sim.gid,
                Body::Genesis {
                    name: "sim".into(),
                    nonce,
                    owner_name: "owner".into(),
                },
                at,
            )
            .unwrap();
        for i in 0..members {
            let dev = [(i % 200) as u8 + 2; 32];
            sim.join(device(
                &identity::generate().0,
                dev,
                GroupStore::in_memory("group receipts sim"),
            ));
        }
        sim.meta_all();
        sim
    }

    fn tick(&mut self) -> i64 {
        self.clock += 1000;
        self.clock
    }

    /// A new member account (its first device) joins with an invite of the owner.
    fn join(&mut self, core: GroupCore) -> usize {
        self.attach(&core);
        let invite = issue_invite(
            &self.nodes[0].account,
            &self.gid,
            1,
            crate::group::record::MAX_INVITE_TTL_MS,
        );
        let at = self.tick();
        core.write_at(
            &self.gid,
            Body::Join {
                invite,
                name: "member".into(),
            },
            at,
        )
        .unwrap();
        self.meta(&core, &self.nodes[0]);
        self.nodes.push(core);
        self.nodes.len() - 1
    }

    /// Another device of an existing member account (membership is per DID: no join record).
    fn link(&mut self, of: usize, dev: [u8; 32]) -> usize {
        let core = device(
            &self.nodes[of].account.clone(),
            dev,
            GroupStore::in_memory("group receipts sim"),
        );
        self.attach(&core);
        self.nodes.push(core);
        self.nodes.len() - 1
    }

    fn attach(&self, core: &GroupCore) {
        core.groups.lock().insert(
            self.gid.clone(),
            Live::empty(&self.gid, core.wid, GroupMeta::default()),
        );
        self.meta(&self.nodes[0], core);
    }

    fn meta(&self, from: &GroupCore, to: &GroupCore) {
        let vv = to.doc_vv(&self.gid, &DocId::Meta);
        let update = from.groups.lock()[self.gid.as_str()].doc.export_since(&vv);
        if let Some(u) = update {
            to.apply(&self.gid, &DocId::Meta, &u, |_, _, _| Ok(()))
                .unwrap();
        }
    }

    /// The owner's meta to everyone, then everyone's to the owner and back.
    fn meta_all(&self) {
        for n in &self.nodes[1..] {
            self.meta(n, &self.nodes[0]);
        }
        for n in &self.nodes[1..] {
            self.meta(&self.nodes[0], n);
        }
    }

    fn day_update(&self, from: &GroupCore, theirs: &VersionVector) -> Option<Vec<u8>> {
        if from.store.shard_meta(&self.gid, DAY).unwrap().is_none() {
            return None;
        }
        let mut groups = from.groups.lock();
        let Live { state, shards, .. } = groups.get_mut(&self.gid).unwrap();
        shards
            .get(&from.store, &self.gid, from.wid, DAY, state)
            .unwrap()
            .doc
            .export_since(theirs)
    }

    /// The posts of `from`'s day doc that `to` lacks; true if anything arrived.
    fn day(&self, from: usize, to: usize) -> bool {
        let (from, to) = (&self.nodes[from], &self.nodes[to]);
        let theirs = to.doc_vv(&self.gid, &DocId::Day(DAY.into()));
        match self.day_update(from, &theirs) {
            Some(u) => to
                .apply(&self.gid, &DocId::Day(DAY.into()), &u, |_, _, _| Ok(()))
                .unwrap()
                .is_some(),
            None => false,
        }
    }

    fn post(&mut self, by: usize, text: &str) -> String {
        let at = self.tick();
        self.nodes[by]
            .write_at(
                &self.gid,
                Body::Post {
                    text: text.into(),
                    reply_to: None,
                },
                at,
            )
            .unwrap()
            .0
            .id
    }

    fn write(&mut self, by: usize, body: Body) {
        let at = self.tick();
        self.nodes[by].write_at(&self.gid, body, at).unwrap();
    }

    /// Signs pending intents; the statements this device issued.
    fn pump(&self, n: usize) -> usize {
        self.nodes[n].member_receipts_pump().unwrap().len()
    }

    fn read(&self, n: usize) -> usize {
        self.nodes[n].mark_read(&self.gid, true).unwrap();
        self.pump(n)
    }

    fn vv(&self, n: usize) -> VersionVector {
        self.nodes[n].member_receipts_vv(&self.gid, DAY).unwrap()
    }

    /// One receipt frame: what `from` holds of the day ledger beyond `theirs`.
    fn frame(&self, from: usize, theirs: &VersionVector) -> Option<Vec<u8>> {
        self.nodes[from]
            .member_receipts_export(&self.gid, DAY, theirs, LIMITS)
            .unwrap()
            .map(|b| {
                assert!(!b.more, "simulation frames fit one batch");
                b.update
            })
    }

    fn import(&self, to: usize, update: &[u8]) -> Result<usize, ReceiptError> {
        self.nodes[to]
            .member_receipts_import(&self.gid, DAY, update)
            .map(|v| v.len())
    }

    /// Catch-up from `from` to `to` by the receiver's committed frontier (what a hello carries).
    fn receipts(&self, from: usize, to: usize) -> Result<usize, ReceiptError> {
        match self.frame(from, &self.vv(to)) {
            Some(u) => self.import(to, &u),
            None => Ok(0),
        }
    }

    fn status(&self, at: usize, post: &str) -> GroupReceiptStatus {
        self.nodes[at]
            .member_receipt_status(&self.gid, DAY, post)
            .unwrap()
    }

    fn entries(&self, at: usize) -> usize {
        self.nodes[at]
            .receipts
            .entries(&scope(&self.gid, DAY))
            .unwrap()
            .len()
    }

    fn frontier(&self, n: usize) -> Vec<(u64, i32)> {
        self.nodes[n].groups.lock()[self.gid.as_str()].doc.seen()
    }

    fn digest(&self, at: usize, post: &str) -> [u8; 32] {
        let core = &self.nodes[at];
        let mut groups = core.groups.lock();
        let Live { state, shards, .. } = groups.get_mut(&self.gid).unwrap();
        let shard = shards
            .get(&core.store, &self.gid, core.wid, DAY, state)
            .unwrap();
        group_version(&shard.doc.raw_value(post).unwrap())
    }

    /// Everyone gets the owner's posts and signs; every member's statements reach the owner.
    fn deliver_all(&self) {
        for n in 1..self.nodes.len() {
            self.day(0, n);
            self.pump(n);
        }
        for n in 1..self.nodes.len() {
            self.receipts(n, 0).unwrap();
        }
    }
}

/// A statement signed by `by`'s DID (a hand-built claim: a modified client or a forger).
fn claim(
    by: &identity::Identity,
    dev: [u8; 32],
    gid: &str,
    kind: Kind,
    mut messages: Vec<[u8; 32]>,
    frontier: Vec<(u64, i32)>,
) -> Vec<u8> {
    messages.sort();
    Receipt {
        scope: scope(gid, DAY),
        kind,
        recipient: by.did().into(),
        device: dev,
        messages,
        metadata: frontier,
        call: None,
    }
    .sign_claim(by)
    .unwrap()
    .to_bytes()
}

/// A native ledger update inserting raw `bytes` under writer `peer` (no checks: the importer's).
fn raw_update(bytes: &[u8], peer: u64) -> Vec<u8> {
    let doc = LoroDoc::new();
    doc.set_peer_id(peer).unwrap();
    doc.get_map("receipts")
        .insert(&receipt_id(bytes), LoroValue::Binary(bytes.to_vec().into()))
        .unwrap();
    doc.commit();
    doc.export(ExportMode::updates(&VersionVector::default()))
        .unwrap()
}

fn levels(s: &GroupReceiptStatus) -> Vec<(ReceiptLevel, bool)> {
    s.members
        .iter()
        .map(|m| (m.receipts.level, m.eligible))
        .collect()
}

#[test]
fn all_members_deliver_then_read_moves_the_mark_waiting_delivered_read() {
    let mut sim = Sim::new(3);
    let post = sim.post(0, "hello group");
    let s = sim.status(0, &post);
    assert_eq!(
        (s.tick, s.eligible, s.delivered, s.read),
        (ReceiptTick::Waiting, 3, 0, 0)
    );
    sim.deliver_all();
    let s = sim.status(0, &post);
    assert_eq!(
        (s.tick, s.eligible, s.delivered, s.read),
        (ReceiptTick::Delivered, 3, 3, 0)
    );
    assert!(s.members.iter().all(|m| m.eligible
        && m.receipts.level == ReceiptLevel::Delivered
        && m.receipts.proofs.len() == 1));
    for n in 1..4 {
        assert_eq!(sim.read(n), 1, "one read statement per member");
        sim.receipts(n, 0).unwrap();
    }
    let s = sim.status(0, &post);
    assert_eq!((s.tick, s.delivered, s.read), (ReceiptTick::Read, 3, 3));
    // Every member sees everyone's statements once the ledger reaches it (any relay works).
    for n in 1..4 {
        sim.receipts(0, n).unwrap();
        let other = sim.status(n, &post);
        assert_eq!(
            (other.tick, other.eligible, other.delivered, other.read),
            (ReceiptTick::None, 3, 3, 3)
        );
        assert_eq!(other.version, s.version);
    }
}

#[test]
fn partial_delivery_stays_one_tick_until_the_last_member_catches_up() {
    let mut sim = Sim::new(3);
    let post = sim.post(0, "who is here?");
    for n in 1..3 {
        sim.day(0, n);
        sim.pump(n);
        sim.receipts(n, 0).unwrap();
    }
    let s = sim.status(0, &post);
    assert_eq!(
        (s.tick, s.delivered, s.eligible),
        (ReceiptTick::Waiting, 2, 3)
    );
    assert_eq!(
        levels(&s)
            .iter()
            .filter(|(l, _)| *l == ReceiptLevel::Unconfirmed)
            .count(),
        1
    );
    sim.read(1);
    sim.receipts(1, 0).unwrap();
    let s = sim.status(0, &post);
    assert_eq!((s.tick, s.delivered, s.read), (ReceiptTick::Waiting, 2, 1));
    // The offline member comes back: it gets the post from another member, not the owner.
    assert!(sim.day(1, 3));
    assert_eq!(sim.pump(3), 1);
    sim.receipts(3, 0).unwrap();
    let s = sim.status(0, &post);
    assert_eq!(
        (s.tick, s.delivered, s.read),
        (ReceiptTick::Delivered, 3, 1)
    );
}

#[test]
fn duplicate_frames_posts_and_relays_add_nothing() {
    let mut sim = Sim::new(2);
    let post = sim.post(0, "once");
    sim.day(0, 1);
    assert_eq!(sim.pump(1), 1);
    // The same post batch again issues no second obligation.
    let again = sim
        .day_update(&sim.nodes[0], &VersionVector::default())
        .unwrap();
    assert!(
        sim.nodes[1]
            .apply(&sim.gid, &DocId::Day(DAY.into()), &again, |_, _, _| Ok(()))
            .unwrap()
            .is_none()
    );
    assert_eq!(sim.pump(1), 0);
    let frame = sim.frame(1, &VersionVector::default()).unwrap();
    assert_eq!(sim.import(0, &frame).unwrap(), 1);
    assert_eq!(
        sim.import(0, &frame).unwrap(),
        0,
        "a replayed frame is idempotent"
    );
    // The same statement relayed by another member: byte-identical, no new entry.
    sim.day(0, 2);
    assert_eq!(sim.import(2, &frame).unwrap(), 1);
    assert_eq!(
        sim.frame(2, &sim.vv(0)),
        None,
        "the relay holds nothing the owner lacks"
    );
    let relayed = sim.frame(2, &VersionVector::default()).unwrap();
    assert_eq!(sim.import(0, &relayed).unwrap(), 0);
    assert_eq!(sim.entries(0), 1);
    let s = sim.status(0, &post);
    let row = s
        .members
        .iter()
        .find(|m| m.receipts.did == sim.nodes[1].did)
        .unwrap();
    assert_eq!((s.delivered, row.receipts.proofs.len()), (1, 1));
}

#[test]
fn lost_and_reordered_frames_are_refused_then_caught_up() {
    let mut sim = Sim::new(2);
    let post = sim.post(0, "order");
    for n in 1..3 {
        sim.day(0, n);
        sim.pump(n);
    }
    let first = sim.frame(1, &VersionVector::default()).unwrap();
    let after_first = sim.vv(1);
    sim.read(1);
    let second = sim.frame(1, &after_first).unwrap();
    // Reordered: the read frame arrives before its predecessor and is not stored.
    assert!(matches!(sim.import(0, &second), Err(ReceiptError::Pending)));
    assert_eq!(sim.entries(0), 0);
    assert_eq!(sim.import(0, &first).unwrap(), 1);
    assert_eq!(sim.import(0, &second).unwrap(), 1);
    // Lost: member 2's delivered frame never arrives; its read frame alone is refused; the
    // next catch-up by the owner's committed frontier carries both.
    let lost_vv = sim.vv(2);
    assert!(
        !lost_vv.is_empty(),
        "member 2 holds its own delivered statement"
    );
    sim.read(2);
    let read_only = sim.frame(2, &lost_vv).unwrap();
    assert!(matches!(
        sim.import(0, &read_only),
        Err(ReceiptError::Pending)
    ));
    assert_eq!(sim.receipts(2, 0).unwrap(), 2);
    let s = sim.status(0, &post);
    assert_eq!((s.tick, s.read), (ReceiptTick::Read, 2));
}

#[test]
fn forged_unsigned_wrong_group_foreign_writer_and_non_member_receipts_are_refused() {
    let mut sim = Sim::new(2);
    let post = sim.post(0, "guarded");
    sim.day(0, 1);
    let d = sim.digest(0, &post);
    let m1 = sim.nodes[1].account.clone();
    let f1 = sim.frontier(1);
    let peer = |did: &str, dev: [u8; 32]| writer_id(did, &dev);
    // Forged: a valid statement with one signature bit flipped.
    let mut forged = claim(
        &m1,
        [40; 32],
        &sim.gid,
        Kind::Delivered,
        vec![d],
        f1.clone(),
    );
    *forged.last_mut().unwrap() ^= 1;
    assert!(
        sim.import(0, &raw_update(&forged, peer(m1.did(), [40; 32])))
            .is_err()
    );
    // Unsigned / not a statement at all.
    assert!(
        sim.import(
            0,
            &raw_update(b"not a signed receipt", peer(m1.did(), [41; 32]))
        )
        .is_err()
    );
    // Wrong group: m1's genuine statement for another group's ledger.
    let other = "ab".repeat(32);
    let wrong = Receipt {
        scope: scope(&other, DAY),
        kind: Kind::Delivered,
        recipient: m1.did().into(),
        device: [42; 32],
        messages: vec![d],
        metadata: f1.clone(),
        call: None,
    }
    .sign_claim(&m1)
    .unwrap()
    .to_bytes();
    assert!(matches!(
        sim.import(0, &raw_update(&wrong, peer(m1.did(), [42; 32]))),
        Err(ReceiptError::Scope)
    ));
    // Foreign writer: m1's genuine statement inserted under another writer's ops.
    let genuine = claim(
        &m1,
        [43; 32],
        &sim.gid,
        Kind::Delivered,
        vec![d],
        f1.clone(),
    );
    assert!(matches!(
        sim.import(0, &raw_update(&genuine, peer(m1.did(), [44; 32]))),
        Err(ReceiptError::Writer)
    ));
    // A non-member's genuine signature.
    let outsider = identity::generate().0;
    let stranger = claim(
        &outsider,
        [45; 32],
        &sim.gid,
        Kind::Read,
        vec![d],
        f1.clone(),
    );
    match sim.import(0, &raw_update(&stranger, peer(outsider.did(), [45; 32]))) {
        Err(ReceiptError::Policy(m)) => assert!(m.contains("issuer not admitted"), "{m}"),
        other => panic!("non-member statement accepted: {other:?}"),
    }
    // The author's own account cannot confirm its own post.
    let owner = sim.nodes[0].account.clone();
    let own = claim(
        &owner,
        [46; 32],
        &sim.gid,
        Kind::Read,
        vec![d],
        sim.frontier(0),
    );
    assert!(matches!(
        sim.import(1, &raw_update(&own, peer(owner.did(), [46; 32]))),
        Err(ReceiptError::Policy(_))
    ));
    assert_eq!(sim.entries(0), 0);
    assert_eq!(sim.status(0, &post).delivered, 0);
    // The genuine statement under its own writer is accepted.
    assert_eq!(
        sim.import(0, &raw_update(&genuine, peer(m1.did(), [43; 32])))
            .unwrap(),
        1
    );
}

#[test]
fn late_joiner_confirms_only_posts_written_after_it_joined() {
    let mut sim = Sim::new(2);
    let before = sim.post(0, "before you came");
    sim.deliver_all();
    let late = sim.join(device(
        &identity::generate().0,
        [30; 32],
        GroupStore::in_memory("group receipts sim"),
    ));
    sim.meta_all();
    let after = sim.post(0, "welcome");
    assert!(sim.day(0, late));
    assert_eq!(sim.pump(late), 1, "only the post written after the join");
    for n in 1..3 {
        sim.day(0, n);
        sim.pump(n);
        sim.receipts(n, 0).unwrap();
    }
    sim.receipts(late, 0).unwrap();
    let s = sim.status(0, &before);
    assert_eq!(
        (s.tick, s.eligible, s.delivered),
        (ReceiptTick::Delivered, 2, 2)
    );
    assert!(
        s.members
            .iter()
            .all(|m| m.receipts.did != sim.nodes[late].did)
    );
    let s = sim.status(0, &after);
    assert_eq!(
        (s.tick, s.eligible, s.delivered),
        (ReceiptTick::Delivered, 3, 3)
    );
    // A modified client signing for the earlier post is refused everywhere.
    let me = sim.nodes[late].account.clone();
    let bad = claim(
        &me,
        [31; 32],
        &sim.gid,
        Kind::Read,
        vec![sim.digest(0, &before)],
        sim.frontier(late),
    );
    match sim.import(0, &raw_update(&bad, writer_id(me.did(), &[31; 32]))) {
        Err(ReceiptError::Policy(m)) => {
            assert!(m.contains("not a member when the post was written"), "{m}")
        }
        other => panic!("late joiner's earlier-post receipt accepted: {other:?}"),
    }
    // The joiner's own read cursor covers both posts but signs only the later one.
    assert_eq!(sim.read(late), 1);
}

#[test]
fn removed_members_signed_while_admitted_receipt_counts_late_and_post_removal_ones_are_refused() {
    let mut sim = Sim::new(3);
    let removed = 3;
    let post = sim.post(0, "before the removal");
    for n in 1..4 {
        sim.day(0, n);
        sim.pump(n);
    }
    // The removed member's statement (signed while admitted) is held back in flight.
    let in_flight = sim.frame(removed, &VersionVector::default()).unwrap();
    sim.receipts(1, 0).unwrap();
    sim.receipts(2, 0).unwrap();
    assert_eq!(sim.status(0, &post).tick, ReceiptTick::Waiting);
    let target = sim.nodes[removed].did.clone();
    for _ in 0..3 {
        sim.write(
            0,
            Body::Flag {
                member: target.clone(),
            },
        );
    }
    // Removed after the post: it no longer blocks the mark.
    let s = sim.status(0, &post);
    assert_eq!(
        (s.tick, s.eligible, s.delivered),
        (ReceiptTick::Delivered, 2, 2)
    );
    // Its late statement is accepted and kept, listed apart from the counted members.
    assert_eq!(sim.import(0, &in_flight).unwrap(), 1);
    let s = sim.status(0, &post);
    assert_eq!(
        (s.tick, s.eligible, s.delivered),
        (ReceiptTick::Delivered, 2, 2)
    );
    let row = s.members.iter().find(|m| m.receipts.did == target).unwrap();
    assert_eq!(
        (row.eligible, row.receipts.level),
        (false, ReceiptLevel::Delivered)
    );
    // A member leaving after the post stops blocking too; its statement stays listed.
    sim.write(2, Body::Leave);
    sim.meta(&sim.nodes[2], &sim.nodes[0]);
    let s = sim.status(0, &post);
    assert_eq!((s.eligible, s.delivered), (1, 1));
    assert_eq!(s.members.iter().filter(|m| !m.eligible).count(), 2);
    // Once it saw its removal, an honest device issues nothing more.
    sim.meta(&sim.nodes[0], &sim.nodes[removed]);
    assert_eq!(sim.read(removed), 0);
    // A modified client signing after it saw the removal is refused.
    let r = sim.nodes[removed].account.clone();
    let d = sim.digest(0, &post);
    let post_removal = claim(
        &r,
        [50; 32],
        &sim.gid,
        Kind::Read,
        vec![d],
        sim.frontier(removed),
    );
    match sim.import(0, &raw_update(&post_removal, writer_id(r.did(), &[50; 32]))) {
        Err(ReceiptError::Policy(m)) => assert!(m.contains("issuer not admitted"), "{m}"),
        other => panic!("post-removal statement accepted: {other:?}"),
    }
    // A stale (pre-removal) frontier cannot cover a post whose author saw the removal.
    let later = sim.post(0, "after the removal");
    let stale = claim(
        &r,
        [51; 32],
        &sim.gid,
        Kind::Delivered,
        vec![sim.digest(0, &later)],
        sim.frontier(1),
    );
    match sim.import(0, &raw_update(&stale, writer_id(r.did(), &[51; 32]))) {
        Err(ReceiptError::Policy(m)) => {
            assert!(m.contains("does not include the post's frontier"), "{m}")
        }
        other => panic!("stale-frontier statement accepted: {other:?}"),
    }
    assert_eq!(sim.entries(0), 3, "every accepted statement is kept");
}

#[test]
fn pending_posts_and_unknown_frontiers_wait_instead_of_failing() {
    let mut sim = Sim::new(2);
    // A post naming a meta record member 1 lacks: hidden (pending) there, no obligation yet.
    sim.write(0, Body::EncKey { key: [7; 32] });
    let post = sim.post(0, "needs the key record");
    sim.meta(&sim.nodes[0], &sim.nodes[2]);
    sim.day(0, 2);
    sim.pump(2);
    // Member 2's statement reaches member 1 before member 1 holds the post: pending.
    let frame = sim.frame(2, &VersionVector::default()).unwrap();
    match sim.import(1, &frame) {
        Err(ReceiptError::Policy(m)) => assert!(m.starts_with(PENDING_PREFIX), "{m}"),
        other => panic!("statement for an unknown post accepted: {other:?}"),
    }
    sim.day(0, 1);
    assert_eq!(sim.pump(1), 0, "a pending post issues nothing");
    match sim.import(1, &frame) {
        Err(ReceiptError::Policy(m)) => assert!(m.starts_with(PENDING_PREFIX), "{m}"),
        other => panic!("statement for a pending post accepted: {other:?}"),
    }
    // The meta record arrives: the post shows, the rejudge transaction records the obligation.
    sim.meta(&sim.nodes[0], &sim.nodes[1]);
    assert_eq!(sim.pump(1), 1);
    assert_eq!(sim.import(1, &frame).unwrap(), 1);
    sim.receipts(1, 0).unwrap();
    sim.receipts(2, 0).unwrap();
    assert_eq!(sim.status(0, &post).tick, ReceiptTick::Delivered);
}

#[test]
fn one_device_of_a_member_account_counts_for_the_account() {
    let mut sim = Sim::new(1);
    let second = sim.link(1, [60; 32]);
    let post = sim.post(0, "two devices");
    sim.day(0, 1);
    sim.pump(1);
    sim.receipts(1, 0).unwrap();
    let s = sim.status(0, &post);
    assert_eq!(
        (s.tick, s.eligible, s.delivered),
        (ReceiptTick::Delivered, 1, 1)
    );
    // The other device also confirms (it had not seen the first one's statement): still one
    // account, now with two proofs from two devices.
    sim.day(0, second);
    assert_eq!(sim.pump(second), 1);
    sim.receipts(second, 0).unwrap();
    let s = sim.status(0, &post);
    assert_eq!((s.eligible, s.delivered, s.members.len()), (1, 1, 1));
    let devices: std::collections::BTreeSet<_> = s.members[0]
        .receipts
        .proofs
        .iter()
        .map(|p| p.signer_device.clone())
        .collect();
    assert_eq!(devices.len(), 2);
    // A device that already holds its sibling's statement does not sign again.
    let third = sim.link(1, [61; 32]);
    sim.receipts(0, third).unwrap_err();
    sim.day(0, third);
    sim.receipts(0, third).unwrap();
    sim.nodes[third].mark_read(&sim.gid, true).unwrap();
    assert_eq!(sim.pump(third), 1, "read was not covered yet");
}

struct Dir(std::path::PathBuf);
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn intents_ledgers_and_kept_statements_survive_reopen() {
    let dir = Dir(std::env::temp_dir().join(format!(
        "tinline-group-receipts-{}",
        hex::encode({
            let r: [u8; 8] = cryptography::rng::bytes();
            r
        })
    )));
    std::fs::create_dir_all(&dir.0).unwrap();
    let (owner_path, member_path) = (dir.0.join("owner.redb"), dir.0.join("member.redb"));
    let mut sim = Sim::with_owner_store(
        1,
        GroupStore::open(owner_path.clone(), "owner phrase").unwrap(),
    );
    // Member 1 on a file store: replace the in-memory device by a durable one of the same account.
    let m = sim.nodes[1].account.clone();
    let durable = device(
        &m,
        [20; 32],
        GroupStore::open(member_path.clone(), "member phrase").unwrap(),
    );
    sim.attach(&durable);
    sim.nodes[1] = durable;
    sim.join(device(
        &identity::generate().0,
        [3; 32],
        GroupStore::in_memory("group receipts sim"),
    ));
    sim.meta_all();
    let post = sim.post(0, "durable");
    sim.day(0, 1);
    // Close before signing: the obligation is durable with the accepted post.
    drop(sim.nodes.remove(1));
    let reopened = device(
        &m,
        [20; 32],
        GroupStore::open(member_path.clone(), "member phrase").unwrap(),
    );
    load_groups(&reopened);
    sim.nodes.insert(1, reopened);
    assert_eq!(sim.pump(1), 1, "the queued intent is signed after reopen");
    sim.receipts(1, 0).unwrap();
    sim.day(0, 2);
    sim.pump(2);
    let held = sim.frame(2, &VersionVector::default()).unwrap();
    sim.import(0, &held).unwrap();
    let before = sim.status(0, &post);
    assert_eq!(before.tick, ReceiptTick::Delivered);
    // Remove member 2 and reopen the owner: kept statements are not re-judged or dropped.
    let target = sim.nodes[2].did.clone();
    for _ in 0..3 {
        sim.write(
            0,
            Body::Flag {
                member: target.clone(),
            },
        );
    }
    let owner_id = sim.nodes[0].account.clone();
    drop(sim.nodes.remove(0));
    let owner = device(
        &owner_id,
        [1; 32],
        GroupStore::open(owner_path.clone(), "owner phrase").unwrap(),
    );
    load_groups(&owner);
    sim.nodes.insert(0, owner);
    let after = sim.status(0, &post);
    assert_eq!(sim.entries(0), 2);
    assert_eq!(
        (after.tick, after.eligible, after.delivered),
        (ReceiptTick::Delivered, 1, 1)
    );
    let kept = after
        .members
        .iter()
        .find(|r| r.receipts.did == target)
        .unwrap();
    assert_eq!(
        (kept.eligible, kept.receipts.level),
        (false, ReceiptLevel::Delivered)
    );
    assert_eq!(
        sim.import(0, &held).unwrap(),
        0,
        "replay after reopen is idempotent"
    );
}

/// Traffic/storage per post with one statement per member per kind (no batching): input for
/// the batching TODO. Run with `--nocapture` to see the table.
#[test]
fn receipt_count_and_bytes_per_post_by_group_size() {
    for members in [3usize, 10, 50] {
        let mut sim = Sim::new(members - 1);
        let post = sim.post(0, "measure");
        sim.deliver_all();
        for n in 1..members {
            sim.read(n);
            sim.receipts(n, 0).unwrap();
        }
        let s = sim.status(0, &post);
        assert_eq!((s.tick, s.read as usize), (ReceiptTick::Read, members - 1));
        let entries = sim.nodes[0]
            .receipts
            .entries(&scope(&sim.gid, DAY))
            .unwrap();
        assert_eq!(entries.len(), 2 * (members - 1));
        let signed: usize = entries
            .iter()
            .map(|e| {
                sim.nodes[0]
                    .receipts
                    .ledger(&scope(&sim.gid, DAY))
                    .unwrap()
                    .raw(&e.id)
                    .unwrap()
                    .len()
            })
            .sum();
        let full = sim.frame(0, &VersionVector::default()).unwrap().len();
        let one = sim.frame(1, &VersionVector::default()).unwrap().len();
        eprintln!(
            "group receipts: members={members} statements={} signed_bytes={signed} ({} per statement) full_day_frame={full} one_member_frame(delivered+read)={one}",
            entries.len(),
            signed / entries.len(),
        );
    }
}

// ---- over the group/4 receipt frames ----------------------------------------------------------
// Sessions are real `Sess` values registered on each core; frames are postcard-encoded and
// decoded like the wire does, then handed to the same handlers `group_read` calls. Members only
// talk to the owner (the host), so member-to-member statements travel through its relay.

struct Link {
    host: usize,
    member: usize,
    /// On the host, for the member; frames the host sends to it.
    at_host: Arc<Sess>,
    to_member: mpsc::UnboundedReceiver<Out>,
    /// On the member, for the host; frames the member sends.
    at_member: Arc<Sess>,
    to_host: mpsc::UnboundedReceiver<Out>,
}

fn open_sess(
    core: &GroupCore,
    gid: &str,
    peer: &GroupCore,
    hosting: bool,
) -> (Arc<Sess>, mpsc::UnboundedReceiver<Out>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let id = core.next.fetch_add(1, Ordering::SeqCst);
    let s = Arc::new(Sess {
        id,
        gid: gid.into(),
        did: peer.did.clone(),
        dev: peer.dev,
        hosting,
        joining: false,
        tx,
        peer_vv: Default::default(),
        peer_days: Default::default(),
        inv_done: AtomicBool::new(false),
        receipt_vv: Default::default(),
        receipt_waiting: Default::default(),
    });
    core.sessions.lock().insert(id, s.clone());
    (s, rx)
}

fn roundtrip(m: &GroupMsg) -> GroupMsg {
    let bytes = cryptography::codec::to_bytes(m).unwrap();
    assert!(bytes.len() <= crate::group::wire::MAX_FRAME);
    cryptography::codec::from_bytes(&bytes, crate::group::wire::MAX_FRAME).unwrap()
}

fn deliver(core: &GroupCore, sess: &Sess, m: GroupMsg) {
    match m {
        GroupMsg::ReceiptHello { days } => core.on_receipt_hello(sess, days).unwrap(),
        GroupMsg::Receipts { day, update } => {
            core.on_receipts(sess, &day, &update).unwrap();
        }
        GroupMsg::ReceiptAck { day, vv } => core.on_receipt_ack(sess, &day, &vv).unwrap(),
        other => panic!("unexpected frame {other:?}"),
    }
}

impl Sim {
    /// A session between the owner (host) and `member`, opened by hellos like `group_run`.
    fn connect(&self, member: usize) -> Link {
        let (h, m) = (&self.nodes[0], &self.nodes[member]);
        let (at_host, to_member) = open_sess(h, &self.gid, m, true);
        let (at_member, to_host) = open_sess(m, &self.gid, h, false);
        let _ = at_host.tx.send(Out::Msg(h.receipt_hello(&self.gid)));
        let _ = at_member.tx.send(Out::Msg(m.receipt_hello(&self.gid)));
        Link {
            host: 0,
            member,
            at_host,
            to_member,
            at_member,
            to_host,
        }
    }

    fn disconnect(&self, l: Link) {
        self.nodes[l.host].sessions.lock().remove(&l.at_host.id);
        self.nodes[l.member].sessions.lock().remove(&l.at_member.id);
    }

    /// Delivers queued frames until quiet. `fate(frame)` = how many copies arrive (0 = lost).
    /// Returns the receipt frames seen.
    fn flush(&self, links: &mut [Link], mut fate: impl FnMut(&GroupMsg) -> usize) -> usize {
        let mut seen = 0;
        loop {
            let mut any = false;
            for l in links.iter_mut() {
                while let Ok(Out::Msg(m)) = l.to_member.try_recv() {
                    any = true;
                    seen += 1;
                    let m = roundtrip(&m);
                    for _ in 0..fate(&m) {
                        deliver(&self.nodes[l.member], &l.at_member, m.clone());
                    }
                }
                while let Ok(Out::Msg(m)) = l.to_host.try_recv() {
                    any = true;
                    seen += 1;
                    let m = roundtrip(&m);
                    for _ in 0..fate(&m) {
                        deliver(&self.nodes[l.host], &l.at_host, m.clone());
                    }
                }
            }
            if !any {
                return seen;
            }
        }
    }
}

#[test]
fn receipt_frames_through_the_host_reach_every_member_byte_identical() {
    let mut sim = Sim::new(3);
    let mut links: Vec<Link> = (1..4).map(|m| sim.connect(m)).collect();
    sim.flush(&mut links, |_| 1);
    let post = sim.post(0, "over the wire");
    for n in 1..4 {
        sim.day(0, n);
        sim.nodes[n].member_receipts_pump_and_push();
    }
    assert!(sim.flush(&mut links, |_| 1) > 0);
    let s = sim.status(0, &post);
    assert_eq!((s.tick, s.delivered), (ReceiptTick::Delivered, 3));
    // The host relayed each member's statement to the others, original bytes unchanged.
    let sc = scope(&sim.gid, DAY);
    let mine = sim.nodes[1].receipts.entries(&sc).unwrap();
    assert_eq!(mine.len(), 3);
    for e in &mine {
        let here = sim.nodes[1]
            .receipts
            .ledger(&sc)
            .unwrap()
            .raw(&e.id)
            .unwrap();
        let signer = (1..4)
            .find(|n| sim.nodes[*n].did == e.receipt.recipient)
            .unwrap();
        assert_eq!(
            sim.nodes[signer]
                .receipts
                .ledger(&sc)
                .unwrap()
                .raw(&e.id)
                .unwrap(),
            here
        );
    }
    for n in 1..4 {
        sim.nodes[n].mark_read(&sim.gid, true).unwrap();
        sim.nodes[n].member_receipts_pump_and_push();
    }
    sim.flush(&mut links, |_| 1);
    assert_eq!(sim.status(0, &post).tick, ReceiptTick::Read);
    let other = sim.status(2, &post);
    assert_eq!(
        (other.delivered, other.read),
        (3, 3),
        "members see everyone's reads"
    );
}

#[test]
fn lost_and_duplicate_receipt_frames_recover_by_ack_and_reconnect() {
    let mut sim = Sim::new(2);
    let mut links: Vec<Link> = (1..3).map(|m| sim.connect(m)).collect();
    sim.flush(&mut links, |_| 1);
    let post = sim.post(0, "lossy");
    for n in 1..3 {
        sim.day(0, n);
    }
    // Every frame arrives twice: idempotent.
    sim.nodes[1].member_receipts_pump_and_push();
    sim.flush(&mut links, |_| 2);
    assert_eq!(sim.entries(0), 1);
    assert_eq!(sim.entries(2), 1);
    // Member 2's statement frame is lost on the way to the host.
    sim.nodes[2].member_receipts_pump_and_push();
    let mut lost = 0;
    sim.flush(&mut links, |m| match m {
        GroupMsg::Receipts { .. } if lost == 0 => {
            lost += 1;
            0
        }
        _ => 1,
    });
    assert_eq!(lost, 1);
    assert_eq!(sim.status(0, &post).delivered, 1);
    // The member's outbox still owes it (no host ack): a reconnect's hello catches up.
    let l = links.remove(1);
    sim.disconnect(l);
    links.push(sim.connect(2));
    sim.flush(&mut links, |_| 1);
    let s = sim.status(0, &post);
    assert_eq!((s.tick, s.delivered), (ReceiptTick::Delivered, 2));
    assert_eq!(sim.entries(1), 2, "relayed on to the other member");
    // Acknowledged: nothing is owed any more.
    let group = crate::receipts::Conversation::Group(sim.gid.clone());
    for n in 1..3 {
        assert!(
            sim.nodes[n]
                .receipts
                .outbox_days(&group, 8)
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn receipts_for_posts_not_yet_held_are_asked_again_after_the_posts_arrive() {
    let mut sim = Sim::new(2);
    let mut links: Vec<Link> = (1..3).map(|m| sim.connect(m)).collect();
    sim.flush(&mut links, |_| 1);
    let post = sim.post(0, "late post");
    sim.day(0, 1);
    sim.nodes[1].member_receipts_pump_and_push();
    sim.flush(&mut links, |_| 1);
    // Member 2 got the relayed statement before the post: not stored, remembered.
    assert_eq!(sim.entries(2), 0);
    assert!(!links[1].at_member.receipt_waiting.lock().is_empty());
    sim.day(0, 2);
    sim.nodes[2].receipts_rehello(&sim.gid);
    sim.flush(&mut links, |_| 1);
    assert!(sim.entries(2) >= 1);
    assert_eq!(sim.status(2, &post).delivered, 1);
}

#[test]
fn read_receipts_off_issues_delivered_only_and_never_backdates() {
    let mut sim = Sim::new(2);
    let first = sim.post(0, "read while off");
    for n in 1..3 {
        sim.day(0, n);
    }
    // Member 1 has read receipts off: delivered still goes out, the read stays local.
    sim.nodes[1].mark_read(&sim.gid, false).unwrap();
    assert_eq!(sim.pump(1), 1, "only the delivered statement");
    sim.receipts(1, 0).unwrap();
    // It still sees other members' reads (no reciprocity).
    assert_eq!(sim.read(2), 2, "member 2 delivers and reads");
    sim.receipts(2, 0).unwrap();
    sim.receipts(0, 1).unwrap();
    let theirs = sim.status(1, &first);
    assert_eq!((theirs.delivered, theirs.read), (2, 1));
    let s = sim.status(0, &first);
    assert_eq!(
        (s.tick, s.delivered, s.read),
        (ReceiptTick::Delivered, 2, 1)
    );
    // Turned back on: the post read while off is never confirmed; only new reads count.
    let second = sim.post(0, "read after on");
    sim.day(0, 1);
    sim.nodes[1].mark_read(&sim.gid, true).unwrap();
    assert_eq!(sim.pump(1), 2, "delivered and read of the new post only");
    sim.receipts(1, 0).unwrap();
    assert_eq!(sim.status(0, &first).read, 1);
    let row = sim.status(0, &second);
    assert!(
        row.members
            .iter()
            .any(|m| m.receipts.did == sim.nodes[1].did && m.receipts.level == ReceiptLevel::Read)
    );
}

#[test]
fn reading_an_old_compacted_day_still_issues_read_receipts() {
    let sim = Sim::new(1);
    // A post ten days back (its day doc closed into one snapshot row on the reader).
    let old_day = "2026-10-06";
    let at = day_start(old_day).unwrap() + 3_600_000;
    let id = sim.nodes[0]
        .write_at(
            &sim.gid,
            Body::Post {
                text: "from last week".into(),
                reply_to: None,
            },
            at,
        )
        .unwrap()
        .0
        .id;
    let theirs = sim.nodes[1].doc_vv(&sim.gid, &DocId::Day(old_day.into()));
    let update = {
        let core = &sim.nodes[0];
        let mut groups = core.groups.lock();
        let Live { state, shards, .. } = groups.get_mut(&sim.gid).unwrap();
        shards
            .get(&core.store, &sim.gid, core.wid, old_day, state)
            .unwrap()
            .doc
            .export_since(&theirs)
            .unwrap()
    };
    sim.nodes[1]
        .apply(&sim.gid, &DocId::Day(old_day.into()), &update, |_, _, _| {
            Ok(())
        })
        .unwrap();
    {
        let core = &sim.nodes[1];
        let mut groups = core.groups.lock();
        let live = groups.get_mut(&sim.gid).unwrap();
        let snap = {
            let Live { state, shards, .. } = &mut *live;
            shards
                .get(&core.store, &sim.gid, core.wid, old_day, state)
                .unwrap()
                .doc
                .snapshot()
        };
        let mut sm = core.store.shard_meta(&sim.gid, old_day).unwrap().unwrap();
        core.store
            .compact_shard(&sim.gid, old_day, &mut sm, &snap)
            .unwrap();
        assert!(sm.closed);
        live.shards = Default::default();
    }
    assert_eq!(sim.pump(1), 1, "delivered");
    sim.nodes[1].mark_read(&sim.gid, true).unwrap();
    assert_eq!(sim.pump(1), 1, "the old post's read receipt");
    let frame = sim.nodes[1]
        .member_receipts_export(&sim.gid, old_day, &VersionVector::default(), LIMITS)
        .unwrap()
        .unwrap();
    assert_eq!(
        sim.nodes[0]
            .member_receipts_import(&sim.gid, old_day, &frame.update)
            .unwrap()
            .len(),
        2
    );
    let s = sim.nodes[0]
        .member_receipt_status(&sim.gid, old_day, &id)
        .unwrap();
    assert_eq!((s.tick, s.read), (ReceiptTick::Read, 1));
}
