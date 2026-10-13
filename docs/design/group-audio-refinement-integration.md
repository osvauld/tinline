# Production group audio refinement / dev integration

User approved production audio improvements followed by merge to `dev`. Work is
isolated on `fix/group-audio-refinement` in
`.claude/worktreess/group-audio-refinement`. Initial base `2111352`; updated through
`cb2ed83` and `0030b17` as the coordinator merged video/search work. Existing
capacity experiments and native Cloud/motion UI stay in their original worktree,
uncommitted and excluded. No push, installation or personal profile changes.

## Owned scope

No parallel edits delegated. Own only core host selection timing, client mixing,
the audio mixer module/oracle, deterministic regressions, authenticated capacity
integration tests, protocol wire-bound tests and this report. Other agents' root
index/video/search work was preserved. Stage only these named paths, never `git add .`.

No participant/admission cap, roster schema, simultaneous-speaker, hysteresis,
dominance, replay retention, group membership or device policy changes.

## Confirmed production improvements

### Fresh arrivals survive scheduler stalls

Before the fix, `HostEngine::tick` consumed newly accumulated levels in the first
historical frame and then processed empty overdue frames. Virtual-time regressions
failed at a 400ms stall and discarded a newly arriving provisional speaker.
`stall-red.log`: 2 failed / 1 passed, before production edits.

Both accepted datagrams and ticks now advance the same bounded selection clock.
An arrival settles strictly older frames **before** adding its current observation;
a tick can include its current boundary. Existing 50-frame catch-up bound remains.
Tests cover 400/1000/2000ms stalls, stale speaker expiry, fresh/provisional recovery,
silence, actual forwarding and exact-boundary admission without an extra frame.
Interim actual-engine verification: 4 passed (`stall-interim-green.log`).

### Remove per-slot heap work from decoded mixing

Previously every 20ms mix allocated boxed PCM per receiver and growable frame / 
reference vectors. A thread-local Rust allocator audit using three actual Opus
sources measured **450 allocations over 90 warmed decoded frames** (5/frame),
failing the new zero-allocation assertion (`mix-red.log`). This is not a C/Opus
allocator audit or a whole audio pipeline allocation claim.

The client now uses one 960-sample scratch frame and one i32 sum on the stack
(5760 bytes total), then applies the **identical** existing equal-gain soft limiter.
No receiver-count-sized buffer or persistent client state is added. The signed
roster bounds source count, so i32 accumulation is safe. All receivers are pulled,
even when muted; the full-frame overwrite contract makes scratch reuse safe.
Decoded mute/unmute testing checks silence while muted and bit-exact alignment
with an always-pulled reference after unmute, preventing stale audio replay.
The allocation assertion passes in the 26-test core run. Independent numerical
oracles cover 1/3/12 speakers, knee edges, extremes and empty input.

## Validation

Logs: `artifacts/group-audio-refinement/` (not staged).

On `cb2ed83` plus these audio changes:
- `core-green.log`: 26 actual gcall engine tests passed, including decoded allocation
  and mute-clock regressions.
- `audio-green.log`: all 31 codec/mixer tests passed.
- `wire-green.log`: 3 passed / 1 deliberately ignored candidate; signed 12-member
  roster body 3763 bytes. Current semantic/4096-byte transport rejects 16/24/32.
- `nodes-green.log`: 2 authenticated real-node capacity/recovery tests passed in
  112.23s: 12-member room rejects extra without disruption and recovers capacity;
  a speaker recovers after previously selected speakers go silent.
- Dependency-free actual mixing-module numerical oracle: 2 passed.

Updated-dev verification on `0030b17`:
- `final-core.log`: 26 passed, including zero Rust allocations in the warmed mix
  and exact decoded mute/unmute clock alignment.
- `final-nodes.log`: 2 authenticated capacity/recovery tests passed, 107.95s.
- Existing real-node group audio, phone degradation/handover and concurrent-host
  glare tests, `policy-green.log`: 3 passed, 18.47s.
- `final-wire.log`: 3 passed / 1 deliberately ignored candidate, 0.15s.

Total targeted Cargo validation: 65 passed / 1 deliberately ignored protocol
candidate. All final checks passed before scoped commit/merge; the baseline
video/search commits are included, and the root index was clean before integration.

Exact commands, from this worktree (heavy commands use the absolute buildlock):

```sh
export CARGO_TARGET_DIR=/home/abe/build-cache/p2p_chat/group-audio-capacity/target
export CARGO_BUILD_JOBS=2
python3 /home/abe/p2p_chat/scripts/buildlock.py --who group-audio-final-core -- cargo test --release --locked -p p2pcore --lib gcall -- --test-threads=1
python3 /home/abe/p2p_chat/scripts/buildlock.py --who group-audio-codec-green -- cargo test --release --locked -p audio --lib -- --test-threads=1
python3 /home/abe/p2p_chat/scripts/buildlock.py --who group-audio-wire-green -- cargo test --release --locked -p proto --test gcall_capacity_bounds -- --nocapture --test-threads=1
python3 /home/abe/p2p_chat/scripts/buildlock.py --who group-audio-final-nodes -- cargo test --release --locked -p p2pcore --test group_call capacity_ -- --nocapture --test-threads=1
python3 /home/abe/p2p_chat/scripts/buildlock.py --who group-audio-policy-green -- cargo test --release --locked -p p2pcore --test gcall -- --test-threads=1
rustc --edition 2024 -O --test tools/group-audio-refinement/mix_oracle.rs -o artifacts/group-audio-refinement/mix-oracle
artifacts/group-audio-refinement/mix-oracle
```

## Limits / follow-up

These are production fixes, not merely benchmark/UI changes. They do not prove
32-person production capacity, fix every speech-coverage metric or explain all
synthetic RSS/bandwidth anomalies. Receivers are created lazily on first valid
slot datagram, then retained until the roster changes; aggregate RSS alone is not
a product-leak diagnosis. Replay/join tracking cannot be pruned casually without
security policy. Larger signed rosters and bounded adaptive admission remain
protocol/product gates. No phone/GPU/battery or physical microphone validation
is claimed. Rendering, FFT, descriptors and file I/O were not added to audio paths.
