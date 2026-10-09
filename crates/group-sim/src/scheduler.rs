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
#[derive(Default, Debug)]
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
    // G-H: observed (not self-incremented) outcomes and typed route failures.
    retired_routes: usize,
    endpoint_gone: usize,
    unlinked: usize,
    duplicate_receipts: usize,
    pending_receipts: usize,
    ignored_controls: usize,
}
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum SendFailure {
    // Sleep or partition: no link right now.
    Unlinked,
    // Sender's directory has no binding live at `now` (retired); see RouteFailure.
    Route(RouteFailure),
    // Bounded transport queue is full; the payload is not retained.
    QueueFull,
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
    fn send(&mut self, from: usize, to: usize, payload: Payload) -> Result<(), SendFailure> {
        self.counts.sends += 1;
        if !self.linked(from, to) {
            self.counts.failed_routes += 1;
            self.counts.unlinked += 1;
            return Err(SendFailure::Unlinked);
        }
        let binding = self.nodes[from].known[&to].active(self.now).cloned();
        let Some(binding) = binding else {
            self.counts.failed_routes += 1;
            self.counts.retired_routes += 1;
            return Err(SendFailure::Route(RouteFailure::NoActiveBinding));
        };
        if !self.nodes[to].path.iter().any(|r| {
            r.endpoint == binding.endpoint
                && r.device_id == binding.device_id
                && self.now < r.retire_at
        }) {
            self.counts.failed_routes += 1;
            self.counts.endpoint_gone += 1;
            return Err(SendFailure::Route(RouteFailure::EndpointGone));
        }
        if self.queue.len() >= QUEUE_LIMIT {
            self.counts.queue_limits += 1;
            return Err(SendFailure::QueueFull);
        }
        self.queue.push(Packet {
            from,
            to,
            endpoint: binding.endpoint,
            device_id: binding.device_id,
            payload,
        });
        self.log(format!("enqueue {from}->{to}"));
        Ok(())
    }
    fn control(&mut self, from: usize, to: usize) -> Result<(), SendFailure> {
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
        // Causal order (as a VV/frontier exchange would send); see the G-H id-order test.
        let updates: Vec<Envelope> = self.nodes[from]
            .replica
            .accepted
            .values()
            .filter(|e| !self.nodes[to].replica.accepted.contains_key(&e.id()))
            .cloned()
            .collect();
        for update in causal_order(updates) {
            let _ = self.send(from, to, Payload::Message(update));
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
                match outcome {
                    Outcome::Duplicate => self.counts.duplicate_receipts += 1,
                    Outcome::Pending => self.counts.pending_receipts += 1,
                    _ => {}
                }
            }
            Payload::Address(path) => {
                // Mock verifier oracle: complete strict extensions only. Actual validation,
                // forks, signed expiry and checkpoints are the rotation agent's contract.
                let nomination = self.nodes[p.from].nomination.clone();
                let directory = self.nodes[p.to].known.get_mut(&p.from).unwrap();
                if path.path.len() >= directory.history.len() {
                    assert!(directory.install(&nomination, &MockRotation(path)));
                } else {
                    // Shorter (rollback/stale) path: ignored by this mock oracle.
                    self.counts.ignored_controls += 1;
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
    // Fault injectors (G-H): counters reflect operations that actually happened.
    fn duplicate(&mut self, index: usize) -> bool {
        if self.queue.is_empty() || self.queue.len() >= QUEUE_LIMIT {
            return false;
        }
        let p = self.queue[index % self.queue.len()].clone();
        self.queue.push(p);
        self.counts.duplicates += 1;
        self.log("duplicate queued frame");
        true
    }
    fn drop_packet(&mut self, index: usize) -> bool {
        if self.queue.is_empty() {
            return false;
        }
        let len = self.queue.len();
        self.queue.remove(index % len);
        self.counts.drops += 1;
        self.log("drop queued frame");
        true
    }
    // Explicit out-of-band re-bootstrap FIXTURE (like Fleet::new's genesis-era install).
    // Not discovery: it models e.g. re-scanning an invite, which the protocol must specify.
    fn rebootstrap(&mut self, node: usize) {
        for other in 0..NODES {
            for (from, to) in [(node, other), (other, node)] {
                let path = self.nodes[to].path.clone();
                let nomination = self.nodes[to].nomination.clone();
                let directory = self.nodes[from].known.get_mut(&to).unwrap();
                assert!(directory.install(&nomination, &MockRotation(VerifiedSuccession { path })));
            }
        }
        self.log(format!("rebootstrap {node}"));
    }
    fn rotate(&mut self, node: usize) {
        let n = &mut self.nodes[node];
        let generation = n.path.len() as u64;
        let device = 100 + node as u64 + 1000 * generation;
        n.path.push(successor(
            n.path.last().unwrap(),
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
                let _ = self.control(owner, to);
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

// KIND: invariant
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
        let ok = f.duplicate(0);
        f.assert(ok, "mandatory duplicate not injected");
        let ok = f.drop_packet(0);
        f.assert(ok, "mandatory drop not injected");
        f.awake(3, false);
        f.flush();
        f.awake(3, true);
        f.partition(true);
        f.reconcile(0, 3);
        f.partition(false);
        // G-H: a duplicated frame that is actually delivered twice (observed Duplicate).
        f.reconcile(0, 4);
        let ok = f.duplicate(0);
        f.assert(ok, "mandatory delivered duplicate not injected");
        f.flush();
        f.now = 4;
        f.rotate(3);
        f.rotate(4);
        f.distribute_rotation(3);
        f.distribute_rotation(4);
        let mut rng = seed;
        // Scope note (G-H): time stays <= 13 here, below BINDING_LIFETIME, so no binding
        // retires in this test. Retirement under load is the long-horizon test below.
        for step in 0..100 {
            f.now = 4 + step / 10;
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
            let from = ((rng >> 32) as usize) % NODES;
            let to = ((rng >> 16) as usize) % NODES;
            // G-H: high bits. `rng % 8` (low bits of this power-of-two LCG) cycles with
            // period 8, so every seed had the same event-kind sequence.
            match (rng >> 40) % 8 {
                0 => f.awake(from, false),
                1 => f.awake(from, true),
                2 => f.partition(!f.partitioned),
                3 if from != to => f.reconcile(from, to),
                4 => f.deliver(to),
                5 if !f.queue.is_empty() && f.queue.len() < QUEUE_LIMIT => {
                    f.duplicate(to);
                }
                6 if !f.queue.is_empty() => {
                    f.drop_packet(to);
                }
                _ if from != to => {
                    let _ = f.control(from, to);
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
                && f.counts.stale_packets > 0
                && f.counts.duplicate_receipts > 0,
            "required fault coverage missing",
        );
    }
}

// KIND: limitation
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
    assert_eq!(
        f.send(0, 3, Payload::Message(e.clone())),
        Err(SendFailure::Route(RouteFailure::NoActiveBinding))
    );
    assert_eq!(f.counts.stale_packets, 1);
    // H1 can push its own fresh address only if A's known endpoint remains usable.
    assert_eq!(
        f.control(3, 0),
        Err(SendFailure::Route(RouteFailure::NoActiveBinding))
    );
    assert!(f.nodes[0].known[&3].active(f.now).is_none());
    assert_eq!(f.nodes[0].replica.accepted.len(), 1);
    assert!(f.nodes[3].replica.accepted.is_empty());
}

// KIND: invariant
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
    assert_eq!(
        f.send(0, 3, Payload::Message(e.clone())),
        Err(SendFailure::Route(RouteFailure::NoActiveBinding))
    );
    assert_eq!(f.control(3, 0), Ok(()));
    f.flush();
    assert_eq!(f.send(0, 3, Payload::Message(e.clone())), Ok(()));
    f.flush();
    assert_eq!(f.nodes[3].replica.accepted.len(), 1);
    assert_eq!(f.nodes[3].replica.accepted[&e.id()].signature, e.signature);
}

// KIND: invariant
#[test]
fn transport_queue_limit_is_reached_refuses_sends_and_later_retry_converges() {
    let (g, mut a, b, c) = signed_fixture();
    let mut f = Fleet::new(103, &g, &[&a, &b, &c]);
    let extra = 6;
    for i in 0..QUEUE_LIMIT + extra {
        let e = a.post(&format!("bulk-{i}"));
        assert_eq!(f.nodes[0].replica.receive(e), Outcome::Accepted);
    }
    f.reconcile(0, 3);
    assert_eq!(f.queue.len(), QUEUE_LIMIT);
    assert_eq!(f.counts.queue_limits, extra);
    let probe = f.nodes[0].replica.accepted.values().next().unwrap().clone();
    assert_eq!(
        f.send(0, 3, Payload::Message(probe)),
        Err(SendFailure::QueueFull)
    );
    assert_eq!(f.queue.len(), QUEUE_LIMIT);
    f.flush();
    // Causal send order: the queued prefix is accepted without Pending or Limit.
    assert_eq!(f.nodes[3].replica.accepted.len(), QUEUE_LIMIT);
    assert!(f.nodes[3].replica.pending.is_empty());
    assert_eq!(f.counts.pending_receipts, 0);
    // Refused frames are not retained by the transport; the next reconcile resends them.
    f.reconcile(0, 3);
    assert_eq!(f.queue.len(), extra);
    f.flush();
    f.nodes[0].replica.assert_same(&f.nodes[3].replica);
}

// Fixture: author learns everything its own node accepted, in causal order.
fn catch_up(author: &mut Author, own: &BTreeSet<String>, replica: &Replica) {
    let unseen: Vec<Envelope> = replica
        .accepted
        .values()
        .filter(|e| !own.contains(&e.id()) && !author.observed.contains(&e.id()))
        .cloned()
        .collect();
    for e in causal_order(unseen) {
        author.observe(&e);
    }
}

// KIND: invariant
#[test]
fn seeded_schedules_past_binding_lifetime_retire_routes_and_isolate_long_sleeper() {
    // Runs to 4x BINDING_LIFETIME with routine staggered rotation (period 10 < lifetime 30).
    // Host 4 is offline for 40 ticks (> lifetime), so every binding it knew and every
    // binding of it that others knew retires: mutual retirement without discovery.
    const END: u64 = 4 * BINDING_LIFETIME;
    const OFFLINE: std::ops::Range<u64> = 40..80;
    let sleeper = 4;
    for seed in 1..=10u64 {
        let (g, mut a, mut b, mut c) = signed_fixture();
        let mut f = Fleet::new(seed, &g, &[&a, &b, &c]);
        let mut authors = [&mut a, &mut b, &mut c];
        let mut own: [BTreeSet<String>; 3] = Default::default();
        let mut envelopes: Vec<(u64, Envelope)> = vec![];
        let mut rng = seed;
        for t in 1..=END {
            f.now = t;
            for node in 0..NODES {
                if t % 10 == (2 * node as u64) % 10 {
                    f.rotate(node);
                    f.distribute_rotation(node);
                }
            }
            if t % 15 == 5 {
                for (node, author) in authors.iter_mut().enumerate() {
                    catch_up(author, &own[node], &f.nodes[node].replica);
                    let e = author.post(&format!("seed-{seed}-node-{node}-t-{t}"));
                    assert_eq!(f.nodes[node].replica.receive(e.clone()), Outcome::Accepted);
                    own[node].insert(e.id());
                    envelopes.push((t, e));
                }
                if t == 5 {
                    // Mandatory fault: a frame delivered twice (observed as Duplicate).
                    for node in [0, 3] {
                        if !f.nodes[node].awake {
                            f.awake(node, true);
                        }
                    }
                    if f.partitioned {
                        f.partition(false);
                    }
                    f.flush();
                    f.reconcile(0, 3);
                    let ok = f.duplicate(0);
                    f.assert(ok, "mandatory duplicate not injected");
                    let before = f.counts.duplicate_receipts;
                    f.flush();
                    f.assert(
                        f.counts.duplicate_receipts > before,
                        "duplicate not observed",
                    );
                }
            }
            for _ in 0..3 {
                rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
                let from = ((rng >> 32) as usize) % NODES;
                let to = ((rng >> 16) as usize) % NODES;
                // High bits: the low 3 bits of this LCG cycle with period 8.
                match (rng >> 40) % 8 {
                    0 => f.awake(from % sleeper, false),
                    1 => f.awake(from % sleeper, true),
                    2 => f.partition(!f.partitioned),
                    3 if from != to => f.reconcile(from, to),
                    4 => f.deliver(to),
                    5 => {
                        f.duplicate(to);
                    }
                    6 => {
                        f.drop_packet(to);
                    }
                    _ if from != to => {
                        let _ = f.control(from, to);
                    }
                    _ => {}
                }
            }
            // Bounded outages: at the END of every 7th tick nodes 0-3 heal and every node
            // re-pushes its current address over still-known routes (update retry, not
            // discovery). 7 < overlap (lifetime 30 - rotation period 10 = 20), so online
            // nodes cannot lose each other. Without a real bound (heal then immediate random
            // re-fault), G-H observed online nodes fragmenting permanently in some seeds.
            if t % 7 == 0 {
                for node in 0..sleeper {
                    if !f.nodes[node].awake {
                        f.awake(node, true);
                    }
                }
                if f.partitioned {
                    f.partition(false);
                }
                for node in 0..NODES {
                    f.distribute_rotation(node);
                }
            }
            if OFFLINE.contains(&t) && f.nodes[sleeper].awake {
                f.awake(sleeper, false);
            } else if t == OFFLINE.end {
                f.awake(sleeper, true);
            }
            f.assert(f.queue.len() <= QUEUE_LIMIT, "queue overflow");
        }
        // Recovery: everyone awake, no partition, address updates first, then repair.
        for node in 0..NODES {
            f.awake(node, true);
        }
        f.partition(false);
        for node in 0..NODES {
            f.distribute_rotation(node);
        }
        for _ in 0..NODES {
            f.healthy_reconcile();
        }
        let all: BTreeSet<String> = envelopes.iter().map(|(_, e)| e.id()).collect();
        for node in 0..sleeper {
            let r = &f.nodes[node].replica;
            f.assert(
                r.accepted.keys().cloned().collect::<BTreeSet<_>>() == all,
                &format!(
                    "online node {node} missing envelopes after repair: counts {:?} live-knowledge {:?}",
                    (0..NODES).map(|n| f.nodes[n].replica.accepted.len()).collect::<Vec<_>>(),
                    (0..NODES)
                        .map(|x| (0..NODES)
                            .map(|y| x == y || f.nodes[x].known[&y].active(f.now).is_some())
                            .map(|k| if k { '1' } else { '0' })
                            .collect::<String>())
                        .collect::<Vec<_>>()
                ),
            );
            f.assert(r.fanout.len() == all.len(), "duplicate fanout");
            f.nodes[0].replica.assert_same(r);
            for (_, e) in &envelopes {
                f.assert(
                    r.accepted[&e.id()].signature == e.signature,
                    "altered author proof",
                );
            }
        }
        // Retirement really happened: initial bindings and many successors are past
        // retire_at, and sends were refused for that reason (not only sleep/partition).
        f.assert(
            f.nodes
                .iter()
                .all(|n| n.path.len() >= 12 && n.path[0].retire_at < f.now),
            "bindings did not rotate/retire",
        );
        f.assert(
            f.counts.retired_routes > 0,
            "no route failed due to retirement",
        );
        // The long sleeper is mutually retired: no side holds a live binding of the other,
        // both directions fail as NoActiveBinding (unreachable, not proof failure), and it
        // misses every envelope posted after it went offline.
        for other in 0..sleeper {
            f.assert(
                f.nodes[other].known[&sleeper].active(f.now).is_none()
                    && f.nodes[sleeper].known[&other].active(f.now).is_none(),
                "long sleeper rediscovered without a mechanism",
            );
            let route = Err(SendFailure::Route(RouteFailure::NoActiveBinding));
            let (to, from) = (f.control(other, sleeper), f.control(sleeper, other));
            f.assert(
                to == route && from == route,
                "expected retired route both ways",
            );
        }
        let isolated = &f.nodes[sleeper].replica;
        f.assert(
            isolated.accepted.keys().all(|id| all.contains(id))
                && envelopes
                    .iter()
                    .filter(|(t, _)| *t >= OFFLINE.start)
                    .all(|(_, e)| !isolated.accepted.contains_key(&e.id())),
            "isolated host received post-offline envelopes",
        );
        f.assert(isolated.accepted.len() < all.len(), "isolation was vacuous");
        // Explicit out-of-band re-bootstrap fixture (NOT discovery) restores convergence.
        f.rebootstrap(sleeper);
        f.healthy_reconcile();
        f.nodes[0].replica.assert_same(&f.nodes[sleeper].replica);
        f.assert(
            f.counts.sleeps > 0
                && f.counts.partitions > 0
                && f.counts.drops > 0
                && f.counts.duplicate_receipts > 0
                && f.counts.stale_packets > 0
                && f.counts.unlinked > 0
                && f.counts.controls > 0,
            &format!("required fault coverage missing: {:?}", f.counts),
        );
    }
}
