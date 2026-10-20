#!/usr/bin/env python3
"""Bounded forwarding/receipt model; no real signatures, disks or transport.

Validation and authenticated receipt origin are explicit test oracles. Stores are
in-memory, but restart preserves only the fields designated durable. Hints are
bounded/lossy; reconciliation reads accepted records, never trusts hints as data.
"""
import random
import unittest
from collections import deque
from dataclasses import dataclass


@dataclass(frozen=True)
class Record:
    identifier: int
    scope: str
    author: str
    target: str
    payload: str
    proof: str
    valid: bool = True


@dataclass(frozen=True)
class Receipt:
    record: Record
    replica: str


class Replica:
    def __init__(self, name, scopes, store_limit=32, hint_limit=2):
        self.name = name
        self.scopes = set(scopes)
        self.store_limit = store_limit
        self.hint_limit = hint_limit
        self.records = {}  # Designated durable accepted originals.
        self.receipts = set()  # Designated durable validated stored receipts.
        self.hints = deque()  # Ephemeral, best-effort wakeups, not the outbox.
        self.dropped_hints = 0
        self.online = True
        self.fail_persistence = False  # Failure injection, not durable configuration.

    def accept(self, record):
        if not record.valid or record.scope not in self.scopes:
            return False
        if record.identifier in self.records:
            return self.records[record.identifier] == record
        if self.fail_persistence or len(self.records) >= self.store_limit:
            return False
        self.records[record.identifier] = record
        self.receipts.add(Receipt(record, self.name))
        if record.identifier not in self.hints:
            if len(self.hints) == self.hint_limit:
                self.dropped_hints += 1
            else:
                self.hints.append(record.identifier)
        return True

    def restart(self):
        restored = Replica(self.name, self.scopes, self.store_limit, self.hint_limit)
        restored.records = dict(self.records)
        restored.receipts = set(self.receipts)
        restored.online = self.online
        return restored

    def pending(self):
        return {r for r in self.records.values()
                if r.author == self.name and Receipt(r, r.target) not in self.receipts}

    def state(self, record):
        if record.identifier not in self.records:
            return "not-stored"
        if Receipt(record, record.target) in self.receipts:
            return "destination-stored"
        if Receipt(record, "desktop") in self.receipts:
            return "laptop-stored"
        return "local-stored"


class Network:
    scopes = ("group:a", "group:b", "dm:alice", "dm:bob")
    target = {"group:a": "official", "group:b": "official",
              "dm:alice": "alice", "dm:bob": "bob"}

    def __init__(self):
        self.nodes = {name: Replica(name, scopes) for name, scopes in (
            ("phone", self.scopes), ("desktop", self.scopes),
            ("official", self.scopes[:2]), ("alice", ("dm:alice",)), ("bob", ("dm:bob",)))}
        self.sequence = 0
        self.partitions = set()
        self.mode = "battery-saver"
        self.cursors = {}  # Ephemeral per-direction scheduling, not delivery state.

    def select_batch(self, items, kind, source, destination, batch):
        if batch < 1:
            raise ValueError("positive transfer budget required")
        by_scope = {}
        for item in items:
            record = item.record if isinstance(item, Receipt) else item
            by_scope.setdefault(record.scope, deque()).append(item)
        scopes = sorted(by_scope)
        if not scopes:
            return []
        key = (kind, source, destination)
        cursor = self.cursors.get(key, 0) % len(scopes)
        selected = []
        while len(selected) < batch and any(by_scope.values()):
            scope = scopes[cursor]
            if by_scope[scope]:
                selected.append(by_scope[scope].popleft())
            cursor = (cursor + 1) % len(scopes)
        self.cursors[key] = cursor
        return selected

    def post(self, author, scope, payload="message"):
        self.sequence += 1
        target = self.target[scope] if author in ("phone", "desktop") else "phone"
        r = Record(self.sequence, scope, author, target, payload, "synthetic-proof:" + str(self.sequence))
        if not self.nodes[author].online or not self.nodes[author].accept(r):
            return None  # Explicit admission failure, never a fake local success.
        return r

    def reachable(self, a, b):
        return (self.nodes[a].online and self.nodes[b].online
                and frozenset((a, b)) not in self.partitions)

    def receive_receipt(self, receiver, receipt):
        node = self.nodes[receiver]
        original = node.records.get(receipt.record.identifier)
        signer = self.nodes.get(receipt.replica)
        # Authentication is an oracle: only exact receipts the named replica
        # persisted may be forwarded. This is NOT a proposed verification scheme.
        if original != receipt.record or signer is None or receipt not in signer.receipts:
            return False
        node.receipts.add(receipt)
        return True

    def transfer(self, source, destination, batch=2, receipts=True):
        if not self.reachable(source, destination):
            return
        a, b = self.nodes[source], self.nodes[destination]
        # Scan accepted inventory so dropped notifications cannot lose content.
        candidates = [r for r in a.records.values() if r.scope in b.scopes
                      and r.identifier not in b.records]
        ordered = sorted(candidates, key=lambda r: r.identifier)
        for record in self.select_batch(ordered, "records", source, destination, batch):
            b.accept(record)  # Quota/authorization failures generate no receipt.
        a.hints.clear()
        if receipts:
            candidates = [r for r in a.receipts if r.record.identifier in b.records
                          and r.record.scope in b.scopes and r not in b.receipts]
            ordered = sorted(candidates, key=lambda r: (r.record.identifier, r.replica))
            for receipt in self.select_batch(ordered, "receipts", source, destination, batch):
                self.receive_receipt(destination, receipt)

    def reconcile(self, a, b, batch=2, receipts=True):
        self.transfer(a, b, batch, receipts)
        self.transfer(b, a, batch, receipts)

    def step(self, batch=2):
        via_desktop = self.mode == "battery-saver" and self.reachable("phone", "desktop")
        if via_desktop:
            self.reconcile("phone", "desktop", batch)
        for target in ("official", "alice", "bob"):
            self.reconcile("desktop", target, batch)
            if not via_desktop:
                self.reconcile("phone", target, batch)
        if via_desktop:
            self.reconcile("phone", "desktop", batch)
        self.check()

    def check(self):
        for node in self.nodes.values():
            assert len(node.records) <= node.store_limit
            assert len(node.hints) <= node.hint_limit
            for receipt in node.receipts:
                assert node.records.get(receipt.record.identifier) == receipt.record
                assert receipt in self.nodes[receipt.replica].receipts
            for record in node.records.values():
                assert record.valid and record.scope in node.scopes


class ForwardingTests(unittest.TestCase):
    def test_persistence_failure_never_issues_storage_receipt(self):
        n = Network()
        r = n.post("phone", "group:a")
        n.nodes["desktop"].fail_persistence = True
        for _ in range(3):
            n.step()
        self.assertEqual(n.nodes["phone"].state(r), "local-stored")
        self.assertNotIn(r.identifier, n.nodes["desktop"].records)
        self.assertIn(r, n.nodes["phone"].pending())
        n.nodes["desktop"].fail_persistence = False
        for _ in range(5):
            n.step()
        self.assertEqual(n.nodes["phone"].state(r), "destination-stored")

    def test_shared_host_hot_group_cannot_starve_second_group(self):
        n = Network()
        for _ in range(24):
            n.post("official", "group:a")
        second_group = n.post("official", "group:b")
        n.transfer("official", "desktop", batch=1)
        n.transfer("official", "desktop", batch=1)
        self.assertIn(second_group.identifier, n.nodes["desktop"].records)
        self.assertEqual(len(n.nodes["desktop"].records), 2)

    def test_laptop_receipt_not_destination_delivery(self):
        n = Network()
        r = n.post("phone", "group:a")
        n.nodes["official"].online = False
        n.step()
        self.assertEqual(n.nodes["phone"].state(r), "laptop-stored")
        self.assertIn(r, n.nodes["phone"].pending())
        n.nodes["official"].online = True
        for _ in range(3):
            n.step()
        self.assertEqual(n.nodes["phone"].state(r), "destination-stored")
        self.assertNotIn(r, n.nodes["phone"].pending())

    def test_hint_overflow_repaired_by_inventory(self):
        n = Network()
        originals = [n.post("official", "group:a") for _ in range(12)]
        self.assertGreater(n.nodes["official"].dropped_hints, 0)
        for _ in range(20):
            n.step(batch=1)
        self.assertTrue(all(n.nodes["phone"].records.get(r.identifier) == r for r in originals))
        self.assertLessEqual(len(n.nodes["official"].hints), 2)

    def test_interrupt_after_store_before_ack_restart_and_direct_retry(self):
        n = Network()
        r = n.post("phone", "group:a")
        n.transfer("phone", "desktop", batch=1, receipts=False)
        self.assertEqual(n.nodes["phone"].state(r), "local-stored")
        n.nodes["desktop"] = n.nodes["desktop"].restart()
        self.assertFalse(n.nodes["desktop"].hints)
        n.nodes["desktop"].online = False
        n.step(batch=1)
        self.assertIn(r, n.nodes["official"].records.values())
        n.nodes["desktop"].online = True
        for _ in range(5):
            n.step(batch=1)
        self.assertEqual(n.nodes["phone"].state(r), "destination-stored")
        self.assertEqual(len(n.nodes["official"].records), 1)
        self.assertEqual(n.nodes["desktop"].records[r.identifier], r)

    def test_quota_rejection_does_not_ack_or_lose_author_write(self):
        n = Network()
        n.nodes["official"].store_limit = 1
        first = n.post("phone", "group:a")
        second = n.post("phone", "group:b")
        for _ in range(5):
            n.step()
        self.assertEqual(n.nodes["phone"].state(first), "destination-stored")
        self.assertEqual(n.nodes["phone"].state(second), "laptop-stored")
        self.assertIn(second, n.nodes["phone"].pending())
        n.nodes["official"].store_limit = 2
        for _ in range(5):
            n.step()
        self.assertEqual(n.nodes["phone"].state(second), "destination-stored")

    def test_full_local_store_returns_explicit_admission_failure(self):
        n = Network()
        n.nodes["phone"].store_limit = 1
        self.assertIsNotNone(n.post("phone", "dm:alice"))
        self.assertIsNone(n.post("phone", "dm:bob"))
        self.assertEqual(len(n.nodes["phone"].records), 1)

    def test_bad_or_substituted_receipt_rejected(self):
        n = Network()
        r = n.post("phone", "group:a")
        self.assertFalse(n.receive_receipt("phone", Receipt(r, "official")))
        forged = Record(r.identifier, r.scope, r.author, r.target, "changed", r.proof)
        n.nodes["official"].accept(forged)
        self.assertFalse(n.receive_receipt("phone", Receipt(forged, "official")))
        self.assertIn(r, n.nodes["phone"].pending())

    def test_invalid_record_never_fanned_out(self):
        n = Network()
        r = Record(1, "group:a", "phone", "official", "bad", "bad-proof", valid=False)
        self.assertFalse(n.nodes["phone"].accept(r))
        n.step()
        self.assertTrue(all(not node.records for node in n.nodes.values()))

    def test_scope_denial_on_laptop_does_not_inherit_other_group(self):
        n = Network()
        n.nodes["desktop"].scopes.remove("group:b")
        denied = n.post("official", "group:b")
        allowed = n.post("official", "group:a")
        for _ in range(5):
            n.step()
        self.assertIn(allowed, n.nodes["phone"].records.values())
        self.assertNotIn(denied, n.nodes["phone"].records.values())
        n.mode = "direct"
        for _ in range(5):
            n.step()
        self.assertIn(denied, n.nodes["phone"].records.values())

    def test_slow_phone_does_not_stop_other_scope_reconciliation(self):
        n = Network()
        n.nodes["phone"].store_limit = 1
        for _ in range(6):
            n.post("official", "group:a")
        outgoing = n.post("desktop", "dm:bob")
        for _ in range(10):
            n.step(batch=1)
        self.assertEqual(len(n.nodes["phone"].records), 1)
        self.assertIn(outgoing, n.nodes["bob"].records.values())
        self.assertEqual(n.nodes["desktop"].state(outgoing), "destination-stored")

    def test_seeded_partial_handover_eventually_converges_with_proofs(self):
        for seed in range(10):
            rng = random.Random(seed)
            n = Network()
            originals = []
            for _ in range(20):
                scope = rng.choice(n.scopes)
                author = rng.choice(("phone", n.target[scope]))
                r = n.post(author, scope)
                self.assertIsNotNone(r)
                originals.append(r)
                n.nodes["desktop"].online = rng.choice((True, False))
                n.mode = rng.choice(("battery-saver", "direct"))
                n.step(batch=1)
                if rng.choice((True, False)):
                    n.nodes["desktop"] = n.nodes["desktop"].restart()
            n.nodes["desktop"].online = True
            n.mode = "battery-saver"
            for _ in range(120):
                n.step(batch=1)
            for r in originals:
                self.assertEqual(n.nodes["phone"].records[r.identifier], r)
                self.assertEqual(n.nodes["desktop"].records[r.identifier], r)
                self.assertEqual(n.nodes[r.target].records[r.identifier], r)
                self.assertIn(Receipt(r, r.target), n.nodes[r.author].receipts)
            self.assertFalse(n.nodes["phone"].pending())


if __name__ == "__main__":
    unittest.main(verbosity=2)
