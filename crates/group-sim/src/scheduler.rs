//! Five-node network scheduler; addresses/succession verification remain mock adapters.
//! In-memory replicas, bounded queues, original signed messages. No disk or real sockets.
use super::*;

const NODES: usize = 5;
const QUEUE_LIMIT: usize = 64;
const BINDING_LIFETIME: u64 = 30;

struct Node {
    replica: Replica,
    nomination: Nomination,
    path: Vec<BindingEvidence>,
    known: BTreeMap<usize, HostDirectory>,
    awake: bool,
}
#[derive(Clone)]
enum Payload {
    Message(Envelope),
    // This stands for evidence returned by the rotation verifier, not unsigned wire data.
    Address(VerifiedSuccession),
}
#[derive(Clone)]
struct Packet {
    from: usize,
    to: usize,
    endpoint: String,
    device_id: u64,
    payload: Payload,
}
#[derive(Default)]
struct Counts {
    sleeps: usize,
    wakes: usize,
    rotations: usize,
    partitions: usize,
    sends: usize,
    delivered: usize,
    failed_routes: usize,
    stale_packets: usize,
    drops: usize,
    duplicates: usize,
    controls: usize,
    queue_limits: usize,
}
struct Fleet {
    nodes: Vec<Node>,
    queue: Vec<Packet>,
    now: u64,
    partitioned: bool,
    counts: Counts,
    seed: u64,
    trace: Vec<String>,
}
impl Fleet {
    fn new(seed: u64, genesis: &Genesis, authors: &[&Author; 3]) -> Self {
        let mut nodes: Vec<Node> = (0..NODES)
            .map(|i| {
                let nomination = Nomination {
                    owner: authors[i % 3].id.did().into(),
                    initial_device: 100 + i as u64,
                    group: genesis.id(),
                };
                let initial = evidence(
                    &nomination,
                    nomination.initial_device,
                    &format!("node-{i}-generation-0"),
                    BINDING_LIFETIME,
                );
                Node {
                    replica: Replica::from_genesis(genesis.clone(), &genesis.id()).unwrap(),
                    nomination,
                    path: vec![initial],
                    known: BTreeMap::new(),
                    awake: true,
                }
            })
            .collect();
        // Explicit trusted genesis-era bootstrap fixtures, not automatic future discovery.
        for from in 0..NODES {
            for to in 0..NODES {
                let mut directory = HostDirectory::default();
                assert!(directory.install(
                    &nodes[to].nomination,
                    &MockRotation(VerifiedSuccession {
                        path: nodes[to].path.clone()
                    })
                ));
                nodes[from].known.insert(to, directory);
            }
        }
        Self {
            nodes,
            queue: vec![],
            now: 0,
            partitioned: false,
            counts: Counts::default(),
            seed,
            trace: vec![],
        }
    }
    fn log(&mut self, event: impl Into<String>) {
        self.trace.push(format!("t={} {}", self.now, event.into()));
        if self.trace.len() > 80 {
            self.trace.remove(0);
        }
    }
    fn assert(&self, condition: bool, message: &str) {
        assert!(
            condition,
            "seed={} {message}\n{}",
            self.seed,
            self.trace.join("\n")
        );
    }
    fn linked(&self, from: usize, to: usize) -> bool {
        self.nodes[from].awake && self.nodes[to].awake && (!self.partitioned || from % 2 == to % 2)
    }
    fn send(&mut self, from: usize, to: usize, payload: Payload) -> bool {
        self.counts.sends += 1;
        if !self.linked(from, to) {
            self.counts.failed_routes += 1;
            return false;
        }
        let binding = self.nodes[from].known[&to].active(self.now).cloned();
        let Some(binding) = binding else {
            self.counts.failed_routes += 1;
            return false;
        };
        if !self.nodes[to].path.iter().any(|r| {
            r.endpoint == binding.endpoint
                && r.device_id == binding.device_id
                && self.now < r.retire_at
        }) {
            self.counts.failed_routes += 1;
            return false;
        }
        if self.queue.len() >= QUEUE_LIMIT {
            self.counts.queue_limits += 1;
            return false;
        }
        self.queue.push(Packet {
            from,
            to,
            endpoint: binding.endpoint,
            device_id: binding.device_id,
            payload,
        });
        self.log(format!("enqueue {from}->{to}"));
        true
    }
    fn control(&mut self, from: usize, to: usize) -> bool {
        self.send(
            from,
            to,
            Payload::Address(VerifiedSuccession {
                path: self.nodes[from].path.clone(),
            }),
        )
    }
    fn reconcile(&mut self, from: usize, to: usize) {
        // Original envelopes only, bounded by the transport queue. No direct state injection.
        let updates: Vec<Envelope> = self.nodes[from]
            .replica
            .accepted
            .values()
            .filter(|e| !self.nodes[to].replica.accepted.contains_key(&e.id()))
            .cloned()
            .collect();
        for update in updates {
            self.send(from, to, Payload::Message(update));
        }
    }
    fn deliver(&mut self, index: usize) {
        if self.queue.is_empty() {
            return;
        }
        let p = self.queue.remove(index % self.queue.len());
        if !self.linked(p.from, p.to)
            || !self.nodes[p.to].path.iter().any(|r| {
                r.endpoint == p.endpoint && r.device_id == p.device_id && self.now < r.retire_at
            })
        {
            self.counts.stale_packets += 1;
            self.log("unreachable queued frame");
            return;
        }
        match p.payload {
            Payload::Message(e) => {
                let outcome = self.nodes[p.to].replica.receive(e);
                self.assert(
                    !matches!(outcome, Outcome::Invalid | Outcome::Limit),
                    "valid scheduled message failed",
                );
                self.counts.delivered += 1;
            }
            Payload::Address(path) => {
                // Mock verifier oracle: complete strict extensions only. Actual validation,
                // forks, signed expiry and checkpoints are the rotation agent's contract.
                let nomination = self.nodes[p.from].nomination.clone();
                let directory = self.nodes[p.to].known.get_mut(&p.from).unwrap();
                if path.path.len() >= directory.history.len() {
                    assert!(directory.install(&nomination, &MockRotation(path)));
                }
                self.counts.controls += 1;
            }
        }
        self.log(format!("deliver {}->{}", p.from, p.to));
    }
    fn flush(&mut self) {
        while !self.queue.is_empty() {
            self.deliver(0);
        }
    }
    fn rotate(&mut self, node: usize) {
        let n = &mut self.nodes[node];
        let generation = n.path.len() as u64;
        let device = 100 + node as u64 + 1000 * generation;
        n.path.push(evidence(
            &n.nomination,
            device,
            &format!("node-{node}-generation-{generation}"),
            self.now + BINDING_LIFETIME,
        ));
        self.counts.rotations += 1;
        self.log(format!("rotate node {node}"));
    }
    fn awake(&mut self, node: usize, value: bool) {
        self.nodes[node].awake = value;
        if value {
            self.counts.wakes += 1;
        } else {
            self.counts.sleeps += 1;
        }
        self.log(format!("awake {node}={value}"));
    }
    fn partition(&mut self, value: bool) {
        self.partitioned = value;
        self.counts.partitions += 1;
        self.log(format!("partition={value}"));
    }
    // Recovery is explicit: someone has to be able to deliver the fresh binding using a
    // still-known destination. This method does NOT copy paths into every directory.
    fn distribute_rotation(&mut self, owner: usize) {
        for to in 0..NODES {
            if to != owner {
                self.control(owner, to);
            }
        }
        self.flush();
    }
    fn healthy_reconcile(&mut self) {
        for from in 0..NODES {
            for to in 0..NODES {
                if from != to {
                    self.reconcile(from, to);
                    self.flush();
                }
            }
        }
    }
}

#[test]
fn five_nodes_seeded_sleep_partition_rotation_loss_and_retry_converge() {
    for seed in 1..=10u64 {
        let (g, mut a, mut b, mut c) = signed_fixture();
        let mut f = Fleet::new(seed, &g, &[&a, &b, &c]);
        let mut envelopes = vec![];
        for round in 0..2 {
            if round == 1 {
                // Establish real cross-author knowledge via scheduled message delivery.
                f.reconcile(0, 1);
                f.flush();
                f.reconcile(1, 2);
                f.flush();
                b.observe(&envelopes[0]);
                c.observe(&envelopes[0]);
                c.observe(&envelopes[1]);
            }
            for (node, author) in [&mut a, &mut b, &mut c].into_iter().enumerate() {
                let e = author.post(&format!("seed-{seed}-node-{node}-round-{round}"));
                assert_eq!(f.nodes[node].replica.receive(e.clone()), Outcome::Accepted);
                envelopes.push(e);
            }
        }
        // Mandatory faults before randomized scheduling, guaranteeing coverage per seed.
        f.reconcile(0, 3);
        let p = f.queue[0].clone();
        f.queue.push(p);
        f.counts.duplicates += 1;
        f.queue.remove(0);
        f.counts.drops += 1;
        f.awake(3, false);
        f.flush();
        f.awake(3, true);
        f.partition(true);
        f.reconcile(0, 3);
        f.partition(false);
        f.now = 4;
        f.rotate(3);
        f.rotate(4);
        f.distribute_rotation(3);
        f.distribute_rotation(4);
        let mut rng = seed;
        for step in 0..100 {
            f.now = 4 + step / 10;
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
            let from = ((rng >> 32) as usize) % NODES;
            let to = ((rng >> 16) as usize) % NODES;
            match rng % 8 {
                0 => f.awake(from, false),
                1 => f.awake(from, true),
                2 => f.partition(!f.partitioned),
                3 if from != to => f.reconcile(from, to),
                4 => f.deliver(to),
                5 if !f.queue.is_empty() && f.queue.len() < QUEUE_LIMIT => {
                    let p = f.queue[to % f.queue.len()].clone();
                    f.queue.push(p);
                    f.counts.duplicates += 1;
                }
                6 if !f.queue.is_empty() => {
                    f.queue.remove(to % f.queue.len());
                    f.counts.drops += 1;
                }
                _ if from != to => {
                    f.control(from, to);
                }
                _ => {}
            }
            // Independent host rotations occur while known overlap still permits updates.
            // Controls sent while sleeping/partitioned can be missed; recovery below retries.
            if step == 35 || step == 70 {
                let host = if step == 35 { 3 } else { 4 };
                f.rotate(host);
                f.distribute_rotation(host);
            }
            f.assert(f.queue.len() <= QUEUE_LIMIT, "queue overflow");
        }
        // Recover within the initial address overlap. Do not resurrect retired routes.
        for node in 0..NODES {
            f.awake(node, true);
        }
        f.partition(false);
        f.distribute_rotation(3);
        f.distribute_rotation(4);
        f.healthy_reconcile();
        for node in 0..NODES {
            f.assert(
                f.nodes[node].replica.accepted.len() == envelopes.len(),
                "missing envelopes after repair",
            );
            f.nodes[0].replica.assert_same(&f.nodes[node].replica);
            f.assert(
                f.nodes[node].replica.fanout.len() == envelopes.len(),
                "duplicate fanout",
            );
            for e in &envelopes {
                f.assert(
                    f.nodes[node].replica.accepted[&e.id()].signature == e.signature,
                    "altered author proof",
                );
            }
        }
        f.assert(
            f.counts.rotations >= 4
                && f.counts.sleeps > 0
                && f.counts.wakes > 0
                && f.counts.partitions > 0
                && f.counts.drops > 0
                && f.counts.duplicates > 0
                && f.counts.failed_routes > 0
                && f.counts.controls > 0
                && f.counts.stale_packets > 0,
            "required fault coverage missing",
        );
    }
}

#[test]
fn mutual_retirement_drops_queued_message_and_retains_local_write_without_discovery() {
    let (g, mut a, b, c) = signed_fixture();
    let mut f = Fleet::new(101, &g, &[&a, &b, &c]);
    let e = a.post("retry after retirement");
    assert_eq!(f.nodes[0].replica.receive(e.clone()), Outcome::Accepted);
    f.reconcile(0, 3);
    f.now = 25;
    f.rotate(3);
    f.now = BINDING_LIFETIME;
    f.flush();
    assert!(f.nodes[3].replica.accepted.is_empty());
    assert!(!f.send(0, 3, Payload::Message(e.clone())));
    assert_eq!(f.counts.stale_packets, 1);
    // H1 can push its own fresh address only if A's known endpoint remains usable.
    assert!(!f.control(3, 0));
    assert!(f.nodes[0].known[&3].active(f.now).is_none());
    assert_eq!(f.nodes[0].replica.accepted.len(), 1);
    assert!(f.nodes[3].replica.accepted.is_empty());
}

#[test]
fn replacement_host_can_push_update_to_still_reachable_member_then_receive_retry() {
    let (g, mut a, b, c) = signed_fixture();
    let mut f = Fleet::new(102, &g, &[&a, &b, &c]);
    let e = a.post("survives missed host update");
    assert_eq!(f.nodes[0].replica.receive(e.clone()), Outcome::Accepted);
    f.now = 20;
    // A publishes its replacement while overlap exists, before H1's retirement.
    f.rotate(0);
    f.distribute_rotation(0);
    f.rotate(3);
    f.now = 31;
    assert!(!f.send(0, 3, Payload::Message(e.clone())));
    assert!(f.control(3, 0));
    f.flush();
    assert!(f.send(0, 3, Payload::Message(e.clone())));
    f.flush();
    assert_eq!(f.nodes[3].replica.accepted.len(), 1);
    assert_eq!(f.nodes[3].replica.accepted[&e.id()].signature, e.signature);
}
