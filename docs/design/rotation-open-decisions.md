# Rotation + group: decisions needed from the user

Status: coordinator digest, 2026-10-10. Collects the policy questions raised by the Wave 1
packets. Nothing here is decided. "Coordinator lean" lines are recommendations to evaluate,
not selections. Source documents hold the evidence and tests; read them before answering.

Packets merged so far: A1, A2, B1, B2, B3, C1, C2, D1, D2, E1, G-A, K1. Still running/queued: C3,
G-H, G-B, G-C (their questions get appended when they land).

## 1. Highest leverage (unblock the most work)

### Q1 — Discovery baseline (G-DISCOVERY, G-AUTHORITY)
Source: `rotation-linked-discovery-options.md` §4, §10; `scripts/test_rotation_linked_discovery_options.py`.

Option I = independent random endpoints, authenticated updates to reachable peers, explicit
recovery when both sides were offline across a rotation. Options P (shared prediction) and R
(rendezvous store) add offline discovery but: under every option a hostile holder of the
recovery phrase who reaches any still-live old address regains discovery; mnemonic-rooted
variants make the phrase a permanent discovery key; prediction hides clones; a public forward
pointer leaks new addresses to blocked contacts.

E1's 8 yes/no questions (§10). The key one: **ship option I first, with offline discovery
decided later?** Coordinator lean: yes; also answer Q6 (rule out forward pointers) = yes.

### Q2 — Author key vs transport key (G-KEYS)
Source: `rotation-author-group-adapter.md` §3.1 table; `rotation-author-key-trace.md`.

Today one device key does ~10 jobs (QUIC identity, chat batch and per-message signatures,
self-sync, Loro peer ID, ...). Options:
- (i) a separate long-lived author key per device path; transport key rotates alone.
  Rotation then touches only networking; old messages stay verifiable with one key per path;
  storage ~2 KB/contact over 5 years. Clone = long-lived Loro peer collision (G-FORK).
- (ii) author key rotates with the transport key, history retained. Every rotation strands
  unacked ops, breaks ticks unless `mc/` schema changes, grows the account-doc VV, breaks
  group-sim's `previous` chain, and needs ~0.8–2.4 MB/contact over 5 years (daily rotation).
Restore already changes the key today, so the restore fixes (stranded ops, ticks, retained
historical keys) are needed under BOTH options.
Coordinator lean: (i). Needs group-side agreement on the author/writer-ID mapping.

### Q3 — History: strict chain vs checkpoint (G-HISTORY)
Source: `rotation-history-policy-options.md`; `crates/proto/tests/rotation_history_paging.rs`.

For 1000 rotations: strict = 1001 signature checks / 1000 records / ~776 KB; checkpoint =
43 / 42 / ~32 KB. Checkpoints silently miss a fork or revoked record between the receiver's
head and the checkpoint, and **cannot satisfy the current group contract** ("full ancestry
since nominated root"). Strict needs senders to retain history back to the oldest offline
contact (unbounded). Options: strict everywhere / checkpoint everywhere (group contract
changes) / hybrid: strict for group audiences, checkpoint for private.
Coordinator lean: depends on Q2 — under (i) routing history no longer carries authorship, so
a bounded/checkpointed private history becomes much less risky. Decide Q2 first.

### Q4 — Forks and revocation (G-FORK, G-REVOCATION, G-AUTHORITY)
Source: `rotation-fork-revocation-options.md` §4–§8; `crates/proto/tests/rotation_fork_revocation.rs`.

Tested facts: forks are always detected when receivers retain per-record evidence, never
resolved by arrival order. Under the shared mnemonic **no** option excludes a hostile linked
device: it can sign successors, "recoveries", and even revoke the owner's records (DoS).
A separate recovery key (needs G-AUTHORITY change) stops hostile revoke/recover but not
hostile DID-signed successors. A checkpoint at a higher generation from a clone silently
replaces the honest one; strict history flags it.
1. On a fork: freeze the path (any mnemonic holder can then DoS it) or keep the first-accepted
   head (race)? Who signs a resolution?
2. Revocation: (a) cascade — descendants suspended until owner-signed recovery; or (b) block
   only that record's endpoint, descendants stand. Is revoking the anchor a rotation or a
   group-nomination operation? Under (a), what happens to group proofs holding revoked
   descendants (group gate D06)?
3. Keep the shared-mnemonic model and document "only a new identity truly cuts off a device",
   or open a separate-authority design? (Matches E1 Q1.)
4. Must receivers retain per-record evidence for every audience (needed to detect older forks)?

### Q5 — Wire/input bounds and ID collisions (G-HISTORY, G-FORK)
Source: `crates/proto/tests/rotation_history_adversarial.rs` (B2; analysis summarised here).

Tested facts: all delivery orders/duplicates converge; splices, malformed frames, forged and
private-on-group input are rejected with state unchanged and before signature work where
possible; a lost cursor yields explicit `RecoveryRequired` and never lowers the high-water mark.
Path-local checks cannot see replacement-ID collisions with siblings or pruned history; a
registry-level ID index catches all of them but grows one entry per rotation forever.
1. Byte cap: per-record (also caps owner/audience string length) or per-page (forces byte-aware
   paging)? Values?
2. Pending-queue drop policy: refuse new arrivals (a mnemonic clone can occupy slots first) or
   evict oldest (a clone flood evicts honest pages); or evict farthest from cursor? Bound per
   path, per DID, or global?
3. After `RecoveryRequired`: full re-verify, checkpoint, or re-nomination?
4. ID collisions: reject or flag; registry scope per (owner, audience) or per owner; forbid
   endpoint reuse across paths; how long to keep the ID index?
5. Wire encoding: sign received bytes, or require canonical encoding? (Pretty-printed JSON
   currently maps to the same signed claims.)

## 2. Timing and renewal (G-TIME, G-RENEWAL)
Source: `rotation-time-contract.md` §7; `crates/proto/tests/rotation_time_contracts.rs`.

1. Default / min / max rotation interval.
2. Interval change: FromLastRotation with floor (tested: cannot be postponed) vs FromNow
   (tested: re-saving before each deadline postpones rotation forever).
   Coordinator lean: FromLastRotation with floor.
3. Jitter: yes/no, width, persisted?
4. Overdue on resume: one successor, then ResumePlusInterval vs AlignedGrid.
5. Clock skew tolerance for signed `issued_at` (currently 0: a sender 1 unit fast is rejected);
   should a clock rollback detected on reboot force rotation?
6. Signed validity = interval + grace; how much grace? (validity < interval = periodic gaps)
7. Rotation failing past validity: fail closed (unreachable) vs a renewal record (claims change);
   show a degraded state?
8. Confirm a suspend-counting clock (Android `elapsedRealtime`/BOOTTIME) and a device
   measurement task. Wall-clock-only scheduling was shown to break invariant 11.

## 3. Lifecycle / retirement (G-RETIREMENT, G-STORAGE)
Source: `rotation-lifecycle-hooks.md` §6–7.

1. Retirement: close the old endpoint immediately vs bounded overlap. Calls, waiting calls,
   transfers, link runs and chat sessions are listed with today's behaviour in §5.
2. Android never calls `lock()` or `stop()` today and core has no resume hook. Add a
   `resumed()` entry point + Android hooks (onStartCommand, network callback, ON_RESUME)?
3. Rotating while unlocked needs a new vault "re-seal under existing data key" function;
   otherwise the remembered Keystore unlock key breaks. Accept adding it?

## 3b. Storage ordering and failure (G-STORAGE, G-RETIREMENT)
Source: `rotation-storage-failure-contract.md` §8; `crates/proto/tests/rotation_storage_contracts.rs`.

Tested: crashing before/after every sender step never reuses a generation, never revives a
retired key, never serves the old key past its deadline. Announcing before commit (two keys for
one generation) and acking before commit (lost update) are shown as broken orderings.
Source finding: a real rotation commit would span `account.json` (vault secret, device_public)
and `account.redb` (registry); no single atomic write covers both today.
1. CommitFirst vs PendingThenPromote (two-phase)? Is a small journal acceptable for cross-file
   atomicity?
2. Replacement endpoint fails to start: FailClosed (outage) vs DegradedGrace (old key serves until
   min(deadline + grace, signed expiry), shown to the user)? What grace?
3. Open sessions at retirement: Terminate vs bounded overlap. Either way a long call is cut —
   is a dropped call at rotation acceptable for v1? (Coordinator lean: yes for v1, schedule
   rotation to avoid active calls if cheap.)
4. Wipe the old secret earlier than RetireOld? What if RetireOld keeps failing?
5. Must per-peer acked generations be durable?
6. Forced transport restart acceptable when closing the old endpoint fails?

## 4. Product semantics
Sources: `rotation-baseline-report.md` §4–5; `rotation-author-key-trace.md` §6.

1. A contact's new device key resets their `verified` flag today. Should verification survive
   an authenticated rotation (safety number is DID-only)? Otherwise every rotation clears it.
2. Contact removal currently also blocks the DID. Keep removal and block as one action, or split?
3. Where should blocked contacts be filtered from address updates (nothing filters on the
   sending side today)?

## 5. CONFIRMED bugs in the shipped product (no rotation needed)
Source: `crates/core/tests/rotation_restore_characterization.rs` (K1; 8 tests, real iroh,
`// KIND: limitation` tests assert today's wrong behaviour). Restore-from-phrase on an existing
install already changes the device key, and:
1. **Chat stall (worst):** if a contact holds our signed device list and we had an unacked
   message at restore time, the contact rejects our history ("signature does not verify") and
   NO further chat from us reaches them (pending and new messages). Root cause: the contact's
   device list for us still names only the old key and is not updated from the live session.
2. **Dead ticket:** after restore (same display name) `my_ticket()` hands out the cached ticket
   naming the old key; redeeming it times out and creates no contact.
3. **Contact's other device** rejects our pre-restore messages relayed by its sibling once it
   learned our new device list.
4. **Ticks:** pre-restore messages go Delivered → Pending after restore.
5. **Verified flag** cleared on the contact's side (safety numbers unchanged — DID-only).
Not tested: O5 (old self entry tombstoned on restore).
Decision: fix 1–4 now on `dev` as a separate bugfix task (independent of rotation design), or
defer? Coordinator lean: fix 1 and 2 now; they are small, user-visible and rotation would make
them happen constantly. 3–5 depend on G-KEYS/G-HISTORY and the verified-flag policy (§4.1).
Also: should a DID-attested key seen on a live session be accepted for verifying history when
the stored device list lacks it? (root of bug 1)

## 6. Group chat (D01–D06)
G-B (candidate oracles for D01–D05) runs after G-H. D06's rotation side is Q4 above.
