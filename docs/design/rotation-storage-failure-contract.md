# Rotation storage boundaries and failure matrix (packet C2)

Status: proposal — not approved

Scope: where the durable commit sits in an endpoint rotation, what each crash or step failure
leaves behind, and what receivers may acknowledge. G-STORAGE and G-RETIREMENT
(`docs/design/rotation-agent-handoff.md:132,134`) stay OPEN. Options are modelled side by side;
nothing here selects one. Every number (interval 10, validity 15, grace 3/100, overlap 3/100)
is a **test-only fixture**, not a product default.

Executable model: `crates/proto/tests/rotation_storage_contracts.rs` (13 tests).
`Claims`/`Permit`/`View` copied from `rotation_time_contracts.rs` @f3cf3ed (`View::live`
omitted). The schedule fields and `fixture_key` are copied from C1 in the same way.

**Not modelled:** fsync, power loss, torn or partial writes, redb transactions, vault sealing,
Android process death, Keystore, real OS clocks and iroh endpoints. A "durable write" is an ideal
atomic struct assignment (`Sender::write`, `rotation_storage_contracts.rs:711`). A crash drops
every volatile field and stops every endpoint of the device (`Sender::crash :718`, `World::crash_device :517`).
Process-crash coverage is not evidence of power-loss durability (same rule as
`docs/group-chat-tests.md:108-109`).

## 1. Verified in source and tests

| Fact | Source |
|---|---|
| The device secret lives only in the sealed vault `{mnemonic, device_secret}` inside `account.json`. A clear `device_public` copy sits in the same file | `crates/core/src/vault.rs:3,27-29`, `crates/core/src/store.rs:23-30,41-46` |
| `account.json` writes go temp file, fsync, rename, then fsync the directory. Each file is atomic on its own | `crates/core/src/store.rs:560-579` |
| The registry, `own_list`, contacts and blocks are in `account.redb`, which is a separate file from `account.json` | `crates/core/src/store.rs:10,212-223`; A1 `rotation-baseline-report.md:87-90,117-119` |
| One endpoint is built per unlock and never rebound. The only key-replacement path today is restore-over-existing, which tombstones the old key | A1 `rotation-baseline-report.md:105,108` (`node.rs:1768-1816`, `node.rs:1273-1293`) |
| The existing lifecycle fixture commits the whole successor in one ideal clone before install/notify. It crash-tests 5 coarse points and has no failure (non-crash) injection | `crates/proto/tests/support/rotation_permit_lifecycle.rs:1-2,91-125` |
| The existing fixture says an unannounced draft may be replaced after a crash, and that overdue install must happen before going online | `rotation_permit_lifecycle.rs:110-118` |
| C1 scheduler persistence is an ideal struct assignment, and the checkpoint frequency is left to C2/G-STORAGE | `docs/design/rotation-time-contract.md` §6 |
| Group persistence vocabulary: durability boundary, "no storage ack before commit", idempotent resend from persisted originals, simulated vs real durability | `docs/group-chat-tests.md:97-109` (S01–S07), `docs/STATUS-group-chat.md:103-112` |

**Finding (from source, not tested):** a production `SealCommit` would need to change the vault
secret and `device_public` in `account.json` and the registry/own list in `account.redb` together.
Today no single atomic write covers both files. Some ordering or a journal is needed. That
choice belongs to G-STORAGE. It is not modelled here.

## 2. Model

- **Sender durable** (`SenderDurable :625`): policy, schedule `{generation, interval,
  last_rotation_at, next_deadline}`, committed `view` + `secret`, signed `history`, optional
  `pending`, optional `retiring`, `retired` set, per-peer `acked`, `contacts`, `blocked`.
- **Sender volatile**: draft key, status (Healthy / Degraded{until} / Unreachable), the old key
  being replaced, and the peers sent to in this tick.
- **Receiver durable**: `view` only. The volatile verify candidate and the message being
  processed are lost on a crash. The inbox stands for the network.
- **World oracle** (`World :440`): online endpoints, an event log (Start/Stop/Retired), `escaped`
  (generation → every key that left the device as a started endpoint or an announced permit),
  `ack_log`, sessions.
- Recovery = `tick` with empty volatile state (`:768-804`, `run :785`). Order: finish durable in-flight work
  (pending/retiring) → if overdue, rotate **before** serving the old key → start the current key →
  re-announce to unacked, non-blocked peers from durable history.
- Fixture keys are `SHA-256(counter)` for determinism only, NOT a derivation proposal
  (G-DISCOVERY unselected).

## 3. Transition table (commit orderings; G-STORAGE candidates)

`k1` = old key (gen 1), `k2` = successor (gen 2). Under both orderings, crash recovery ends at
gen 2. Its whole durable state is byte-equal to a run with no faults (crash test, `:1134`).

### 3a. CommitFirst

| # | Step | Durable write | State after the step | Crash right after this step → recovery |
|---|---|---|---|---|
| 0 | (due) | — | current k1 | before GenerateKey: rotate on restart, k1 never restarted |
| 1 | GenerateKey | none (draft volatile) | draft k2 in RAM | draft lost, nothing escaped. A NEW key is generated (allowed) |
| 2 | SealCommit | ONE write: view/secret/history → k2, schedule, `retiring = {k1, secret, due_at, expires}` | current k2, k1 retiring | resume k2 (no keygen). k1 not restarted |
| 3 | StartEndpoint | none | k2 serving | same as 2 |
| 4 | Announce(peer)* | none (send only) | permit escaped to peer | re-announce from durable history to unacked peers |
| 5 | RetireOld | ONE write: `retired += k1`, `retiring = None` (k1 secret dropped) | k1 retired | finish. k1 never started (I2) |
| 6 | ShutdownOld | none (close endpoint) | k1 offline | — |

### 3b. PendingThenPromote

| # | Step | Durable write | State after the step | Crash right after this step → recovery |
|---|---|---|---|---|
| 1 | GenerateKey | none | draft | new key allowed |
| 2 | SealCommit | ONE write: `pending = {permit k2, secret, due_at}`. View is still k1 | current k1 + pending k2 | resume the SAME pending key. k1 not restarted |
| 3–4 | StartEndpoint, Announce | none | pending escaped | same as 2 |
| 5 | RetireOld | ONE write (promote): view/secret/history → k2, schedule, `retired += k1`, `pending = None` | current k2 | — |
| 6 | ShutdownOld | none | k1 offline | — |

Both orderings use 2 rotation writes + 1 per received ack (`:1645` asserts 4 in total). Both keep
two secrets on disk between steps 2 and 5. They differ in which record names "current" during
that window. Under PendingThenPromote, any reader of `view` other than rotation recovery would see
k1 while k2 is already announced. Which shape fits the real registry/vault split is a G-STORAGE
question.

### 3c. Rejected orderings, kept as counterexamples (KIND: limitation)

- **AnnounceBeforeCommit** (`:1194`): crash after `Announce(bob)` → restart issues a second gen-2
  key → `escaped[2]` has 2 keys, bob rejects with `fork`, bob and dave hold different gen-2
  endpoints, and the sender records bob as acked at gen 2 of the lost key.
- **AckBeforeCommit** (receiver, `:1609`): crash between Ack and Commit → sender records
  `acked=2`, receiver stays at gen 1 on a retired key, and nothing is ever resent.

## 4. Invariants checked after every crash or failure (`assert_invariants :1028`)

| ID | Invariant | Oracle |
|---|---|---|
| I1 | No generation used for two different keys | `escaped[g].len() == 1`. Durable history agrees |
| I2 | Retired key never revived | no `Start(k)` after `Retired(k)`. Retired key never accepts new sessions |
| I3 | Ack only after commit | each `ack_log (peer, g)` ≤ receiver durable generation |
| I4 | Blocked contact never receives an update, across recovery too | `carol.received == 0` |
| I5 | Old key not served past its deadline (+ grace/overlap where a policy grants it) | `assert_served_only_before :1088` |
| I6 | Exactly one successor; durable state equals a fault-free control | `generation == 2`, `bytes == control.bytes()` |

Shared terms with the group harness (S01–S07), so the two harnesses can later share fixtures:
*durability boundary* = `SealCommit` (sender) / `Commit` (receiver). *Ack only after commit* = I3
(S01/S03). *Idempotent retry from persisted originals* = re-announce sends the byte-identical
stored permit (`:1388`, S02/S07). *Simulated atomic snapshot ≠ filesystem durability* = the "not
modelled" note above (STATUS "Next work" item 6).

## 5. Failure matrix (step returns an error; the process keeps running)

| Step | Defined outcome | Durable effect | Test |
|---|---|---|---|
| GenerateKey | error. Draft none. Start-failure policy applies to k1 | **byte-identical**. No escape, no write | `:1225` (invariant) |
| SealCommit (write error) | draft discarded (never escaped). Start-failure policy applies | **byte-identical**. The retry uses a fresh key, which is safe | `:1225` (invariant) |
| StartEndpoint | committed key kept. Retries reuse the SAME key (no regeneration, bytes unchanged across retries) | committed / pending | `:1265`, `:1317` |
| Announce(peer) | that peer stays unacked. Other peers and retirement go ahead. Retry after a crash too, from durable history, byte-identical permit | only ack writes | `:1388` (invariant) |
| RetireOld (write error) | k1 is stopped at once anyway. The durable `retiring`/`pending` is retried next tick or after restart, and k1 is never restarted | unchanged until retry | `:1430` (invariant) |
| ShutdownOld (close error) | escalate to a forced transport stop (**assumed to succeed** in the model) | k1 already retired | `:1430` (invariant) |
| Receiver Verify reject (bad owner sig / wrong possession key) | reject. **Byte-identical**, no ack. Sender retry converges | none | `:1498` (invariant) |
| Receiver Commit (write error / crash before) | no ack. **Byte-identical**. Sender retry | none | `:1498` |
| Receiver Ack (send error / crash before) | committed, no ack. Sender retries. Redelivery leaves bytes unchanged and then acks | commit kept | `:1567` (invariant) |

### Start-failure policy (G-RETIREMENT candidates)

| Option | Behaviour (tested) | Cost |
|---|---|---|
| FailClosed (`:1265`) | k1 stops at the deadline (t=20). Device unreachable even though receivers hold an unexpired k1 binding (expires 25). Retry brings up the committed k2 | outage until the start succeeds |
| DegradedGrace{grace} (`:1317`) | k1 keeps serving with status `Degraded{until}`, `until = min(due_at + grace, k1 signed expiry)` from DURABLE fields. A retry never extends it. A crash inside the window does not restart k1 | old key exposed up to `grace` longer. The UI must show it |

### Active sessions at retirement (G-RETIREMENT candidates)

| Option | Behaviour (tested) |
|---|---|
| Terminate (`:1714`) | at ShutdownOld every session on k1 closes (`retired`). Continuing means a NEW session on k2, so there is a gap |
| Bounded{max} (`:1739`) | k1 refuses new sessions (`draining`). Existing sessions run until `min(retire + max, k1 signed expiry)`, then are cut (`overlap_cap`). A crash in the window kills them and k1 is not restarted |

Under both options a call longer than the window is interrupted. **No uninterrupted-call claim**
(`assert_no_session_spans :1703`).

## 6. Tests (`cargo test -p proto --test rotation_storage_contracts`)

| Test | KIND |
|---|---|
| `crash_before_or_after_every_sender_step_recovers_without_reuse_revival_or_extension` (2 orders × 7 steps × before/after = 28 crash runs) | invariant |
| `announce_before_commit_ordering_reuses_a_generation_for_two_keys_after_crash` | limitation |
| `failures_before_the_durable_commit_change_nothing_and_leak_nothing` | invariant |
| `start_endpoint_failure_fail_closed_stops_old_at_deadline_and_retries_the_same_key` | candidate-policy G-RETIREMENT |
| `start_endpoint_failure_degraded_grace_keeps_old_visibly_until_bounded_grace` | candidate-policy G-RETIREMENT |
| `announce_failure_to_one_peer_is_retried_from_the_persisted_original_and_isolated` | invariant |
| `retire_and_shutdown_failures_never_let_the_old_endpoint_serve_again` | invariant |
| `receiver_crash_or_failure_before_commit_sends_no_ack_and_leaves_state_unchanged` | invariant |
| `receiver_crash_or_ack_failure_after_commit_keeps_update_and_redelivery_is_idempotent` | invariant |
| `ack_before_commit_ordering_acknowledges_an_update_that_a_crash_then_loses` | limitation |
| `pending_or_committed_next_key_is_persisted_before_announce_and_restart_resumes_it` | candidate-policy G-STORAGE |
| `terminate_policy_interrupts_active_old_sessions_at_retirement` | candidate-policy G-RETIREMENT |
| `bounded_overlap_drains_existing_sessions_until_cap_and_long_calls_are_still_cut` | candidate-policy G-RETIREMENT |

Oracle sanity check (temporary mutations, not committed): (1) serving the overdue current key
before rotating → crash test fails. (2) discarding a persisted pending key on restart → 3 tests
fail.

## 7. What this does NOT prove

- No fsync, power loss, torn writes, redb/JSON transactions, vault re-seal, cross-file atomicity
  (§1 finding), Android process death or Keystore behaviour. No disk-full or corruption recovery
  (group S04 analogue not modelled).
- A crash happens only before or after a whole step. There is no crash in the middle of a write.
  Every write is atomic by construction.
- Forced shutdown after a close error is assumed to succeed. A real stuck socket is not modelled.
- Status (Degraded/Unreachable) is volatile and has no UI. Timer wake-up at `until` is assumed
  (C1 L3 covers clocks that pause in suspend).
- The announcement proof is a fixture over (peer, head id), not the C3 challenge design. Receiver
  prefix filtering stands in for B1 paging/cursors.
- Single sender device, one endpoint per device. No linked-device races, forks (B3) or group
  delegation. Blocking only shows exclusion from updates.
- The `acked` map is durable per sender. Whether production must persist it (vs. resending to
  everyone after a restart) is open.

## 8. Questions for the user (G-STORAGE / G-RETIREMENT)

1. **Commit shape.** CommitFirst (successor becomes current in one write, old key in `retiring`)
   or PendingThenPromote (two-phase pending record)? Both pass the invariants. Which matches the
   vault (`account.json`) + registry (`account.redb`) split, and is a journal acceptable for
   cross-file atomicity?
2. **Start failure.** FailClosed (outage) or DegradedGrace (old key stays visible for ≤ grace,
   capped by its signed expiry)? What grace, and what does the user see?
3. **Active sessions.** Terminate at retirement, or Bounded overlap (no new sessions, cap = min(max,
   signed expiry))? Is a dropped/re-established call at rotation acceptable product behaviour?
4. **Old secret retention.** Both orderings keep the old secret on disk until RetireOld succeeds.
   Must it be zeroized/re-sealed earlier? What if RetireOld keeps failing?
5. **Ack persistence.** Must per-peer acked generations be durable, or can a restart re-announce
   to all contacts (idempotent, more traffic)?
6. **Close failure.** Is "force transport restart" an acceptable escalation when closing the old
   endpoint fails?

## 9. Interface notes for the coordinator

- No shared files changed. If one ordering is adopted, `support/rotation_permit_lifecycle.rs`
  `DurableDevice::prepare` (`:23-35`) matches CommitFirst minus the `retiring` record. Its crash
  test (`:91-125`) could reuse this packet's step names and `assert_invariants`.
- A group persistence harness (G-C) could share `World`-style oracles: durability boundary, ack
  log vs durable state, escaped originals.
