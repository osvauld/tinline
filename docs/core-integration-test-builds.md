# Core integration test build proposal

Branch: `fast-tests-dev`, based on `dev` at `9f3d1533f8720f97b33ec4ceddebb264d9dfd7ea`.
Worktree: `/home/abe/.codex/worktrees/824c/p2p_chat` (created with the worktree MCP).

## Measured results on nixos

| Configuration | Cold build | Warm core rebuild | Full package test run | Module suite wall time |
| --- | --- | --- | --- | --- | --- |
| Release, 37 integration binaries | 51m55s (#389) | 46m53s (#394) | 21m00s (#396) | — |
| A: fasttest, 37 integration binaries | 7m44s (#406) | 1m27s (#413) | — | — |
| A+B: fasttest, one integration binary | 3m21s (#440) | 1m08s (#443) | 7m41s (#444) | 8m03s (plan 3) |

Times above are tasks2 coordinator run durations, rounded to the nearest second. Exact
milliseconds and all job IDs are in the ledger below. Suite wall time is measured from its
build job's start to the last module step's end, rather than summing overlapping jobs.

All p2pcore builds/tests used `tasks2 run --who fast-tests --peer nixos -- ...`, sequentially
except the explicitly parallel suite. No Cargo `-j` or cache-policy override was passed.
Nixos reported 12 cores and 15.5 GiB RAM. Shared sccache stayed enabled. Cold means no compiled
target artifacts for the measured configuration: the new checkout was empty for baseline;
#405 removed release outputs before A; #439 removed fasttest outputs before A+B. Compiler
cache contents were retained, so the cold comparisons include any compiler-cache warming.
These are single observations per configuration, not repeated statistical estimates.

Each warm build invalidated `crates/core/src/lib.rs`. A temporary comment transported the
invalidation through content-based source sync; it is removed from the committed proposal.
This is a warm rebuild after a core edit, not a no-change Cargo invocation. The three build
commands use identical package-level `--no-run` scope, retaining the unit and bindgen harnesses:

```text
cargo test --release -p p2pcore --features dev-tools --no-run
cargo test --profile fasttest -p p2pcore --features dev-tools --no-run
```

The second command was used for both A and A+B. The integration executable count falls from
37 to one; the package-level harness count falls from 39 to three. `[profile.release]` and
all pre-existing workspace Cargo.toml content are preserved; fasttest inherits release and
sets `lto = false`, `codegen-units = 16`, and `incremental = true`.

Reference job #330 from another checkout timed out after 40m00s (exit 143). It is a lower
bound, not a completed baseline. Our baseline started only after #330 was terminal.

## Test comparison and isolation changes

Baseline #396: 69 unit tests passed; integration tests totaled
292 passed, 0 failed,
4 ignored across 37 modules.
A+B #444: all 69 unit tests passed; integration summary 292 passed, 0 failed,
4 ignored. Full-run retries: none.
Suite outcome: 37 steps passed initially; calls failed its known glare test under load and passed alone in retry #486. Raw plan remains failed.
Suite test totals before retry: 291 passed, one failed, four ignored; retry #486 passed.
Validation wall time including the retry: 8m11s.

There are 297 source test annotations including nested group-call capacity tests. The VP8-only
video test is feature-gated and is absent with dev-tools alone, leaving 296 compiled
integration tests. Static review preserved every module's test-name list and cfg/ignore
attributes; no tests were disabled. Existing ignored cases:

- `blocking_acceptance::blocked_contact_is_excluded_from_device_list_notes`
- `blocking_acceptance::personal_block_does_not_change_group_membership`
- `golden_core::dump`
- `group_call::the_next_call_follows_a_rotated_call_host_and_a_revoked_call_host_hands_over`

Fixes required when the former test crates share one executable:

- Process-wide environment overrides now run the exact test in a child invocation of this
  same executable, with variables supplied before startup. The parent checks both successful
  exit and that exactly one test passed. This removes unsafe concurrent environment mutation
  without globally serializing unrelated tests. It covers calls' waiting-ring override,
  endpoint-close's zero close wait, link-expiry's short TTL, and all nine idle-close tests'
  idle/self-sync overrides: 12 test cases and five environment variables across four modules.
  The preserved values are `P2P_WAITING_RING_SECS=4`, `P2P_CLOSE_WAIT_MS=0`,
  `P2P_LINK_TTL_SECS=2`, `P2P_IDLE_CLOSE_MS=2000`, and `P2P_SELF_SYNC_SECS=1`.
- Shared TempDir paths now contain a per-process run nonce and an atomic allocation number.
  Repeated prefix/name pairs cannot clear or remove another live allocation. The old eager
  deletion of a possibly shared path was removed; each instance still cleans up its own path.
- Several modules previously extended separate copies of Peer with duplicate inherent methods.
  Identical refusals/refused_chat/key/dials/devices_of/contact/contacts-dump helpers are now
  defined once. Grant characterization's distinct contacts-plus-blocked diagnostic and its
  assertions are preserved under `grant_dump()`.
- The harness includes support once. Relative paths for nested group-call capacity tests and
  grant characterization's two source includes were adjusted. Golden fixture paths and assets
  remain unchanged.

Tracing already initializes through Once; no extra subscriber workaround was needed. Endpoint
construction already uses dynamically assigned addresses; no hard-coded port fix was needed.
No custom test main or harness=false target was present. The module-private PLACED registry
was already mutex protected. No production behavior was changed.

## Parallel suite

`core-it` has one build and 37 independent module steps, each after build and placed on nixos.
Calls and video_calls are long/light; calls and chat_shards retry failures alone; link is
isolated. These flags live on the steps so the shared `test:it` target does not apply them to
every module. Run it from the worktree root:

```text
tasks2 policy show
tasks2 suite run core-it --who fast-tests --peer nixos
```

In this single observation the suite did not beat the combined full run: its benefit is
module visibility and explicit scheduling/isolation/retry policy.

The direct full Cargo runs above are comparison controls. Baseline runs binaries serially
with parallel tests within each binary. A+B runs tests in one binary with parallel libtest
threads. The suite lets tasks2 schedule multiple module processes and exposes them as UI
subtasks. Explicit --peer nixos also supports installations missing the older step-placement
fix. The installed policy show was checked and displayed all requested flags. Runtime trace
intervals for calls, video_calls, and blocking_acceptance overlap at 16:42:28 UTC, proving
concurrent module execution. During link, the queue explicitly reported its isolate blocker.

Libtest filters are substrings: bare `calls::` also matches `video_calls::`. The user approved
preserving both names and using `--test it -- calls:: --skip video_calls::` for exact calls-only
selection. The calls suite step includes that skip. Final-source check #490 listed exactly
the original 12 calls tests and no video_calls tests after removing the benchmark comment.

## Job ledger and task-manager feedback

| Job | Command/purpose | Outcome | Run time | Run ms |
| --- | --- | --- | --- | --- |
| 389 | baseline cold package release no-run | Passed | 51m55s | 3115037 |
| 394 | baseline warm package release no-run after core lib invalidation | Passed | 46m53s | 2813489 |
| 396 | baseline full package tests | Passed | 21m00s | 1259568 |
| 405 | remove completed baseline release outputs before A cold build | Passed | 0.65s | 652 |
| 406 | A-only cold fasttest package no-run; 37 integration targets unchanged | Passed | 7m44s | 464272 |
| 413 | A-only warm fasttest rebuild after core lib invalidation; 37 targets | Passed | 1m27s | 87331 |
| 416 | approved native tasks2 CLI + daemon update build | Passed | 6m24s | 384423 |
| 429 | A+B cold setup clean | NotStarted | not started | — |
| 439 | user-authorized resubmission of A+B cold setup | Passed | 0.55s | 553 |
| 440 | cargo test --profile fasttest -p p2pcore --features dev-tools --no-run | Passed | 3m21s | 201139 |
| 443 | cargo test --profile fasttest -p p2pcore --features dev-tools --no-run | Passed | 1m08s | 67679 |
| 444 | cargo test --profile fasttest -p p2pcore --features dev-tools --no-fail-fast | Passed | 7m41s | 461454 |
| 448 | core-it/build | Passed | 0.45s | 453 |
| 449 | core-it/calls | Failed | 21s | 21222 |
| 450 | core-it/video_calls | Passed | 19s | 19459 |
| 451 | core-it/accounts | Passed | 3s | 2717 |
| 452 | core-it/blocking_acceptance | Passed | 27s | 26599 |
| 453 | core-it/call_tokens_acceptance | Passed | 26s | 26029 |
| 454 | core-it/chat_shards | Passed | 41s | 41425 |
| 455 | core-it/contact_requests | Passed | 19s | 19053 |
| 456 | core-it/device_list_characterization | Passed | 25s | 25498 |
| 457 | core-it/device_list_push | Passed | 35s | 34917 |
| 458 | core-it/devices | Passed | 8s | 8014 |
| 459 | core-it/endpoint_close | Passed | 19s | 18730 |
| 460 | core-it/fanout | Passed | 30s | 30449 |
| 461 | core-it/gcall | Passed | 12s | 11519 |
| 462 | core-it/golden_core | Passed | 1s | 1254 |
| 463 | core-it/grant_characterization | Passed | 13s | 12521 |
| 464 | core-it/group_call | Passed | 1m59s | 118659 |
| 465 | core-it/group_core | Passed | 39s | 38567 |
| 466 | core-it/group_g2 | Passed | 1m18s | 77617 |
| 467 | core-it/group_g3 | Passed | 37s | 37210 |
| 468 | core-it/group_shards | Passed | 48s | 47521 |
| 469 | core-it/idle_close | Passed | 53s | 53035 |
| 470 | core-it/lifecycle | Passed | 2m37s | 156578 |
| 471 | core-it/link | Passed | 3m44s | 224343 |
| 472 | core-it/link_expiry | Passed | 3m37s | 217393 |
| 473 | core-it/message_receipts | Passed | 3m30s | 210238 |
| 474 | core-it/old_writer_delivery | Passed | 1m57s | 117078 |
| 475 | core-it/private_rotation | Passed | 37s | 36730 |
| 476 | core-it/profile_sync | Passed | 1m05s | 65380 |
| 477 | core-it/public_group | Passed | 34s | 33734 |
| 478 | core-it/readd | Passed | 36s | 36046 |
| 479 | core-it/restore | Passed | 11s | 10980 |
| 480 | core-it/restore_characterization | Passed | 35s | 34661 |
| 481 | core-it/restore_ticket | Passed | 33s | 33072 |
| 482 | core-it/search | Passed | 22s | 21745 |
| 483 | core-it/tickets | Passed | 16s | 16002 |
| 484 | core-it/vault | Passed | 2s | 2204 |
| 485 | core-it/writer_id | Passed | 14s | 14081 |
| 486 | cargo test --profile fasttest -p p2pcore --features dev-tools --test it --no-fail-fast -- --exact calls::glare_leaves_exactly_one_call --test-threads=1 | Passed | 17s | 16888 |
| 490 | final-source compile and exact 12-test calls filter listing | Passed | 44s | 43537 |

#429 did not start: source sync falsely flagged an existing expression in the moved,
untracked grant-characterization test as a secret-named assignment. No worker command ran.
After the user requested resubmission, #439 passed with the source unchanged and without
staging or a guard bypass.

The installed tooling initially lacked step-level long/isolate parsing. Source support was
already merged in task-manager dev. #416 was an explicitly approved native tooling build;
the user performed the actual installation and daemon restart. No p2pcore build/test was
run locally. The original task-manager checkout has no lasting source edits from this task.

Session feedback: `/home/abe/task-manager/feedback/fast-tests-389-build-cost.md`.
The proposal is committed on its branch for review; it has not been merged or pushed.
