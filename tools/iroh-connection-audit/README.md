# Local iroh connection characterization

An isolated test package pinned to iroh 1.3.0, not imported by Tinline. Its lockfile
is intentional reproducibility metadata; build outputs/logs are not tracked.
Feature set and transitive lockfile differ from the application: these are transport
characterization tests, not exact production integration or capacity measurements.

All endpoints use fresh synthetic keys, IPv4 localhost only, no relay transports and
no address lookup. Tests use one Tokio runtime thread and serial dialing. No phone,
public host, external discovery, installation or app data is used.

## Run only these owned targets through the shared queue

From the `sync-connection-audit` worktree:

```sh
python3 /home/abe/p2p_chat/scripts/buildlock.py --status
CARGO_BUILD_JOBS=2 \
CARGO_TARGET_DIR=/home/abe/build-cache/p2p_chat/sync-connection-audit/iroh-audit-target \
python3 /home/abe/p2p_chat/scripts/buildlock.py --who sync-connection-audit -- \
  cargo test --offline --locked \
  --manifest-path tools/iroh-connection-audit/Cargo.toml \
  --test transport -- --test-threads=1 --nocapture
```

Run `--test inventory` as a separate queued batch. Plain `cargo test` lets buildlock
release compilation admission before running tests. Check contention between cases;
do not increase scale merely because a small test passes.

## Results

- Transport: **4 passed, 0 ignored**, 0.12 s execution on latest run.
  - Repeated connect to the same endpoint/ALPN creates distinct QUIC connections.
  - Two bidirectional streams exchange distinct payloads on one connection.
  - Resetting one stream leaves another stream usable on the same connection.
  - Different ALPNs use distinct connections.
  - Each case asserts remote close observation; held handles are closed before
    endpoints. This is not a proof that every allocation was reclaimed.
- Inventory: **1 passed, 0 ignored**, 0.81 s execution. Cases 1, 4 and 16 simultaneous
  connections all remained usable and observed close. Each case has two endpoints
  in one process; the whole test has a thirty-second deadline.
- Initial dependency compilation: 1m11s with two jobs. Subsequent test compilation
  under one second. Latest transport batch waited behind another agent's desktop
  check rather than bypassing the queue.

Single-run inventory observations (Linux localhost, unoptimized test profile):

| Connections per endpoint | Serial dial ms | All echoes ms | Process RSS before/held KiB | Client tx bytes |
|---:|---:|---:|---:|---:|
| 1 | 17 | 1 | 22560 / 25096 | 12340 |
| 4 | 35 | 3 | 27184 / 27448 | 40855 |
| 16 | 109 | 7 | 31648 / 32772 | 154683 |

RSS includes both endpoints, runtime, TLS, transient work and retained allocator
memory, and changes across cases. These samples must NOT be divided into a
per-connection memory claim. Tx includes handshake/control/echo traffic; the
200 ms hold does NOT measure five-second keepalive overhead or an idle byte rate.
No CPU/energy measurements, stressed distinct peers, network switches, sustained
load, relay performance, Android doze tests or maximum-capacity claims are made.

Before production pooling, the application must accept multiple streams, authorize
each group separately, maintain per-stream VVs and queues, and stop closing the
whole connection when a single group session ends. A successful echo test supplies
none of those group-level policies.
