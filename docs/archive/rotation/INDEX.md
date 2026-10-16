# Archived rotation studies

Option studies and simulator-era models for endpoint rotation, archived 2026-10-16 when rotation
became periodic only (D11). Current behaviour: `docs/rotation.md`; decisions:
`docs/design/rotation-open-decisions.md`. Kept for their evidence; not maintained.

- `rotation-open-decisions-2026-10-10.md` — the full decision digest (U1–U25, questions §1–§15) before it was shrunk.
- `endpoint-rotation-blocking.md` — the original "rotation as blocking" idea (superseded: blocking never triggers rotation).
- `endpoint-rotation-coordination.md` — coordination notes between rotation and group chat.
- `endpoint-rotation-device-audit.md` — linked-device audit and initial discovery model.
- `endpoint-rotation-implementation-plan.md` — the earlier test-first implementation plan.
- `endpoint-rotation-permit-model.md` — owner-replaceable device IDs via succession permits (candidate).
- `group-rotation-integration-contract.md` — group/rotation integration constraints.
- `group-flag-removal-model.md` — W2-F flag removal + group rotation model (proposal).
- `group-host-only-model.md` — W2-H host-only DID-signed group model (proposal).
- `group-multi-host-model.md` — W2-M multi-host model (proposal).
- `rotation-author-group-adapter.md` — D2 author-key options and group adapter contract.
- `rotation-author-key-trace.md` — D1 trace of transport-key uses (author keys, Loro peer IDs, proofs).
- `rotation-baseline-report.md` — A1 core baseline and EndpointId dependency map.
- `rotation-challenge-options.md` — C3 live-possession challenge storage options.
- `rotation-fork-revocation-options.md` — B3 forks and older-ancestor revocation options.
- `rotation-history-policy-options.md` — B1 history paging and G-HISTORY options.
- `rotation-lifecycle-hooks.md` — A2 endpoint lifecycle hook map.
- `rotation-linked-discovery-options.md` — E1 linked-device discovery decision matrix.
- `rotation-storage-failure-contract.md` — C2 storage boundaries and failure matrix.
- `rotation-time-contract.md` — C1 time contract (intervals, jitter, overdue, clocks).
