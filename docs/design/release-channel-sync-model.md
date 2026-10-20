# Official host and mobile sync routing — simulation first

Status: proposal with executable offline model, not production integration.
Worktree: `.claude/worktrees/sync-connection-audit`, branch `feature/sync-connection-audit`.
Copied from the release-channel worktree as the starting model for the connection
audit and laptop-forwarding simulations; the original remains untouched.

## User requirements

- A dedicated headless node hosts a public release channel (APK/desktop builds)
  and acts as an authorized, always-on member host for an official group.
- Browser bootstrap downloads use HTTPS; native subscriptions are a separate role.
- Hosting does not grant group administration or permission to forge author proofs.
- Optional group-facing endpoints retain the same account DID, but use separate
  device keys/iroh endpoints from the calling endpoint.
- Mobile has two modes: **Battery saver** and **Direct**.
- Battery saver uses an authenticated reachable own desktop for message/history
  sync. If the desktop app is offline or that link is unreachable, use direct
  connections. Return to desktop sync after successful reconciliation.
- Direct mode uses independent connections even when the desktop is available.
- Cover multiple groups and contacts, not just a single conversation.

This proposes an explicit exception to `docs/group-chat.md`'s current “no service
operated by us” statement. It does not change membership validation, original
proof preservation, or the independence of call hosting. That document and
`docs/chat.md` remain authoritative for existing protocol behavior.

## Model topology and observations

`docs/archive/models/test_sync_routing_model.py` has one phone, one own desktop, one official
host serving two separate groups, and two individual contacts. Identities are
synthetic. Authenticated identity, admitted scope and valid immutable author
records are assumed oracles, not implemented cryptography.

- Battery saver, desktop reachable: phone has one own-device sync session;
  desktop has three upstream sessions (one host, two contacts).
- Direct/fallback: phone has three upstream sessions. An online desktop may still
  maintain its own sessions; direct mode is not a promise of fewer total sessions.
- Two groups multiplex one host connection, but scope authorization stays separate.
- Counts describe modeled logical sessions, not measured sockets, ALPN pooling,
  radio wakes, bytes, or battery savings in iroh/Android.
- Copies preserve the original author record across phone/desktop forwarding.
  Sets model idempotent import and retained local writes, not durable databases.
- If the desktop is reachable but its host connection is down, it retains queued
  updates until that upstream recovers. Per-scope mobile bypass in this condition
  is an open choice; the model does not silently enable it.
- No route available means queued updates, not guaranteed offline delivery.

## Endpoint separation constraints

The official host sees group-facing endpoints and the stable DID. Group-authored
records also reference only the group-facing author endpoint, even when relayed
by a desktop. They must not reference the private call endpoint.

Actual integration must audit attestations, device directories, self-sync exports,
profile/history proofs, succession records, discovery and routing hints. Do not
export private own-device directories to a host. Connecting with a second endpoint
alone does not prove calling-address separation. Same-DID correlation and network
metadata remain visible; this is not anonymity or independent compromise isolation.

The model always enables endpoint separation to test this proposed option. It
contains no real endpoint rotation, registration, revocation, discovery or dialing.
Calls and existing call grants are untouched; routing message sync through a
laptop is not permission to forward calls or mint contact capabilities.

## Covered scenarios

1. Two groups and two contacts sync through one phone-to-desktop session.
2. Desktop sleeps: phone falls back, delivers incoming/outgoing updates, then
   catches the desktop up and returns to battery saver.
3. Desktop online but unreachable: direct fallback.
4. Explicit direct mode and live switch back to battery saver.
5. Phone offline and eventual catch-up.
6. Desktop and host unavailable: local retention and later delivery.
7. Desktop reachable but upstream unavailable: queue and recover.
8. Denied second group cannot inherit authorization from the first group session.
9. Host observes same DID with separate group endpoints, not call endpoints.
10. Twenty seeded schedules of 100 routing/availability transitions: monotonic
    retention, invariant checks, eventual convergence after all links recover.

Failure detection, connection establishment, reconciliation and route handover are
idealized atomic steps. No timeout/backoff/hysteresis value is selected or tested.
Proof rejection, causal dependencies, membership changes, policy/block propagation,
crash persistence, quotas, attachments and partial transfers require additional
models and real integration tests. These results do not prove production behavior.

## Next gates before networking/UI

- Specify authenticated desktop readiness, failure deadlines, backoff and a stable
  recovery window; avoid route flapping and preserve unacknowledged local writes.
- Decide per-scope bypass if the desktop is reachable but an upstream is stalled.
- Specify message forwarding for DMs without replacing existing two-party
  authorization/proof rules; the model does not authorize arbitrary relays.
- Integrate real scope validators, proof-preserving self-sync and separate endpoint
  audiences, including rotation; verify no call endpoint leak in wire captures.
- Test two groups/two contacts on real independent endpoints, partitions, reconnect,
  restart, queue pressure and partial handover before Android integration.
- Measure physical-device screen-off/doze behavior and energy before claiming savings.

Run the lightweight offline model:

```sh
python3 docs/archive/models/test_sync_routing_model.py
```
