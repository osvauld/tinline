#!/usr/bin/env python3
"""Executable discovery requirements model, NOT cryptography or production code.

Opaque labels model knowledge of addresses/secrets. No signatures, iroh,
clock skew, storage failures, or Android behavior are simulated here.
Run: python3 scripts/test_rotation_discovery_model.py
"""
import unittest
from dataclasses import dataclass, field


@dataclass
class Device:
    slot: str
    interval: int
    epoch: int = 0

    def address(self, tick):
        return (self.slot, self.epoch, tick // self.interval)


@dataclass
class Observer:
    # Knowing an epoch represents possession of its hypothetical prediction secret.
    epochs: set = field(default_factory=set)
    records: dict = field(default_factory=dict)

    def learn(self, address):
        slot, epoch, generation = address
        old = self.records.get(slot)
        if old is None or (epoch, generation) > old[1:]:
            self.records[slot] = address

    def find(self, device, tick):
        # Idealized predictor assumes known schedule and perfect clock alignment.
        current = device.address(tick)
        if device.epoch in self.epochs:
            return current
        return current if self.records.get(device.slot) == current else None


class DiscoveryRequirements(unittest.TestCase):
    def test_random_addresses_need_updates_after_rotation(self):
        phone = Device('phone', 10)
        friend = Observer()
        friend.learn(phone.address(0))
        self.assertIsNotNone(friend.find(phone, 9))
        self.assertIsNone(friend.find(phone, 10))
        friend.learn(phone.address(10))
        self.assertEqual(friend.find(phone, 10), phone.address(10))

    def test_shared_prediction_finds_independent_schedules_offline(self):
        phone, laptop = Device('phone', 10), Device('laptop', 17)
        phone_view, laptop_view = Observer({0}), Observer({0})
        for tick in (0, 10, 17, 31, 10000):
            self.assertEqual(phone_view.find(laptop, tick), laptop.address(tick))
            self.assertEqual(laptop_view.find(phone, tick), phone.address(tick))

    def test_unlink_cannot_erase_prediction_knowledge(self):
        phone = Device('phone', 10)
        removed_laptop = Observer({0})
        # Deleting registry membership does not erase another machine's secret.
        self.assertEqual(removed_laptop.find(phone, 10000), phone.address(10000))

    def test_fresh_epoch_excludes_removed_but_strands_offline_allowed_device(self):
        phone = Device('phone', 10)
        removed, offline_allowed = Observer({0}), Observer({0})
        phone.epoch = 1
        self.assertIsNone(removed.find(phone, 20))
        self.assertIsNone(offline_allowed.find(phone, 20))
        # A separate authenticated recovery path must deliver fresh knowledge.
        offline_allowed.epochs.add(1)
        self.assertEqual(offline_allowed.find(phone, 20), phone.address(20))
        self.assertIsNone(removed.find(phone, 20))

    def test_block_excludes_updates_not_the_rotation_schedule(self):
        phone, trusted, blocked = Device('phone', 10), Observer(), Observer()
        for peer in (trusted, blocked):
            peer.learn(phone.address(0))
        # Blocking at tick 4 does not change the endpoint or its deadline.
        self.assertEqual(phone.address(4), phone.address(0))
        trusted.learn(phone.address(10))
        self.assertEqual(trusted.find(phone, 10), phone.address(10))
        self.assertIsNone(blocked.find(phone, 10))
        # Access enforcement is separate: leaked address knowledge is possible.
        blocked.learn(phone.address(10))
        self.assertIsNotNone(blocked.find(phone, 10))

    def test_stale_update_does_not_overwrite_sibling_or_new_generation(self):
        phone, laptop, peer = Device('phone', 10), Device('laptop', 17), Observer()
        peer.learn(phone.address(30))
        peer.learn(laptop.address(34))
        peer.learn(phone.address(10))
        self.assertEqual(peer.find(phone, 30), phone.address(30))
        self.assertEqual(peer.find(laptop, 34), laptop.address(34))


if __name__ == '__main__':
    unittest.main()
