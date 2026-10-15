#!/usr/bin/env python3
"""Symbolic paged frontier/update/blob reconciliation, NOT real Loro or crypto.

No sender inspects the receiver's store to select missing operations: knowledge
comes only from received advertisements/Acks. Test assertions may inspect stores.
One source per document is a fixture limitation, not group authority.
"""
import unittest
from collections import Counter
from dataclasses import dataclass


@dataclass(frozen=True)
class Op:
    doc: str
    writer: str
    counter: int
    proof: str
    blob: tuple = ()  # Synthetic content hash/size; not encrypted bytes.


@dataclass(frozen=True)
class Frame:
    src: str
    dst: str
    kind: str
    body: object
    generation: int
    due: int


class Model:
    def __init__(self, page=2, bytes_per_node=256, queue=16):
        if min(page, bytes_per_node, queue) < 1:
            raise ValueError("positive model limits required")
        self.page_size, self.byte_limit, self.queue_limit = page, bytes_per_node, queue
        self.nodes = ("host", "relay", "phone")
        self.links = (("host", "relay"), ("relay", "phone"))
        self.stores = {n: {} for n in self.nodes}
        self.blobs = {n: {} for n in self.nodes}
        self.authority = {}  # Synthetic immutable-original validation oracle.
        self.version = Counter()  # Source inventory mutation revision.
        self.epoch = Counter()  # Session generation, not account epoch.
        self.known = {}  # Bounded per-link advisory peer frontier.
        self.cursor = {}
        self.pass_version = {}
        self.discovered = {}  # Receiver catalog for each source.
        self.complete_pass = {}  # Receipt of an empty page, NOT readiness.
        self.pending = []
        self.tick = 0
        self.delay = 1
        self.online = {n: True for n in self.nodes}
        self.partition = set()
        self.drop = set()
        self.bytes = Counter()
        self.stats = Counter()
        self.round_cursor = 0
        self.page_sent = {}  # Rate-limit control traffic; retry lost pages.
        self.blob_quota = {n: 100_000 for n in self.nodes}
        self.delivery_remaining = 0  # Inbound slots reserved while callbacks run.

    def append(self, node, doc, count=1, blob=()):
        writer = node + ":original-writer"
        counter = self.vv(node, doc).get(writer, 0)
        result = []
        for _ in range(count):
            counter += 1
            op = Op(doc, writer, counter, f"synthetic-proof:{doc}:{writer}:{counter}", blob)
            self.authority[doc, writer, counter] = op
            self.stores[node].setdefault(doc, {})[writer, counter] = op
            result.append(op)
        self.version[node] += 1
        if blob:
            self.blobs[node][blob[0]] = blob[1]
        return result

    def vv(self, node, doc):
        vv = {}
        for writer, counter in self.stores[node].get(doc, {}):
            vv[writer] = max(vv.get(writer, 0), counter)
        return vv

    def reachable(self, a, b):
        return self.online[a] and self.online[b] and frozenset((a, b)) not in self.partition

    def enqueue(self, src, dst, kind, body, size):
        if size > self.byte_limit or self.bytes[src] + size > self.byte_limit:
            self.stats["budget-deferred"] += 1
            return
        if len(self.pending) + self.delivery_remaining >= self.queue_limit:
            self.stats["queue-deferred"] += 1
            return
        self.bytes[src] += size
        if kind in self.drop:
            self.stats["dropped"] += 1
            return
        self.pending.append((Frame(src, dst, kind, body, self.epoch[src, dst], self.tick + self.delay), size))

    def restart_session(self, a, b):
        for src, dst in ((a, b), (b, a)):
            self.epoch[src, dst] += 1
            for table in (self.known, self.cursor, self.pass_version, self.complete_pass, self.discovered):
                table.pop((src, dst), None)
                table.pop((dst, src), None)
        # Stores and verified prefixes remain designated durable. In-flight
        # callbacks from the old route must not publish new readiness or Acks.

    def deliver(self, incoming):
        ordered = reversed(incoming) if self.tick % 2 else incoming
        for frame, size in ordered:  # Reordering with periodic oldest-first service.
            self.delivery_remaining -= 1
            src, dst = frame.src, frame.dst
            if frame.due > self.tick:
                self.pending.append((frame, size))
                continue
            if frame.generation != self.epoch[src, dst]:
                self.stats["stale-generation"] += 1
                continue
            if not self.reachable(src, dst):
                continue
            if self.bytes[dst] + size > self.byte_limit:
                self.pending.append((frame, size))
                continue
            self.bytes[dst] += size
            if frame.kind == "page":
                revision, after, docs = frame.body
                key = (src, dst)
                expected = self.cursor.get(key)
                if after != expected:
                    self.stats["stale-page"] += 1
                    continue
                old = self.pass_version.get(key)
                if old is not None and old != revision:
                    self.cursor[key] = None
                    self.discovered[key] = {}
                    self.complete_pass.pop(key, None)
                    self.stats["rescan"] += 1
                    self.pass_version[key] = revision
                    continue
                self.pass_version[key] = revision
                self.discovered.setdefault(key, {}).update(docs)
                for doc, frontier in docs:
                    local = self.vv(dst, doc)
                    if any(local.get(writer, 0) < count for writer, count in frontier.items()):
                        self.enqueue(dst, src, "want", (doc, local), 32)
                if docs:
                    self.cursor[key] = docs[-1][0]
                else:
                    self.complete_pass[key] = (revision, frame.generation)
                    self.cursor[key] = None  # Continue periodic passes, not a permanent EOF.
            elif frame.kind == "want":
                doc, peer_vv = frame.body
                # Explicit receiver frontier avoids replaying only the first op
                # forever when a bounded advisory cache evicts a document VV.
                candidates = sorted((op for op in self.stores[dst].get(doc, {}).values()
                                     if op.counter > peer_vv.get(op.writer, 0)),
                                    key=lambda op: (op.counter, op.writer))
                if candidates:
                    self.enqueue(dst, src, "op", candidates[0], 64)
            elif frame.kind == "ack":
                doc, vv = frame.body
                known = self.known.setdefault((dst, src), {})
                if len(known) >= self.page_size * 2 and doc not in known:
                    known.pop(next(iter(known)))  # Advisory eviction permits safe resend.
                prior = known.setdefault(doc, {})
                for writer, counter in vv.items():
                    prior[writer] = max(prior.get(writer, 0), counter)
            elif frame.kind == "op":
                op = frame.body
                if self.authority.get((op.doc, op.writer, op.counter)) != op:
                    continue
                have = self.vv(dst, op.doc).get(op.writer, 0)
                if op.counter > have + 1:
                    self.stats["dependency-deferred"] += 1
                    continue  # No Ack for missing causal prefix.
                if op.counter == have + 1:
                    self.stores[dst].setdefault(op.doc, {})[op.writer, op.counter] = op
                    self.version[dst] += 1
                local = self.vv(dst, op.doc)
                self.enqueue(dst, src, "ack", (op.doc, local), 32)
                advertised = self.discovered.get((src, dst), {}).get(op.doc, {})
                if any(local.get(writer, 0) < count for writer, count in advertised.items()):
                    self.enqueue(dst, src, "want", (op.doc, local), 32)
            elif frame.kind == "blob-want":
                identifier, start, total = frame.body
                available = self.blobs[dst].get(identifier, 0)
                referenced = any(op.blob == (identifier, total) for ops in self.stores[dst].values() for op in ops.values())
                count = min(32, available - start, total - start)
                if referenced and count > 0:
                    self.enqueue(dst, src, "blob", (identifier, start, count, total), 24 + count)
            elif frame.kind == "blob":
                identifier, start, count, total = frame.body
                # WANT authority derives from a locally accepted original ref.
                allowed = any(op.blob == (identifier, total) for ops in self.stores[dst].values() for op in ops.values())
                have = self.blobs[dst].get(identifier, 0)
                if allowed and start == have and sum(self.blobs[dst].values()) + count <= self.blob_quota[dst]:
                    self.blobs[dst][identifier] = have + count
                    self.enqueue(dst, src, "blob-ack", (identifier, have + count), 24)
                elif allowed and start < have:
                    self.enqueue(dst, src, "blob-ack", (identifier, have), 24)
            elif frame.kind == "blob-ack":
                identifier, count = frame.body
                key = ("blob", dst, src, identifier)
                self.known[key] = max(self.known.get(key, 0), count)
    def messages(self, src, dst):
        key = (src, dst)
        docs = sorted(self.stores[src])
        after = self.cursor.get(key)
        page = [(doc, self.vv(src, doc)) for doc in docs if after is None or doc > after][:self.page_size]
        if self.tick - self.page_sent.get(key, -5) >= 5:
            self.enqueue(src, dst, "page", (self.version[src], after, page), 24 + 24 * len(page))
            self.page_sent[key] = self.tick
        # Updates are responses to explicit received WANT frontiers. Blindly
        # pushing the first range after advisory-cache eviction can livelock.

    def files(self, src, dst):
        refs = sorted({op.blob for ops in self.stores[src].values() for op in ops.values() if op.blob})
        if not refs:
            return
        key = ("file-fair", src, dst)
        index = self.cursor.get(key, 0) % len(refs)
        self.cursor[key] = (index + 1) % len(refs)
        for identifier, total in refs[index:] + refs[:index]:
            have = self.blobs[src].get(identifier, 0)
            # Requester reads only its own accepted references/verified prefix;
            # blob progress does not depend on an evictable message-VV hint.
            if have < total and sum(self.blobs[src].values()) < self.blob_quota[src]:
                self.enqueue(src, dst, "blob-want", (identifier, have, total), 24)
                return

    def step(self):
        self.tick += 1
        self.bytes = Counter()
        incoming, self.pending = self.pending, []
        # Incoming callbacks enqueue replies into a separate outbound queue;
        # reserve unprocessed slots so queue bounds also hold during delivery.
        self.delivery_remaining = len(incoming)
        self.deliver(incoming)
        directions = [pair for a, b in self.links for pair in ((a, b), (b, a))]
        index = self.round_cursor % len(directions)
        self.round_cursor += 1
        directions = directions[index:] + directions[:index]
        # All links consume the same node budget. Messages take the first share;
        # blobs use remaining space, preventing a file from taking the whole round.
        for src, dst in directions:
            if self.reachable(src, dst):
                self.messages(src, dst)
        for src, dst in directions:
            if self.reachable(src, dst):
                self.files(src, dst)
        assert len(self.pending) <= self.queue_limit
        assert all(count <= self.byte_limit for count in self.bytes.values())

    def caught_up(self, src, dst):
        # Conservative model gate: a complete current-generation pass plus local
        # receipt-covered advertised frontiers; never EOF alone. May be stale if
        # the source changes after observation, so no exclusive routing uses it.
        token = self.complete_pass.get((src, dst))
        if token is None or token[1] != self.epoch[src, dst]:
            return False
        for doc, advertised in self.discovered.get((src, dst), {}).items():
            local = self.vv(dst, doc)
            if any(local.get(writer, 0) < count for writer, count in advertised.items()):
                return False
        return True


def settle(model, rounds):
    for _ in range(rounds):
        model.step()


class PagedTests(unittest.TestCase):
    def test_two_hops_reconcile_archives_with_bounded_peer_cache_and_original_proofs(self):
        m = Model()
        expected = [m.append("host", f"group:a/day:{i:04}", 2) for i in range(12)]
        settle(m, 900)
        self.assertEqual(m.stores["host"], m.stores["relay"])
        self.assertEqual(m.stores["relay"], m.stores["phone"])
        for key, values in m.known.items():
            if len(key) == 2:
                self.assertLessEqual(len(values), m.page_size * 2)
        self.assertTrue(all(op.writer == "host:original-writer" for batch in expected for op in batch))

    def test_insert_behind_page_cursor_forces_rescan(self):
        m = Model(page=1)
        m.append("host", "group:a/day:0010")
        m.append("host", "group:a/day:0020")
        settle(m, 2)
        m.append("host", "group:a/day:0001")
        settle(m, 200)
        self.assertEqual(m.stores["host"], m.stores["phone"])
        self.assertGreater(m.stats["rescan"], 0)

    def test_lost_page_or_ack_retries_without_assuming_receiver_frontier(self):
        m = Model()
        m.append("host", "group:a/day:0001", 3)
        m.drop = {"page"}
        settle(m, 10)
        self.assertFalse(m.stores["relay"])
        m.drop = {"ack"}
        settle(m, 30)
        self.assertTrue(m.stores["relay"])
        self.assertFalse(m.known.get(("host", "relay")))
        m.drop.clear()
        settle(m, 160)
        self.assertEqual(m.stores["host"], m.stores["phone"])

    def test_old_generation_pages_and_acks_cannot_mark_returning_session_ready(self):
        m = Model()
        m.append("host", "group:a/day:0001")
        m.delay = 4
        m.step()
        m.restart_session("host", "relay")
        settle(m, 4)
        self.assertGreater(m.stats["stale-generation"], 0)
        self.assertFalse(m.caught_up("host", "relay"))
        m.delay = 1
        settle(m, 100)
        self.assertEqual(m.stores["host"], m.stores["phone"])

    def test_large_file_does_not_prevent_new_messages_and_partial_quota_recovers(self):
        m = Model(bytes_per_node=384)
        m.append("host", "group:a/day:0001", blob=("encrypted-large", 8192))
        settle(m, 40)
        m.blob_quota["phone"] = 256
        m.append("host", "group:a/day:0002")
        settle(m, 100)
        self.assertEqual(m.stores["host"], m.stores["phone"])
        self.assertLess(m.blobs["phone"].get("encrypted-large", 0), 8192)
        m.blob_quota["phone"] = 100_000
        settle(m, 1000)
        self.assertEqual(m.blobs["phone"].get("encrypted-large"), 8192)

    def test_empty_page_is_not_message_or_attachment_completion(self):
        m = Model()
        m.append("host", "group:a/day:0001", blob=("file", 2048))
        m.drop = {"op"}
        settle(m, 12)
        self.assertIn(("host", "relay"), m.complete_pass)
        self.assertFalse(m.caught_up("host", "relay"))
        m.drop.clear()
        settle(m, 40)
        self.assertEqual(m.stores["host"], m.stores["phone"])
        self.assertLess(m.blobs["phone"].get("file", 0), 2048)

    def test_lost_wants_retry_through_inventory_without_advancing_known_frontier(self):
        m = Model()
        m.append("host", "group:a/day:0001", 2)
        m.drop = {"want"}
        settle(m, 20)
        self.assertFalse(m.stores["relay"])
        self.assertFalse(m.known.get(("host", "relay")))
        m.drop.clear()
        settle(m, 100)
        self.assertEqual(m.stores["host"], m.stores["phone"])

    def test_dependency_gap_rejects_later_operation_without_ack(self):
        m = Model()
        first, second = m.append("host", "group:a/day:0001", 2)
        m.enqueue("host", "relay", "op", second, 64)
        m.tick += 1
        incoming, m.pending = m.pending, []
        m.delivery_remaining = len(incoming)
        m.deliver(incoming)
        self.assertFalse(m.stores["relay"])
        self.assertFalse(m.pending)
        self.assertEqual(m.stats["dependency-deferred"], 1)
        settle(m, 100)
        self.assertEqual(m.stores["host"], m.stores["phone"])
        self.assertEqual(m.stores["phone"][first.doc][first.writer, first.counter], first)

    def test_shared_message_file_budgets_and_queue_remain_bounded_each_round(self):
        m = Model(bytes_per_node=256, queue=8)
        m.append("host", "group:a/day:0001", blob=("large", 4096))
        for tick in range(100):
            if tick % 20 == 0:
                m.append("host", f"group:a/day:{tick + 2:04}")
            m.step()
            self.assertLessEqual(len(m.pending), 8)
            self.assertTrue(all(value <= 256 for value in m.bytes.values()))
        settle(m, 1500)
        self.assertEqual(m.stores["host"], m.stores["phone"])
        self.assertEqual(m.blobs["phone"].get("large"), 4096)

    def test_disconnected_phone_catches_up_without_whole_database_copy(self):
        m = Model()
        m.online["phone"] = False
        m.append("host", "group:a/day:0001", 4)
        settle(m, 100)
        self.assertEqual(m.stores["host"], m.stores["relay"])
        self.assertFalse(m.stores["phone"])
        m.online["phone"] = True
        settle(m, 200)
        self.assertEqual(m.stores["host"], m.stores["phone"])


if __name__ == "__main__":
    unittest.main(verbosity=2)
