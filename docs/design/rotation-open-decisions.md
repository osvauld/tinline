# Rotation + group: decisions needed from the user

Status: coordinator digest, 2026-10-10. Collects the policy questions raised by the Wave 1
packets. Nothing here is decided. "Coordinator lean" lines are recommendations to evaluate,
not selections. Source documents hold the evidence and tests; read them before answering.

Packets merged so far: A1, A2, B1, C1, D1, D2, E1, G-A. Still running/queued: B2, B3, C2, C3,
K1, G-H, G-B, G-C (their questions get appended when they land).

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

## 4. Product semantics
Sources: `rotation-baseline-report.md` §4–5; `rotation-author-key-trace.md` §6.

1. A contact's new device key resets their `verified` flag today. Should verification survive
   an authenticated rotation (safety number is DID-only)? Otherwise every rotation clears it.
2. Contact removal currently also blocks the DID. Keep removal and block as one action, or split?
3. Where should blocked contacts be filtered from address updates (nothing filters on the
   sending side today)?

## 5. Possible bugs in the shipped product (no rotation needed)
Source: A1 O1–O5, D1 §7. Restore-over-existing already changes the device key.
- Cached contact ticket is reused after the device key changes and cannot be redeemed.
- Unacked messages from before restore may be stranded; ticks may be wrong; a contact's other
  devices may fail to verify our pre-restore history once our old key drops off our list.
K1 is writing characterization tests to confirm or refute these. Decision after K1: fix now
on `dev` (separately from rotation) or defer?

## 6. Group chat (D01–D06)
G-B (candidate oracles for D01–D05) has not run yet. D06 is covered by B3 (running).
