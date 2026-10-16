# Rotation E1: linked-device discovery decision matrix

Status: proposal — not approved

Packet E1 (gate **G-DISCOVERY**). This compares three ways for sibling devices of one DID to
find each other after their endpoints rotate. **No scheme is selected.** No derivation
formula, ratchet or wire format is proposed. Nothing in production code changed. The
executable part is a symbolic Python model,
`design-models:scripts/test_rotation_linked_discovery_options.py` (14 tests, standard library only).

Sections 1 and 2 are **verified in source**. Sections 3–7 are **proposal/analysis**. Every
matrix cell comes from the model. None of it is a production or network test.

## 1. Verified in source: how discovery works today

Core:

| Fact | Where |
|---|---|
| The account endpoint is built with `presets::N0`, keyed by the per-install `device_secret` | `crates/core/src/node.rs:1780-1781` |
| Link endpoints use the same preset | `crates/core/src/sync/api.rs:68` |
| A test knob removes IP transports (relay only) | `crates/core/src/node.rs:1793` |
| Dial addresses are just `EndpointId` plus an optional relay URL hint. Without a hint, iroh's address lookup resolves the id | `crates/core/src/node.rs:3183-3190`, comment at `:3171` |
| Siblings are dialed only from the registry: non-removed keys plus retained relay hints | `crates/core/src/sync/engine.rs:659` |
| Any DID-attested, non-tombstoned key that reaches a sibling is registered (trust on first use by DID) | `crates/core/src/sync/engine.rs:255-282`, `crates/proto/src/lib.rs:780` (`accept_self_hello`) |
| Any mnemonic holder can attest any key (no expiry, no sequence) | `crates/proto/src/lib.rs:200` (`attest`); A1 report §4 O8 |
| Unlink tombstones the key. Erasure on the target is best effort and needs the target to be reachable | `crates/core/src/sync/api.rs:179`, `docs/design/device-linking.md:115-122` |
| "Once a contact connects to a known device…" does not solve discovery when all known addresses are offline | `docs/design/multi-device.md:28` |

iroh 1.3.0 (read in `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`; the version is
pinned in `Cargo.lock`):

| Fact | Where |
|---|---|
| `N0` adds `PkarrPublisher::n0_dns()`, `PkarrResolver::n0_dns()`, `DnsAddressLookup::n0_dns()` and the default n0 relays | `iroh-1.3.0/src/endpoint/presets.rs:113-140` |
| A pkarr record is published under a name derived from the **endpoint's own public key** and is signed by that same secret key. By default only relay addresses are published | `iroh-1.3.0/src/address_lookup/pkarr.rs:1-25` |
| The default republish interval is 5 min and the TTL is 30 s | `iroh-1.3.0/src/address_lookup/pkarr.rs:143-146` |
| The record name is `_iroh.<z32-endpoint-id>.<origin>`. Apps can attach `UserData` of at most 245 bytes | `iroh-dns-1.3.0/src/endpoint_info.rs:13-15,314`; `iroh-1.3.0/src/endpoint.rs:650,1685` |
| The signed packet is at most 1000 bytes of DNS data | `iroh-dns-1.3.0/src/pkarr.rs:18,28-30` |
| **A relay is not a mailbox.** A packet for an endpoint that is not connected is dropped ("no connected client, dropped packet") | `iroh-relay-1.3.0/src/server/clients.rs:200-211` |

Consequences (verified):

- Address lookup resolves **a known `EndpointId` → its current relay/addresses**. It cannot
  produce a *new* `EndpointId` after rotation. So discovery after rotation reduces to **learning
  the new id**. Once the id is known, reaching it is already solved by `N0` while the target is
  online.
- Lookup is public. Anyone who knows an id can resolve its relay. Discovery depends on a
  third party (n0 DNS, `iroh.link`). How long n0's server or the Mainline DHT retains records
  is **unverified**.
- There is no store-and-forward anywhere today. Both devices must be online at the same time.
  "Relay mailbox" (option 3) would be **new infrastructure**, not an iroh feature.

## 2. Existing guarantees audited (no rotation yet)

- Today every install keeps one endpoint forever. Siblings find each other through
  registry keys plus `N0` lookup. Offline-to-offline works only because ids never change.
- No guarantee excludes a hostile mnemonic holder. It can attest a fresh key and is admitted by
  any sibling it reaches (`engine.rs:255-282`). Rotation cannot change this (handoff §1,
  `device-linking.md:19-20`).
- No send-side filter keeps blocked DIDs from receiving endpoint information. Today
  `DeviceList` travels only in call/chat hellos that blocked DIDs cannot complete (A1 report
  §5).

## 3. Options compared

| ID | Option | What siblings hold | Private keys shared? |
|---|---|---|---|
| **I** | Independent random endpoint per rotation + authenticated sibling updates + explicit recovery route | Last verified endpoint per sibling | No |
| **P-d** | Shared prediction capability per epoch, distributed at link time and on refresh | Epoch capability, from which a sibling's **public** id per time window can be computed | Must be no. Whether a construction gives public-only prediction for Ed25519/iroh keys is **unverified** (§6) |
| **P-m** | As P-d, but the capability is held by anyone who holds the mnemonic | Same, plus the mnemonic implies the capability | Same as P-d |
| **R-d** | Random endpoints. Current endpoint published to a rendezvous store under an account-level read capability, distributed like P-d | Read capability | No |
| **R-m** | As R-d, with the read capability implied by the mnemonic | Mnemonic implies read capability | No |
| R-ptr | R variant: a public forward pointer from the old id to the new one (e.g. pkarr `UserData` under the old key) | Nothing beyond an old id | No |

The "-m" variants are modeled only as "mnemonic implies capability". How that would be
derived is deliberately not specified.

## 4. Decision matrix (executable)

Generated by `python3 design-models:scripts/test_rotation_linked_discovery_options.py --matrix`. The
test `test_decision_matrix_matches_documented_table` asserts every cell.

| Scenario | I | P-d | P-m | R-d | R-m |
|---|---|---|---|---|---|
| both online across rotation | found | found | found | found | found |
| mutual offline, both rotated | unreachable | found | found | found | found |
| mutual offline, skew 25 > tolerance | unreachable | unreachable | unreachable | found | found |
| restore from mnemonic only | unreachable | unreachable | found | unreachable | found |
| clone of whole device state | fork-detectable | fork-hidden | fork-hidden | fork-detectable | fork-detectable |
| honest unlinked, offline at unlink, no refresh | stale | locates-refused | locates-refused | locates-refused | locates-refused |
| honest unlinked, refresh at unlink | stale | stale | locates-refused | stale | locates-refused |
| hostile mnemonic, returns before next rotation | regains | regains | regains | regains | regains |
| hostile mnemonic, returns later, no refresh | excluded | regains | regains | regains | regains |
| hostile mnemonic, returns later, refresh | excluded | excluded | regains | excluded | regains |
| leaked discovery knowledge to blocked contact | stale | locates-refused | locates-refused | locates-refused | locates-refused |
| leaked, then refresh | stale | stale | locates-refused | stale | locates-refused |
| refresh while allowed sibling offline (strict) | allowed:unreachable removed:unreachable | allowed:unreachable removed:unreachable | allowed:found removed:found | allowed:unreachable removed:unreachable | allowed:found removed:found |
| same, old epoch kept as grace (listen) | allowed:unreachable removed:unreachable | allowed:found removed:found | allowed:found removed:found | allowed:found removed:found | allowed:found removed:found |
| same, old epoch kept for outbound dial | allowed:unreachable removed:unreachable | allowed:found removed:unreachable | allowed:found removed:found | allowed:found removed:unreachable | allowed:found removed:found |

Legend:
- **found/unreachable**: the first device obtains an id the target is listening on now.
- **stale**: the retained knowledge points only at retired ids.
- **locates-refused**: discovery succeeds, and the verified-DID or tombstone check refuses
  access.
- **regains**: an admitted session gives the hostile holder the current directory and
  capabilities.
- **excluded**: excluded from *discovery only*. DID authority remains (asserted in
  `test_hostile_mnemonic_regains_full_discovery_via_any_live_old_address`).
- **fork-hidden**: a clone and the original answer on identical ids, so no observer sees two
  successors.
- Column I has no epochs. In the refresh rows, "unreachable" there is the plain
  mutual-offline effect.

### What needs a reachable peer or fresh secret distribution

| Event | I | P-d | P-m | R-d | R-m |
|---|---|---|---|---|---|
| Normal rotation, overlapping online | sibling session | nothing | nothing | store reachable | store reachable |
| Both offline across each other's rotation | a sibling whose id is still known **and** live (a "stable" sibling), or in-person relink | nothing, if clocks agree within tolerance | same | store reachable + record retained | same |
| Restore from phrase | in-person relink (`device-linking.md` §6) | relink (capability is not in the phrase) | nothing | relink | nothing |
| Remove a device from discovery | nothing; leak ends after ≤1 rotation of each sibling | fresh epoch to every allowed sibling | **impossible without a new mnemonic** (G-AUTHORITY) | fresh read capability to every allowed sibling | **impossible without a new mnemonic** |
| Offline allowed sibling after refresh | — | reachable holder dials out with the old epoch, or relink | — | same | — |
| Leaked discovery knowledge | nothing; bounded by 1 rotation | fresh epoch | new mnemonic | fresh capability | new mnemonic |

## 5. Counterexample traces (tests)

Each item names the test and the trace it asserts. Labels are as used in the file.

1. `test_independent_mutual_offline_rotation_is_unreachable_counterexample` (candidate-policy
   G-DISCOVERY). Laptop is online at [0,5), [20,25) and [40,60). Phone is online at [0,5) and
   [40,60). The trace shows `t=20 laptop rotates`, then `t=40 phone rotates`. Both hold only
   generation 0 of the other, so they cannot reach each other. Anchoring overlap to the
   scheduled deadline matters here (see §7, item 3).
2. `test_independent_recovery_needs_a_peer_whose_address_is_still_known`. A desktop with a
   long interval recovers them (`session phone->desktop`). The negative control is a helper
   that also rotated while they were away: it does not help.
3. `test_prediction_clock_skew_beyond_tolerance_misses_both_directions`. Skew 5 works, skew 25
   fails in both directions at tolerance ±1 window. The cost is 2k+1 candidate ids per sibling
   per held epoch.
4. `test_prediction_clone_is_indistinguishable_no_fork_evidence` (limitation). Under P, the
   clone and the original listen on identical ids, so G-FORK has no evidence to detect. The
   control under I shows `('phone', 1)` with two endpoints.
5. `test_hostile_mnemonic_regains_full_discovery_via_any_live_old_address` (limitation, all
   options). A hostile device that returns while any of its retained ids is still live gets an
   admitted session and the full directory and capabilities. This matches current behavior
   (`engine.rs:255-282`).
6. `test_mnemonic_rooted_capability_cannot_be_refreshed`. A refresh under P-m or R-m logs
   `GATE G-AUTHORITY`. The same property is what makes restore work under P-m.
7. `test_discovery_never_implies_access_for_blocked_holder` (invariant). A blocked contact with
   leaked knowledge may locate an endpoint, but it never gets a session and sibling state does
   not change.
8. `test_public_forward_pointer_leaks_new_endpoint_to_blocked_contact`. Under R-ptr, a blocked
   contact holding only a generation-0 id follows 4 pointers to the current id, with no
   update ever sent to it. This breaks "exclude blocked contacts from updates" in spirit. The
   negative control is a capability-gated store, which does not leak.
9. `test_refresh_strands_offline_allowed_sibling_and_grace_serves_removed_equally`
   (invariant). A strict refresh strands an offline allowed sibling. A listening grace on the
   old epoch serves the removed device equally. After that, only the access check separates
   them, and a hostile mnemonic holder passes that check.
10. `test_outbound_dial_with_old_epoch_recovers_allowed_and_rearms_hostile` (limitation). If
    the refresher keeps the old epoch only for dialing out, it recovers the allowed sibling.
    It also dials the hostile removed device (`session phone->removed`), which shows a fresh
    attested key and receives the new epoch.
11. `test_rendezvous_unavailable_is_unreachable_not_proof_failure`. Store down means
    unreachable, and nothing is recorded.
12. `test_independent_honest_unlinked_leak_is_bounded_by_one_rotation`. Under I, the
    unlinked device holds generation 0, which the phone retired at t=10. Under P-d without a
    refresh, the same device still locates the phone.
13. `test_model_is_deterministic_and_has_no_derivation_formula`. Two runs give identical
    traces. Predicted ids are allocation-order labels.

## 6. External constructions: verified vs unverified

| Construction | Status | Relevance |
|---|---|---|
| iroh `N0` preset, pkarr publish/resolve, relay drop semantics, `UserData` 245 B | **verified** in source (§1) | Resolves a known id. A forward pointer (R-ptr) via `UserData` under the old key is technically possible, but it is public and needs the old key to stay alive to republish |
| pkarr / Mainline DHT retention period, n0 DNS server retention and availability guarantees | **unverified** | Determines R-* offline windows |
| Public-only child key derivation (BIP32 non-hardened style) that would let siblings compute public ids without private keys | **unverified**; not read in this packet. Applicability to Ed25519/iroh keys is unknown | The only way P-d/P-m avoids "siblings can impersonate each other's transport keys". Without it, P degenerates into shared private-key derivation, which the audit rejects (`endpoint-rotation-device-audit.md:30-32`) |
| Encrypting rendezvous records to an audience | **unverified**; no library inspected | Needed for R-d/R-m to keep pkarr-style public records away from blocked contacts |

## 7. Observations from building the model (analysis, not decided)

1. **P and R-m make the stable mnemonic a permanent discovery key.** Unlink, refresh and
   rotation cannot remove a hostile phrase holder. A new mnemonic changes the DID's key under
   today's model (G-AUTHORITY).
2. **Distributed capabilities (P-d, R-d) trade restore for revocability.** A fresh epoch
   excludes a removed device only until any holder of the old epoch dials out to it, or until
   it reaches a live old id. A hostile holder then rejoins with a fresh attested key. Under
   shared DID authority, refresh is hygiene against *honest* stale holders and leaks, not
   hostile ones.
3. **Overlap on overdue resume would undo retirement.** An earlier draft of the model gave the
   retired id a grace period starting at the resume time. That draft made mutual-offline
   reachable under I, because both devices answered on ids that were past their deadlines.
   Overlap must be anchored to the scheduled deadline. This agrees with the network model's
   "retire overdue before accepting" (G-RETIREMENT, G-TIME).
4. **I has the smallest leak and the worst offline story.** Retained knowledge goes stale
   after one rotation per sibling. Mutual offline, restore and the "all known ids rotated"
   case need a stable sibling or an in-person relink.
5. **P hides clones** (G-FORK). Determinism removes the divergence that I and R expose.
6. **R needs new or third-party infrastructure.** iroh relays drop packets for absent
   endpoints. Existing pkarr records are public and keyed by the id itself. R adds an
   availability dependency and a metadata observer, the store operator. The store sees
   publish timing and record count per account locator.

## 8. User-facing recovery limits (draft wording, proposal)

- "If all your devices are offline at the same time across an address change, they may not
  find each other automatically. Open the app on a device that stayed online, or link again
  in person." (I; R when the store is unavailable; P when clocks drift.)
- "Restoring from your recovery phrase gives you your identity, not your other devices'
  addresses. Link from another device." (I, P-d, R-d.)
- "Unlinking stops sync. It does not stop someone who has your recovery phrase from finding
  or impersonating your account. Move to a new identity to fully cut a device off." (all
  options; strengthens `device-linking.md` §5.)

## 9. What this does NOT prove

- Not cryptography. It does not show that any prediction or encryption construction exists
  or is safe. Authentication is an oracle.
- No relays, NAT, DNS latency, record retention, Android background limits or battery cost.
  One endpoint per device, integer time, instant in-tick delivery, ideal clones.
- The parameters `INTERVAL=10`, `OVERLAP=2`, tolerance ±1 and skew 25 are test-only.
- Contacts' discovery (vs siblings') is touched only through the blocked-contact leak rows.
  Audience separation (group vs private) is not modeled.
- Passing the matrix test means the doc and the model agree. It does not mean any option is
  acceptable.

## 10. Questions for the user (yes/no)

1. Do you accept that, under the current shared-mnemonic model, **no** discovery option can
   keep a hostile holder of the recovery phrase from finding and rejoining your devices?
   (Yes/No)
2. Is it acceptable that two of your devices which are both offline across an address change
   may need a third online device or an in-person relink to reconnect (option I)? (Yes/No)
3. Should restore-from-phrase alone be able to find your other devices, accepting that the
   phrase then becomes a permanent, unrevocable discovery key (P-m / R-m)? (Yes/No)
4. May the app depend on a third-party or Tinline-run rendezvous store, which sees
   publish timing and account-level metadata but not contents, for offline discovery
   (R-d / R-m)? (Yes/No)
5. Should we spend research effort on a public-only key-prediction construction for iroh
   Ed25519 keys (needed before P-d/P-m can even be evaluated), knowing it makes clones
   undetectable? (Yes/No)
6. Must a public forward pointer from an old address to the new one (R-ptr) be ruled out,
   because it reveals new addresses to blocked contacts who knew an old one? (Yes/No)
7. When refreshing a discovery epoch, may a device keep the old epoch for dialing out, so
   offline allowed siblings recover automatically, accepting that a hostile removed device
   can receive the new epoch the same way? (Yes/No)
8. Should option I (no shared discovery secret) be the baseline that ships first, with any
   offline-discovery addition decided later? This is a recommendation to evaluate, not a
   selection. (Yes/No)
