# Rotation time contract (packet C1)

Status: proposal — not approved

Scope: semantics of the rotation timer, interval configuration, clocks, overdue
resume, reboot and signed validity. G-TIME and G-RENEWAL
(`docs/design/rotation-agent-handoff.md:129-130`) remain OPEN. Options are modelled
side by side; nothing here selects a default. Every numeric value below is a
**test-only fixture**, not a product default.

Executable model: `design-models:crates/proto/tests/rotation_time_contracts.rs` (19 tests).
Fixtures `Claims`/`Permit`/`View` are copied from
`design-models:crates/proto/tests/rotation_permit_model.rs` @01ee106 (only change: `Serialize` on
`View`).

## 1. Verified in source/tests

| Fact | Source |
|---|---|
| Existing overdue candidate: one successor on resume, next deadline = resume + interval | `design-models:crates/proto/tests/support/rotation_permit_lifecycle.rs:32-33`, `docs/design/rotation-agent-handoff.md:107` |
| Existing lifecycle fixture uses a single explicit monotonic fake time; no wall clock, no reconfiguration | `design-models:crates/proto/tests/support/rotation_permit_lifecycle.rs:10-11,23-24` |
| Python network model schedules on an aligned grid of a single global clock (`due = now // interval`) and notes "no wall-clock rollback in this model" | `design-models:scripts/test_rotation_network_model.py:103-105` |
| Receivers reject `issued_at > now` with zero skew tolerance | `design-models:crates/proto/tests/rotation_permit_model.rs:82` |
| Liveness requires `now < expires_at` (wall time at the receiver) | `design-models:crates/proto/tests/rotation_permit_model.rs:110` |
| Claims require a changed replacement ID; same-ID re-signing is rejected | `design-models:crates/proto/tests/rotation_permit_model.rs:95` |
| Required invariants: rotation without blocks, block does not reset deadline; wall clock cannot roll back generation or extend reachability indefinitely | `docs/design/endpoint-rotation-implementation-plan.md:78-80` |
| Android cannot run timers while suspended; enforce overdue retirement on resume | `docs/design/endpoint-rotation-implementation-plan.md:157-159` |

## 2. Model (test-only)

- Two fake clocks (`Clocks` in the test file): `monotonic` resets to 0 on reboot and
  never decreases within a boot; `wall` can be set arbitrarily. A flag selects whether
  monotonic advances during suspend (BOOTTIME-like) or pauses (CLOCK_MONOTONIC-like).
- Durable scheduler state `{generation, interval, elapsed, last_rotation_at,
  next_deadline, wall_checkpoint, view, secret}`; volatile `{boot, mono_checkpoint}`.
  Block policy is a separate store.
- **Scheduler time is `elapsed`**: accumulated monotonic time plus policy-credited
  reboot downtime. It never decreases. Wall time is used only for signed
  `issued_at/expires_at` and for estimating downtime across a reboot.
- `sync` / `poll` / `set_interval` are atomic: on error the durable bytes are
  unchanged (`rotation_time_contracts.rs:341,382,442`).
- Fixture endpoint keys are SHA-256 of a seed and generation **for test determinism
  only**. This is not a derivation proposal (G-DISCOVERY is unselected).

## 3. Proposed semantics table

| # | Topic | Invariant (any acceptable policy) | Options modelled (G-TIME / G-RENEWAL) | Test-only params |
|---|---|---|---|---|
| T1 | Deadline vs blocking | Block/unblock never touches schedule state; rotation happens with no blocks | — | interval 10 |
| T2 | Invalid interval input | 0, below min, above max, arithmetic overflow rejected; state byte-for-byte unchanged | — | min 5, max 100 (and max = u64::MAX for overflow) |
| T3 | Interval change effective time | Must not allow indefinite postponement (see limitation L1) | (a) FromNow: `now + new`; (b) FromLastRotation: `last + new`, floored at `now + min_floor`, shortening never later than current deadline | min_floor 3 (and 0 for jitter) |
| T4 | Jitter | Deadline stays in a bounded window; independent of blocks | Optional deterministic per-rotation offset (only a fixture hook) | offsets 0..=3 |
| T5 | Overdue resume / restart (k missed periods) | Exactly one successor, no fabricated generations, second poll does not rotate | (a) ResumePlusInterval: `resume + interval` (existing candidate); (b) AlignedGrid: next grid point after now | k = 0..4 |
| T6 | Wall rollback / jump while running | Scheduler driven by elapsed: rotations exactly every interval; generation never decreases | — | rollback 45 & 2000, jump +1000 |
| T7 | Reboot (monotonic lost) | `elapsed`, generation never decrease; uptime always counted | (a) CreditNonNegative: credit `max(wall delta, mono since boot)`, rollback credits only uptime; (b) OverdueOnRollback{skew}: shortfall > skew forces immediate rotation | skew 5 |
| T8 | Signed validity | Validity is wall-time at the receiver; scheduler time is separate | — | validity 15 |
| T9 | Rotation failure > validity | Failure commits nothing; old binding expires at signed validity — explicit unreachability, not silent extension | (a) fail-closed, no renewal (tested); (b) renew-same-endpoint: not expressible in copied claims (L5) | validity 15, 10, 8 |

## 4. Tests (`cargo test -p proto --test rotation_time_contracts`)

| Test | KIND |
|---|---|
| `no_blocks_rotates_at_each_deadline_and_block_unblock_never_moves_deadline` | invariant |
| `invalid_interval_inputs_are_rejected_byte_for_byte_unchanged` | invariant |
| `monotonic_regression_within_one_boot_is_rejected_unchanged` | invariant |
| `interval_change_apply_from_now_shorter_and_longer_near_deadline` | candidate-policy G-TIME |
| `apply_from_now_lets_repeated_reconfiguration_postpone_rotation_indefinitely` | limitation |
| `interval_change_from_last_rotation_with_min_floor_cannot_postpone_rotation` | candidate-policy G-TIME |
| `seeded_jitter_keeps_deadline_in_bounded_window_and_independent_of_blocks` | candidate-policy G-TIME |
| `overdue_resume_plus_interval_one_successor_for_sleep_and_restart` | candidate-policy G-TIME |
| `overdue_aligned_grid_one_successor_for_sleep_and_restart` | candidate-policy G-TIME |
| `monotonic_source_that_pauses_in_suspend_misses_overdue_rotation` | limitation |
| `elapsed_scheduler_ignores_wall_rollback_and_jump_and_generation_never_decreases` | invariant |
| `wall_only_scheduler_extends_old_endpoint_on_rollback_and_fabricates_generations_on_jump` | limitation |
| `signed_validity_uses_sender_wall_time_so_skewed_sender_becomes_unreachable` | limitation |
| `reboot_credit_non_negative_counts_forward_downtime_and_ignores_rollback` | candidate-policy G-TIME |
| `reboot_rollback_beyond_skew_forces_immediate_rotation` | candidate-policy G-TIME |
| `repeated_reboots_with_rollback_never_decrease_elapsed_or_generation` | invariant |
| `fail_closed_rotation_failure_beyond_validity_expires_old_binding_unreachable` | candidate-policy G-RENEWAL |
| `validity_shorter_than_interval_leaves_gaps_even_when_rotation_succeeds` | candidate-policy G-RENEWAL |
| `renewing_same_endpoint_is_not_expressible_in_copied_permit_claims` | limitation |

## 5. Findings (limitations demonstrated by passing tests)

- **L1 FromNow is postponable.** Re-saving the interval (even unchanged) before each
  deadline kept generation 0 across 45 intervals. FromLastRotation with "shortening
  never later" kept the cadence identical to a never-reconfigured control device.
- **L2 Wall-only scheduling breaks invariant 11.** A `last + interval` wall scheduler
  never rotates under repeated rollback; one rollback of 45 kept the old endpoint 55
  units (interval 10). The grid scheduler style of
  `design-models:scripts/test_rotation_network_model.py:105` jumps generation 1 -> 6 on a forward
  jump of 50 (numbers 2..=5 never had keys) and stalls 55 units after rollback.
- **L3 Suspend-paused monotonic source.** If `elapsed` is fed by a clock that stops
  in suspend, a 3.5-interval sleep is not overdue on resume. Receivers still bound
  the old binding by signed wall validity, so the result is unreachability, not
  indefinite extension. Production needs a clock that counts suspend
  (e.g. Android `elapsedRealtime`/Linux `CLOCK_BOOTTIME`) — **unverified on device**.
- **L4 Sender wall skew vs zero receiver tolerance.** A sender wall clock 1 unit
  ahead produces updates rejected (`validity`, receiver unchanged); a sender 40 units
  behind (> validity 15) produces bindings that advance history but are expired on
  arrival. Correct scheduling does not imply reachability.
- **L5 Renewal format absent.** Re-signing the same endpoint at generation + 1 is
  rejected (`unchanged_id`). Renewal-without-rotation needs a G-RENEWAL decision.
- Reboot with wall rollback under CreditNonNegative does not count the downtime, so
  the local schedule runs late by that downtime; uptime is still counted, and
  receivers are bounded by signed validity. OverdueOnRollback trades this for an
  extra rotation whenever the rollback exceeds the skew tolerance.
- Validity < interval leaves unreachable gaps every period even with healthy
  rotation (validity 8, interval 10: 6 unreachable units over 3 periods).

## 6. What this does NOT prove

- No real OS clocks, Android Doze/AlarmManager/WorkManager behaviour, NTP, RTC loss,
  or measured wake latency. Suspend semantics are a flag, not a measurement.
- Persistence is an ideal atomic struct assignment; the checkpoint is written on
  every `sync`. Real checkpoint frequency (and lost uptime between checkpoints) and
  power-loss atomicity belong to C2/G-STORAGE.
- Receivers are modelled with a correct wall clock and zero skew tolerance.
- Jitter is a test hook writing `next_deadline`, not an implemented scheduler path.
- No networking, notification, retirement-of-sessions (G-RETIREMENT) or block
  delivery semantics; block tests only show schedule independence.
- Identities come from `identity::generate()` (OS randomness); assertions do not
  depend on key values.

## 7. Questions for the user (G-TIME / G-RENEWAL)

1. **Default and minimum interval.** What default rotation interval, and what
   minimum/maximum may users configure? (Battery/network cost vs exposure window.)
2. **Interval change semantics.** FromLastRotation-with-floor (non-postponable) or
   FromNow (simpler, but postponable — L1)? What minimum floor after a shortening?
3. **Jitter.** Should deadlines be jittered? If so, width, and must the jittered
   deadline be persisted so a restart cannot re-roll it?
4. **Overdue policy.** On resume after k missed periods: ResumePlusInterval (existing
   candidate) or AlignedGrid (keeps cadence, next rotation may come sooner)?
   Both create exactly one successor; is that agreed?
5. **Skew tolerance.** Receiver tolerance for `issued_at` in the future (currently 0),
   and reboot rollback tolerance before forcing rotation (fixture 5). Should a
   detected rollback on reboot force rotation (OverdueOnRollback) or only credit
   uptime (CreditNonNegative)?
6. **Validity vs interval.** Signed validity = interval + grace? How much grace?
   Validity < interval guarantees periodic unreachability.
7. **Renewal (G-RENEWAL).** When rotation fails past validity: fail closed and become
   unreachable (tested option), or define a renewal record (needs a claims change;
   L5)? Should the user see a degraded state?
8. **Clock source.** Confirm production elapsed accounting must use a suspend-counting
   clock, with a device measurement task (L3).

## 8. Interface notes for the coordinator

- No shared files changed. If adopted, the scheduler should keep wall time out of
  scheduling and feed `elapsed` from a suspend-counting monotonic source; the
  existing lifecycle fixture (`support/rotation_permit_lifecycle.rs:23-33`) could
  adopt `elapsed` terminology when consolidated.
