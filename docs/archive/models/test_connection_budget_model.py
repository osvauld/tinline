#!/usr/bin/env python3
"""Small offline count/scheduler model, not iroh capacity or production pooling.

Pooling is proposed, not present in current per-group dialing. Streams, identity,
permissions and reachability are symbolic. Lease rotation idealizes reconnect and
reconciliation; no timing, network discovery or power claims follow.
"""
import unittest
from dataclasses import dataclass


@dataclass(frozen=True, order=True)
class Subscription:
    scope: str
    remote: str
    alpn: str
    local_endpoint: str
    authorized: bool = True
    reachable: bool = True

    @property
    def transport_key(self):
        # Different ALPNs/local endpoint identities cannot share this transport.
        return (self.local_endpoint, self.remote, self.alpn)


class Planner:
    def __init__(self, connection_budget, dial_budget):
        if connection_budget < 1 or dial_budget < 1:
            raise ValueError("positive model budgets required")
        self.connection_budget = connection_budget
        self.dial_budget = dial_budget
        self.cursor = 0

    def leases(self, subscriptions):
        eligible = [s for s in subscriptions if s.authorized and s.reachable]
        keys = sorted({s.transport_key for s in eligible})
        if not keys:
            return {}
        start = self.cursor % len(keys)
        ordered = keys[start:] + keys[:start]
        selected = ordered[:self.connection_budget]
        self.cursor = (start + len(selected)) % len(keys)
        return {key: tuple(s.scope for s in eligible if s.transport_key == key) for key in selected}

    def dial_batches(self, leases):
        keys = sorted(leases)
        return [keys[i:i + self.dial_budget] for i in range(0, len(keys), self.dial_budget)]


def fixture(groups, hosts, people, devices=1):
    if groups and hosts < 1:
        raise ValueError("groups need a host")
    subscriptions = [Subscription(f"group:{i}", f"host:{i % hosts}", "group/2", "phone:group")
                     for i in range(groups)]
    subscriptions += [Subscription(f"dm:{i}", f"person:{i}:device:{d}", "chat/4", "phone:main")
                      for i in range(people) for d in range(devices)]
    return subscriptions


def counts(subscriptions):
    eligible = [s for s in subscriptions if s.authorized and s.reachable]
    # Current baseline: one connection per eligible group-host relation plus each
    # explicit contact device session. Contact outbound dialing usually selects
    # one device; listing all devices models inbound/additional active sessions.
    return len(eligible), len({s.transport_key for s in eligible})


class BudgetTests(unittest.TestCase):
    def test_two_groups_two_people(self):
        self.assertEqual(counts(fixture(2, 1, 2)), (4, 3))

    def test_many_groups_one_host(self):
        self.assertEqual(counts(fixture(100, 1, 0)), (100, 1))

    def test_many_groups_ten_hosts_fifty_people(self):
        self.assertEqual(counts(fixture(100, 10, 50)), (150, 60))

    def test_multiple_contact_devices_not_one_connection_per_did(self):
        self.assertEqual(counts(fixture(2, 1, 2, devices=3)), (8, 7))

    def test_same_host_endpoint_different_alpn_separate(self):
        subscriptions = [Subscription("group:a", "host", "group/2", "phone:group"),
                         Subscription("call:a", "host", "gcall", "phone:group")]
        self.assertEqual(counts(subscriptions), (2, 2))

    def test_separate_local_identity_not_pooled(self):
        subscriptions = [Subscription("group:a", "host", "group/2", "phone:group"),
                         Subscription("group:b", "host", "group/2", "phone:other-group")]
        self.assertEqual(counts(subscriptions), (2, 2))

    def test_scope_permission_checked_before_shared_transport(self):
        subscriptions = [Subscription("group:a", "host", "group/2", "phone:group"),
                         Subscription("group:b", "host", "group/2", "phone:group", authorized=False)]
        leases = Planner(1, 1).leases(subscriptions)
        self.assertEqual(list(leases.values()), [("group:a",)])

    def test_unreachable_host_does_not_consume_active_budget(self):
        subscriptions = [Subscription("group:a", "offline", "group/2", "phone:group", reachable=False),
                         Subscription("group:b", "online", "group/2", "phone:group")]
        self.assertEqual(len(Planner(1, 1).leases(subscriptions)), 1)

    def test_bounded_leases_and_dials_with_fair_eventual_service(self):
        subscriptions = fixture(100, 10, 50)
        planner = Planner(8, 2)  # Test inputs, NOT proposed production defaults.
        expected = {s.transport_key for s in subscriptions}
        served = set()
        for _ in range(8):
            leases = planner.leases(subscriptions)
            self.assertLessEqual(len(leases), 8)
            self.assertTrue(all(len(b) <= 2 for b in planner.dial_batches(leases)))
            served.update(leases)
        self.assertEqual(served, expected)

    def test_budget_one_eventually_serves_all_shared_host_groups(self):
        subscriptions = fixture(100, 10, 0)
        planner = Planner(1, 1)
        scopes = set()
        for _ in range(10):
            leases = planner.leases(subscriptions)
            self.assertEqual(len(leases), 1)
            scopes.update(next(iter(leases.values())))
        self.assertEqual(scopes, {s.scope for s in subscriptions})

    def test_permission_removed_next_lease_is_excluded(self):
        planner = Planner(1, 1)
        allowed = Subscription("group:a", "host", "group/2", "phone:group")
        denied = Subscription("group:a", "host", "group/2", "phone:group", authorized=False)
        self.assertTrue(planner.leases([allowed]))
        self.assertEqual(planner.leases([denied]), {})

    def test_zero_budget_rejected(self):
        with self.assertRaises(ValueError):
            Planner(0, 1)


if __name__ == "__main__":
    unittest.main(verbosity=2)
