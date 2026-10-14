use super::*;

// KIND: invariant
#[test]
fn five_nodes_seeded_sleep_partition_rotation_loss_and_retry_converge() {
    for seed in 1..=10u64 {
        let (g, mut a, mut b, mut c) = signed_fixture();
        let mut f = Fleet::new(seed, &g, &[&a, &b, &c]);
        let mut envelopes = vec![];
        for round in 0..2 {
            if round == 1 {
                // Establish real cross-author knowledge via scheduled message delivery,
                // member -> host -> member (U7).
                for (from, to) in [(0, 3), (3, 1), (1, 3), (3, 2)] {
                    f.reconcile(from, to);
                    f.flush();
                }
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
        // U7 by construction: members hold host routes only and cannot reach each other.
        for m in 0..FIRST_HOST {
            f.assert(
                f.nodes[m].known.keys().all(|&k| is_host(k)),
                "member holds a member route",
            );
            let other = (m + 1) % FIRST_HOST;
            let sent = f.control(m, other);
            f.assert(
                sent == Err(SendFailure::NoAddress),
                "member reached a member",
            );
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
pub(super) fn catch_up(author: &mut Author, own: &BTreeSet<String>, replica: &Replica) {
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
                    // Mandatory fault (R1b): a queued frame whose receiver sleeps before
                    // delivery is stale. The U7 star has fewer links than the old full mesh,
                    // so random events alone no longer guarantee one for every seed.
                    let ok = f.control(0, 3).is_ok();
                    f.assert(ok, "mandatory stale frame not queued");
                    f.awake(3, false);
                    let before = f.counts.stale_packets;
                    f.flush();
                    f.awake(3, true);
                    f.assert(f.counts.stale_packets > before, "stale frame not observed");
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
                            .map(|y| {
                                x == y
                                    || f.nodes[x]
                                        .known
                                        .get(&y)
                                        .is_some_and(|d| d.active(f.now).is_some())
                            })
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
