use super::*;

pub(super) const NODES: usize = 5;
// U7 topology: members [0, 3), hosts [3, NODES).
pub(super) const FIRST_HOST: usize = 3;
pub(super) fn is_host(node: usize) -> bool {
    node >= FIRST_HOST
}
// Whether `from` can hold a route to `to` at all (U7): one end must be a host.
pub(super) fn linkable(from: usize, to: usize) -> bool {
    from != to && (is_host(from) || is_host(to))
}
pub(super) const QUEUE_LIMIT: usize = 64;
pub(super) const BINDING_LIFETIME: u64 = 30;

pub(super) struct Node {
    pub(super) replica: Replica,
    pub(super) nomination: Nomination,
    pub(super) path: Vec<BindingEvidence>,
    pub(super) known: BTreeMap<usize, HostDirectory>,
    pub(super) awake: bool,
}
#[derive(Clone)]
pub(super) enum Payload {
    Message(Envelope),
    // This stands for evidence returned by the rotation verifier, not unsigned wire data.
    Address(VerifiedSuccession),
}
#[derive(Clone)]
pub(super) struct Packet {
    pub(super) from: usize,
    pub(super) to: usize,
    pub(super) endpoint: String,
    pub(super) device_id: u64,
    pub(super) payload: Payload,
}
#[derive(Default, Debug)]
pub(super) struct Counts {
    pub(super) sleeps: usize,
    pub(super) wakes: usize,
    pub(super) rotations: usize,
    pub(super) partitions: usize,
    pub(super) sends: usize,
    pub(super) delivered: usize,
    pub(super) failed_routes: usize,
    pub(super) stale_packets: usize,
    pub(super) drops: usize,
    pub(super) duplicates: usize,
    pub(super) controls: usize,
    pub(super) queue_limits: usize,
    pub(super) no_address: usize,
    // G-H: observed (not self-incremented) outcomes and typed route failures.
    pub(super) retired_routes: usize,
    pub(super) endpoint_gone: usize,
    pub(super) unlinked: usize,
    pub(super) duplicate_receipts: usize,
    pub(super) pending_receipts: usize,
    pub(super) ignored_controls: usize,
}
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(super) enum SendFailure {
    // U7: two members never hold each other's address.
    NoAddress,
    // Sleep or partition: no link right now.
    Unlinked,
    // Sender's directory has no binding live at `now` (retired); see RouteFailure.
    Route(RouteFailure),
    // Bounded transport queue is full; the payload is not retained.
    QueueFull,
}
pub(super) struct Fleet {
    pub(super) nodes: Vec<Node>,
    pub(super) queue: Vec<Packet>,
    pub(super) now: u64,
    pub(super) partitioned: bool,
    pub(super) counts: Counts,
    pub(super) seed: u64,
    pub(super) trace: Vec<String>,
}
impl Fleet {
    pub(super) fn new(seed: u64, genesis: &Genesis, authors: &[&Author; 3]) -> Self {
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
        // U7: only member-host and host-host routes exist.
        for from in 0..NODES {
            for to in (0..NODES).filter(|&to| linkable(from, to)) {
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
    pub(super) fn log(&mut self, event: impl Into<String>) {
        self.trace.push(format!("t={} {}", self.now, event.into()));
        if self.trace.len() > 80 {
            self.trace.remove(0);
        }
    }
    pub(super) fn assert(&self, condition: bool, message: &str) {
        assert!(
            condition,
            "seed={} {message}\n{}",
            self.seed,
            self.trace.join("\n")
        );
    }
    pub(super) fn linked(&self, from: usize, to: usize) -> bool {
        self.nodes[from].awake && self.nodes[to].awake && (!self.partitioned || from % 2 == to % 2)
    }
    pub(super) fn send(&mut self, from: usize, to: usize, payload: Payload) -> Result<(), SendFailure> {
        self.counts.sends += 1;
        if !linkable(from, to) {
            self.counts.no_address += 1;
            return Err(SendFailure::NoAddress);
        }
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
    pub(super) fn control(&mut self, from: usize, to: usize) -> Result<(), SendFailure> {
        self.send(
            from,
            to,
            Payload::Address(VerifiedSuccession {
                path: self.nodes[from].path.clone(),
            }),
        )
    }
    pub(super) fn reconcile(&mut self, from: usize, to: usize) {
        // Original envelopes only, bounded by the transport queue. No direct state injection.
        // Causal order (as a VV/frontier exchange would send); see the G-H id-order test.
        // U7: no member-to-member sync at all.
        if !linkable(from, to) {
            return;
        }
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
    pub(super) fn deliver(&mut self, index: usize) {
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
    pub(super) fn flush(&mut self) {
        while !self.queue.is_empty() {
            self.deliver(0);
        }
    }
    // Fault injectors (G-H): counters reflect operations that actually happened.
    pub(super) fn duplicate(&mut self, index: usize) -> bool {
        if self.queue.is_empty() || self.queue.len() >= QUEUE_LIMIT {
            return false;
        }
        let p = self.queue[index % self.queue.len()].clone();
        self.queue.push(p);
        self.counts.duplicates += 1;
        self.log("duplicate queued frame");
        true
    }
    pub(super) fn drop_packet(&mut self, index: usize) -> bool {
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
    pub(super) fn rebootstrap(&mut self, node: usize) {
        for other in (0..NODES).filter(|&o| linkable(node, o)) {
            for (from, to) in [(node, other), (other, node)] {
                let path = self.nodes[to].path.clone();
                let nomination = self.nodes[to].nomination.clone();
                let directory = self.nodes[from].known.get_mut(&to).unwrap();
                assert!(directory.install(&nomination, &MockRotation(VerifiedSuccession { path })));
            }
        }
        self.log(format!("rebootstrap {node}"));
    }
    pub(super) fn rotate(&mut self, node: usize) {
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
    pub(super) fn awake(&mut self, node: usize, value: bool) {
        self.nodes[node].awake = value;
        if value {
            self.counts.wakes += 1;
        } else {
            self.counts.sleeps += 1;
        }
        self.log(format!("awake {node}={value}"));
    }
    pub(super) fn partition(&mut self, value: bool) {
        self.partitioned = value;
        self.counts.partitions += 1;
        self.log(format!("partition={value}"));
    }
    // Recovery is explicit: someone has to be able to deliver the fresh binding using a
    // still-known destination. This method does NOT copy paths into every directory.
    pub(super) fn distribute_rotation(&mut self, owner: usize) {
        for to in 0..NODES {
            if linkable(owner, to) {
                let _ = self.control(owner, to);
            }
        }
        self.flush();
    }
    pub(super) fn healthy_reconcile(&mut self) {
        for from in 0..NODES {
            for to in 0..NODES {
                if linkable(from, to) {
                    self.reconcile(from, to);
                    self.flush();
                }
            }
        }
    }
}
