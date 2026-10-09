//! Test-only direct-session queue composed with succession permits. Endpoint possession
//! uses real signatures, but sockets, atomic storage and group authorization are not modeled.
use super::*;
use std::collections::{BTreeMap, VecDeque};

#[path = "rotation_permit_lifecycle.rs"]
mod lifecycle;

type Path = (String, String, [u8; 32]); // owner, audience, historical nomination
fn path(v: &View) -> Path { (v.owner.clone(), v.audience.clone(), v.anchor) }

#[derive(Clone)]
struct Peer {
    did: String,
    online: bool,
    endpoint: [u8; 32],
    blocked: HashSet<String>,
    revoked: HashSet<[u8; 32]>,
    histories: BTreeMap<Path, View>,
    // Recipient IDs are simulator handles, not an authoritative address directory.
    known: BTreeMap<usize, [u8; 32]>,
    learned: BTreeMap<usize, Path>,
    // Persisted high-water mark; outstanding challenges are volatile and bounded.
    next_nonce: u64,
    outstanding: BTreeMap<u64, u64>,
}

impl Peer {
    fn route(&self, p: &Path, now: u64) -> Option<[u8; 32]> {
        let v = self.histories.get(p)?;
        (v.generation > 0 && now < v.expires_at && !self.blocked.contains(&v.owner)
            && !self.revoked.contains(&v.anchor) && !self.revoked.contains(&v.head))
            .then_some(v.endpoint)
    }
}

#[derive(Clone)]
struct Packet {
    from: usize,
    to: usize,
    destination: [u8; 32],
    path: Path,
    permits: Vec<Permit>,
    session_nonce: u64,
    possession: [u8; 64],
}

fn challenge(to: usize, nonce: u64, binding: &Permit) -> Vec<u8> {
    // Test-only challenge includes target, nonce and the signed update's ID.
    [b"test-only/rotation-control\0".as_slice(), &(to as u64).to_be_bytes(),
     &nonce.to_be_bytes(), &binding.id()].concat()
}

struct Network {
    now: u64,
    peers: Vec<Peer>,
    queue: VecDeque<Packet>,

}

impl Network {
    fn new() -> Self {
        Self { now: 2, peers: Vec::new(), queue: VecDeque::new() }
    }
    fn add(&mut self, id: &Identity, seeds: &[View]) -> usize {
        let key = SigningKey::from_bytes(&proto::new_device_secret());
        self.peers.push(Peer { did: id.did().into(), online: true,
            endpoint: key.verifying_key().to_bytes(), blocked: HashSet::new(),
            revoked: HashSet::new(), histories: seeds.iter().map(|v| (path(v), v.clone())).collect(),
            known: BTreeMap::new(), learned: BTreeMap::new(),
            next_nonce: 0, outstanding: BTreeMap::new() });
        self.peers.len() - 1
    }
    fn bootstrap(&mut self) {
        // Explicit fixture-only initial address exchange. Never called after rotations.
        let addresses: Vec<_> = self.peers.iter().map(|p| p.endpoint).collect();
        for p in &mut self.peers {
            p.known = addresses.iter().enumerate().map(|(i, ep)| (i, *ep)).collect();
        }
    }
    fn block(&mut self, peer: usize, did: &str) {
        self.peers[peer].blocked.insert(did.into());
        let recipients: HashSet<_> = self.peers.iter().enumerate()
            .filter(|(_, p)| p.did == did).map(|(i, _)| i).collect();
        self.queue.retain(|p| !(p.from == peer && recipients.contains(&p.to)));
    }
    fn send(&mut self, from: usize, to: usize, p: &Path, permits: &[Permit], key: &SigningKey) -> Result<(), &'static str> {
        if self.peers[from].blocked.contains(&self.peers[to].did) { return Err("blocked"); }
        if permits.is_empty() || permits.len() > PAGE || self.queue.len() >= 32 { return Err("bounded"); }
        let last = permits.last().unwrap();
        let destination = if let Some(learned) = self.peers[from].learned.get(&to) {
            self.peers[from].route(learned, self.now).ok_or("expired_or_denied_route")?
        } else {
            *self.peers[from].known.get(&to).ok_or("unknown_address")?
        };
        // Models receiver-issued challenge exchange before sending a signed update.
        // Counter allocation is assumed durable before issuance; real persistence is pending.
        let receiver = &mut self.peers[to];
        if !receiver.online { return Err("offline"); }
        receiver.outstanding.retain(|_, deadline| self.now < *deadline);
        if receiver.outstanding.len() >= 8 { return Err("challenge_capacity"); }
        receiver.next_nonce = receiver.next_nonce.checked_add(1).ok_or("nonce_exhausted")?;
        let nonce = receiver.next_nonce;
        receiver.outstanding.insert(nonce, self.now + 5);
        self.queue.push_back(Packet { from, to, destination, path: p.clone(), permits: permits.to_vec(),
            session_nonce: nonce,
            possession: key.sign(&challenge(to, nonce, last)).to_bytes() });
        Ok(())
    }
    fn deliver(&mut self) -> Result<(), &'static str> {
        let packet = self.queue.pop_front().ok_or("empty")?;
        let last = packet.permits.last().ok_or("empty")?;
        let endpoint = last.claims.endpoint;
        if !self.peers[packet.from].online || !self.peers[packet.to].online { return Err("offline"); }
        if self.peers[packet.to].endpoint != packet.destination || self.peers[packet.from].endpoint != endpoint {
            return Err("retired_address");
        }
        if self.peers[packet.from].blocked.contains(&self.peers[packet.to].did) { return Err("blocked"); }
        if !identity::verify(&endpoint, &challenge(packet.to, packet.session_nonce, last), &packet.possession) {
            return Err("possession");
        }
        let receiver = &mut self.peers[packet.to];
        let deadline = receiver.outstanding.remove(&packet.session_nonce).ok_or("session_replay")?;
        if self.now >= deadline { return Err("challenge_expired"); }
        let before = receiver.histories.get(&packet.path).ok_or("unknown_path")?;
        let mut candidate = before.clone();
        candidate.page(&packet.permits, self.now, &receiver.revoked)?;
        if receiver.blocked.contains(&candidate.owner) { return Err("blocked"); }
        // No sibling path may be overwritten by a colliding current device ID.
        if receiver.histories.iter().any(|(p, v)| p != &packet.path
            && v.owner == candidate.owner && v.audience == candidate.audience
            && v.current_id == candidate.current_id) { return Err("sibling_collision"); }
        let current = self.now < candidate.expires_at;
        receiver.learned.insert(packet.from, packet.path.clone());
        receiver.histories.insert(packet.path, candidate);
        // Expired history invalidates even a cached bootstrap address; never fall back
        // to an older route after accepting a newer binding.
        if current { receiver.known.insert(packet.from, endpoint); }
        else { receiver.known.remove(&packet.from); }
        Ok(())
    }
}

#[test]
fn private_rotation_updates_multiple_peers_not_siblings_or_blocked_contacts() {
    let (alice, _) = identity::generate();
    let (bob, _) = identity::generate();
    let (carol, _) = identity::generate();
    let phone = View::initial(&alice, 1);
    let laptop = View::initial(&alice, 2);
    let mut n = Network::new();
    let seeds = [phone.clone(), laptop.clone()];
    let a = n.add(&alice, &seeds);
    let a2 = n.add(&alice, &seeds);
    let b = n.add(&bob, &seeds);
    let b2 = n.add(&bob, &seeds);
    let c = n.add(&carol, &seeds);
    n.bootstrap();
    let old = n.peers[b].known[&a];
    let (permit, key) = next(&alice, &phone, 3, 1, 10);
    n.peers[a].endpoint = permit.claims.endpoint;
    n.block(a, carol.did());
    assert_eq!(n.send(a, c, &path(&phone), std::slice::from_ref(&permit), &key), Err("blocked"));
    for target in [a2, b, b2] {
        n.send(a, target, &path(&phone), std::slice::from_ref(&permit), &key).unwrap();
    }
    assert_eq!(n.peers[b].known[&a], old); // no magic address propagation
    while !n.queue.is_empty() { n.deliver().unwrap(); }
    for target in [a2, b, b2] {
        assert_eq!(n.peers[target].route(&path(&phone), n.now), Some(permit.claims.endpoint));
        assert_eq!(n.peers[target].histories[&path(&laptop)], laptop);
        assert_eq!(n.peers[target].histories[&path(&phone)].current_id, [3; 32]);
    }
    assert_eq!(n.peers[c].known[&a], old);
    n.now = 10;
    assert_eq!(n.peers[b].route(&path(&phone), n.now), None);
    assert_eq!(n.peers[b].histories[&path(&phone)].generation, 1);
    let bob_initial = View::initial(&bob, 1);
    let (bob_update, bob_key) = next(&bob, &bob_initial, 2, 2, 20);
    assert_eq!(n.send(b, a, &path(&bob_initial), &[bob_update], &bob_key), Err("expired_or_denied_route"));
}

#[test]
fn expired_history_never_installs_a_new_dial_target() {
    let (alice, _) = identity::generate();
    let (bob, _) = identity::generate();
    let initial = View::initial(&alice, 1);
    let mut n = Network::new();
    let a = n.add(&alice, &[]);
    let b = n.add(&bob, std::slice::from_ref(&initial));
    n.bootstrap();
    let (p, key) = next(&alice, &initial, 2, 1, 2);
    n.peers[a].endpoint = p.claims.endpoint;
    n.send(a, b, &path(&initial), &[p], &key).unwrap();
    n.deliver().unwrap();
    assert!(!n.peers[b].known.contains_key(&a));
    assert_eq!(n.peers[b].route(&path(&initial), 2), None);
    assert_eq!(n.peers[b].histories[&path(&initial)].generation, 1);
}

#[test]
fn forged_colliding_and_reordered_updates_do_not_corrupt_maps() {
    let (alice, _) = identity::generate();
    let (bob, _) = identity::generate();
    let initial = View::initial(&alice, 1);
    let sibling = View::initial(&alice, 2);
    let mut n = Network::new();
    let a = n.add(&alice, &[]);
    let b = n.add(&bob, &[initial.clone(), sibling]);
    n.bootstrap();
    let before = n.peers[b].histories.clone();
    let (colliding, key) = next(&alice, &initial, 2, 1, 20);
    n.peers[a].endpoint = colliding.claims.endpoint;
    n.send(a, b, &path(&initial), &[colliding], &key).unwrap();
    assert_eq!(n.deliver(), Err("sibling_collision"));
    assert_eq!(n.peers[b].histories, before);
    let (first, key) = next(&alice, &initial, 3, 1, 20);
    n.peers[a].endpoint = first.claims.endpoint;
    let mut bad = first.clone();
    bad.signature[0] ^= 1;
    n.send(a, b, &path(&initial), &[bad], &key).unwrap();
    assert_eq!(n.deliver(), Err("owner_signature"));
    assert_eq!(n.peers[b].histories, before);
    let mut after_first = initial.clone();
    after_first.page(std::slice::from_ref(&first), 2, &HashSet::new()).unwrap();
    let (second, key2) = next(&alice, &after_first, 4, 2, 20);
    n.peers[a].endpoint = second.claims.endpoint;
    n.send(a, b, &path(&initial), std::slice::from_ref(&second), &key2).unwrap();
    assert_eq!(n.deliver(), Err("gap"));
    assert_eq!(n.peers[b].histories, before);
    n.send(a, b, &path(&initial), &[first, second.clone()], &key2).unwrap();
    n.deliver().unwrap();
    let good = n.peers[b].histories.clone();
    n.send(a, b, &path(&initial), &[second], &key2).unwrap();
    let duplicate = n.queue.front().unwrap().clone();
    n.deliver().unwrap();
    n.queue.push_back(duplicate);
    assert_eq!(n.deliver(), Err("session_replay"));
    assert_eq!(n.peers[b].histories, good);
}

#[test]
fn mutual_retirement_cannot_use_a_global_current_address_oracle() {
    let (alice, _) = identity::generate();
    let (bob, _) = identity::generate();
    let av = View::initial(&alice, 1);
    let bv = View::initial(&bob, 1);
    let mut n = Network::new();
    let a = n.add(&alice, std::slice::from_ref(&bv));
    let b = n.add(&bob, std::slice::from_ref(&av));
    n.bootstrap();
    let (ap, ak) = next(&alice, &av, 2, 1, 20);
    let (bp, bk) = next(&bob, &bv, 2, 1, 20);
    n.peers[a].endpoint = ap.claims.endpoint;
    n.peers[b].endpoint = bp.claims.endpoint;
    n.send(a, b, &path(&av), &[ap], &ak).unwrap();
    n.send(b, a, &path(&bv), &[bp], &bk).unwrap();
    assert_eq!(n.deliver(), Err("retired_address"));
    assert_eq!(n.deliver(), Err("retired_address"));
}

#[test]
fn group_history_verifies_without_private_records_or_endpoint_disclosure() {
    let (alice, _) = identity::generate();
    let private = View::initial(&alice, 1);
    let (private_permit, _) = next(&alice, &private, 2, 1, 20);
    let mut group = View::initial(&alice, 50);
    group.audience = "group:g1".into();
    let initial_group = group.clone();
    let (first, _) = next(&alice, &group, 51, 1, 20);
    group.page(std::slice::from_ref(&first), 2, &HashSet::new()).unwrap();
    let (second, _) = next(&alice, &group, 52, 2, 20);
    let export = [first, second];
    // Every field is inspected via a serialized claims tree, not just the leaf.
    let wire = serde_json::to_string(&export.iter().map(|p| &p.claims).collect::<Vec<_>>()).unwrap();
    for secret_address in [private_permit.claims.endpoint, private_permit.id(), private.anchor] {
        assert!(!wire.contains(&serde_json::to_string(&secret_address).unwrap()));
    }
    let mut recipient = initial_group;
    recipient.page(&export, 3, &HashSet::new()).unwrap();
    assert_eq!(recipient.generation, 2);
    // A group-signed child spliced to a private predecessor still fails.
    let mut splice = export[1].claims.clone();
    splice.predecessor = private_permit.id();
    let mut at_first = group.clone();
    assert_eq!(at_first.page(&[Permit::issue(&alice, splice)], 3, &HashSet::new()), Err("predecessor"));
    assert_eq!(at_first, group);
}
