# Group chat — conceptual acceptance tests

Status: **conceptual test specification; partial executable simulation now exists**.
`design-models:crates/group-sim` implements the first insert-only slice (see its README for exact coverage
and limits). The cases below are not all executable or passing; G1 remains incomplete.
Design: [group-chat.md](group-chat.md). Define the oracle before implementing the harness,
then implement pure tests, persistence tests, real iroh tests and app tests in that order.
No transport or UI wiring is justified merely by writing this file.

## 1. Fixtures and observables

Actors:

- A: creator/admin; B, C: members; X: outsider.
- A2: another attested device of A. Identity is DID; devices have distinct keys/Loro peers.
- H1, H2: independently authorized member-host devices. Neither is an authority.
- M: authorized malicious member or host, with its own legitimate keys.

Each test starts with isolated profiles and a verified genesis G unless it tests admission.
Use a second group G2 for scope/replay tests. Use deterministic clocks, keys, message IDs,
network scheduling and fault injection in the pure harness. Production randomness remains real.
Record a reproducible seed for generated schedules.

Observe more than rendered text:

- accepted envelope IDs and authored operation ranges;
- pending dependencies separately from accepted state;
- semantic meta/message state and accepted VV per document;
- persisted proofs/indexes and blob references;
- outgoing fan-out, storage and recipient acknowledgements;
- notifications, subscription/access decisions and bounded resource counters.

Compare canonical semantic state and operation sets, **not serialized snapshot byte equality**.
Pure tests use deterministic steps; socket tests use bounded eventual waits and report traces
on timeout. Do not use arbitrary sleeps as the sole correctness oracle.

## 2. Shared invariants

I1. No accepted mutation without valid original author proof and group-scoped authority.
I2. A forwarder's identity never substitutes for an author's identity.
I3. Invalid operations never enter accepted storage, snapshots, indexes or fan-out.
I4. Missing dependencies are pending, not authorized implicitly or rejected due to arrival order.
I5. Valid operation sets converge after all dependencies are exchanged, regardless of route,
    duplicate deliveries and delivery order.
I6. A host VV/refusal cannot erase accepted local operations or establish authoritative truth.
I7. Storage acknowledgement follows the specified durable commit boundary, not queue insertion.
I8. Host receipt, recipient delivery and read receipt are distinct facts.
I9. Every pull/subscription/blob request is authorized independently of address/hash knowledge.
I10. Pending work, queues, decoding and verification are bounded under malicious input.

For rejection tests, assert I3 explicitly—not just an error return or an empty UI.
For convergence tests, run a negative control: omit one dependency and confirm the oracle does
not incorrectly declare convergence.

## 3. Pure identity, authority and validation (G1)

| ID | Given / action | Expected oracle |
|---|---|---|
| P01 | A creates genesis; B verifies invitation | same G on both; creator signature and hash verified |
| P02 | alter genesis nonce/creator/version without re-signing | rejection; no group admitted |
| P03 | X reuses an invitation addressed to B | recipient binding refuses X |
| P04 | valid device attestation; then wrong DID/key or tampered attestation | valid admitted; invalid rejected before mutation |
| P05 | A2 authors using its own derived peer ID; then claims A's device peer ID | own authorized write accepted; impersonated operation range refused |
| P06 | reuse G permit/update against G2 or another shard | scope/signature binding refuses replay |
| P07 | member tries grant/admin/meta mutation; admin delegates allowed member/admin role | member refused; permitted delegation accepted under frozen grant cone |
| P08 | widen scope, exceed chain depth, delegate nondelegable grant, alter chain | each rejected with no accepted changes |
| P09 | valid envelope forwarded unchanged by H1; replace author proof with H1 signature | original accepted; substitution refused |
| P10 | unknown field/type, unknown root container, unsupported protocol version | reject atomically, bounded decoding |
| P11 | B changes C's text/file/author/id/time; tries physical message deletion | author/immutable/schema violations rejected |
| P12 | B validly edits/deletes its own message | accepted; tombstone/signature valid; unrelated fields unchanged |
| P13 | flip any signed envelope field or message attachment key | signature/semantic validation refuses; no blob ref created |
| P14 | receive envelope before required meta/dependency; later provide prerequisite | pending first; accepted only after full validation |
| P15 | supply forged prerequisite or impossible frontier | never accepted; bounded pending/request behavior |
| P16 | duplicate an accepted envelope or resend overlapping original batches | no duplicate messages/events/receipts or repeated mutation |
| P17 | legitimate concurrent same-author edits from A/A2 in every order | same accepted op set and deterministic field resolution |
| P18 | independent B/C posts on same day in every order | same messages and VV once complete |

P07, P12 and P17 require exact fixed schema/conflict semantics before executable assertions.
Do not validate an old envelope only against whichever merged present-day state arrived first.

### Mandatory policy gates (not yet settled)

Write the concrete expected outcomes here **before implementing these tests**:

| ID | Adversarial schedule | Decision needed |
|---|---|---|
| D01 | revocation vs truly concurrent post, both delivery orders | historical acceptance rule and dependency binding |
| D02 | removed member signs new post against old frontier with old timestamp | stale-frontier/backdating treatment; cannot infer creation time from signature |
| D03 | concurrent regrant/removal, competing admins, parent revocation after delegation | deterministic membership/cascade conflict rules |
| D04 | grant expires before delayed update arrives; receivers have different clocks | deterministic historical validity and current access policy |
| D05 | member offline across removal/epoch changes and later rejoins | activation/rejoin boundaries and withheld-dependency treatment |
| D06 | owner rotates assigned device slot/ID; conflicting succession or revoked predecessor | DID-signed succession preserves existing capability bounds without admin regrant; define fork/revocation/recovery policy |

G1 may develop the resolved P tests, but is not complete and must not enable group networking
until applicable D gates have explicit oracles. A malicious host is not the referee for them.

## 4. Persistence and proof retention (G1–G3)

| ID | Given / action | Expected oracle |
|---|---|---|
| S01 | valid update; crash before commit | no storage ack; recovery exposes no partially accepted state |
| S02 | commit; crash before fan-out/ack | restart retains update/proof/index; resend idempotent; reconciliation repairs delivery |
| S03 | host sends StoredAck; immediate forced restart | acknowledged operation/proof survives required durable boundary |
| S04 | corrupt local proof/snapshot/index | fail closed or reconstruct from verified proofs; never serve corrupted state as valid |
| S05 | evict cached history/blob under quota | truthful availability after eviction; accepted identity/meta integrity retained; no future retention guarantee implied |
| S06 | restart with pending dependency queue | defined bounded recover/re-request behavior; nothing pending becomes implicitly accepted |
| S07 | export missing operations after original batch overlap | original signatures preserved; no synthesized author-signed diff |

Document the storage durability contract used by S03 (including flush behavior). Process-crash
coverage is not evidence of power-loss durability unless explicitly tested.

## 5. Direct sync and hosted delivery (G2–G4)

| ID | Given / action | Expected oracle |
|---|---|---|
| N01 | three direct members independently post during partition, then reconcile | all valid posts/proofs converge without replacing a replica |
| N02 | A reaches H1; B offline; A disconnects; B later reaches H1 | B receives/validates A's original envelopes without A online |
| N03 | H1/H2 partition; members write on both sides; reconnect hosts | meta/dependencies then messages converge; each host's posts retain original author |
| N04 | stop H1 during delivery; B connects H2 with missing data | no false recipient ack; catch up when H2 obtains missing proofs |
| N05 | all hosts down, B/C directly reachable | direct authorized sync works; no host required for correctness |
| N06 | no reachable replica overlaps recipient | stays pending/not-delivered; recovery when overlap returns, not imaginary offline delivery |
| N07 | restored/stale host advertises lower VV or asks client to replace state | client retains accepted local operations and supplies missing originals |
| N08 | host advertises fake high VV or false receipt | local acceptance/delivery not inferred from claimed VV; retention cannot be guaranteed against lying host |
| N09 | drop pushes/acks; duplicate and reorder frames; restart session | anti-entropy repairs gaps; no duplicate user notifications |
| N10 | slow subscriber fills bounded queue while another member posts | bounded memory; healthy subscriber progresses; slow member later catches up |
| N11 | unauthorized X knows endpoint/group ID and tries Hello/pull/subscribe | no protected records/keys/inventory revealed; session limits enforced |
| N12 | authorization changes while subscribed or streaming history | informed host rechecks serving/fan-out; cutoff behavior specified for in-flight work |
| N13 | connection carries G and G2, requester authorized only for G | no cross-group permissions inherited |
| N14 | malicious host forwards forged author envelope/raw snapshot | recipients independently reject; malicious host cannot force rollback |

Use original proof fixtures from pure tests, then run equivalent scenarios with real iroh and
isolated profiles. Exercise direct and relay-assisted routing where supported.

## 6. History, shards and attachments (G5)

| ID | Given / action | Expected oracle |
|---|---|---|
| F01 | posts straddle UTC day boundary; receiver in different timezone | identical doc addresses; presentation timezone has no effect on shard choice |
| F02 | join with history-all; only host online | verified meta + lazy paginated history; no trusting unsigned snapshot |
| F03 | host changes signed history/message/attachment or strips proofs | rejection without accepted state/ref mutation |
| F04 | old shard batch depends on operations absent locally | retrieve original dependencies; no DM-style vouched snapshot shortcut |
| F05 | two members publish identical plaintext attachments | independent random-key encryption; no expected ciphertext hash deduplication |
| F06 | sender stops; authorized H1 has ciphertext and referencing proofs | recipient fetches/verifies/decrypts from H1 |
| F07 | kill/restart provider mid-transfer; switch to another provider holding same ciphertext | bounded failure/retry, resumable valid bytes, correct final plaintext |
| F08 | X knows blob hash; member requests unrelated/unreferenced blob | authorization refuses; no keys or protected data disclosed |
| F09 | cancel, quota exhaustion, wrong key/hash, malformed ciphertext | safe terminal/retry state; no corrupt attachment presented as valid |
| F10 | since-join admission with old shard/dependency requests | **deferred** until proof/dependency and confidentiality policy is specified |

Initial scope is history-all. Time/shard validation must not depend on receiver arrival time
in a way that makes valid delayed group history disagree across devices.

## 7. Host runtime and user-visible behavior (G3–G5)

Run the same engine contract for desktop, headless and Android; passing one is not evidence
that another survives OS lifecycle behavior.

| ID | Given / action | Expected oracle |
|---|---|---|
| R01 | nominate device without local hosting consent; enable locally without assignment | neither condition alone activates an authorized host |
| R02 | desktop sleeps/wakes, headless restarts, Android process dies/restarts | honest presence/reachability; recovered cache; clients reconcile |
| R03 | Android host screen off/doze/reboot/network switch on physical phone | measured availability/recovery; explicit foreground notification and supported start behavior |
| R04 | hosting storage/network limit hit, user disables hosting | bounded work; stop advertisements/serving appropriately; group history elsewhere unaffected |
| R05 | chat host lacks call-host capability | no implication it can host a call; chat remains usable |
| R06 | only H1 stores message; B has not received it | UI shows replica storage, not recipient delivery/read |
| R07 | recipient imports but loses ack; later ack arrives | delivery changes only on verified recipient acknowledgement; no duplicate message |
| R08 | new message while backgrounded vs conversation visible | notification policy respected; host forwarding does not change author attribution |

Phone eligibility requires supported build/security/storage conditions. Do not use emulator
success alone as evidence of old-phone always-on suitability.

## 8. Integration boundaries

Blocking/address rotation are owned by the separate agent. Define integration contracts,
not a second implementation:

- Personal DID block filters direct access/notifications and original-author presentation;
  valid shared-group operations remain retained for convergence.
- Host selection honors local block policy; policy changes do not rewrite shared membership.
- Address updates use lightweight authenticated sessions, independently of calls. Host lookup
  consumes verified endpoint generations and owner-signed device-slot succession, never an
  unverified presence advertisement.
- Device-ID succession tests: owner-signed replacement retains nominated hosting capabilities
  without admin regrant; another DID cannot replace it; replacement possession is proven;
  replay/rollback and capability widening fail; sibling slots are not overwritten. Old author
  proofs remain verifiable. Forks, revoked predecessors and offline recovery need frozen oracles.
- A blocked peer learning an endpoint still cannot gain authority by knowing its address.

Require shared fixtures/tests for these boundaries once the other agent freezes its API.
Group-call selection, media encryption and failover need their own spec/test plan (G6), not
implicit coverage from chat host tests.

## 9. Harness implementation sequence

1. Freeze the signing format, schema and applicable D policy oracles; record kunki/courier
   source revisions for reused code.
2. Pure fixture builders: identities/attestations, genesis, delegation, original envelopes.
3. Pure replica simulator: deliver/defer/drop/duplicate, inspect accepted/pending sets;
   permutations for small traces, seeded schedules for larger traces.
4. Storage harness: temporary sealed stores and explicit crash-boundary injection.
5. Headless peer harness: real iroh, temporary profiles, structured events/assertions;
   process kill/restart/partition cases. Route app-independent host behavior through core.
6. Desktop/Android UI and lifecycle acceptance on the same verified engine.

Implement tests per stage before production wiring. Do not create ignored tests that pass CI
while claiming their security behavior is covered. Track unresolved policy gates separately.
Use `scripts/buildlock.py` for cargo/Gradle builds; rerun existing DM/call regression suites.
