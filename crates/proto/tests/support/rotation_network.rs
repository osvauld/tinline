//! Signed queue and crash-boundary model. Atomic durable snapshots are an assumption,
//! not a test of filesystem/vault durability. No live QUIC or discovery simulation.
use super::*;
use std::collections::{BTreeMap, HashSet, VecDeque};

type Scope = (String, u64);
const PAGE_LIMIT: usize = 4;

#[derive(Default)]
struct Peer {
    directory: BTreeMap<Scope, Receiver>,
    blocked: HashSet<String>,
    online: bool,
}

#[derive(Clone)]
struct Packet {
    to: usize,
    owner: String,
    record: Signed,
}

#[derive(Default)]
struct Network {
    peers: Vec<Peer>,
    queue: VecDeque<Packet>,
}

impl Network {
    fn peer(&mut self, bindings: &[(&Identity, u64)]) -> usize {
        let directory = bindings.iter().map(|(id, slot)| {
            ((id.did().to_owned(), *slot), Receiver::new(id, *slot))
        }).collect();
        self.peers.push(Peer { directory, online: true, ..Default::default() });
        self.peers.len() - 1
    }

    fn page(&mut self, to: usize, owner: &Identity, records: &[Signed]) -> Result<(), &'static str> {
        if records.len() > PAGE_LIMIT { return Err("page_limit"); }
        if self.queue.len() + records.len() > 32 { return Err("queue_limit"); }
        for record in records {
            self.queue.push_back(Packet { to, owner: owner.did().to_owned(), record: record.clone() });
        }
        Ok(())
    }

    fn deliver(&mut self) -> Result<(), &'static str> {
        let packet = self.queue.pop_front().ok_or("empty")?;
        let peer = &mut self.peers[packet.to];
        if !peer.online { return Err("offline"); }
        // The claim selects a stored expected key; it never supplies authority.
        let scope = (packet.owner.clone(), packet.record.record.slot);
        let receiver = peer.directory.get_mut(&scope).ok_or("unknown_slot")?;
        // Authenticate even blocked claims; no mutation before policy/verification.
        let mut candidate = receiver.clone();
        candidate.accept(&packet.record)?;
        if peer.blocked.contains(&packet.owner) { return Err("blocked"); }
        *receiver = candidate;
        Ok(())
    }
}

#[derive(Clone)]
struct DurableRotation {
    // Real transport secret paired atomically with its signed binding in this model.
    secret: [u8; 32],
    binding: Signed,
}

impl DurableRotation {
    fn initial(owner: &Identity, slot: u64) -> Self {
        let secret = proto::new_device_secret();
        let binding = Signed::new(owner, Record {
            slot, audience: "private".into(), generation: 0, previous: None,
            endpoint: proto::device_public(&secret),
        });
        Self { secret, binding }
    }

    fn prepare(&self, owner: &Identity) -> Self {
        let secret = proto::new_device_secret();
        let binding = Signed::new(owner, Record {
            slot: self.binding.record.slot,
            audience: self.binding.record.audience.clone(),
            generation: self.binding.record.generation.checked_add(1).unwrap(),
            previous: Some(self.binding.hash()), endpoint: proto::device_public(&secret),
        });
        Self { secret, binding }
    }

    fn announce(&self, network: &mut Network, to: usize, owner: &Identity) {
        assert_eq!(proto::device_public(&self.secret), self.binding.record.endpoint);
        network.page(to, owner, std::slice::from_ref(&self.binding)).unwrap();
    }
}

#[test]
fn signed_presence_routes_to_independent_device_directories() {
    let (alice, _) = identity::generate();
    let (bob, _) = identity::generate();
    let mut n = Network::default();
    // Alice laptop, Bob phone and Bob laptop each track Alice phone independently.
    let recipients: Vec<_> = (0..3).map(|_| n.peer(&[(&alice, 1), (&alice, 2), (&bob, 1)])).collect();
    let records = chain(&alice, 1, 3);
    for &to in &recipients { n.page(to, &alice, &records).unwrap(); }
    assert!(n.peers.iter().all(|p| p.directory.values().all(|r| r.head.is_none())));
    while !n.queue.is_empty() { n.deliver().unwrap(); }
    for peer in &n.peers {
        assert_eq!(peer.directory[&(alice.did().into(), 1)].head.unwrap().0, 2);
        assert!(peer.directory[&(alice.did().into(), 2)].head.is_none());
        assert!(peer.directory[&(bob.did().into(), 1)].head.is_none());
    }
}

#[test]
fn queued_updates_verify_signatures_and_apply_receiver_block_policy() {
    let (alice, _) = identity::generate();
    let (mallory, _) = identity::generate();
    let mut n = Network::default();
    let to = n.peer(&[(&alice, 1)]);
    let records = chain(&alice, 1, 2);
    n.page(to, &alice, &records[..1]).unwrap();
    n.deliver().unwrap();
    let before = n.peers[to].directory.clone();
    // Valid signature from the wrong account under a claimed Alice packet owner.
    let forged = Signed::new(&mallory, records[1].record.clone());
    n.page(to, &alice, &[forged]).unwrap();
    assert_eq!(n.deliver(), Err("signature"));
    n.page(to, &alice, &records[1..]).unwrap();
    n.peers[to].blocked.insert(alice.did().into());
    assert_eq!(n.deliver(), Err("blocked"));
    assert_eq!(n.peers[to].directory, before);
}

#[test]
fn offline_catchup_uses_bounded_pages_and_rejects_gap_without_mutation() {
    let (alice, _) = identity::generate();
    let records = chain(&alice, 1, 19);
    let mut n = Network::default();
    let to = n.peer(&[(&alice, 1)]);
    n.page(to, &alice, &records[..1]).unwrap();
    n.deliver().unwrap();
    n.peers[to].online = false;
    n.page(to, &alice, &records[1..2]).unwrap();
    assert_eq!(n.deliver(), Err("offline"));
    n.peers[to].online = true;
    assert_eq!(n.page(to, &alice, &records), Err("page_limit"));
    assert!(n.queue.is_empty());
    n.page(to, &alice, &records[18..]).unwrap();
    assert_eq!(n.deliver(), Err("gap"));
    for page in records[1..].chunks(PAGE_LIMIT) {
        n.page(to, &alice, page).unwrap();
        while !n.queue.is_empty() { n.deliver().unwrap(); }
    }
    assert_eq!(n.peers[to].directory[&(alice.did().into(), 1)].head.unwrap().0, 18);
    // Loss of retained history must be explicit; newest state alone is not recovery.
    let newcomer = n.peer(&[(&alice, 1)]);
    n.page(newcomer, &alice, &records[18..]).unwrap();
    assert_eq!(n.deliver(), Err("gap"));
}

#[test]
fn crash_boundaries_never_publish_a_lost_key_or_regress_committed_generation() {
    let (alice, _) = identity::generate();
    // 0=before commit, 1=after commit, 2=after enqueue, 3=after recipient delivery.
    for crash_at in 0..4 {
        let mut n = Network::default();
        let to = n.peer(&[(&alice, 1)]);
        let mut disk = DurableRotation::initial(&alice, 1);
        disk.announce(&mut n, to, &alice);
        n.deliver().unwrap();
        let prepared = disk.prepare(&alice);
        if crash_at >= 1 { disk = prepared.clone(); } // ideal atomic commit
        if crash_at >= 2 { disk.announce(&mut n, to, &alice); }
        if crash_at >= 3 { n.deliver().unwrap(); }
        // Crash discards uncommitted prepared state, not the durable snapshot.
        let restarted = disk.clone();
        assert_eq!(restarted.binding.record.generation, u64::from(crash_at >= 1));
        assert_eq!(proto::device_public(&restarted.secret), restarted.binding.record.endpoint);
        if crash_at >= 1 { assert_eq!(restarted.binding.hash(), prepared.binding.hash()); }
        restarted.announce(&mut n, to, &alice);
        while !n.queue.is_empty() { n.deliver().unwrap(); }
        assert_eq!(n.peers[to].directory[&(alice.did().into(), 1)].head.unwrap().2,
                   restarted.binding.record.endpoint);
    }
}

#[test]
fn cloned_device_forks_are_detected_when_partitioned_peers_exchange_successors() {
    let (alice, _) = identity::generate();
    let initial = DurableRotation::initial(&alice, 1);
    let fork_a = initial.prepare(&alice);
    let fork_b = initial.prepare(&alice);
    assert_ne!(fork_a.binding.hash(), fork_b.binding.hash());
    let mut n = Network::default();
    let a = n.peer(&[(&alice, 1)]);
    let b = n.peer(&[(&alice, 1)]);
    for to in [a, b] { initial.announce(&mut n, to, &alice); n.deliver().unwrap(); }
    fork_a.announce(&mut n, a, &alice);
    n.deliver().unwrap();
    fork_b.announce(&mut n, b, &alice);
    n.deliver().unwrap();
    let before_a = n.peers[a].directory.clone();
    let before_b = n.peers[b].directory.clone();
    fork_b.announce(&mut n, a, &alice);
    fork_a.announce(&mut n, b, &alice);
    assert_eq!(n.deliver(), Err("fork"));
    assert_eq!(n.deliver(), Err("fork"));
    assert_eq!(n.peers[a].directory, before_a);
    assert_eq!(n.peers[b].directory, before_b);
    // Detection is not convergence. Neither branch is silently made authoritative.
    assert_ne!(n.peers[a].directory, n.peers[b].directory);
}
