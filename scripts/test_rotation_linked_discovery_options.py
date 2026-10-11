#!/usr/bin/env python3
"""E1: linked-device discovery decision matrix as a deterministic symbolic model.

Status: proposal — not approved. This is NOT cryptography, NOT a key-derivation
scheme and NOT production code. It compares three discovery FAMILIES for sibling
devices of one DID (packet E1, gate G-DISCOVERY):

  independent  random endpoint per rotation + authenticated sibling updates
               + explicit recovery route (a still-reachable sibling or relink)
  prediction   a shared "prediction capability" per epoch lets holders compute a
               sibling's public endpoint for a time window
  rendezvous   devices keep random endpoints and publish current endpoints to a
               third-party store; reading needs a capability (or, in the
               forward-pointer variant, only an old endpoint id)

Symbolic stand-ins (read these before trusting any result):
  * Endpoint ids are opaque fresh labels from a counter ('ep7', 'pr3').
  * The prediction capability is an opaque epoch NUMBER. `World.predicted` is a
    lazily filled lookup table, not a formula: it stands for an UNSPECIFIED
    construction and ASSUMES holders learn public ids only, never private keys.
    Whether such a construction exists for iroh Ed25519 endpoint keys is
    unverified (see docs/design/rotation-linked-discovery-options.md).
  * "Rooted in mnemonic" means: anyone holding the mnemonic also holds the
    capability. How it would be rooted is deliberately not modeled.
  * Authentication is an oracle. A hostile mnemonic holder is admitted by
    siblings because it can attest a fresh device key (verified current
    behavior: crates/core/src/sync/engine.rs:255-282, proto accept_self_hello).
  * One endpoint per device, integer fake time, perfect delivery inside a tick,
    no relays/NAT/DNS latency, no storage faults.
  * A retired endpoint keeps listening until its scheduled deadline + OVERLAP
    (bounded overlap); an overdue resume gets no overlap.
  * Rotation when online: rotate at deadline; on overdue resume exactly one
    successor and next deadline = resume + interval (existing candidate policy).

Run tests:          python3 scripts/test_rotation_linked_discovery_options.py
Print the matrix:   python3 scripts/test_rotation_linked_discovery_options.py --matrix
"""
import copy
import sys
import unittest
from collections import defaultdict
from dataclasses import dataclass, field

INF = 10 ** 9
INTERVAL = 10  # test-only parameter; prediction assumes one account-wide interval
OVERLAP = 2    # test-only: ticks a retired endpoint keeps listening (G-RETIREMENT candidate)


@dataclass(frozen=True)
class Scheme:
    kind: str                      # 'independent' | 'prediction' | 'rendezvous'
    root: str = 'distributed'      # 'distributed' | 'mnemonic' (capability origin)
    tolerance: int = 1             # prediction: windows tried on each side
    grace: bool = False            # keep listening/publishing under older held epochs
    keep_old: bool = False         # refresher keeps old epoch for outbound dialing
    service_up: bool = True        # rendezvous store availability
    pointer: bool = False          # rendezvous: public forward pointer keyed by old id

    @property
    def label(self):
        if self.kind == 'independent':
            return 'I'
        tag = {'prediction': 'P', 'rendezvous': 'R'}[self.kind]
        return f"{tag}-{'m' if self.root == 'mnemonic' else 'd'}"


I = Scheme('independent')
P_D = Scheme('prediction')
P_M = Scheme('prediction', root='mnemonic')
R_D = Scheme('rendezvous')
R_M = Scheme('rendezvous', root='mnemonic')
COLUMNS = (I, P_D, P_M, R_D, R_M)


@dataclass
class Dev:
    name: str
    install: str               # replaceable install label, NOT a permanent slot
    spans: list                # half-open online intervals [(start, end), ...]
    skew: int = 0
    mnemonic: bool = True
    hostile: bool = False
    outsider: bool = False     # e.g. a blocked contact holding leaked knowledge
    endpoint: str = None
    prev: str = None           # retired endpoint still listening until prev_until
    prev_until: int = -1
    generation: int = 0
    deadline: int = 0
    known: dict = field(default_factory=dict)   # install -> (generation, endpoint)
    caps: set = field(default_factory=set)      # held epoch capabilities

    def online(self, t):
        return any(s <= t < e for s, e in self.spans)

    def local(self, t):
        return t + self.skew


class World:
    def __init__(self, scheme):
        self.scheme = scheme
        self.t = -1
        self.devs = {}
        self.counter = 0
        self.table = {}                  # (epoch, install, window) -> opaque id
        self.records = {}                # rendezvous store
        self.removed = set()             # tombstoned installs (registry)
        self.excluded = defaultdict(set)  # epoch -> installs excluded from it
        self.seen = defaultdict(set)     # (install, generation) -> endpoints observed
        self.met = set()                 # (dialer, receiver) pairs with an admitted session
        self.trace = []

    # ---- symbolic primitives ------------------------------------------------
    def fresh(self, prefix='ep'):
        self.counter += 1
        return f'{prefix}{self.counter}'

    def predicted(self, epoch, install, window):
        # Lookup table standing for an unspecified construction. NOT a formula.
        key = (epoch, install, window)
        if key not in self.table:
            self.table[key] = self.fresh('pr')
        return self.table[key]

    def log(self, t, msg):
        self.trace.append(f't={t} {msg}')

    # ---- setup --------------------------------------------------------------
    def add(self, name, spans=((0, INF),), skew=0, install=None, at=0,
            hostile=False, link=True):
        d = Dev(name, install or name, list(spans), skew=skew, hostile=hostile)
        if self.scheme.kind != 'prediction':
            d.endpoint = self.fresh()
            d.deadline = d.local(at) + INTERVAL
        if self.scheme.root == 'mnemonic' or link:
            d.caps = {0} if self.scheme.kind != 'independent' else set()
        self.devs[name] = d
        return d

    def bootstrap(self):
        # Explicit initial in-person link of every listed sibling; never implicit.
        for a in self.devs.values():
            for b in self.devs.values():
                if a is not b and not a.outsider and not b.outsider:
                    a.known[b.install] = (b.generation, self.endpoint_of(b, 0))

    def restore(self, name, at):
        # Mnemonic-only restore: same DID, new install, no addresses, no data.
        d = self.add(name, spans=((at, INF),), at=at, link=False)
        self.log(at, f'{name} restored from mnemonic; knows {sorted(d.known)}; caps {sorted(d.caps)}')
        return d

    def clone(self, src, name, at):
        d = copy.deepcopy(self.devs[src])
        d.name = name
        d.spans = [(at, INF)]
        self.devs[name] = d
        self.log(at, f'{name} cloned from {src} (same install {d.install}, same state)')
        return d

    def outsider(self, name, leaked_from=None, at=0):
        d = Dev(name, name, [], mnemonic=False, outsider=True)
        if leaked_from is not None:
            src = self.devs[leaked_from]
            d.known = dict(src.known)
            d.caps = set(src.caps)
            self.log(at, f'{name} obtains copy of {leaked_from} discovery knowledge caps={sorted(d.caps)}')
        self.devs[name] = d
        return d

    # ---- endpoints ----------------------------------------------------------
    def epochs_listening(self, d):
        if not d.caps:
            return []
        top = max(d.caps)
        return sorted(d.caps) if self.scheme.grace else [top]

    def listening(self, d, t):
        if self.scheme.kind == 'prediction' and d.caps:
            w = d.local(t) // INTERVAL
            return {self.predicted(e, d.install, w) for e in self.epochs_listening(d)}
        out = {d.endpoint}
        if d.prev is not None and t < d.prev_until:
            out.add(d.prev)
        return out

    def endpoint_of(self, d, t):
        if self.scheme.kind == 'prediction' and d.caps:
            return self.predicted(max(d.caps), d.install, d.local(t) // INTERVAL)
        return d.endpoint

    def generation_of(self, d, t):
        if self.scheme.kind == 'prediction' and d.caps:
            return d.local(t) // INTERVAL
        return d.generation

    def candidates(self, a, install, t):
        s = self.scheme
        out = set()
        if install in a.known:
            out.add(a.known[install][1])
        if s.kind == 'prediction':
            w = a.local(t) // INTERVAL
            for e in sorted(a.caps):
                for k in range(-s.tolerance, s.tolerance + 1):
                    out.add(self.predicted(e, install, w + k))
        if s.kind == 'rendezvous' and s.service_up:
            if s.pointer:
                cur = a.known.get(install, (0, None))[1]
                hops = 0
                while ('ptr', cur) in self.records and hops < 1000:
                    cur = self.records[('ptr', cur)]
                    hops += 1
                if cur is not None:
                    out.add(cur)
            for e in sorted(a.caps):
                rec = self.records.get((e, install))
                if rec:
                    out.add(rec[1])
        out.discard(None)
        return out

    def locate(self, a, b, t):
        """Discovery only: an endpoint `a` would try that `b` listens on now."""
        a, b = self.devs[a], self.devs[b]
        if not b.online(t):
            return None
        hit = sorted(self.candidates(a, b.install, t) & self.listening(b, t))
        return hit[0] if hit else None

    def admit(self, receiver, presenter):
        """Access policy oracle; separate from discovery."""
        if presenter.outsider:
            return False                 # blocked DID: verified-DID policy refuses
        if presenter.hostile:
            return True                  # mints a fresh DID-attested key (limitation)
        return presenter.mnemonic and presenter.install not in self.removed

    # ---- dynamics -----------------------------------------------------------
    def rotate_and_publish(self, t):
        s = self.scheme
        for d in sorted(self.devs.values(), key=lambda x: x.name):
            if d.outsider or not d.online(t):
                continue
            if s.kind != 'prediction' and d.local(t) >= d.deadline:
                prev = d.endpoint
                d.endpoint = self.fresh()
                # Overlap is anchored to the scheduled deadline, so an overdue resume
                # never revives the expired endpoint (matches the network model).
                d.prev, d.prev_until = prev, d.deadline - d.skew + OVERLAP
                d.generation += 1
                d.deadline = d.local(t) + INTERVAL
                self.log(t, f'{d.name} rotates {prev}->{d.endpoint} gen={d.generation}')
                if s.kind == 'rendezvous' and s.pointer and s.service_up:
                    self.records[('ptr', prev)] = d.endpoint
            if s.kind == 'rendezvous' and s.service_up and not s.pointer:
                for e in self.epochs_listening(d):
                    self.records[(e, d.install)] = (d.generation, d.endpoint, t)

    def learn(self, a, install, gen, ep):
        self.seen[(install, gen)].add(ep)
        old = a.known.get(install)
        if old is None or gen > old[0]:
            a.known[install] = (gen, ep)
            return True
        return False

    def sync(self, a, b, t):
        changed = False
        for x, y in ((a, b), (b, a)):
            changed |= self.learn(x, y.install, self.generation_of(y, t), self.endpoint_of(y, t))
            for inst, (g, ep) in sorted(y.known.items()):
                if inst != x.install and inst not in self.removed:
                    changed |= self.learn(x, inst, g, ep)
            # A hostile presenter shows a fresh key, so install-keyed exclusion misses it.
            shown = x.install + '#fresh' if x.hostile else x.install
            gift = {e for e in y.caps if shown not in self.excluded[e]} - x.caps
            if gift:
                x.caps |= gift
                changed = True
        return changed

    def gossip(self, t):
        changed = True
        while changed:
            changed = False
            on = [d for d in sorted(self.devs.values(), key=lambda x: x.name)
                  if d.online(t) and not d.outsider]
            for a in on:
                for b in on:
                    if a.install == b.install:
                        continue
                    if not (self.candidates(a, b.install, t) & self.listening(b, t)):
                        continue
                    if not (self.admit(b, a) and self.admit(a, b)):
                        continue
                    first = (a.name, b.name) not in self.met
                    self.met.add((a.name, b.name))
                    if self.sync(a, b, t) or first:
                        self.log(t, f'session {a.name}->{b.name}')
                        changed = True

    def run_to(self, end):
        while self.t < end:
            self.t += 1
            self.rotate_and_publish(self.t)
            self.gossip(self.t)

    # ---- owner actions ------------------------------------------------------
    def unlink(self, by, name, t):
        target = self.devs[name]
        self.removed.add(target.install)
        self.log(t, f'{by} unlinks {name} (tombstone install {target.install})')
        if target.online(t) and not target.hostile and self.locate(by, name, t):
            target.known, target.caps, target.spans = {}, set(), []
            self.log(t, f'{name} honest and reachable: erases account data')

    def refresh(self, by, exclude, t):
        if self.scheme.kind == 'independent':
            self.log(t, 'refresh: no epoch exists under independent endpoints')
            return 'n/a'
        if self.scheme.root == 'mnemonic':
            self.log(t, 'GATE G-AUTHORITY: capability rooted in mnemonic; refresh needs a new mnemonic')
            return 'gate'
        d = self.devs[by]
        e = max(d.caps) + 1
        self.excluded[e] |= set(exclude)
        # grace/keep_old retain older epochs; strict refresh holds only the new one.
        d.caps = d.caps | {e} if (self.scheme.grace or self.scheme.keep_old) else {e}
        self.log(t, f'{by} refreshes to epoch {e} excluding {sorted(exclude)}')
        return 'ok'

    def forks(self):
        return sorted(k for k, v in self.seen.items() if len(v) > 1)


# ---- scenarios (each returns (outcome, world)) --------------------------------

def sc_overlap_online(s):
    w = World(s)
    w.add('laptop'), w.add('phone')
    w.bootstrap()
    w.run_to(25)
    return ('found' if w.locate('laptop', 'phone', 25) else 'unreachable'), w


def sc_mutual_offline(s, skew=0, helper_interval=None):
    w = World(s)
    w.add('laptop', spans=((0, 5), (20, 25), (40, 60)))
    w.add('phone', spans=((0, 5), (40, 60)), skew=skew)
    if helper_interval is not None:
        h = w.add('desktop')
        h.deadline = helper_interval
    w.bootstrap()
    w.run_to(40)
    return ('found' if w.locate('laptop', 'phone', 40) else 'unreachable'), w


def sc_restore(s):
    w = World(s)
    w.add('phone')
    w.bootstrap()
    w.run_to(39)
    w.restore('restored', 40)
    w.run_to(40)
    return ('found' if w.locate('restored', 'phone', 40) else 'unreachable'), w


def sc_clone(s):
    w = World(s)
    w.add('laptop'), w.add('phone')
    w.bootstrap()
    w.run_to(5)
    w.clone('phone', 'phone-clone', 6)
    w.run_to(15)
    return ('fork-detectable' if w.forks() else 'fork-hidden'), w


def sc_unlinked_honest(s, refresh=False):
    w = World(s)
    w.add('laptop', spans=((0, 5), (200, 210)))
    w.add('phone')
    w.bootstrap()
    w.run_to(12)
    w.unlink('phone', 'laptop', 12)
    if refresh:
        w.refresh('phone', {'laptop'}, 12)
    w.run_to(200)
    if not w.locate('laptop', 'phone', 200):
        return 'stale', w
    return ('locates-admitted' if w.admit(w.devs['phone'], w.devs['laptop'])
            else 'locates-refused'), w


def sc_hostile_mnemonic(s, back_at, refresh=False):
    w = World(s)
    w.add('laptop', spans=((0, 5), (back_at, back_at + 1)), hostile=True)
    w.add('phone')
    w.bootstrap()
    w.run_to(7)
    w.unlink('phone', 'laptop', 7)
    if refresh:
        w.refresh('phone', {'laptop'}, 7)
    w.run_to(back_at)
    if 'phone' in w.devs['laptop'].known and \
            w.devs['laptop'].known['phone'][1] in w.listening(w.devs['phone'], back_at):
        return 'regains', w
    return 'excluded', w


def sc_leak_to_blocked(s, refresh=False):
    w = World(s)
    w.add('laptop'), w.add('phone')
    w.bootstrap()
    w.run_to(5)
    w.outsider('blocked', leaked_from='laptop', at=5)
    w.run_to(10)
    if refresh:
        w.refresh('phone', {'blocked'}, 10)
    w.run_to(40)
    if not w.locate('blocked', 'phone', 40):
        return 'stale', w
    return ('locates-admitted' if w.admit(w.devs['phone'], w.devs['blocked'])
            else 'locates-refused'), w


def sc_refresh_strands(s, grace=False, keep_old=False, hostile_removed=False):
    s = Scheme(s.kind, root=s.root, tolerance=s.tolerance, grace=grace,
               keep_old=keep_old, service_up=s.service_up, pointer=s.pointer)
    w = World(s)
    w.add('allowed', spans=((0, 5), (50, 60)))
    w.add('removed', spans=((0, 5), (50, 60)), hostile=hostile_removed)
    w.add('phone')
    w.bootstrap()
    w.run_to(10)
    w.unlink('phone', 'removed', 10)
    w.refresh('phone', {'removed'}, 10)
    w.run_to(50)
    a = 'found' if w.locate('allowed', 'phone', 50) else 'unreachable'
    r = 'found' if w.locate('removed', 'phone', 50) else 'unreachable'
    return f'allowed:{a} removed:{r}', w


SCENARIOS = [
    ('both online across rotation', sc_overlap_online),
    ('mutual offline, both rotated', sc_mutual_offline),
    ('mutual offline, skew 25 > tolerance', lambda s: sc_mutual_offline(s, skew=25)),
    ('restore from mnemonic only', sc_restore),
    ('clone of whole device state', sc_clone),
    ('honest unlinked, offline at unlink, no refresh', sc_unlinked_honest),
    ('honest unlinked, refresh at unlink', lambda s: sc_unlinked_honest(s, refresh=True)),
    ('hostile mnemonic, returns before next rotation', lambda s: sc_hostile_mnemonic(s, 8)),
    ('hostile mnemonic, returns later, no refresh', lambda s: sc_hostile_mnemonic(s, 15)),
    ('hostile mnemonic, returns later, refresh', lambda s: sc_hostile_mnemonic(s, 15, refresh=True)),
    ('leaked discovery knowledge to blocked contact', sc_leak_to_blocked),
    ('leaked, then refresh', lambda s: sc_leak_to_blocked(s, refresh=True)),
    ('refresh while allowed sibling offline (strict)', sc_refresh_strands),
    ('same, old epoch kept as grace (listen)', lambda s: sc_refresh_strands(s, grace=True)),
    ('same, old epoch kept for outbound dial', lambda s: sc_refresh_strands(s, keep_old=True)),
]

# Expected decision matrix. Mirrors the table in
# docs/design/rotation-linked-discovery-options.md section 4.
EXPECTED = {
    'both online across rotation': ['found'] * 5,
    'mutual offline, both rotated': ['unreachable', 'found', 'found', 'found', 'found'],
    'mutual offline, skew 25 > tolerance': ['unreachable', 'unreachable', 'unreachable', 'found', 'found'],
    'restore from mnemonic only': ['unreachable', 'unreachable', 'found', 'unreachable', 'found'],
    'clone of whole device state': ['fork-detectable', 'fork-hidden', 'fork-hidden', 'fork-detectable', 'fork-detectable'],
    'honest unlinked, offline at unlink, no refresh': ['stale'] + ['locates-refused'] * 4,
    'honest unlinked, refresh at unlink': ['stale', 'stale', 'locates-refused', 'stale', 'locates-refused'],
    'hostile mnemonic, returns before next rotation': ['regains'] * 5,
    'hostile mnemonic, returns later, no refresh': ['excluded', 'regains', 'regains', 'regains', 'regains'],
    'hostile mnemonic, returns later, refresh': ['excluded', 'excluded', 'regains', 'excluded', 'regains'],
    'leaked discovery knowledge to blocked contact': ['stale'] + ['locates-refused'] * 4,
    'leaked, then refresh': ['stale', 'stale', 'locates-refused', 'stale', 'locates-refused'],
    'refresh while allowed sibling offline (strict)': [
        'allowed:unreachable removed:unreachable',
        'allowed:unreachable removed:unreachable',
        'allowed:found removed:found',
        'allowed:unreachable removed:unreachable',
        'allowed:found removed:found'],
    'same, old epoch kept for outbound dial': [
        'allowed:unreachable removed:unreachable',
        'allowed:found removed:unreachable',
        'allowed:found removed:found',
        'allowed:found removed:unreachable',
        'allowed:found removed:found'],
    'same, old epoch kept as grace (listen)': [
        'allowed:unreachable removed:unreachable',
        'allowed:found removed:found',
        'allowed:found removed:found',
        'allowed:found removed:found',
        'allowed:found removed:found'],
}


def matrix():
    return {name: [fn(s)[0] for s in COLUMNS] for name, fn in SCENARIOS}


def print_matrix():
    cols = [s.label for s in COLUMNS]
    print('| Scenario | ' + ' | '.join(cols) + ' |')
    print('|---' * (len(cols) + 1) + '|')
    for name, row in matrix().items():
        print(f'| {name} | ' + ' | '.join(row) + ' |')


class LinkedDiscoveryOptions(unittest.TestCase):
    def assertTrace(self, w, needle):
        self.assertTrue(any(needle in line for line in w.trace),
                        f'missing {needle!r} in trace:\n' + '\n'.join(w.trace))

    # KIND: candidate-policy G-DISCOVERY (all options side by side)
    def test_decision_matrix_matches_documented_table(self):
        self.assertEqual(matrix(), EXPECTED)

    # KIND: invariant
    def test_model_is_deterministic_and_has_no_derivation_formula(self):
        a, b = sc_mutual_offline(P_D)[1], sc_mutual_offline(P_D)[1]
        self.assertEqual(a.trace, b.trace)
        self.assertEqual(a.table, b.table)
        # Predicted ids are allocation-order labels, unrelated to their key.
        self.assertTrue(all(v.startswith('pr') and v[2:].isdigit() for v in a.table.values()))

    # KIND: candidate-policy G-DISCOVERY
    def test_independent_mutual_offline_rotation_is_unreachable_counterexample(self):
        outcome, w = sc_mutual_offline(I)
        self.assertEqual(outcome, 'unreachable')
        lap, ph = w.devs['laptop'], w.devs['phone']
        # Both hold only generation-0 knowledge of the other; both moved on.
        self.assertEqual(lap.known['phone'][0], 0)
        self.assertEqual(ph.known['laptop'][0], 0)
        self.assertEqual((lap.generation, ph.generation), (2, 1))
        self.assertTrace(w, 't=20 laptop rotates')
        self.assertTrace(w, 't=40 phone rotates')

    # KIND: candidate-policy G-DISCOVERY
    def test_independent_recovery_needs_a_peer_whose_address_is_still_known(self):
        stable, w = sc_mutual_offline(I, helper_interval=100)
        self.assertEqual(stable, 'found')
        self.assertTrace(w, 'session phone->desktop')
        # Negative control: a helper that also rotated while they were away does not help.
        rotating, w2 = sc_mutual_offline(I, helper_interval=10)
        self.assertEqual(rotating, 'unreachable')

    # KIND: candidate-policy G-DISCOVERY
    def test_prediction_clock_skew_beyond_tolerance_misses_both_directions(self):
        ok, _ = sc_mutual_offline(P_D, skew=5)
        self.assertEqual(ok, 'found')
        miss, w = sc_mutual_offline(P_D, skew=25)
        self.assertEqual(miss, 'unreachable')
        self.assertIsNone(w.locate('phone', 'laptop', 40))
        # Cost of tolerance k: 2k+1 candidate ids per sibling per epoch held.
        self.assertEqual(len(w.candidates(w.devs['laptop'], 'phone', 40)), 2 * P_D.tolerance + 1 + 1)

    # KIND: limitation
    def test_prediction_clone_is_indistinguishable_no_fork_evidence(self):
        outcome, w = sc_clone(P_D)
        self.assertEqual(outcome, 'fork-hidden')
        self.assertEqual(w.listening(w.devs['phone'], 15), w.listening(w.devs['phone-clone'], 15))
        control, wi = sc_clone(I)
        self.assertEqual(control, 'fork-detectable')
        self.assertIn(('phone', 1), wi.forks())

    # KIND: limitation
    def test_hostile_mnemonic_regains_full_discovery_via_any_live_old_address(self):
        for s in COLUMNS:
            outcome, w = sc_hostile_mnemonic(s, 8)
            self.assertEqual(outcome, 'regains', s.label)
            self.assertTrace(w, 'session laptop->phone')
        # Even when discovery is excluded, authority is not: a reachable address admits it.
        _, w = sc_hostile_mnemonic(I, 15)
        self.assertTrue(w.admit(w.devs['phone'], w.devs['laptop']))

    # KIND: candidate-policy G-DISCOVERY
    def test_mnemonic_rooted_capability_cannot_be_refreshed(self):
        for s in (P_M, R_M):
            outcome, w = sc_hostile_mnemonic(s, 15, refresh=True)
            self.assertEqual(outcome, 'regains', s.label)
            self.assertTrace(w, 'GATE G-AUTHORITY')
        outcome, w = sc_restore(P_M)
        self.assertEqual(outcome, 'found')  # the same property that helps restore

    # KIND: invariant
    def test_discovery_never_implies_access_for_blocked_holder(self):
        for s in COLUMNS:
            outcome, w = sc_leak_to_blocked(s)
            self.assertNotEqual(outcome, 'locates-admitted', s.label)
            before = copy.deepcopy(w.devs['phone'].known)
            w.gossip(40)  # outsider never participates in sessions
            self.assertEqual(w.devs['phone'].known, before)
            self.assertNotIn('blocked', ' '.join(l for l in w.trace if 'session' in l))

    # KIND: candidate-policy G-DISCOVERY
    def test_public_forward_pointer_leaks_new_endpoint_to_blocked_contact(self):
        s = Scheme('rendezvous', pointer=True)
        w = World(s)
        w.add('phone')
        w.bootstrap()
        w.outsider('blocked')
        w.devs['blocked'].known['phone'] = w.devs['phone'].known.get('phone', (0, w.devs['phone'].endpoint))
        w.run_to(40)
        # No capability, no update sent: the old id alone follows four pointers.
        self.assertEqual(w.devs['phone'].generation, 4)
        self.assertIsNotNone(w.locate('blocked', 'phone', 40))
        self.assertFalse(w.admit(w.devs['phone'], w.devs['blocked']))
        # Negative control: a capability-gated store does not leak to the same holder.
        w2 = World(R_D)
        w2.add('phone')
        w2.outsider('blocked')
        w2.devs['blocked'].known['phone'] = (0, w2.devs['phone'].endpoint)
        w2.run_to(40)
        self.assertIsNone(w2.locate('blocked', 'phone', 40))

    # KIND: invariant
    def test_refresh_strands_offline_allowed_sibling_and_grace_serves_removed_equally(self):
        for s in (P_D, R_D):
            strict, w = sc_refresh_strands(s)
            self.assertEqual(strict, 'allowed:unreachable removed:unreachable', s.label)
            graced, wg = sc_refresh_strands(s, grace=True)
            self.assertEqual(graced, 'allowed:found removed:found', s.label)
            # The difference is only the access check, which a hostile mnemonic passes.
            self.assertFalse(wg.admit(wg.devs['phone'], wg.devs['removed']))
            wg.devs['removed'].hostile = True
            self.assertTrue(wg.admit(wg.devs['phone'], wg.devs['removed']))

    # KIND: candidate-policy G-DISCOVERY
    def test_rendezvous_unavailable_is_unreachable_not_proof_failure(self):
        down = Scheme('rendezvous', service_up=False)
        outcome, w = sc_mutual_offline(down)
        self.assertEqual(outcome, 'unreachable')
        self.assertEqual(w.records, {})
        # Positive control with the store up.
        self.assertEqual(sc_mutual_offline(R_D)[0], 'found')

    # KIND: candidate-policy G-DISCOVERY
    def test_independent_honest_unlinked_leak_is_bounded_by_one_rotation(self):
        outcome, w = sc_unlinked_honest(I)
        self.assertEqual(outcome, 'stale')
        lap = w.devs['laptop']
        # Its retained address was generation 0; phone moved past it at t=10.
        self.assertEqual(lap.known['phone'][0], 0)
        self.assertTrace(w, 't=10 phone rotates')
        outcome_p, _ = sc_unlinked_honest(P_D)
        self.assertEqual(outcome_p, 'locates-refused')


    # KIND: limitation
    def test_outbound_dial_with_old_epoch_recovers_allowed_and_rearms_hostile(self):
        for s in (P_D, R_D):
            honest, _ = sc_refresh_strands(s, keep_old=True)
            self.assertEqual(honest, 'allowed:found removed:unreachable', s.label)
            hostile, w = sc_refresh_strands(s, keep_old=True, hostile_removed=True)
            # The refresher dials the removed device's old-epoch address; a hostile
            # holder answers with a fresh attested key and receives the new epoch.
            self.assertEqual(hostile, 'allowed:found removed:found', s.label)
            self.assertIn(1, w.devs['removed'].caps, s.label)
            self.assertTrace(w, 'session phone->removed')


if __name__ == '__main__':
    if '--matrix' in sys.argv:
        print_matrix()
        sys.exit(0)
    unittest.main()
