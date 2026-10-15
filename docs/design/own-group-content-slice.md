# Own-device group contents: implemented slice, not hub readiness

Worktree: `feature/sync-connection-audit`. Not merged, installed or committed.

## Actual application paths changed

- Remote/local group meta/day writes now fail on persistence errors and restore
  in-memory state before returning. The actual group reader closes without Ack or
  fan-out on persistence failure. A meta update with no new record IDs but a new
  accepted operation VV is still persisted.
- `group/own/contents.rs` exports persisted native group operations and imports
  through GroupDoc's original signature/writer/doc-placement validators.
- `sync/engine.rs` actually dispatches these exports/imports on authenticated
  own-device sessions. It is no longer a simulation or an unused adapter.
- The `cap/group-contents/v1` Hello entry, with an encoded empty VV, negotiates
  contents without changing the self/3 enum or ALPN. Legacy peers ignore unknown
  inventory names; sender emits no group-content Sync to them and preserves their
  complete legacy DM slice. Combined advertised data docs reserve a capability
  slot under the existing `MAX_OWN_DOCS + 4` Hello bound.
- Metadata inventories precede day inventories; tokens precede contents on the
  ordered stream. Recent days and DM docs share a bounded initial catalog.
  Per-session advisory peer-doc knowledge is bounded; evictions can cause duplicate
  sends, not deletion of persisted messages or destination receipts.
- A negotiated own-device token without host hints can initialize an empty local
  group context. It grants no membership/hosting. Validated original signed meta
  supplies roles; this does not promise host discovery or future availability.
  Legacy token-only bootstrap still requires host hints. Current epoch/own-device
  admission is checked before handling group tokens and contents.
- Own-device metadata imports now atomically persist `own_owed_meta`, the validated
  accepted frontier, with their native ops. `group_owes` includes this obligation
  even when the laptop writer has no ops. VV-only advancement is covered too.
  Host meta Acks clear it only if they cover the saved frontier; the import/clear
  group lock prevents an older Ack from clearing a newer obligation. A failed
  metadata-clear write leaves both memory and disk pending. Legacy local metadata
  lacking the field decodes with no obligation and preserves user state.
- Own-device imports of account-authored day records atomically persist the day
  and its upstream-owed marker. The original phone writer is retained, not changed
  to the laptop writer. Failed append restores both. A host Ack can clear the
  day obligation only when it covers the full persisted day VV, read under the
  append's group lock; an older/concurrent/empty VV cannot clear it. Metadata-write
  failure while clearing leaves the in-memory obligation intact.

## Deliberately incomplete

This capability is **not** a laptop-readiness certificate. Direct chat/group dial
paths remain enabled. No battery-saver UI, mobile route suppression, shared-host
transport pooling, release server or official-group deployment is implemented by
this slice.

Initial group transport inventories cover meta and held recent days, not all
archived days. A new storage/adapter foundation pages retained day keys using the
existing bounded `keys_after` API: at most 64 days and 128 KiB of day/VV payload,
validated exclusive cursors, persisted frontiers, and errors for inconsistent rows.
The storage helper is tested across 130 archived days. It is now consumed by the
[negotiated history wire slice](own-history-wire-slice.md), which passed real
two-node self-sync across 70 cold days and a 70-operation day. An empty page still
ends one pass, not a stable-snapshot/catch-up certificate; inserts behind a cursor
require another pass. The [bounded native exporter](bounded-loro-export.md) selects
whole causal operation ranges from persisted originals. Legacy/content-only peers
retain their whole-diff path. Next: actual reconnect/loss and multi-scope load tests,
host receipt provenance and freshness/readiness gates.
Oversized legacy exports above 12 MiB are retained/deferred, never acknowledged as sent;
other own-device documents continue. The paged/range-batched own-device history
path is now present, but it does not by itself authorize exclusive laptop routing. Existing direct
host history sync remains unchanged.

Host-storage receipts are not yet carried back from laptop to phone. Own Sync VVs
are local-cache/advisory knowledge and do not enter external host/contact Ack
stores. They must not be interpreted as delivery/read by group members. Per-scope
catch-up/health, quota/fair scheduling,
receipt provenance and route-manager integration remain gates in
`laptop-forwarding-integration-gates.md`.

Author proofs are preserved, but this is not anonymity or strong cryptographic
revocation of a hostile device that shares the account mnemonic. Personal blocking,
group membership/delegation and device replacement remain separate policies.

## Targeted validation

Owned Rust tests cover durability rollback/retry, original signed operation export,
signature substitution/cross-group replay rejection, admission guards, capability
and inventory bounds, actual localhost outbound self frames, token/meta/day order,
legacy no-content behavior, unhosted context with no pre-proof role, atomic borrowed
writer obligations and conservative host-ack coverage.

The authenticated self-reader fault test uses actual self_run DID/device auth over
localhost QUIC and proves no Ack/durable VV advance after injected append failure.
Other producer tests seed registry/PeerChat. None prove QR pairing, complete
multi-node hosting integration, production capacity/energy, Android behavior, all
fault timings, or hostile-shared-mnemonic isolation. A new metadata-obligation test
releases/reopens an actual temporary sealed redb before Ack, retries the duplicate
and checks the obligation persists, then reopens again after clearing. This is
clean close/reopen, not process-kill or OS/power-loss durability. Other in-memory
sealed fixtures test transaction behavior only.

Command (shared queue, two compile jobs, owned filters only):

```sh
CARGO_BUILD_JOBS=2 python3 /home/abe/p2p_chat/scripts/buildlock.py \
  --who sync-connection-audit -- cargo test --offline --locked -p p2pcore --lib -- \
  group::engine::durability:: group::own::contents::auth_tests:: \
  sync::engine::group_contents:: --test-threads=1 --nocapture
```

Final combined batch: **40 passed, 0 failed, 0 ignored, 185 filtered out**.
Five new inventory tests cover archived pagination, exclusive cursors/page bounds,
unknown groups/invalid cursors, missing frontiers and mutation behind the cursor.
The new metadata-obligation regression first failed against the preceding slice
(`group_owes` incorrectly returned false); all seven new obligation tests pass.
No broad workspace suite or physical device test. `git diff --check` passed.
