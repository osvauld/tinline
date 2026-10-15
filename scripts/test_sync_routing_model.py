#!/usr/bin/env python3
"""Offline routing model only; no real transport, signatures, storage or power data.

Synthetic authenticated identity and authorized immutable records are oracles.
A step idealizes failure detection and reconciliation; it proves no timeout bound.
Calls are excluded. No production module imports this model.
"""
import random
import unittest
from dataclasses import dataclass


@dataclass(frozen=True)
class Record:
    scope: str
    author_did: str
    author_endpoint: str
    sequence: int
    payload: str


class Model:
    scopes = ("group:releases", "group:community", "dm:alice", "dm:bob")
    destination = {
        "group:releases": "official",
        "group:community": "official",
        "dm:alice": "alice",
        "dm:bob": "bob",
    }

    def __init__(self):
        self.mode = "battery-saver"
        self.online = {n: True for n in ("phone", "desktop", "official", "alice", "bob")}
        self.blocked_links = set()
        self.stores = {n: set() for n in self.online}
        # Independent scope admission, even on a multiplexed transport.
        self.admitted = {scope: True for scope in self.scopes}
        self.sessions = {n: set() for n in ("phone", "desktop")}
        self.call_endpoint = {n: n + ":call" for n in self.sessions}
        self.external_observations = {n: set() for n in ("official", "alice", "bob")}
        self.sequence = 0

    def reachable(self, a, b):
        return self.online[a] and self.online[b] and frozenset((a, b)) not in self.blocked_links

    def set_mode(self, mode):
        if mode not in ("battery-saver", "direct"):
            raise ValueError(mode)
        self.mode = mode

    def post(self, node, scope, payload="message"):
        if not self.online[node]:
            raise ValueError("app is offline")
        if scope not in self.scopes:
            raise ValueError(scope)
        self.sequence += 1
        # Group authorship uses the group endpoint, never the calling endpoint.
        endpoint = node + (":group" if scope.startswith("group:") else ":chat")
        did = "did:synthetic:owner" if node in self.sessions else "did:synthetic:" + node
        record = Record(scope, did, endpoint, self.sequence, payload)
        self.stores[node].add(record)
        return record

    def exchange(self, a, b, scopes):
        for scope in scopes:
            if not self.admitted[scope]:
                continue
            records = {r for r in self.stores[a] | self.stores[b] if r.scope == scope}
            # Forward originals, not rewritten author identities or endpoint lists.
            self.stores[a].update(records)
            self.stores[b].update(records)

    def direct_sync(self, node):
        for scope in self.scopes:
            target = self.destination[scope]
            if not self.admitted[scope] or not self.reachable(node, target):
                continue
            endpoint = node + (":group" if scope.startswith("group:") else ":chat")
            self.sessions[node].add((endpoint, target))
            self.external_observations[target].add(("did:synthetic:owner", endpoint))
            self.exchange(node, target, (scope,))

    def step(self):
        self.sessions = {n: set() for n in self.sessions}
        via_desktop = self.mode == "battery-saver" and self.reachable("phone", "desktop")
        # Own-device catch-up precedes closing the old route in this idealized model.
        if via_desktop:
            self.sessions["phone"].add(("phone:self", "desktop"))
            self.exchange("phone", "desktop", self.scopes)
        if self.online["desktop"]:
            self.direct_sync("desktop")
        if via_desktop:
            self.exchange("phone", "desktop", self.scopes)
        elif self.online["phone"]:
            self.direct_sync("phone")
        self.check()

    def check(self):
        for target, observations in self.external_observations.items():
            for _, endpoint in observations:
                assert endpoint not in self.call_endpoint.values()
                if target == "official":
                    assert endpoint.endswith(":group")
            for record in self.stores[target]:
                if target == "official":
                    assert record.scope.startswith("group:")
                    assert record.author_endpoint.endswith(":group")
        for node, sessions in self.sessions.items():
            for _, target in sessions:
                assert self.reachable(node, target)
        if self.mode == "battery-saver" and self.reachable("phone", "desktop"):
            assert self.sessions["phone"] == {("phone:self", "desktop")}


class RoutingTests(unittest.TestCase):
    def test_battery_saver_two_groups_two_contacts(self):
        m = Model()
        originals = {m.post(m.destination[s], s) for s in m.scopes}
        m.step()
        self.assertEqual(m.sessions["phone"], {("phone:self", "desktop")})
        self.assertEqual(m.sessions["desktop"], {
            ("desktop:group", "official"), ("desktop:chat", "alice"), ("desktop:chat", "bob")})
        self.assertEqual(m.stores["phone"], originals)

    def test_desktop_sleep_direct_fallback_then_return(self):
        m = Model()
        m.step()
        m.online["desktop"] = False
        outgoing = m.post("phone", "group:community")
        incoming = m.post("alice", "dm:alice")
        m.step()
        self.assertEqual(len(m.sessions["phone"]), 3)
        self.assertIn(outgoing, m.stores["official"])
        self.assertIn(incoming, m.stores["phone"])
        m.online["desktop"] = True
        m.step()
        self.assertEqual(len(m.sessions["phone"]), 1)
        self.assertIn(outgoing, m.stores["desktop"])
        self.assertIn(incoming, m.stores["desktop"])
        self.assertEqual(next(r for r in m.stores["desktop"] if r == outgoing), outgoing)

    def test_unreachable_online_desktop_uses_direct(self):
        m = Model()
        m.blocked_links.add(frozenset(("phone", "desktop")))
        m.step()
        self.assertTrue(m.online["desktop"])
        self.assertEqual(len(m.sessions["phone"]), 3)

    def test_direct_mode_ignores_available_desktop(self):
        m = Model()
        m.set_mode("direct")
        m.step()
        self.assertEqual(m.sessions["phone"], {
            ("phone:group", "official"), ("phone:chat", "alice"), ("phone:chat", "bob")})
        m.set_mode("battery-saver")
        m.step()
        self.assertEqual(m.sessions["phone"], {("phone:self", "desktop")})

    def test_offline_phone_catches_up(self):
        m = Model()
        m.online["phone"] = False
        r = m.post("official", "group:releases")
        m.step()
        self.assertFalse(m.sessions["phone"])
        self.assertNotIn(r, m.stores["phone"])
        m.online["phone"] = True
        m.step()
        self.assertIn(r, m.stores["phone"])

    def test_both_routes_unavailable_keep_local_outbox(self):
        m = Model()
        m.online["desktop"] = m.online["official"] = False
        r = m.post("phone", "group:community")
        m.step()
        self.assertIn(r, m.stores["phone"])
        self.assertNotIn(r, m.stores["official"])
        m.online["official"] = True
        m.step()
        self.assertIn(r, m.stores["official"])

    def test_desktop_without_upstream_queues_until_recovery(self):
        m = Model()
        m.blocked_links.add(frozenset(("desktop", "official")))
        r = m.post("phone", "group:community")
        m.step()
        self.assertEqual(len(m.sessions["phone"]), 1)
        self.assertIn(r, m.stores["desktop"])
        self.assertNotIn(r, m.stores["official"])
        m.blocked_links.clear()
        m.step()
        self.assertIn(r, m.stores["official"])

    def test_group_admission_not_inherited_from_shared_connection(self):
        m = Model()
        m.admitted["group:community"] = False
        denied = m.post("official", "group:community")
        allowed = m.post("official", "group:releases")
        m.step()
        self.assertIn(("desktop:group", "official"), m.sessions["desktop"])
        self.assertIn(allowed, m.stores["phone"])
        self.assertNotIn(denied, m.stores["desktop"])
        self.assertNotIn(denied, m.stores["phone"])

    def test_group_observes_same_did_only_group_endpoints(self):
        m = Model()
        r = m.post("phone", "group:community")
        m.step()
        m.online["desktop"] = False
        m.step()
        self.assertEqual(m.external_observations["official"], {
            ("did:synthetic:owner", "desktop:group"),
            ("did:synthetic:owner", "phone:group")})
        self.assertIn(r, m.stores["official"])
        self.assertEqual(r.author_did, "did:synthetic:owner")
        self.assertEqual(r.author_endpoint, "phone:group")

    def test_seeded_route_flapping_eventually_converges_without_duplicates(self):
        for seed in range(20):
            rng = random.Random(seed)
            m = Model()
            originals = set()
            for _ in range(100):
                m.set_mode(rng.choice(("battery-saver", "direct")))
                for node in m.online:
                    m.online[node] = rng.choice((True, False))
                m.blocked_links = ({frozenset(("phone", "desktop"))} if rng.choice((True, False)) else set())
                scope = rng.choice(m.scopes)
                node = rng.choice(("phone", m.destination[scope]))
                if m.online[node]:
                    originals.add(m.post(node, scope))
                before = {n: set(records) for n, records in m.stores.items()}
                m.step()
                for n in before:
                    self.assertLessEqual(before[n], m.stores[n])
            m.online = {n: True for n in m.online}
            m.blocked_links.clear()
            # First reconcile independent phone history, then desktop history.
            m.set_mode("battery-saver")
            m.step()
            m.step()
            self.assertEqual(m.stores["phone"], originals)
            self.assertEqual(m.stores["desktop"], originals)
            for scope, target in m.destination.items():
                self.assertLessEqual({r for r in originals if r.scope == scope}, m.stores[target])


if __name__ == "__main__":
    unittest.main(verbosity=2)
