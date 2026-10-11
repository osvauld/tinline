#!/usr/bin/env python3
"""Deterministic behavioral model, not production networking/cryptography.

Each device has its own directory and policy snapshot. Authenticated packets are
an oracle here; real signature checks live in proto's rotation tests. Latest-state
catch-up is modeled, not a decision to replace the strict-history candidate.
No discovery magic, shared-key prediction, group hosts, or automatic block sync.
"""
import random
import unittest
from dataclasses import dataclass, field


@dataclass
class Device:
    did: str
    slot: str
    interval: int
    generation: int = 0
    online: bool = True
    contacts: set = field(default_factory=set)
    blocked: set = field(default_factory=set)
    directory: dict = field(default_factory=dict)
    sessions: set = field(default_factory=set)

    @property
    def address(self):
        return (self.did, self.slot, self.generation)


@dataclass(frozen=True)
class Packet:
    sender: tuple
    destination: tuple
    authenticated: bool = True
    kind: str = 'presence'


class Network:
    def __init__(self):
        self.now = 0
        self.devices = {}
        self.queue = []
        self.partitions = set()
        self.trace = []
        self.accepted = {}

    def add(self, did, slot, interval):
        d = Device(did, slot, interval)
        self.devices[(did, slot)] = d
        return d

    def bootstrap(self):
        # Explicit initial contact/link setup, never run implicitly on rotation.
        for d in self.devices.values():
            for peer in self.devices.values():
                if peer is not d:
                    d.directory[(peer.did, peer.slot)] = peer.address
                    if peer.did != d.did:
                        d.contacts.add(peer.did)

    def allowed(self, receiver, did):
        return did not in receiver.blocked and (did == receiver.did or did in receiver.contacts)

    def announce(self, sender):
        if not sender.online:
            return
        for (did, _), destination in sorted(sender.directory.items()):
            if self.allowed(sender, did):
                # Bounded one pending latest update per sender slot/destination slot.
                self.queue = [p for p in self.queue if not (
                    p.sender[:2] == sender.address[:2] and p.destination[:2] == destination[:2])]
                self.queue.append(Packet(sender.address, destination))

    def disconnect(self, d):
        for peer in self.devices.values():
            peer.sessions.discard((d.did, d.slot))
        d.sessions.clear()

    def block(self, d, did):
        d.blocked.add(did)
        for key in list(d.sessions):
            if key[0] == did:
                d.sessions.remove(key)
                self.devices[key].sessions.discard((d.did, d.slot))
        # Model only locally queued unsent work. Delivered information is not recalled.
        self.queue = [p for p in self.queue if not (
            p.sender[:2] == (d.did, d.slot) and p.destination[0] == did)]
        self.trace.append(('block', d.did, d.slot, did))

    def tick(self, amount=1):
        self.now += amount
        for d in self.devices.values():
            if d.online:
                self.resume(d)
        self.trace.append(('tick', self.now))

    def sleep(self, d):
        d.online = False
        self.disconnect(d)

    def resume(self, d):
        # Idealized persisted schedule, no wall-clock rollback in this model.
        d.online = True
        due = self.now // d.interval
        if due > d.generation:
            self.disconnect(d)
            d.generation = due
            self.announce(d)

    def deliver(self, index=0):
        p = self.queue.pop(index)
        self.trace.append(('deliver', p))
        source = self.devices.get(p.sender[:2])
        target = self.devices.get(p.destination[:2])
        if not source or not target or not source.online or not target.online:
            return False
        if frozenset((p.sender[:2], p.destination[:2])) in self.partitions:
            return False
        # A live authenticated control session must reach both current endpoints.
        if p.sender != source.address or p.destination != target.address:
            return False
        if not p.authenticated or not self.allowed(target, source.did) or not self.allowed(source, target.did):
            return False
        old = target.directory.get(p.sender[:2])
        if old and old[2] > p.sender[2]:
            return False
        target.directory[p.sender[:2]] = p.sender
        source.sessions.add(p.destination[:2])
        target.sessions.add(p.sender[:2])
        return True

    def flush(self):
        while self.queue:
            self.deliver()
        self.check()

    def check(self):
        for d in self.devices.values():
            for key, binding in d.directory.items():
                assert binding[:2] == key, self.trace
                scope = (d.did, d.slot, key)
                assert binding[2] >= self.accepted.get(scope, -1), self.trace
                self.accepted[scope] = binding[2]
            for key in d.sessions:
                peer = self.devices[key]
                assert d.online and peer.online, self.trace
                assert self.allowed(d, peer.did) and self.allowed(peer, d.did), self.trace
                assert (d.did, d.slot) in peer.sessions, self.trace
        assert len(self.queue) <= len(self.devices) ** 2, self.trace


def fixture():
    n = Network()
    devices = [n.add('alice', 'phone', 10), n.add('alice', 'laptop', 17),
               n.add('bob', 'phone', 23), n.add('bob', 'laptop', 29),
               n.add('carol', 'phone', 31)]
    n.bootstrap()
    return n, devices


class DeviceNetworkTests(unittest.TestCase):
    def test_presence_rotation_updates_one_slot_not_siblings(self):
        n, (a, sibling, b, _, _) = fixture()
        old_sibling = b.directory[('alice', 'laptop')]
        n.tick(10)
        # Directory has not magically changed before delivery.
        self.assertEqual(b.directory[('alice', 'phone')][2], 0)
        n.flush()
        self.assertEqual(b.directory[('alice', 'phone')], a.address)
        self.assertEqual(b.directory[('alice', 'laptop')], old_sibling)
        self.assertEqual(sibling.directory[('alice', 'phone')], a.address)

    def test_block_closes_sessions_cancels_queue_and_denies_leaked_address(self):
        n, (a, _, _, _, c) = fixture()
        n.announce(c)
        n.flush()
        self.assertIn(('carol', 'phone'), a.sessions)
        n.announce(a)
        n.block(a, c.did)
        self.assertNotIn(('carol', 'phone'), a.sessions)
        self.assertFalse(any(p.sender[:2] == ('alice', 'phone') and p.destination[0] == 'carol' for p in n.queue))
        n.tick(10)
        n.flush()
        self.assertEqual(c.directory[('alice', 'phone')][2], 0)
        n.queue.append(Packet(c.address, a.address))  # leaked new address
        self.assertFalse(n.deliver())
        n.check()

    def test_alternate_device_of_blocked_account_is_denied(self):
        n, (a, _, _, _, c) = fixture()
        second = n.add(c.did, 'laptop', 40)
        second.contacts.add(a.did)
        n.block(a, c.did)
        n.queue.append(Packet(second.address, a.address))
        self.assertFalse(n.deliver())

    def test_sleep_resume_retires_overdue_endpoint_before_accepting(self):
        n, (a, _, b, _, _) = fixture()
        old = a.address
        n.sleep(a)
        n.tick(21)
        n.resume(a)
        self.assertEqual(a.generation, 2)
        n.queue.clear()
        n.queue.append(Packet(b.address, old))
        self.assertFalse(n.deliver())
        n.announce(a)
        n.flush()
        self.assertEqual(b.directory[('alice', 'phone')], a.address)

    def test_mutual_offline_rotation_does_not_invent_recovery(self):
        n = Network()
        a, b = n.add('alice', 'phone', 10), n.add('bob', 'phone', 10)
        n.bootstrap()
        n.sleep(a)
        n.sleep(b)
        n.tick(30)
        n.resume(a)
        n.resume(b)
        n.flush()
        self.assertEqual(a.directory[('bob', 'phone')][2], 0)
        self.assertEqual(b.directory[('alice', 'phone')][2], 0)
        self.assertFalse(a.sessions or b.sessions)

    def test_forged_presence_and_old_live_endpoint_do_not_update_directory(self):
        n, (a, _, b, _, _) = fixture()
        before = dict(b.directory)
        n.queue.append(Packet(a.address, b.address, authenticated=False))
        self.assertFalse(n.deliver())
        self.assertEqual(before, b.directory)
        old = a.address
        n.tick(10)
        n.queue.clear()
        n.queue.append(Packet(old, b.address))
        self.assertFalse(n.deliver())
        self.assertEqual(before, b.directory)

    def test_block_policy_is_local_until_explicitly_synced(self):
        n, (a, sibling, _, _, c) = fixture()
        n.block(a, c.did)
        self.assertNotIn(c.did, sibling.blocked)
        n.block(sibling, c.did)  # explicit account-sync policy application
        self.assertIn(c.did, sibling.blocked)

    def test_partition_heals_with_explicit_retry_and_duplicate_is_idempotent(self):
        n, (a, _, b, _, _) = fixture()
        edge = frozenset((('alice', 'phone'), ('bob', 'phone')))
        n.partitions.add(edge)
        n.tick(10)
        n.flush()
        self.assertEqual(b.directory[('alice', 'phone')][2], 0)
        n.partitions.remove(edge)
        n.announce(a)
        n.flush()
        before = dict(b.directory)
        n.queue.append(Packet(a.address, b.address))
        self.assertTrue(n.deliver())
        self.assertEqual(b.directory, before)
        self.assertEqual(b.directory[('alice', 'phone')], a.address)
        n.check()

    def test_unblock_does_not_restore_cancelled_work_or_sessions(self):
        n, (a, _, _, _, c) = fixture()
        n.announce(a)
        n.block(a, c.did)
        a.blocked.remove(c.did)
        n.flush()
        self.assertNotIn(('carol', 'phone'), a.sessions)
        # No implicit queue replay or automatic reconnection on policy change.
        self.assertFalse(n.queue)

    def test_seeded_schedules_preserve_invariants(self):
        for seed in range(20):
            rng = random.Random(seed)
            n, devices = fixture()
            for _ in range(150):
                d = rng.choice(devices)
                event = rng.randrange(7)
                n.trace.append(('seed/event', seed, event, d.did, d.slot))
                if event == 0:
                    n.tick(rng.randrange(1, 8))
                elif event == 1:
                    n.announce(d)
                elif event == 2 and n.queue:
                    n.deliver(rng.randrange(len(n.queue)))
                elif event == 3:
                    n.sleep(d)
                elif event == 4:
                    n.resume(d)
                elif event == 5:
                    n.block(d, 'carol' if d.did != 'carol' else 'alice')
                elif event == 6 and n.queue:
                    n.queue.pop(rng.randrange(len(n.queue)))
                n.check()


if __name__ == '__main__':
    unittest.main()
