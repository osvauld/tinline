#!/usr/bin/env python3
"""Offline candidate topology, not production routing, presence, crypto or energy.

Reuses the bounded record/receipt replica model. Membership, original validation,
verified blob ranges, and authenticated receipt origin are explicit global oracles.
Ticks are abstract scheduling rounds, NOT seconds or failure-detection deadlines.
"""
import random
import unittest
from dataclasses import dataclass

from test_forwarding_queue_model import Network, Receipt, Record, Replica


@dataclass(frozen=True)
class Blob:
    identifier: str  # Synthetic encrypted-content hash; no real hashing here.
    size: int


class Device(Replica):
    def __init__(self, name, owner, scopes, quota=512, blob_quota=100_000):
        super().__init__(name, scopes, store_limit=quota)
        self.owner = owner
        self.blob_quota = blob_quota
        self.blobs = {}  # Designated durable verified contiguous byte prefixes.
        self.read = set()  # Explicit user action, never inferred from a receipt.

    def restart(self):
        other = Device(self.name, self.owner, self.scopes, self.store_limit, self.blob_quota)
        other.records = dict(self.records)
        other.receipts = set(self.receipts)
        other.blobs = dict(self.blobs)
        other.read = set(self.read)
        other.online = self.online
        return other


class Topology(Network):
    def __init__(self, policy="queue", pooled=True, own_blob_mux=False):
        if policy not in ("queue", "scope-bypass"):
            raise ValueError("explicit upstream-failure policy required")
        self.nodes = {}
        self.sequence = 0
        self.partitions = set()
        self.cursors = {}
        self.relays = {}  # Phone -> own device, NOT group-host nomination.
        self.group_hosts = {}  # Scope -> explicitly nominated member devices.
        self.contacts = {}  # (owner, scope) -> contact device set.
        self.modes = {}
        self.originals = {}  # Original-proof validation oracle.
        self.attachments = {}  # Accepted signed-ref oracle indexed by original ID.
        self.policy = policy
        self.pooled = pooled
        self.own_blob_mux = own_blob_mux  # Candidate feature, not current app behavior.
        self.sessions = set()
        self.routes = {}
        self.lost_acks = set()
        self.byte_counts = {}
        self.round_bytes = {}
        self.record_budget = 2
        self.byte_budget = 64  # Per directed link, fixture bytes, not a global cap.

    def add(self, name, owner, scopes, **kwargs):
        self.nodes[name] = Device(name, owner, scopes, **kwargs)
        return self.nodes[name]

    def set_relay(self, phone, relay):
        if self.nodes[phone].owner != self.nodes[relay].owner:
            raise ValueError("not an own device")
        self.relays[phone] = relay
        self.modes[phone] = "battery-saver"

    def nominate(self, scope, name):
        if scope not in self.nodes[name].scopes:
            raise ValueError("host must have scope admission")
        self.group_hosts.setdefault(scope, set()).add(name)

    def targets(self, device, scope):
        candidates = (self.group_hosts.get(scope, set()) if scope.startswith("group:")
                      else self.contacts.get((self.nodes[device].owner, scope), set()))
        return sorted(n for n in candidates if n != device and scope in self.nodes[n].scopes)

    def post(self, author, scope, blobs=(), payload="message"):
        if not self.nodes[author].online or scope not in self.nodes[author].scopes:
            return None
        if any(b.size < 1 for b in blobs):
            raise ValueError("positive blob size required")
        node = self.nodes[author]
        sizes = {b.identifier: b.size for refs in self.attachments.values() for b in refs}
        for blob in blobs:
            if blob.identifier in sizes and sizes[blob.identifier] != blob.size:
                raise ValueError("inconsistent content-addressed blob size")
            sizes[blob.identifier] = blob.size
        additions = {b.identifier: b.size - node.blobs.get(b.identifier, 0) for b in blobs}
        if sum(node.blobs.values()) + sum(additions.values()) > node.blob_quota:
            return None
        # Destination is a logical scope, not one host's authority. Tests use
        # explicit issuer-aware host/member receipt queries, NOT Replica.state.
        self.sequence += 1
        result = Record(self.sequence, scope, author, scope, payload,
                        f"synthetic-proof:{node.owner}:{author}:{self.sequence}")
        if not node.accept(result):
            return None
        self.originals[result.identifier] = result
        self.attachments[result.identifier] = tuple(blobs)
        for blob in blobs:
            self.nodes[author].blobs[blob.identifier] = blob.size
        return result

    def ready(self, phone, relay, scope):
        a, b = self.nodes[phone], self.nodes[relay]
        return (scope in a.scopes and scope in b.scopes
                and {r for r in a.records.values() if r.scope == scope}
                == {r for r in b.records.values() if r.scope == scope})

    def session(self, a, b, protocol, scope=""):
        # Different local/remote identities and ALPNs cannot be pooled.
        own = self.nodes[a].owner == self.nodes[b].owner
        identity = "group" if protocol == "group" or (protocol == "blob" and scope.startswith("group:") and not own) else "main"
        suffix = "" if self.pooled or protocol in ("self", "blob") else scope
        self.sessions.add((min(a, b), max(a, b), identity, protocol, suffix))

    def exchange(self, a, b, scopes, protocol):
        if not self.reachable(a, b):
            return
        allowed = set(scopes) & self.nodes[a].scopes & self.nodes[b].scopes
        if not allowed:
            return
        for scope in sorted(allowed):
            self.session(a, b, protocol, scope)
        self.transfer_scoped(a, b, allowed)
        self.transfer_scoped(b, a, allowed)

    def transfer_scoped(self, source, destination, scopes):
        a, b = self.nodes[source], self.nodes[destination]
        candidates = sorted((r for r in a.records.values() if r.scope in scopes
                             and r.identifier not in b.records), key=lambda r: r.identifier)
        for r in self.select_batch(candidates, "records", source, destination, self.record_budget):
            if r == self.originals.get(r.identifier):
                b.accept(r)
        a.hints.clear()
        if (source, destination) not in self.lost_acks:
            receipts = sorted((r for r in a.receipts if r.record.scope in scopes
                               and r.record.identifier in b.records and r not in b.receipts),
                              key=lambda r: (r.record.identifier, r.replica))
            for r in self.select_batch(receipts, "receipts", source, destination, self.record_budget):
                self.receive_receipt(destination, r)
        self.transfer_blobs(source, destination, scopes)

    def transfer_blobs(self, source, destination, scopes):
        a, b = self.nodes[source], self.nodes[destination]
        # A link can be visited twice around upstream reconciliation. Do not
        # advance fairness cursors when no bytes can actually be scheduled.
        byte_key = (source, destination)
        remaining = self.byte_budget - self.round_bytes.get(byte_key, 0)
        if remaining <= 0:
            return
        # Authorized WANT comes from the receiver's stored signed reference.
        wanted = {}
        for record in b.records.values():
            if record.scope in scopes:
                for blob in self.attachments[record.identifier]:
                    have = b.blobs.get(blob.identifier, 0)
                    if a.blobs.get(blob.identifier, 0) > have:
                        wanted.setdefault(record.scope, {})[blob.identifier] = blob
        scope_names = sorted(wanted)
        if not scope_names:
            return
        key = ("blob-scopes", source, destination)
        start = self.cursors.get(key, 0) % len(scope_names)
        self.cursors[key] = (start + 1) % len(scope_names)
        for scope in scope_names[start:] + scope_names[:start]:
            items = sorted(wanted[scope].values(), key=lambda item: item.identifier)
            # Rotate files too: a huge file cannot indefinitely starve another.
            file_key = ("blob-files", source, destination, scope)
            index = self.cursors.get(file_key, 0) % len(items)
            self.cursors[file_key] = (index + 1) % len(items)
            for blob in items[index:] + items[:index]:
                have = b.blobs.get(blob.identifier, 0)
                space = b.blob_quota - sum(b.blobs.values())
                count = min(remaining, space, a.blobs.get(blob.identifier, 0) - have, blob.size - have)
                if count <= 0:
                    continue
                is_own = a.owner == b.owner
                if not (is_own and self.own_blob_mux):
                    self.session(source, destination, "blob", scope)
                b.blobs[blob.identifier] = have + count
                self.byte_counts[(source, destination)] = self.byte_counts.get((source, destination), 0) + count
                self.round_bytes[byte_key] = self.round_bytes.get(byte_key, 0) + count
                remaining -= count
                if remaining == 0:
                    return

    def step(self):
        self.sessions = set()
        self.routes = {}
        self.round_bytes = {}
        # Classify before transferring. A returning relay must catch up first;
        # previously active direct routing is not closed just because it is online.
        via = set()
        for phone, relay in sorted(self.relays.items()):
            for scope in sorted(self.nodes[phone].scopes):
                healthy = any(self.reachable(relay, h) for h in self.targets(relay, scope))
                use = (self.modes[phone] == "battery-saver" and self.reachable(phone, relay)
                       and self.ready(phone, relay, scope)
                       and (self.policy == "queue" or healthy))
                self.routes[phone, scope] = "relay" if use else "direct"
                if use:
                    via.add((phone, scope))
            self.exchange(phone, relay, self.nodes[phone].scopes, "self")
        # Hosts exchange with one another, personal relays connect upstream, and
        # independent phones connect directly. No nominated host => no auto-mesh.
        links = {}
        for name, node in sorted(self.nodes.items()):
            for scope in sorted(node.scopes):
                if (name, scope) in via:
                    continue
                for target in self.targets(name, scope):
                    pair = tuple(sorted((name, target)))
                    protocol = "group" if scope.startswith("group:") else "chat"
                    links.setdefault((*pair, protocol), set()).add(scope)
        for (a, b, protocol), scopes in sorted(links.items()):
            self.exchange(a, b, scopes, protocol)
        for phone, relay in sorted(self.relays.items()):
            self.exchange(phone, relay, self.nodes[phone].scopes, "self")
        self.check()

    def host_stored(self, observer, record):
        return any(Receipt(record, host) in self.nodes[observer].receipts
                   for host in self.group_hosts.get(record.scope, set()))

    def member_stored(self, observer, record, member):
        return Receipt(record, member) in self.nodes[observer].receipts

    def blob_complete(self, device, record):
        return all(self.nodes[device].blobs.get(b.identifier, 0) == b.size
                   for b in self.attachments[record.identifier])

    def connections(self, device):
        return {s for s in self.sessions if device in s[:2]}

    def check(self):
        # Revocation may retain previously accepted data; no wipe/revocation claim.
        for node in self.nodes.values():
            assert len(node.records) <= node.store_limit
            assert len(node.hints) <= node.hint_limit
            assert sum(node.blobs.values()) <= node.blob_quota
            for r in node.records.values():
                assert r == self.originals[r.identifier]
            for receipt in node.receipts:
                assert node.records.get(receipt.record.identifier) == receipt.record
                assert receipt in self.nodes[receipt.replica].receipts
            for blob, count in node.blobs.items():
                size = next(b.size for refs in self.attachments.values() for b in refs if b.identifier == blob)
                assert 0 <= count <= size
        assert all(count <= self.byte_budget for count in self.round_bytes.values())


def fixture(policy="queue", users=3, groups=2, hosts=1, **kwargs):
    m = Topology(policy, **kwargs)
    scopes = [f"group:{i}" for i in range(groups)]
    for h in range(hosts):
        name = f"host:{h}"
        m.add(name, name, scopes)
        for scope in scopes:
            m.nominate(scope, name)
    for u in range(users):
        m.add(f"phone:{u}", f"user:{u}", scopes)
        m.add(f"relay:{u}", f"user:{u}", scopes)
        m.set_relay(f"phone:{u}", f"relay:{u}")
    return m


def settle(model, ticks=30):
    for _ in range(ticks):
        model.step()


class TopologyTests(unittest.TestCase):
    def test_group_host_fanout_to_multiple_users_and_phone_one_message_connection(self):
        m = fixture(users=6)
        r = m.post("phone:0", "group:0")
        settle(m)
        for name in m.nodes:
            self.assertEqual(m.nodes[name].records[r.identifier], r)
        self.assertTrue(m.host_stored("phone:0", r))
        for u in range(6):
            self.assertEqual(len(m.connections(f"phone:{u}")), 1)
        self.assertEqual(len(m.connections("host:0")), 6)
        self.assertEqual(r.author, "phone:0")
        self.assertFalse(any(node.read for node in m.nodes.values()))

    def test_offline_phone_not_globally_offline_owner_and_not_member_delivered(self):
        m = fixture()
        m.nodes["phone:2"].online = False
        r = m.post("phone:0", "group:0")
        settle(m)
        self.assertIn(r.identifier, m.nodes["relay:2"].records)
        self.assertTrue(m.host_stored("phone:0", r))
        self.assertFalse(m.member_stored("phone:0", r, "phone:2"))
        self.assertFalse(m.connections("phone:2"))
        m.nodes["phone:2"].online = True
        settle(m)
        self.assertTrue(m.member_stored("phone:0", r, "phone:2"))
        self.assertFalse(m.nodes["phone:2"].read)

    def test_returning_relay_catches_up_before_exclusive_route(self):
        m = fixture()
        m.nodes["relay:0"].online = False
        r = m.post("phone:0", "group:0")
        m.step()
        self.assertEqual(m.routes["phone:0", "group:0"], "direct")
        m.nodes["relay:0"].online = True
        m.step()
        self.assertEqual(m.routes["phone:0", "group:0"], "direct")
        self.assertEqual(m.nodes["relay:0"].records[r.identifier], r)
        settle(m)
        self.assertEqual(m.routes["phone:0", "group:0"], "relay")
        self.assertEqual(len(m.connections("phone:0")), 1)

    def test_upstream_partition_compares_queue_and_scope_bypass_without_choosing_policy(self):
        for policy, arrives in (("queue", False), ("scope-bypass", True)):
            m = fixture(policy)
            m.partitions.add(frozenset(("relay:0", "host:0")))
            r = m.post("phone:0", "group:0")
            # Prime own-device storage without an upstream/direct session. The
            # policy comparison starts from a caught-up relay, not initial handover.
            m.exchange("phone:0", "relay:0", ["group:0"], "self")
            settle(m, 5)
            self.assertEqual(r.identifier in m.nodes["host:0"].records, arrives)
            self.assertEqual(m.routes["phone:0", "group:0"], "relay" if policy == "queue" else "direct")
            m.partitions.clear()
            settle(m)
            self.assertTrue(m.host_stored("phone:0", r))

    def test_multiple_hosts_failover_and_replication_do_not_duplicate_authors(self):
        m = fixture(hosts=2)
        m.nodes["host:0"].online = False
        r = m.post("phone:0", "group:0")
        settle(m)
        self.assertTrue(m.host_stored("phone:0", r))
        self.assertEqual(m.nodes["host:1"].records[r.identifier], r)
        m.nodes["host:0"].online = True
        settle(m)
        self.assertEqual(m.nodes["host:0"].records[r.identifier], r)
        self.assertEqual(len(m.nodes["host:0"].records), 1)

    def test_no_host_does_not_invent_mesh_but_member_can_explicitly_host(self):
        m = fixture(hosts=0)
        r = m.post("phone:0", "group:0")
        settle(m, 5)
        self.assertNotIn(r.identifier, m.nodes["phone:1"].records)
        m.nominate("group:0", "relay:1")
        settle(m)
        self.assertIn(r.identifier, m.nodes["phone:2"].records)
        self.assertTrue(m.host_stored("phone:0", r))

    def test_personal_relay_is_not_implicitly_group_host(self):
        m = fixture()
        self.assertTrue(all(n.startswith("host:") for n in m.group_hosts["group:0"]))
        with self.assertRaises(ValueError):
            m.set_relay("phone:0", "relay:1")

    def test_membership_removal_stops_new_records_and_ranges_without_wiping_old_data(self):
        m = fixture()
        first = m.post("phone:0", "group:0")
        settle(m)
        m.nodes["phone:2"].scopes.remove("group:0")
        m.nodes["relay:2"].scopes.remove("group:0")
        second = m.post("phone:0", "group:0", [Blob("private-second", 512)])
        settle(m)
        self.assertIn(first.identifier, m.nodes["phone:2"].records)
        self.assertNotIn(second.identifier, m.nodes["phone:2"].records)
        self.assertNotIn("private-second", m.nodes["relay:2"].blobs)

    def test_lost_host_ack_restart_repairs_receipt_not_fake_all_member_read(self):
        m = fixture()
        m.lost_acks.update({("host:0", f"relay:{u}") for u in range(3)})
        r = m.post("phone:0", "group:0")
        m.exchange("phone:0", "relay:0", ["group:0"], "self")
        settle(m, 5)
        self.assertIn(r.identifier, m.nodes["host:0"].records)
        self.assertFalse(m.host_stored("phone:0", r))
        m.nodes["host:0"] = m.nodes["host:0"].restart()
        m.lost_acks.clear()
        settle(m)
        self.assertTrue(m.host_stored("phone:0", r))
        self.assertEqual(len(m.nodes["host:0"].records), 1)
        self.assertFalse(any(node.read for node in m.nodes.values()))

    def test_slow_full_member_does_not_stop_other_members_or_fabricate_receipt(self):
        m = fixture()
        m.nodes["phone:2"].store_limit = 0
        r = m.post("phone:0", "group:0")
        settle(m)
        self.assertIn(r.identifier, m.nodes["phone:1"].records)
        self.assertFalse(m.member_stored("phone:0", r, "phone:2"))
        m.nodes["phone:2"].store_limit = 512
        settle(m)
        self.assertTrue(m.member_stored("phone:0", r, "phone:2"))

    def test_blob_resume_dedup_and_message_vs_attachment_completion(self):
        m = fixture(users=2)
        r = m.post("phone:0", "group:0", [Blob("large", 4096)])
        settle(m, 3)
        self.assertIn(r.identifier, m.nodes["phone:1"].records)
        self.assertFalse(m.blob_complete("phone:1", r))
        prefix = m.nodes["phone:1"].blobs["large"]
        m.nodes["phone:1"] = m.nodes["phone:1"].restart()
        self.assertEqual(m.nodes["phone:1"].blobs["large"], prefix)
        settle(m, 100)
        self.assertTrue(m.blob_complete("phone:1", r))
        before = dict(m.byte_counts)
        settle(m, 5)
        self.assertEqual(before, m.byte_counts)
        self.assertEqual(m.byte_counts["relay:1", "phone:1"], 4096)

    def test_blob_quota_keeps_message_and_resume_obligation(self):
        m = fixture(users=2)
        m.nodes["phone:1"].blob_quota = 100
        r = m.post("phone:0", "group:0", [Blob("quota", 512)])
        settle(m)
        self.assertEqual(m.nodes["phone:1"].blobs["quota"], 100)
        self.assertIn(r.identifier, m.nodes["phone:1"].records)
        self.assertFalse(m.blob_complete("phone:1", r))
        m.nodes["phone:1"].blob_quota = 1000
        settle(m)
        self.assertTrue(m.blob_complete("phone:1", r))

    def test_blob_alpn_adds_connection_unless_candidate_own_mux_exists(self):
        for mux, expected in ((False, 2), (True, 1)):
            m = fixture(users=2, own_blob_mux=mux)
            m.post("phone:0", "group:0", [Blob("active-transfer", 4096)])
            settle(m, 5)
            self.assertEqual(len(m.connections("phone:1")), expected)

    def test_many_groups_shared_host_pooling_vs_current_per_group_shape(self):
        for pooled, expected in ((True, 1), (False, 100)):
            m = fixture(users=3, groups=100, pooled=pooled)
            m.step()
            self.assertEqual(len(m.connections("relay:0")), expected + 1)
            self.assertEqual(len(m.connections("phone:0")), 1)
            self.assertEqual(len(m.connections("host:0")), expected * 3)

    def test_large_blob_cannot_starve_another_scope_or_same_scope_file(self):
        m = fixture(users=2)
        r = m.post("phone:0", "group:0", [Blob("a-huge", 20_000), Blob("z-small", 32)])
        s = m.post("phone:0", "group:1", [Blob("other-scope", 32)])
        settle(m, 8)
        self.assertEqual(m.nodes["phone:1"].blobs.get("z-small"), 32)
        self.assertTrue(m.blob_complete("phone:1", s))
        self.assertFalse(m.blob_complete("phone:1", r))

    def test_same_personal_device_handles_groups_contacts_and_multiple_contact_devices(self):
        m = fixture(users=1)
        for name in ("phone:0", "relay:0"):
            m.nodes[name].scopes.add("dm:alice")
        for i in range(3):
            m.add(f"alice:{i}", "alice", ["dm:alice"])
        m.contacts["user:0", "dm:alice"] = {f"alice:{i}" for i in range(3)}
        m.contacts["alice", "dm:alice"] = {"relay:0"}
        outgoing = m.post("phone:0", "dm:alice")
        incoming = m.post("alice:2", "dm:alice")
        group = m.post("phone:0", "group:0")
        settle(m)
        self.assertEqual(len(m.connections("phone:0")), 1)
        self.assertEqual(len(m.connections("relay:0")), 5)  # Own, group, 3 contact devices.
        for i in range(3):
            self.assertEqual(m.nodes[f"alice:{i}"].records[outgoing.identifier], outgoing)
            self.assertNotIn(group.identifier, m.nodes[f"alice:{i}"].records)
        self.assertEqual(m.nodes["phone:0"].records[incoming.identifier], incoming)

    def test_phone_relay_partition_and_direct_mode_each_keep_upstream_path(self):
        m = fixture()
        m.partitions.add(frozenset(("phone:0", "relay:0")))
        r = m.post("phone:0", "group:0")
        settle(m)
        self.assertTrue(m.host_stored("phone:0", r))
        self.assertEqual(m.routes["phone:0", "group:0"], "direct")
        m.partitions.clear()
        m.modes["phone:0"] = "direct"
        settle(m)
        self.assertEqual(m.routes["phone:0", "group:0"], "direct")
        self.assertTrue(any(s[3] == "group" for s in m.connections("phone:0")))

    def test_host_persistence_failure_does_not_fabricate_host_receipt(self):
        m = fixture()
        m.nodes["host:0"].fail_persistence = True
        r = m.post("phone:0", "group:0")
        settle(m, 5)
        self.assertFalse(m.host_stored("phone:0", r))
        self.assertNotIn(r.identifier, m.nodes["host:0"].records)
        self.assertEqual(m.nodes["relay:0"].records[r.identifier], r)
        m.nodes["host:0"].fail_persistence = False
        settle(m)
        self.assertTrue(m.host_stored("phone:0", r))

    def test_global_host_outage_retains_data_and_catches_up_after_recovery(self):
        m = fixture(hosts=2)
        m.nodes["host:0"].online = m.nodes["host:1"].online = False
        r = m.post("phone:0", "group:0")
        settle(m, 5)
        self.assertFalse(m.host_stored("phone:0", r))
        self.assertNotIn(r.identifier, m.nodes["phone:1"].records)
        m.nodes["host:1"].online = True
        settle(m)
        self.assertEqual(m.nodes["phone:1"].records[r.identifier], r)
        self.assertTrue(m.host_stored("phone:0", r))

    def test_blob_local_quota_and_content_address_mismatch_are_explicit_failures(self):
        m = fixture(users=1)
        m.nodes["phone:0"].blob_quota = 32
        self.assertIsNone(m.post("phone:0", "group:0", [Blob("too-large", 33)]))
        self.assertFalse(m.nodes["phone:0"].records)
        r = m.post("phone:0", "group:0", [Blob("hash", 16)])
        self.assertIsNotNone(r)
        with self.assertRaises(ValueError):
            m.post("phone:0", "group:0", [Blob("hash", 20)])
        self.assertEqual(len(m.nodes["phone:0"].records), 1)

    def test_byte_bounds_apply_across_repeated_own_link_visits(self):
        m = fixture(users=2)
        m.post("phone:0", "group:0", [Blob("bounded", 4096)])
        for _ in range(10):
            m.step()
            self.assertTrue(all(count <= m.byte_budget for count in m.round_bytes.values()))

    def test_scope_denial_on_relay_keeps_other_scope_relay_and_denied_scope_direct(self):
        m = fixture()
        m.nodes["relay:0"].scopes.remove("group:1")
        denied = m.post("host:0", "group:1")
        allowed = m.post("host:0", "group:0")
        settle(m)
        self.assertEqual(m.nodes["phone:0"].records[denied.identifier], denied)
        self.assertNotIn(denied.identifier, m.nodes["relay:0"].records)
        self.assertEqual(m.nodes["relay:0"].records[allowed.identifier], allowed)
        self.assertEqual(m.routes["phone:0", "group:0"], "relay")
        self.assertEqual(m.routes["phone:0", "group:1"], "direct")
        self.assertEqual(len(m.connections("phone:0")), 2)

    def test_seeded_churn_converges_after_faults_end_with_original_proofs(self):
        for seed in range(8):
            rng = random.Random(seed)
            m = fixture(users=4, groups=3, hosts=2, policy="scope-bypass")
            originals = []
            for i in range(40):
                author = f"phone:{rng.randrange(4)}"
                m.nodes[author].online = True
                r = m.post(author, f"group:{rng.randrange(3)}")
                originals.append(r)
                node = rng.choice(list(m.nodes))
                m.nodes[node].online = rng.choice((True, False))
                if i % 7 == 0:
                    m.nodes[node] = m.nodes[node].restart()
                m.step()
            for node in m.nodes.values():
                node.online = True
            settle(m, 140)
            for node in m.nodes.values():
                self.assertEqual(node.records, {r.identifier: r for r in originals})
            for r in originals:
                self.assertTrue(m.host_stored(r.author, r))


if __name__ == "__main__":
    unittest.main(verbosity=2)
