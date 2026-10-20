#!/usr/bin/env python3
"""Bounded message-frame scheduling/fault overlay on the topology simulation.

No real sockets, crypto, Loro dependencies, filesystem or timers. Inventory,
current admission and authenticated origins remain explicit test oracles. Blob
ranges stay in the separate topology model: this overlay budgets messages/Acks.
"""
import random
import unittest
from collections import Counter
from dataclasses import dataclass, replace

from test_forwarding_queue_model import Receipt
from test_host_topology_model import Topology, settle


@dataclass(frozen=True)
class Packet:
    source: str
    destination: str
    payload: object
    due: int
    sequence: int

    @property
    def record(self):
        return self.payload.record if isinstance(self.payload, Receipt) else self.payload

    @property
    def size(self):
        # Fixture framing estimate, NOT a measured production serialization size.
        r = self.record
        return 32 + len(r.scope.encode()) + len(r.payload.encode()) + len(r.proof.encode()) + (24 if isinstance(self.payload, Receipt) else 0)


class FaultTopology(Topology):
    def __init__(self, *, connections=2, dials=2, byte_limit=512, work_limit=8,
                 queue_limit=24, failure_ticks=3, return_ticks=2, seed=0):
        super().__init__(policy="scope-bypass")
        if min(connections, dials, byte_limit, work_limit, queue_limit, failure_ticks, return_ticks) < 1:
            raise ValueError("positive fixture limits required")
        self.connection_limit = connections  # Per device across scopes/protocols.
        self.dial_limit = dials  # Across the entire model per round.
        self.aggregate_bytes = byte_limit  # Per device TX + RX message/Ack bytes.
        self.work_limit = work_limit  # Per device sends + receive-verification jobs.
        self.queue_limit = queue_limit  # Global in-flight frame bound.
        self.failure_ticks = failure_ticks
        self.return_ticks = return_ticks
        self.tick = 0
        self.last_good = {}
        self.streak = {}
        self.planning = False
        self.plan = {}
        self.lease_cursor = 0
        self.leases = set()
        self.packets = []
        self.packet_sequence = 0
        self.rng = random.Random(seed)
        self.loss = 0.0
        self.duplication = 0.0
        self.delay = 1
        self.drop_receipts = False
        self.drop_records = False
        self.reorder = False
        self.bytes = Counter()
        self.work = Counter()
        self.dials = 0
        self.stats = Counter()

    def physical(self, a, b):
        return super().reachable(a, b)

    def reachable(self, a, b):
        if not self.planning:
            return self.physical(a, b)
        seen = self.last_good.get(frozenset((a, b)))
        # Cached link health can lag reality, but an offline phone cannot act.
        actor_can_run = a not in self.relays or self.nodes[a].online
        return actor_can_run and seen is not None and self.tick - seen < self.failure_ticks

    def ready(self, phone, relay, scope):
        return super().ready(phone, relay, scope) and self.streak.get((phone, scope), 0) >= self.return_ticks

    def exchange(self, a, b, scopes, protocol):
        # Topology.step builds desired links; admission happens after planning.
        pair = tuple(sorted((a, b)))
        self.plan.setdefault((*pair, protocol), set()).update(scopes)

    def fits(self, node, size):
        return self.bytes[node] + size <= self.aggregate_bytes and self.work[node] < self.work_limit

    def emit(self, source, destination, payload):
        self.packet_sequence += 1
        packet = Packet(source, destination, payload,
                        self.tick + self.rng.randint(1, self.delay), self.packet_sequence)
        if packet.size > self.aggregate_bytes:
            self.stats["oversize-deferred"] += 1
            return
        if len(self.packets) >= self.queue_limit:
            self.stats["backpressure"] += 1
            return
        if not self.fits(source, packet.size):
            self.stats["source-budget-deferred"] += 1
            return
        self.bytes[source] += packet.size
        self.work[source] += 1
        kind_drop = self.drop_receipts if isinstance(payload, Receipt) else self.drop_records
        if kind_drop or self.rng.random() < self.loss:
            self.stats["lost"] += 1
            return
        self.packets.append(packet)
        if self.rng.random() < self.duplication and len(self.packets) < self.queue_limit and self.fits(source, packet.size):
            self.bytes[source] += packet.size
            self.work[source] += 1
            self.packets.append(packet)  # Same immutable original, duplicate delivery.
            self.stats["duplicated"] += 1

    def deliver(self):
        ordered = sorted(self.packets, key=lambda p: (p.due, -p.sequence if self.reorder else p.sequence))
        self.packets = []
        for packet in ordered:
            if packet.due > self.tick:
                self.packets.append(packet)
                continue
            src, dst, record = packet.source, packet.destination, packet.record
            if not self.physical(src, dst):
                self.stats["partition-dropped"] += 1
                continue
            if packet.size > self.aggregate_bytes:
                self.stats["oversize-rejected"] += 1
                continue
            if not self.fits(dst, packet.size):
                self.packets.append(packet)
                self.stats["receive-budget-deferred"] += 1
                continue
            self.bytes[dst] += packet.size
            self.work[dst] += 1
            # Revalidate at delivery: a packet queued before removal cannot
            # regrant admission, substitute an author or create a forged receipt.
            if (record.scope not in self.nodes[src].scopes or record.scope not in self.nodes[dst].scopes
                    or record != self.originals.get(record.identifier)):
                self.stats["rejected"] += 1
                continue
            if isinstance(packet.payload, Receipt):
                if not self.receive_receipt(dst, packet.payload):
                    self.stats["rejected"] += 1
            else:
                self.nodes[dst].accept(record)  # Persists before generating its receipt.
            self.stats["delivered"] += 1

    def transfer_scoped(self, source, destination, scopes):
        a, b = self.nodes[source], self.nodes[destination]
        candidates = sorted((r for r in a.records.values() if r.scope in scopes and r.identifier not in b.records), key=lambda r: r.identifier)
        for record in self.select_batch(candidates, "records", source, destination, self.record_budget):
            self.emit(source, destination, record)
        receipts = sorted((r for r in a.receipts if r.record.scope in scopes
                           and r.record.identifier in b.records and r not in b.receipts),
                          key=lambda r: (r.record.identifier, r.replica))
        for receipt in self.select_batch(receipts, "receipts", source, destination, self.record_budget):
            self.emit(source, destination, receipt)

    def step(self):
        self.tick += 1
        self.bytes = Counter()
        self.work = Counter()
        self.dials = 0
        self.deliver()
        # Idealized periodic probes update observations. Probe traffic/cost is
        # excluded; actual failure detection needs authenticated session timers.
        names = sorted(self.nodes)
        for i, a in enumerate(names):
            for b in names[i + 1:]:
                if self.physical(a, b):
                    self.last_good[frozenset((a, b))] = self.tick
        for phone, relay in self.relays.items():
            for scope in self.nodes[phone].scopes:
                key = (phone, scope)
                seen = self.last_good.get(frozenset((phone, relay)))
                fresh = seen is not None and self.tick - seen < self.failure_ticks
                healthy = fresh and super().ready(phone, relay, scope)
                # Retain the last ready observation during the failure-detection
                # grace period; missing actual service cannot generate receipts.
                self.streak[key] = self.streak.get(key, 0) + 1 if healthy else 0
        self.plan = {}
        self.planning = True
        super().step()
        self.planning = False
        eligible = sorted(key for key, scopes in self.plan.items()
                          if self.physical(*key[:2]) and scopes & self.nodes[key[0]].scopes & self.nodes[key[1]].scopes)
        previous = self.leases
        self.leases = set()
        degrees = Counter()
        if eligible:
            start = self.lease_cursor % len(eligible)
            self.lease_cursor = (start + 1) % len(eligible)
            for key in eligible[start:] + eligible[:start]:
                a, b, _ = key
                if degrees[a] >= self.connection_limit or degrees[b] >= self.connection_limit:
                    continue
                if key not in previous:
                    if self.dials >= self.dial_limit:
                        continue
                    self.dials += 1
                self.leases.add(key)
                degrees[a] += 1
                degrees[b] += 1
        # Fairness must rotate transfer priority too, not just lease admission.
        ordered = eligible[start:] + eligible[:start] if eligible else []
        self.sessions = set()
        for a, b, protocol in ordered:
            if (a, b, protocol) not in self.leases:
                continue
            scopes = self.plan[a, b, protocol] & self.nodes[a].scopes & self.nodes[b].scopes
            for scope in scopes:
                self.session(a, b, protocol, scope)
            self.transfer_scoped(a, b, scopes)
            self.transfer_scoped(b, a, scopes)
        self.check()
        assert self.dials <= self.dial_limit
        assert len(self.packets) <= self.queue_limit
        assert sum(packet.size for packet in self.packets) <= self.queue_limit * self.aggregate_bytes
        assert all(len(self.connections(name)) <= self.connection_limit for name in names)
        assert all(n <= self.aggregate_bytes for n in self.bytes.values())
        assert all(n <= self.work_limit for n in self.work.values())


def fixture(users=3, groups=2, **kwargs):
    m = FaultTopology(**kwargs)
    scopes = [f"group:{g}" for g in range(groups)]
    m.add("host", "host-owner", scopes)
    for scope in scopes:
        m.nominate(scope, "host")
    for i in range(users):
        m.add(f"phone:{i}", f"owner:{i}", scopes)
        m.add(f"relay:{i}", f"owner:{i}", scopes)
        m.set_relay(f"phone:{i}", f"relay:{i}")
    return m


class FaultBudgetTests(unittest.TestCase):
    def assert_converged(self, model, records):
        expected = {r.identifier: r for r in records}
        for node in model.nodes.values():
            self.assertEqual(node.records, expected)
        for record in records:
            self.assertTrue(model.host_stored(record.author, record))

    def test_host_global_budgets_rotate_members_not_one_budget_per_group(self):
        m = fixture(users=8, groups=12, connections=2, dials=2)
        records = [m.post("phone:0", f"group:{i}") for i in range(12)]
        settle(m, 500)
        self.assert_converged(m, records)
        self.assertTrue(m.stats["delivered"])

    def test_small_queue_backpressure_preserves_originals_and_eventual_convergence(self):
        m = fixture(queue_limit=2, connections=2, dials=1)
        records = [m.post("phone:0", "group:0") for _ in range(8)]
        settle(m, 400)
        self.assert_converged(m, records)
        self.assertGreater(m.stats["backpressure"], 0)

    def test_loss_duplication_reordering_repair_without_duplicate_authorship(self):
        m = fixture(seed=19)
        m.loss, m.duplication, m.delay, m.reorder = 0.35, 0.6, 4, True
        records = [m.post("phone:0", f"group:{i % 2}") for i in range(8)]
        settle(m, 120)
        m.loss = m.duplication = 0
        m.delay = 1
        settle(m, 300)
        self.assert_converged(m, records)
        self.assertGreater(m.stats["lost"], 0)
        self.assertGreater(m.stats["duplicated"], 0)

    def test_lost_ack_after_host_storage_and_restart_keeps_receipt_pending(self):
        m = fixture()
        m.drop_receipts = True
        r = m.post("phone:0", "group:0")
        settle(m, 50)
        self.assertIn(r.identifier, m.nodes["host"].records)
        self.assertFalse(m.host_stored("phone:0", r))
        m.nodes["host"] = m.nodes["host"].restart()
        m.drop_receipts = False
        settle(m, 100)
        self.assert_converged(m, [r])
        self.assertFalse(any(node.read for node in m.nodes.values()))

    def test_delayed_relay_failure_then_direct_fallback_and_stable_return(self):
        m = fixture(users=1, failure_ticks=3, return_ticks=2)
        settle(m, 4)
        self.assertEqual(m.routes["phone:0", "group:0"], "relay")
        m.nodes["relay:0"].online = False
        m.step()
        self.assertEqual(m.routes["phone:0", "group:0"], "relay")  # Stale health, no fake delivery.
        self.assertFalse(m.connections("phone:0"))
        settle(m, 3)
        self.assertEqual(m.routes["phone:0", "group:0"], "direct")
        r = m.post("phone:0", "group:0")
        settle(m, 10)
        self.assertTrue(m.host_stored("phone:0", r))
        m.nodes["relay:0"].online = True
        m.step()
        self.assertEqual(m.routes["phone:0", "group:0"], "direct")
        settle(m, 40)
        self.assertEqual(m.routes["phone:0", "group:0"], "relay")
        self.assert_converged(m, [r])

    def test_upstream_health_lag_does_not_fabricate_host_receipt(self):
        m = fixture(users=1)
        settle(m, 4)
        m.partitions.add(frozenset(("relay:0", "host")))
        m.step()
        self.assertEqual(m.routes["phone:0", "group:0"], "relay")
        settle(m, 3)
        self.assertEqual(m.routes["phone:0", "group:0"], "direct")
        r = m.post("phone:0", "group:0")
        settle(m, 20)
        self.assertTrue(m.host_stored("phone:0", r))

    def test_queued_frame_rechecks_removed_membership_at_delivery(self):
        m = fixture(users=1)
        r = m.post("host", "group:0")
        m.emit("host", "relay:0", r)
        m.nodes["relay:0"].scopes.remove("group:0")
        m.step()
        self.assertNotIn(r.identifier, m.nodes["relay:0"].records)
        self.assertGreater(m.stats["rejected"], 0)

    def test_forged_and_substituted_frames_never_issue_receipts(self):
        m = fixture(users=1)
        r = m.post("phone:0", "group:0")
        m.emit("phone:0", "host", replace(r, payload="substituted"))
        # Exercise only these injected wire events, not a legitimate concurrent
        # upload that could make the later host receipt genuinely valid.
        m.tick += 1
        m.deliver()
        self.assertNotIn(r.identifier, m.nodes["host"].records)
        # A receipt naming the host before host persistence is not authentic.
        m.emit("host", "phone:0", Receipt(r, "host"))
        m.tick += 1
        m.deliver()
        self.assertFalse(m.host_stored("phone:0", r))
        self.assertGreaterEqual(m.stats["rejected"], 2)

    def test_host_failure_keeps_originals_until_service_returns(self):
        m = fixture()
        m.nodes["host"].online = False
        r = m.post("phone:0", "group:0")
        settle(m, 30)
        self.assertFalse(m.host_stored("phone:0", r))
        self.assertEqual(m.nodes["phone:0"].records[r.identifier], r)
        m.nodes["host"].online = True
        settle(m, 100)
        self.assert_converged(m, [r])

    def test_hot_scope_does_not_starve_other_scope_with_bounded_work(self):
        m = fixture(users=2, work_limit=2, byte_limit=256)
        for _ in range(24):
            m.post("host", "group:0")
        r = m.post("host", "group:1")
        settle(m, 30)
        self.assertIn(r.identifier, m.nodes["phone:1"].records)

    def test_seeded_faults_end_and_budgeted_reconciliation_converges(self):
        for seed in range(4):
            m = fixture(seed=seed, queue_limit=8)
            m.loss, m.duplication, m.delay, m.reorder = 0.25, 0.3, 3, True
            rng = random.Random(seed)
            records = []
            for i in range(20):
                author = f"phone:{rng.randrange(3)}"
                m.nodes[author].online = True
                records.append(m.post(author, f"group:{i % 2}"))
                m.nodes[f"relay:{rng.randrange(3)}"].online = rng.choice((True, False))
                m.step()
            for node in m.nodes.values():
                node.online = True
            m.loss = m.duplication = 0
            m.delay = 1
            settle(m, 400)
            self.assert_converged(m, records)

    def test_oversized_message_retained_without_fake_receipt_or_poisoning_small_scope(self):
        m = fixture(users=1, byte_limit=256)
        large = m.post("phone:0", "group:0", payload="x" * 1024)
        small = m.post("phone:0", "group:1")
        settle(m, 30)
        self.assertFalse(m.host_stored("phone:0", large))
        self.assertEqual(m.nodes["phone:0"].records[large.identifier], large)
        self.assertTrue(m.host_stored("phone:0", small))
        self.assertGreater(m.stats["oversize-deferred"], 0)
        # Native bounded history/message fragmentation remains a separate gate.

    def test_single_connection_lease_rotates_own_and_upstream_without_dial_burst(self):
        m = fixture(users=2, connections=1, dials=1)
        r = m.post("phone:0", "group:0")
        settle(m, 160)
        self.assert_converged(m, [r])

    def test_zero_limits_rejected(self):
        for parameter in ("connections", "dials", "byte_limit", "work_limit", "queue_limit", "failure_ticks", "return_ticks"):
            with self.assertRaises(ValueError):
                FaultTopology(**{parameter: 0})


if __name__ == "__main__":
    unittest.main(verbosity=2)
