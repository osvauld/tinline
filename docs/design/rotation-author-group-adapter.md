# Rotation D2: author-key options and group adapter contract

Status: proposal — not approved. Documentation-only packet D2. No code, tests, or builds were
changed or run. Source state: branch `rot/d2` at `bb73c8b`. Every `path:line` was read at that
commit.

Inputs: `docs/design/rotation-author-key-trace.md` (D1, cited as `D1 §n`),
`docs/design/rotation-baseline-report.md` (A1, `O1`–`O8`), `docs/group-sim-coverage-audit.md`
(G-A), `docs/group-rotation-integration-contract.md` (cited as "contract item n"),
`docs/group-chat-coordination-{request,response}.md`, `docs/group-chat.md`,
`design-models:crates/group-sim/src/{reachability,authority,tests}.rs`,
`design-models:crates/proto/tests/rotation_permit_model.rs`. B1 output (`rotation-history-policy-options.md`)
did not exist at this commit. The checkpoint discussion therefore uses the B1 *packet* text
(`docs/design/rotation-agent-packets.md:155-188`), not B1 results.

Labels:
- **[src]**: verified by reading source at the cited line.
- **[D1]/[A1]/[G-A]**: a finding of that packet that this packet did not re-check. The few lines
  re-checked here are marked [src].
- **[proposal]**: this document's suggestion. It is not agreed.
- **[estimate]**: back-of-envelope arithmetic with stated assumptions. It was not measured.
- **[GROUP]**: needs agreement with the group-chat agent before either side relies on it.
- **[USER: G-xxx]**: a user policy gate from `docs/design/rotation-agent-handoff.md:122-134`.
  This document lays out options and does not decide them.

Out of scope, and not done here: any key derivation, ratchet, or future-endpoint prediction
scheme. Any claim that unlinking revokes a hostile holder of the mnemonic. Any group membership
or revocation policy.

---

## 1. Summary

1. Today one per-install key is the iroh EndpointId, the chat/sync batch signer, the
   per-message author signer, and the input to the Loro peer ID [D1 §1; src
   `crates/core/src/chat/doc.rs:33-40`]. Group-sim mirrors this coupling. The envelope's
   `device` is both the signing key and the peer-ID input, and the DID attestation names that
   key [src `design-models:crates/group-sim/src/tests.rs:75`, `:77`, `:126`, `:138`, `:206`, `:213`].
2. Two author-key options are compared (§3):
   - **(i)** a separate long-lived author key per device path;
   - **(ii)** the author key rotates with the transport key, with a retained proof chain.

   Neither option removes the need to handle **an author-key change for an existing device
   path**. Restore already changes the key [src `crates/core/src/node.rs:1248-1250`]. An
   owner replacing a device ID with a new install changes it too. Option (i) makes these
   changes rare. Option (ii) makes them periodic. So the fixes for stranded ops, ticks, and
   retained historical keys (D1 Q5, Q6, Q9) are needed under **both** options.
3. Under (ii), with today's attestation format, every group envelope would carry the
   transport EndpointId that signed it. The signer key *is* the endpoint
   [src `design-models:crates/group-sim/src/tests.rs:126`, `:135`]. That puts an endpoint into group-audience
   history. Unless the signing endpoint is itself group-audience, this conflicts with contract
   item 6. Option (i) avoids this structurally, because an author key is not an address.
4. Group-sim's per-author `previous` chain requires the same signing key and `sequence + 1`
   [src `design-models:crates/group-sim/src/tests.rs:259-267`]. Under (ii), every rotation would end that chain.
   Either a cross-key continuity rule is needed, or each rotation starts a new chain at
   sequence 1 under a new peer. That is a **[GROUP]** decision.
5. The mock `SuccessionAdapter` can be kept as the *shape* of the boundary. It is missing
   fields and typed outcomes (§5): `version`, a predecessor record ID, `previous_id`,
   `generation`, permit ID, `issued_at`, an evidence/signature reference, revocation and fork
   status, live-possession status, an anchor reference in `Nomination`, and typed failure
   reasons. Its contract, "full verified ancestry since the nominated root"
   [src `design-models:crates/group-sim/src/reachability.rs:24-26`], conflicts with "bounded verification" (contract
   item 1) once a nominated path has thousands of rotations. That needs **[GROUP]** agreement
   plus a **[USER: G-HISTORY]** decision.

---

## 2. Verified facts the comparison relies on

| # | Fact | Evidence |
|---|---|---|
| F1 | Loro peer ID = `BLAKE3("tinline-loro-peer-v1" ‖ device ‖ doc)[..8]` | [src] `crates/core/src/chat/doc.rs:33-40` |
| F2 | Message signature input names doc, id, author DID, `at`, … but **no key ID** | [src] `crates/core/src/chat/doc.rs:132-140`; D1 §4.2 |
| F3 | Verifier tries every `(did, key)` in context; any match is accepted | [src] `crates/core/src/chat/doc.rs:555` |
| F4 | `chat_keys` = current own key + all registry entries (incl. removed) + contact's **current** devices | [src] `crates/core/src/chat/own.rs:54-62` |
| F5 | Contact device set is **replaced** by the newest list | [src] `crates/core/src/node.rs:3142`; `verified` cleared on any new device `:3139-3141` |
| F6 | `mc` stores only a counter; delivery pairs it with `peer_id(me.device, doc)` | [src] `crates/core/src/chat/store.rs:240-242`, `crates/core/src/chat/engine.rs:1144-1150` |
| F7 | Restore makes a **new** device secret and tombstones the old self entry | [src] `crates/core/src/node.rs:1248-1250`, `:1288-1293` |
| F8 | Tombstoned dialer gets `Unlinked` and wipes itself | [D1 §6.5] `crates/core/src/sync/engine.rs:385-405` |
| F9 | Attestations have no expiry/sequence; any mnemonic holder can attest any key | [A1 O8] `crates/proto/src/lib.rs:197-213` |
| F10 | Group-sim envelope: `device` = signing key, peer bound to `(device, doc)`, attestation must name `device` and `author` | [src] `design-models:crates/group-sim/src/tests.rs:19-20`, `:206`, `:213-219` |
| F11 | Group-sim `previous` must have same `device` and `sequence + 1`; `key()` = `peer:sequence` must be unique | [src] `design-models:crates/group-sim/src/tests.rs:259-273`, `:41-43` |
| F12 | Group-sim peer range: update may only contain the envelope's own peer | [src] `design-models:crates/group-sim/src/tests.rs:277-279` (untested per [G-A] §2 item 7) |
| F13 | Mock `install` checks only `first.device_id`, owner and audience of each record; ordering/continuity delegated to adapter; mock ignores `Nomination` | [src] `design-models:crates/group-sim/src/reachability.rs:39`, `:49-72`; [G-A] §3 row R:216 |
| F14 | `retire_at` is "routing retirement time ONLY, not access-grant expiration" | [src] `design-models:crates/group-sim/src/reachability.rs:19-20` |
| F15 | Permit model `Claims` carries `version, owner, audience, previous_id, replacement_id, endpoint, predecessor, generation, issued_at, expires_at`; `live` requires `now < expires_at` and possession of `endpoint` | [src] `design-models:crates/proto/tests/rotation_permit_model.rs:18-29`, `:107-113` |
| F16 | `PostAuthorization::allows_post(author_did, permit)` only; succession/hosting deliberately absent | [src] `design-models:crates/group-sim/src/authority.rs:148-152` |
| F17 | Group delegation chain cap `MAX_CHAIN = 8` | [src] `design-models:crates/group-sim/src/authority.rs:8`, `:110` |

---

## 3. Author-key options compared

Definitions, all [proposal] vocabulary:
- **Device path**: the owner-replaceable succession of device IDs that starts at one nominated
  or linked anchor. It is not a permanent slot, and it is not physical hardware continuity.
- **Option (i), separate author key.** Each device path has an author key, distinct from every
  transport endpoint key. It changes only on author-key events: restore, replacement by a new
  install, or explicit author-key retirement. It does not change on periodic endpoint rotation.
- **Option (ii), rotating author key.** The author key is the current transport key (today's
  behaviour, F1/F10). Every rotation creates a new author key. Old ones remain verifiable
  through retained evidence: attestations and/or the succession chain.

Option (i) still needs a DID-signed statement binding the author key to the DID, and to the
device path if paths matter. Today's `attest` binds a DID to *a* key with no role field
(F9). Whether to reuse it, or add a separate author-role statement, is a **[USER: G-KEYS]**
question. No new cryptographic construction is proposed here. Either way the statement is
DID-signed like existing attestations, so any mnemonic holder can mint one (F9,
**[USER: G-AUTHORITY]**).

### 3.1 Comparison

| Aspect | (i) separate long-lived author key per device path | (ii) author key rotates with transport key + retained chain |
|---|---|---|
| **Old-signature verification** | One key per path verifies all its messages. The verifier must retain one key record per path ever seen. Today F5 replaces contact keys from the newest list, so a separate retained author-key set is needed either way (D1 Q3). | One key per rotation. Group envelopes are self-contained if they carry the attestation, as group-sim does (F10): the authorship check needs no chain. DM message sigs carry no key ID (F2), so the verifier must retain *every* historical key of the contact (F3/F4) or messages fail as `BadSignature` (A1 O2). |
| **What the chain is needed for** | Routing and host continuity only. Transport succession history can be pruned or checkpointed without affecting signature verification. | Routing **and** linking "this old key is the same path as the nominated device" (hosting, `previous` continuity F11). Pruning routing history can break path attribution of old authorship. |
| **Historical vs current authority** (contract item 2) | One key is valid for the path's lifetime. A leaked author key forges messages until some author-key retirement/validity rule exists **[USER: G-REVOCATION, G-HISTORY]**. Transport rotation does not reduce this exposure. | Many retired keys stay valid forever under F9 (no expiry). Every old transport secret that leaks, for example one not deleted after retirement, can forge backdated messages. `at` is author-chosen, so a time window is weak (D1 Q4). Exposure grows with the rotation count unless old secrets are reliably destroyed (**[USER: G-STORAGE]**). |
| **Loro writer IDs** | `peer = f(author_key, doc)` is stable across endpoint rotation. No new peer per rotation, no VV growth from rotation, and F11's `previous` chain is unaffected. The live-path `ForeignPeer` check (D1 §5.2, `engine.rs:816`) must map **session transport key → author key** through an authenticated binding (adapter §6.3). | New peer per rotation per written doc. Stranded unacked ops (A1 O3, D1 §5.4.1), wrong ticks (O4), and VV growth in the long-lived `account` doc (D1 §5.4.3). F11's `previous` chain breaks at each rotation **[GROUP]**. |
| **`mc/` / `out/` delivery state** | Unaffected by periodic rotation. Still wrong after restore or install replacement (F6/F7), so the `(peer, counter)` fix (D1 Q6) is still required, but it is exercised rarely. | Wrong on **every** rotation unless `mc` becomes `(peer, counter)` and `out` covers all own historical peers (D1 Q6). This is a schema migration for existing `chat.redb`. |
| **Clone (vault/directory copy)** | Both copies hold the same author key for the **lifetime** of the path. Same peer ID + divergent counters = Loro op-ID collision (D1 §5.4.5). Group-sim would reject the second `peer:sequence` (F11) at receivers that saw the first, and replicas diverge. The fork never ends by itself. **[USER: G-FORK]** | Both copies share the key only until the next rotation. They then rotate independently, which is a **succession fork** (B3 scope, **[USER: G-FORK]**). Op-ID collisions are possible in the window before that. Fork evidence moves from the CRDT layer to the succession layer. |
| **Restore from phrase** (F7) | The vault, and with it the author key, is not in the phrase, so restore needs a **new** author key for the path. That is an author-key change, the same as one (ii) rotation. Restore over an old vault backup instead would be a clone. | Already a new key today (F7). Indistinguishable from a rotation, except that today it tombstones the old key (F7, F8). |
| **Audience separation** (contract item 6) | The author key is not an address, so envelopes reveal no endpoint. It does correlate a path's group and DM activity if one key serves both audiences. Correlation is allowed and no anonymity is promised. A per-audience author key is possible **[USER: G-KEYS]**. | The envelope attestation names the signing endpoint key (F10). Group history would then contain transport EndpointIds. That is acceptable only if the signing endpoint is a **group-audience** endpoint, which means DM and group writes from one install use different keys and different peers. Otherwise private reachability leaks. **[GROUP]** |
| **Storage cost** [estimate] | Per contact: one author record per device path. ~150 B each (32 B key + 64 B sig + DID + iat), bounded by link/restore/replace events, not by time. Example: 3 paths × 5 replacements over 5 y ≈ 15 records ≈ 2 KB. Routing history is separate and prunable. | Per contact: one key record per rotation. Example: 3 devices × daily rotation × 5 y ≈ 5 475 records ≈ 0.8 MB of attestations. With a chain of permits (`Claims` + sig, roughly 300 B JSON) it is ≈ 2.4 MB per contact, retained to keep old signatures attributable. Account-doc VV: about 12 B per entry × 5 475 ≈ 65 KB, sent in every self `Hello` (D1 §5.4.3). Group envelopes add ~150 B each for the inline attestation under both options (already true in group-sim). |
| **Secrets at rest** | One extra long-lived sealed secret per install. Its compromise matters more than any single transport key's. | Only the current transport secret, if retired secrets are deleted. Deletion durability is **[USER: G-STORAGE]**. |
| **Code churn** | New key role through Profile/vault/attestation/registry. Every R4/R6 call site switches from `me.device` to the author key (D1 §1 table). Transport rotation then touches only R1/R2/R8/R9/R10. | No new key type, but R3–R7 must all become history-aware. Rotation touches every role. |
| **Interaction with blocking** | None: blocks are DID-level (D1 §1). | None. |

### 3.2 What neither option settles

- **Shared-mnemonic authority** (F9). Under both options, anyone holding the mnemonic can attest a
  new author key and sign as the DID. A separate author key does not create device-compromise
  isolation **[USER: G-AUTHORITY]**.
- **Clone detection** is required under both. Option (i) needs CRDT-level detection of duplicate
  `(peer, counter)` with different content. Option (ii) needs succession-fork detection (B3).
- **Validity windows** for retired keys (D1 Q4) are open under both **[USER: G-HISTORY,
  G-REVOCATION]**.

### 3.3 Proposed tests to discriminate the options (not written; owner TBD)

Each test is meant to run under both options as `candidate-policy G-KEYS`:
1. A pending pre-change message on the old peer reaches `Delivered` only after it is actually
   acked. This exercises F6 after restore (both options) and after rotation (option ii).
2. An old message relayed by the contact's *other* device verifies after the author key leaves
   the newest `DeviceList`. This exercises A1 O2.
3. Clone: two copies write to the same doc before diverging. Assert detection, and that no
   receiver silently accepts both histories as one writer.
4. A group envelope history export contains no private-audience endpoint key. This is contract
   item 6, and it fails under option (ii) if the private endpoint signs group envelopes.
5. More than 8 author changes on one path leave the group delegation depth unchanged
   (contract item 1, F17).
6. A group `previous` chain across an author-key change (F11): either a cross-key continuity
   proof is accepted or a new chain is started. Two candidate tests **[GROUP]**.

---

## 4. Answers to D1 §10 migration questions

Each answer lays out options and names the gate. None of them is a decision.

1. **Author key vs transport key.** See §3. Gate **[USER: G-KEYS]**. If (i) is chosen, the
   author key is still replaced on restore or install replacement (F7), so "is it ever
   rotated" is answered by "yes, on author-key events". Whether it also has scheduled
   rotation is **[USER: G-KEYS]**. How it is attested: a DID-signed statement binding the
   author key to the DID and the path anchor. Whether to reuse `attest` or add an
   author-role statement is part of **[USER: G-KEYS]**.
2. **Which key signed a message.** Options:
   - (a) add a key ID or full key to `msg_sig_input`. This is a wire/sig change and makes
     verification O(1).
   - (b) keep "try all keys of the DID" (F3) over a retained set.

   Under (ii), (b) costs O(rotations) per message verification. Under (i) it costs O(paths).
   The bound on the set comes from G-HISTORY retention. Group envelopes already carry the key
   and attestation (F10), so this question is about DM only. **[USER: G-KEYS]** for the DM
   format.
3. **Historical key retention for contacts.** A retained, append-only "author keys seen for
   DID X" set, separate from the routing `devices` list that F5 replaces. Contents: DID-signed
   author-key statements only, with no endpoint, so it is audience-safe under (i). Under (ii)
   the retained entries *are* endpoints, so a contact-audience store is acceptable for DM, but
   the same set must not be exported to group audiences (contract item 6). Retention and
   pruning: **[USER: G-HISTORY]**.
4. **Validity windows.** `at` is author-chosen, so a window "signed before retirement" can only
   be enforced against the receiver's own *first-seen* time or a causal frontier, not `at`.
   Options:
   - (a) accept indefinitely (today, F4);
   - (b) accept a retired key only for ops already in the receiver's accepted history, or
     causally before a retirement marker the receiver has seen;
   - (c) accept by `at` (weak).

   **[USER: G-HISTORY, G-REVOCATION]**. In groups, causal-frontier rules overlap the group's
   D01–D05 gates **[GROUP]**.
5. **Stranded ops on rotation.** Under (i) this applies only on author-key events. Under (ii)
   it applies on every rotation. Options:
   - (a) flush, meaning the old peer's ops are acked by each contact, before the old key may
     sign nothing more. This can block retirement indefinitely while a contact is offline,
     which is **[USER: G-RETIREMENT]**.
   - (b) a session authenticated by the new key may carry old-peer ops when accompanied by
     their retained author evidence. This relaxes `ForeignPeer` (D1 §5.2) for peers proven to
     belong to the same DID and path.
   - (c) rely on history pull. This is insufficient for plain new messages (D1 §5.4.1).

   Option (b) is the only one that works when the contact is offline for the whole overlap. It
   needs **[USER: G-KEYS]**, plus agreement on the evidence format **[GROUP]** for group
   envelopes, where the original envelope already carries its proof (F10).
6. **Tick and outbox bookkeeping.** Needed under **both** options because of restore (F7). The
   options are `mc` → `(peer, counter)` (mirroring `mo`), and `out` computed over all own
   historical peers that still have unacked ops. Migration rule for existing `mc` rows: the
   peer is unknown and was implicitly the then-current device. Without a recorded history of
   own peers this cannot be recovered exactly. Candidate: treat legacy rows as "Delivered
   unless `out` says otherwise", with a known-limitation test. **[USER: G-STORAGE]** for the
   migration semantics.
7. **Loro writer identity.** Option (i) gives one peer per `(author key, doc)`. Option (ii)
   gives a new peer per rotation. For clone and restore, peer reuse with diverged state is
   avoided only if the author key (or whatever feeds the peer ID) is never duplicated across
   live copies. A detect-and-quarantine rule is needed for the case where it is. Whether the
   peer-ID input should include anything besides the author key and doc is **[USER: G-KEYS,
   G-FORK]**. No new derivation is proposed here. Group-sim's binding (F10, F12) must change in
   lockstep **[GROUP]**.
8. **Retirement vs tombstone.** A distinct state is required: "retired endpoint of a live
   path" ≠ `removed`, because F8 would make a live install wipe itself. Indexing: the registry
   should be keyed by path anchor or device ID, with endpoints as a history under it. Today it
   is keyed by endpoint (D1 §6.5). Device IDs are owner-replaceable, so the stable index is the
   path anchor record ID, not a device ID **[USER: G-KEYS, G-RETIREMENT]**.
9. **Registry and `DeviceList` growth.** Under (i), `DeviceList` (cap 8) should list current
   paths' *current* endpoints only. Author keys and retired endpoints live in separate
   retained sets. Under (ii), the minimum retention for signature verification equals every
   key that signed anything still held by anyone. That is unbounded unless signatures carry
   attestations, as group envelopes do and DM does not. **[USER: G-HISTORY]**
10. **`verified` semantics.** Candidate: an authenticated succession on an already-verified
    path preserves `verified`, while a new path still clears it. This needs the
    verified-succession adapter (§6.1) to distinguish the two. **[USER: G-KEYS]** for the
    product meaning of "verified" (A1 O6).
11. **Tickets.** Options:
    - (a) re-mint when `claim.device != me.device` (fixes A1 O1);
    - (b) name the DID plus path anchor instead of the endpoint, and carry a separate
      short-lived route hint.

    Option (b) affects the invite wire format. A ticket still needs *some* reachable address,
    and signatures do not solve discovery (contract item 7). **[USER: G-DISCOVERY]** for (b).
    (a) is a local fix that the coordinator can schedule independently.
12. **Session-keyed state during overlap.** Two live endpoints of one path during overlap
    should count as **one** logical peer for presence/chat dedupe and as two transport
    sessions. That requires mapping session endpoint → path (adapter §6.2). Glare/tie-break
    ordering by endpoint key (D1 R10) changes at rotation; a transient effect is likely
    tolerable but untested. **[USER: G-RETIREMENT]**
13. **Group envelopes.** Under (i), the path author key signs envelopes, and old envelopes
    verify with their inline statement (F10) indefinitely. Under (ii), the signing key must be
    a group-audience key (§3.1, audience row), and old envelopes verify through their inline
    attestation. Path attribution, needed for hosting but not for posting, then needs a
    group-audience succession proof. Under both, group *authority* stays
    `PostAuthorization(author DID, permit)` (F16), unchanged by rotation (contract item 4).
    **[GROUP]**
14. **Restore as the first test case.** Recommended [proposal]: yes. Restore already performs
    an author-key change (F7), and its hazards are untested beyond local presence (D1 §7). The
    §3.3 tests 1–3 should first be written against restore as `limitation` tests.

---

## 5. Mapping onto the group-sim mock types

| Mock type / field [src] | Keep? | Rotation-side counterpart (`rotation_permit_model.rs:18-29`) | Missing / mismatch | Agreement |
|---|---|---|---|---|
| `Audience::{Group(String), Private}` (`reachability.rs:8-12`) | Keep the concept | `Claims.audience: String` | Needs a canonical encoding shared by both. Possibly more audiences, for example a per-contact private audience. | [GROUP] |
| `BindingEvidence.owner` | Keep | `owner` | — | — |
| `BindingEvidence.device_id: u64` | Keep, widen | `replacement_id: [u8;32]` | Width/type. Also missing `previous_id`, which is needed so a record cannot be spliced (B2 item 2). | [GROUP] |
| `BindingEvidence.endpoint: String` | Keep | `endpoint: [u8;32]` | Typed key. Must be the audience's endpoint, never a private one on a group path. | — |
| `BindingEvidence.audience` | Keep | `audience` | — | — |
| `BindingEvidence.retire_at` | Keep the meaning, rename | `expires_at` (live cutoff in `View::live`, F15) | Same role: routing/live-binding cutoff. **Neither is group access expiry** (F14). Agree one name and the timebase. Ticks vs wall clock is **[USER: G-TIME]**. | [GROUP] |
| — | add | `version` | Record version | [GROUP] |
| — | add | `predecessor` (record ID) | Without it the path order is unverifiable. The mock delegates order (F13). | [GROUP] |
| — | add | `generation` | Per path and audience. Needed for stale/gap/fork outcomes. | [GROUP] |
| — | add | permit ID (`Permit::id`, `rotation_permit_model.rs:45-48`) | Needed to reference a record in revocation and fork evidence | [GROUP] |
| — | add | `issued_at` | Validity check (`rotation_permit_model.rs:82`) | [USER: G-TIME] |
| — | add | signature / evidence reference | The mock carries no signature. A verifier-constructed type would make it unnecessary in the consumer (§6.1). | [GROUP] |
| — | add | revocation status | `View::page` checks revoked anchor/head/permit (`rotation_permit_model.rs:83-85`). Policy is open. | [USER: G-REVOCATION] |
| — | add | author key (option i) | If (i), the binding or a sibling record maps path → author key | [USER: G-KEYS], [GROUP] |
| `VerifiedSuccession { path: Vec<BindingEvidence> }` (`reachability.rs:22-27`) | Keep as an **opaque** result | `View` (anchor, head, current, generation, expires) | Public field means any code can construct one. Proposed: only the rotation verifier constructs it. No live-possession flag: B1 separates `verify_history_page` from `bind_live` (`rotation-agent-packets.md:163-166`). Full-ancestry contract vs bounded work: see below. | [GROUP] |
| `Nomination { owner, initial_device: u64, group }` (`reachability.rs:28-33`) | Keep, extend | `View.anchor` | Needs an anchor record ID (the authenticated initial nomination), the assignment/grant ID, and the audience. `initial_device` alone is a replaceable ID, not an unambiguous anchor. | [GROUP] |
| `SuccessionAdapter::verified_path(&Nomination) -> Option<_>` (`reachability.rs:34-36`) | Keep the name, change the return type | `View::page` errors `owner_signature/scope/validity/revoked/fork/stale/gap/predecessor/unchanged_id` (`rotation_permit_model.rs:72-101`) | `Option` merges unreachable, proof failure, fork, and revoked into one case. G-A asked for typed reasons (`group-sim-coverage-audit.md:228`). | [GROUP] |
| `HostDirectory::install` (`reachability.rs:49-72`) | Group-owned | — | Checks `first.device_id` only; continuity is trusted to the adapter (F13). A `[nominated, sibling]` path is accepted by the mock (G-A §3). The real adapter must refuse it. | [GROUP] |
| `HostDirectory::active(now)` (`reachability.rs:73-75`) | Replace with §6.2 | `View::live` | Needs typed status (Live / HistoricalOnly / Unreachable / Revoked / Forked). Contract table row "Offline mutual retirement" requires "unreachable, not proof failure" (`group-rotation-integration-contract.md:63`). | [GROUP] |

### 5.1 Full ancestry vs checkpoint

- The mock contract is "full verified ancestry since nominated root, with no omitted
  endpoint-bearing predecessors" [src `reachability.rs:24-26`].
- Contract item 1 requires succession verification and storage to be bounded, with "a separate
  validated checkpoint/history policy" [src `group-rotation-integration-contract.md:12-14`].
- These two statements conflict for long paths. At daily rotation for 5 years (about 1 800
  records), a full path is several hundred KB per nominated host [estimate, ~300 B/record].
  Every installer would have to verify it.

Options, none chosen. The decision is **[USER: G-HISTORY]** and **[GROUP]**:

- **(a) Strict full ancestry.** Matches the mock as written. Cost is O(rotations) per new
  verifier. History cannot be pruned while any group may need to install the path.
- **(b) Owner-signed per-audience checkpoint.** A DID-signed `{owner, audience, path anchor,
  generation, head id}`, from B1 item 6, is accepted as a new root.
  - The mock's `first.device_id == initial_device` check (`reachability.rs:56`) would then
    fail. The group side would need to accept "checkpoint referencing the nomination anchor"
    as the first element. That changes the group contract text.
  - Security note: an owner checkpoint is DID-signed, so any mnemonic holder can mint one (F9).
    It proves owner intent, not continuity. It can skip a revoked intermediate unless
    revocation is checked against the checkpoint's covered range **[USER: G-REVOCATION]**.
- **(c) Hybrid.** Strict history for receivers whose cursor is recent, and a checkpoint only
  for new or long-offline receivers. This needs B1's cursor semantics.

B1 test 7 (`rotation-agent-packets.md:179-182`) is designed to show whether (b) can satisfy the
current contract. That result is not available at this commit.

---

## 6. Candidate adapter signatures (pseudocode, NOT added to code)

All [proposal]. The type names are placeholders. No wire or storage commitment is made.
`Tick` is the fake-time `u64` used in models. The real timebase is **[USER: G-TIME]**.

### 6.1 Verified succession (rotation-owned; consumer shape needs [GROUP])

```rust
// Constructible only by the rotation verifier (private fields, no pub ctor).
struct VerifiedSuccession { anchor: RecordId, audience: Audience, owner: Did,
                            head: RecordId, generation: u64, current_device: DeviceId,
                            route: Option<RouteRecord>,         // head binding, if any
                            disclosed: Vec<AudienceRecord> }    // only records of `audience`
enum SuccessionError { BadSignature, WrongOwner, WrongAudience, Gap{have: u64, got: u64},
                       Stale, Fork(ForkEvidence), Revoked(RecordId), Predecessor,
                       RecoveryRequired /* cursor pruned/unknown, B2 item 5 */, Limit }
trait SuccessionAdapter {
    fn verified_path(&self, n: &Nomination) -> Result<VerifiedSuccession, SuccessionError>;
    // Bounded incremental update from a receiver cursor; atomic: Err leaves state unchanged.
    fn apply_page(&mut self, cursor: &SuccessionCursor, page: &[SignedRecord], now: Tick)
        -> Result<SuccessionCursor, SuccessionError>;
}
```
**[GROUP]** for: the `Result` replacing `Option`; the extended `Nomination` (anchor
record ID, assignment ID, audience); whether `disclosed` is the full path or a
checkpoint-rooted path (§5.1, **[USER: G-HISTORY]**).

### 6.2 Current route (rotation-owned; consumed by group routing and core dialing)

```rust
enum RouteStatus {
    Live { endpoint: EndpointKey, until: Tick, binding: RecordId }, // possession proven (B1 bind_live)
    HistoricalOnly { head: RecordId },   // history verified, no live possession yet
    Unreachable { last_known: RecordId },// retired/expired, no newer binding known: NOT a proof failure
    Revoked(RecordId), Forked(ForkEvidence),
}
fn current_route(&self, path: &VerifiedSuccession, now: Tick) -> RouteStatus;
fn path_of_session(&self, remote_endpoint: &EndpointKey) -> Option<(Did, RecordId /*anchor*/)>;
```
**[GROUP]** for the enum's variants: it replaces `HostDirectory::active` and the `bool` from
`Network::connect`. `path_of_session` serves D1 Q12 and the option (i) author mapping.
Grace and overlap durations are **[USER: G-TIME, G-RETIREMENT]**.

### 6.3 Author-proof verification (shared: rotation provides key evidence, group keeps authority)

```rust
struct AuthorClaim<'a> { did: &'a Did, author_key: &'a [u8; 32],
                         evidence: &'a AuthorEvidence, // DID-signed statement (attestation today)
                         doc: &'a str, peer: u64 }
enum AuthorVerdict { Valid { path: Option<RecordId> },   // path only if provable for this audience
                     UnknownKey, BadEvidence, PeerMismatch, RetiredOutsideWindow }
fn verify_author(&self, c: &AuthorClaim, signed_bytes: &[u8], sig: &[u8; 64],
                 audience: &Audience) -> AuthorVerdict;
fn writer_id(author_key: &[u8; 32], doc: &str) -> u64;   // derivation input set: [USER: G-KEYS]
// option (i) only: bind a live transport session to an author key for ForeignPeer checks
fn author_key_for_session(&self, remote_endpoint: &EndpointKey) -> Option<[u8; 32]>;
```
This verdict is about **authorship only**. Group authority stays
`PostAuthorization::allows_post(did, permit)` (F16). `RetiredOutsideWindow` exists only if
**[USER: G-HISTORY/G-REVOCATION]** adopts a window (§4 Q4). **[GROUP]** for: whether `path`
is required for posting (proposed: no, only for hosting), the `writer_id` inputs (F10, F12),
and the cross-key `previous` continuity rule (F11).

### 6.4 Host assignment check (group-owned; consumes 6.1/6.2)

```rust
enum HostDecision { Allow, NotNominatedPath, AssignmentRevoked, NoLiveBinding(RouteStatus),
                    CapabilityNotGranted }
fn may_host(a: &HostAssignment, path: &VerifiedSuccession, route: &RouteStatus,
            cap: HostCapability, group: &GroupState) -> HostDecision;
// Invariants (contract item 3): path.anchor == a.anchor; path.owner == a.holder_did;
// a not revoked at group's frontier (group policy, untouched by rotation);
// cap ∈ a.capabilities (succession never widens); DID equality alone never suffices.
```
**[GROUP]** owns this. Rotation's only input is the verified path and its route.
Whether succession may move hosting to a *different physical installation* is an open user
question (`docs/group-chat-coordination-response.md:84-88`) **[USER: G-REVOCATION /
G-AUTHORITY]**.

### 6.5 Verified-DID policy query (core/rotation-owned blocking API; group consumes)

```rust
// Constructible only from an authenticated session (attestation verified against transport key).
struct VerifiedDid(Did);
enum OpClass { DirectSession, EndpointUpdateRecipient, BlobServe, CallSetup,
               GroupServeTo /* host serving a member */, GroupPresentation /* UI only */ }
enum PolicyDecision { Allow, DenyBlocked, DenyNotContact }
fn did_policy(&self, who: &VerifiedDid, op: OpClass) -> PolicyDecision;
fn subscribe_policy_changes(&self) -> PolicyEvents; // cancel live sessions/retries/transfers
```
There is deliberately no `OpClass` for "validate/retain a group operation". Valid group ops
stay validated and retained for convergence regardless of personal blocks, and are filtered
only at presentation (`docs/endpoint-rotation-coordination.md:82-87`). The block/unblock
sync semantics and the existence of `DenyNotContact` are **[USER: G-BLOCK]**. Whether a host
may refuse to serve a blocked member, which is the `GroupServeTo` case, is **[GROUP]** plus
**[USER: G-BLOCK]**. That decision must not change group membership (AGENTS.md rules).

### 6.6 Audience-safe export (rotation-owned)

```rust
struct AudienceRecord { /* owner-signed record whose audience == target; no private fields */ }
enum ExportError { WouldDisclosePrivate(RecordId), NotInAudience, RecoveryRequired }
fn export_for_audience(&self, anchor: &RecordId, target: &Audience,
                       from: Option<&SuccessionCursor>, max: usize)
    -> Result<Vec<AudienceRecord>, ExportError>;
// Invariant (contract item 6): every returned record, AND every predecessor it references,
// has audience == target; a group path never names a private record ID or endpoint.
```
Open: whether group-audience and private-audience paths for one device share an anchor or a
record ID namespace. A shared ID correlates them; correlation is permitted, but the ID must
not let anyone fetch a private record. **[GROUP]** plus **[USER: G-KEYS]**. Under option
(ii), author evidence in group envelopes must also pass this filter (§3.1).

### 6.7 Agreement summary

| Adapter | Owner | Needs [GROUP] agreement | User gates touched |
|---|---|---|---|
| 6.1 verified succession | rotation | yes: result type, `Nomination` fields, full vs checkpoint | G-HISTORY, G-FORK, G-REVOCATION |
| 6.2 current route | rotation | yes: status enum replacing `bool`/`Option` | G-TIME, G-RETIREMENT, G-DISCOVERY |
| 6.3 author proof | shared | yes: writer ID inputs, `previous` continuity, path-for-posting | G-KEYS, G-HISTORY, G-REVOCATION |
| 6.4 host assignment | group | yes (theirs) | G-REVOCATION, G-AUTHORITY |
| 6.5 verified-DID policy | core/rotation | yes: `GroupServeTo` semantics | G-BLOCK |
| 6.6 audience-safe export | rotation | yes: ID namespace sharing | G-KEYS, G-HISTORY |

---

## 7. Not established by this document

- Nothing here was executed. Every option row is analysis, not a test result.
- Storage numbers are estimates under the stated assumptions, not measurements.
- B1/B2/B3 results were not available. §5.1 depends on them.
- No claim is made that either option gives device-compromise isolation under the shared
  mnemonic (F9).
- Group-sim fields cited are test-only fixtures. They are not wire formats
  (`design-models:crates/group-sim/src/authority.rs:1`, `docs/group-rotation-integration-contract.md:49-50`).
