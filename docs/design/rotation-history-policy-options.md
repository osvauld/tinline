# Rotation history paging and G-HISTORY options (packet B1)

Status: proposal — not approved. G-HISTORY is open; nothing here selects an option.

Model: `crates/proto/tests/rotation_history_paging.rs` (12 tests, fake time, real Ed25519,
deterministic fixture owners/keys). Not a production wire format, verifier API or storage design.

## 1. The failure this packet addresses (verified in source)

- The direct-update queue signs the possession challenge with the key of the page's LAST
  record and verifies it against that record's endpoint:
  `crates/proto/tests/support/rotation_permit_network.rs:90`, `:107`, `:112`, `:119`.
- When catch-up needs several pages, the last record of an early page names an endpoint whose
  key has already been retired. Binding possession per page therefore either fails for honest
  senders or (if old keys were kept) accepts a retired key.
- `View::live` in `crates/proto/tests/rotation_permit_model.rs:107` already checks possession of
  the view's current endpoint only; paging itself (`:72`) does not consult possession. B1 makes
  that separation explicit and testable.

## 2. Modeled interface (verified in tests; candidate shape, not frozen)

| Operation | Checks | Never checks |
|---|---|---|
| `verify_history_page(view, page, now, revoked, work) -> Result<View>` (`rotation_history_paging.rs:121`) | owner DID signature, version/audience scope, `issued_at <= now`, revocation of anchor/head/record, generation order, predecessor + previous-ID chain, ID change; page size `<= PAGE` | endpoint keys, possession, expiry of the record |
| `bind_live(view, newest, challenge, proof, now, revoked, work) -> Result<LiveBinding>` (`:211`) | `newest.id() == view.head`; challenge names the same head; head not revoked; `issued_at <= now < expires_at`; Ed25519 proof by `newest.endpoint` | owner signatures (already verified via the head ID) |
| `Receiver::route(now)` (`:435`) | live binding exists, names the CURRENT verified head, unexpired, not revoked | — |

Receiver state (`:341`) holds view, retained evidence, optional checkpoint, revocation set,
nonce high-water + outstanding challenges, and an optional live binding. Every rejection test
compares a clone of the whole receiver before/after. The work counter is outside receiver state.

Model choice (not policy): a failed `bind` does not consume the challenge, so state is unchanged;
a successful `bind` consumes it. Challenge lifetime/replay semantics belong to packet C3.

## 3. Tests and KIND labels

| Test | KIND | Shows |
|---|---|---|
| `multi_page_catch_up_then_live_binding_proves_only_the_newest_key` | invariant | 13 successors, PAGE=4 → 4 pages, 13 sig checks; only gen 13 unexpired; one possession check binds gen 13, not gen 12 (page-3 tail) |
| `retired_intermediate_key_cannot_bind_live_and_state_is_unchanged` | invariant | retired gen-12 key: `possession` / `not_head` (even while gen 12 unexpired); foreign and consumed challenges refused |
| `verified_history_without_live_proof_advances_knowledge_but_gives_no_route` | invariant | gen advances to 13, `route == None` until bound; wrong key `possession`; expired head `expired` |
| `live_proof_before_the_verified_head_is_reached_is_rejected` | invariant | cursor 8 + genuine gen-13 proof → `not_head`; stale gen-8 → `expired`; challenge from gen 8 → `challenge_head` after advancing |
| `interrupted_paging_resumes_from_the_receivers_cursor` | invariant | damaged page 3 rejected atomically (cursor stays 8); sender rotates twice more; resume from cursor 8 sends 4+3 records; old newest (gen 13) → `not_head` |
| `possession_by_a_key_cannot_substitute_for_owner_signed_history` | invariant | correctly chained gen 14 signed by another DID → `owner_signature`; attacker possession → `not_head` |
| `route_requires_live_binding_to_the_current_head_and_lapses_with_it` | invariant | new verified head withdraws the old binding's route before its expiry; rebinding works; expiry/revocation withdraw route, history retained |
| `strict_full_history_option_requires_every_record_since_the_cursor` | candidate-policy G-HISTORY | 40 records verified; pruned sender → `history_unavailable`, skipping → `gap`; revoked intermediate gen 7 → `revoked` |
| `checkpoint_option_lets_a_stale_cursor_jump_with_anti_rollback_and_scope_checks` | candidate-policy G-HISTORY | same scenario: checkpoint@30 + 10 tail = 12 sig checks; revoked gen 7 NOT detected; rollback `stale`, tamper/foreign `owner_signature`, wrong anchor/audience `scope`, wrong head `checkpoint_head`, idempotent redelivery |
| `checkpoint_cannot_detect_a_fork_between_the_receivers_head_and_the_checkpoint` | limitation | receiver at honest gen 5; mnemonic-holding clone forks at gen 5. Strict: `predecessor`/`fork`. Checkpoint@12 of fork: accepted silently |
| `group_full_ancestry_contract_is_met_by_strict_history_but_not_by_a_checkpoint` | candidate-policy G-HISTORY | strict export (21 bindings) satisfies the mirrored group check; checkpoint export omits gens 1..14 and fails with or without the root |
| `work_bounds_for_one_thousand_rotations_under_both_options` | candidate-policy G-HISTORY | computed bounds asserted (see §4) |

## 4. Strict full history vs owner-signed checkpoint

Checkpoint (model, `:251`, `:278`): DID-signed `{version, owner, audience, anchor, generation,
head}` delivered with the head permit it names (needed to continue chaining: `current_id`,
`endpoint`, `expires_at`). Anti-rollback: reject `generation <= cursor` unless identical.

Measured for N = 1000 rotations, receiver cursor 0, checkpoint at gen 960 (fixture choice),
JSON fixture encoding (bytes are illustrative only; schema is not frozen):

| Metric | Strict | Checkpoint |
|---|---|---|
| Signature verifications per catch-up | N + 1 = 1001 | 2 + (N − cp) + 1 = 43 |
| Messages (PAGE = 4) | ceil(N/4) = 250 | 1 + ceil(40/4) = 11 |
| Bytes (fixture JSON + sig) | 775,826 | 32,366 |
| Records retained by receiver (if it keeps evidence) | N = 1000 | 1 checkpoint + 1 head + 40 = 42 |
| Records sender must retain | back to the OLDEST receiver cursor (unbounded per offline peer) | latest checkpoint's head + tail |
| Anti-rollback state | receiver `(generation, head)` cursor | same cursor; plus latest accepted checkpoint |
| Stale cursor, pruned sender | cannot catch up (`history_unavailable`) | catches up |

What each record proves:

| Claim | Strict | Checkpoint |
|---|---|---|
| Historical legitimacy (each step owner-signed and chained from receiver's head) | yes, every step | only that the owner DID signed the checkpoint head; no ancestry from the receiver's head |
| Current permission | neither: requires `bind_live` + unexpired head + policy | same |
| Creation time | no: signed `issued_at` is an owner assertion, not a trusted timestamp | same |
| Revoked intermediate record detected | yes (`revoked`) | no |
| Fork between receiver head and new head detected | yes | no (limitation test) |
| Group "full ancestry, no omitted predecessors" contract | satisfied | not satisfied |

Note under the shared-mnemonic model (G-AUTHORITY), the checkpoint signer has the same authority
as any permit signer, so a checkpoint is not weaker in *who* may sign, only in *evidence*.

## 5. Group compatibility (verified against mirrored contract; contract NOT changed)

`crates/group-sim/src/reachability.rs:24` requires "full verified ancestry since nominated root,
with no omitted endpoint-bearing predecessors"; `HostDirectory::install` checks the first
record's device ID against the nomination (`:49`, `:56`). The model mirrors this in
`group_contract_accepts` (`rotation_history_paging.rs:1152`), adding the documented contiguity
requirement explicitly.

Finding: the checkpoint option CANNOT satisfy the current contract. Accepting a per-audience
checkpoint as the root would be a group contract change (root ≠ nominated device; first
generation ≠ 0; omitted predecessors). Reported to coordinator; not decided here. A hybrid
(strict for group audiences, checkpoint for private) is possible but is itself a G-HISTORY decision.

## 6. What is NOT proven

- No real session/handshake, cursor-exchange message, or iroh endpoint authentication.
- No persistence; snapshots are ideal clones. Sender pruning is a test operation, not a policy.
- Checkpoint cadence, who issues checkpoints (any linked device?), and checkpoint expiry are not
  modeled. Fork/revocation semantics belong to G-FORK/G-REVOCATION (packet B3).
- Fixture keys/IDs derived from seeds are test determinism only, NOT a derivation scheme.
- Byte counts use the fixture JSON encoding; production bounds need the frozen schema.

## 7. Open questions for G-HISTORY

1. Must every audience keep strict evidence, or only group audiences (to meet the group contract)?
2. How long must a sender retain history for an offline contact before it may prune, and what
   does a contact with a pruned cursor see (re-invitation? checkpoint?)?
3. If checkpoints are adopted: issuer, cadence, how a receiver detects a fork across a checkpoint
   (e.g. requiring a link to the receiver's last known checkpoint), and whether revocation of an
   intermediate record must survive a checkpoint.
4. Should receivers retain evidence at all for private audiences, or only `(generation, head)`?
5. Is a "dial hint" (verified head endpoint without possession) allowed for initiating the
   connection that produces the possession proof? The model treats it as no route.
