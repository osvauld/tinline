# Rotation + group: decisions needed from the user

Status: coordinator digest, 2026-10-10. Collects the policy questions raised by the Wave 1
packets. Section 0 records what the user has decided; everything after it is still open.
"Coordinator lean" lines are recommendations to evaluate, not selections. Source documents hold
the evidence and tests; read them before answering.

Packets merged so far: A1, A2, B1, B2, B3, C1, C2, D1, D2, E1, G-A, G-B, G-C, G-H, K1, C3 — all of Wave 1. Final regression at 7ff55f0: proto 166,
p2pcore 129, group-sim 73, Python 6 + 10 + 14 — all passing. G-B, G-C (their questions get appended when they land).

## 0. Decided by the user (2026-10-10)

Recorded from conversation with the user. These supersede the matching leans below.

**U1 — Discovery = option I (answers Q1 / E1 §10 Q8; E1 Q6 forward pointer = ruled out).**
No server, no lookup: each device is responsible for announcing its own rotation.
- Private audience: the device sends a DID-signed "new endpoint" note directly to every
  unblocked contact. Contacts that were offline get it when either side next reaches the
  other (the side whose address is still live retries).
- Group audience: the device sends the note to the group host, which relays it to members.
  When the host itself rotates, members that got the note forward it to members that missed it.
- Both sides rotated while apart: recover through a shared group's host if one exists,
  otherwise the app shows "lost contact, re-share ticket/QR" (manual re-share).
- No public DHT forward pointer and no predictable/derived future addresses.

**U2 — Two iroh endpoints per device, one DID.** A *private* endpoint known only to accepted
contacts, and a *group* endpoint known to group members via the host. Same DID; two separate
transport keys, each DID-signed per audience and each rotating independently. Group traffic
must never reveal the private endpoint. (Battery cost of two endpoints on Android to be
measured before production; group endpoint may only need a link to each group's host.)

**U3 — 1:1 chat with a group member goes through a request via the host.** Requester sends a
chat request to the target through the host, end-to-end encrypted to the target. If the target
accepts, the private endpoints are exchanged inside that encrypted exchange; afterwards the two
chat directly and the host is not involved. The host must not learn the private endpoints.

**U4 — Separate long-lived author key (answers Q2 = option (i)).** Content signing / Loro peer
identity uses a long-lived per-device author key; the two transport keys rotate alone. (Implied
by U2: content cannot depend on two rotating network keys.)

**U5 — Silent blocking.** A block is not announced to the blocked party; their traffic is
dropped. The blocked contact is excluded from the next private rotation note, so they lose
reach to the private endpoint at the next periodic rotation ("silent until the key rotates").
Blocking still does not *trigger* a rotation. A personal block does not change group membership.
Note: today `crates/core/src/node.rs:280` documents an explicit `Reject` to blocked callers,
which is not silent; W2-B characterises this.

**U6 — Goal.** Working group chat for launch, with all safeties including blocking. Production
work starts after the Wave 2 simulations pass and D01–D05 (section 6) are answered.

**U4 revised (user, 2026-10-10) — no device or transport key in any content.** Group (and 1:1)
content is signed by the DID key itself, plus a random per-device writer ID (for Loro peer ID
and per-device ordering) that is NOT derived from any device or transport key. Rationale: today
the device key is the iroh EndpointId (`crates/core/src/node.rs:1781`) and also signs messages
(`crates/core/src/chat/doc.rs:172`), so every message carries a dialable address. Rule: no key
that appears in content may ever be a network key. Replaces "separate long-lived author key".

**U7 (user, 2026-10-10) — only the host knows members' group endpoints.** The host holds each
member's group endpoint so it can deliver; members only know the host's endpoint and receive
DID-signed content from it. Members never receive other members' endpoints or device IDs.
1:1 chat is requested through the host (U3) and then runs directly. Consequences to model:
no member-to-member mesh sync (all group traffic via the host); the host sees all members'
group endpoints and delivery metadata; host offline = group stalls until it returns
(multi-host / host succession is an open question, see D01–D06).

**U8 (user, 2026-10-10) — push the signed device list to contacts when it changes.** Today the
DID-signed `DeviceList` (`proto::sign_device_list`; `crates/core/src/node.rs:2316`) travels only
inside call hello/accept (`node.rs:2510`, `node.rs:2754`), so adding a device tells no contact.
Decision: whenever the list changes (device added, unlinked, restored, or rotated) the device
sends the new DID-signed list (higher seq) to every unblocked contact; one message carries the
device list and private endpoints (combined with the U1 rotation note). Group side: only the
host receives group endpoints (U7). Any phrase holder can sign a list (shared-mnemonic
limitation); concurrent edits by two own devices are the Q4 fork question. Also addresses the
K1 restore stall (contacts reject a key missing from their stored list).

**U9 (user, 2026-10-10) — group removal by admin flags.** Admins can flag a member; at 3 flags
the member is removed and their group grant (token) is revoked. Flags and the removal are
DID-signed group ops every member verifies (not host fiat); with U7 the host also stops
relaying for and accepting from the removed DID and drops its group endpoint. Removal timing
for concurrent/offline posts is still D01–D05. Open sub-questions (section 8).

**U5 revised (user, 2026-10-10) — blocking is visible to the blocked party.** A contact blocked
in individual chat, and a member removed from a group (U9), is told so explicitly. This
replaces "silent blocking". Unchanged: a blocked contact is still excluded from later private
rotation notes / device lists (U8) and its traffic is refused; blocking still does not
trigger a rotation; a *personal* block still does not change group membership (only U9
removal does, and that removal is announced to the removed member). Consequences: W2-B/W2-Q
"indistinguishable from offline" tests now encode the rejected option (relabel as
candidate-policy G-BLOCK); today's explicit `Reject` (`node.rs:2168-2177`) is closer to the
decision but says "not accepted" — wording/shape of the notice is open (section 8b).

**U10 (user, 2026-10-10).**
- Removal threshold: 3 flags, kept simple (no withdrawal/expiry variants). Whether the 3 must be
  distinct admins and what happens in groups with fewer than 3 admins: confirm (section 11).
- Every device verifies the signature of every message and every op (flags, grants, host
  permits, rotation notes, device lists) — no trust in hosts or relays. Invariant.
- When a host is appointed, every member is updated (signed host permit delivered to all).
- K1 restore stall for legacy device-key-signed history: accept history per message, not
  all-or-nothing (section 12 Q1 option b).

**U11 (user, 2026-10-10) — D01/D02: signed frontier (option A).** A post is kept if its signed
frontier predates the removal, in every delivery order. Honest offline/concurrent posts survive.
Accepted cost: a removed member can backdate posts by citing an old frontier (D02; W2-F
finding 6 applies the same to a removed admin's ops). Implication (W2-F finding 4): for this to
help under U7, hosts must accept a removed member's upload of posts whose frontier predates
the removal (section 11 Q8 -> yes). Open: bound that upload (count cap or a cut-off) so a
removed member cannot keep injecting backdated posts indefinitely?

**U12 (user, 2026-10-10) — hosts rotate after a member removal, announced, not immediate.**
After a member is removed, every host announces an upcoming rotation of its group endpoint
(signed, to remaining members only) and performs it after a delay. After the rotation the
removed member holds only dead host addresses. The host reconnects members itself (section 9
Q6), so members need not do anything. Coordinator note: the delay doubles as the U11 upload
cut-off — the removed member's pre-removal posts are accepted until the hosts rotate, then it
cannot reach them. Open: delay length; batch several removals into one rotation; does a
member removal also rotate members' own group endpoints (W2-F says unnecessary under U7)?

**U12 detail (user):** host rotation delay after a removal = 2 days. Refusal is immediate: from
the removal on, the removed/blocked DID cannot send new messages or call. (Under U11, posts whose
signed frontier predates the removal are still accepted until the hosts rotate.)

**U13 (user, 2026-10-10) — separate capability tokens for messaging and calling.** Some contacts
may only message; calling needs its own token. Today a contact grant exists with one capability
`cap = "call"` (`crates/proto/src/lib.rs:34`, `:542`, checked at `:570`), 1-year TTL
(`crates/core/src/node.rs:33`), renewed on answered calls (`node.rs:2753`), with a revocation
set. Proposal to model: caps `message` and `call` as separate grants (or a cap set); the
issuer is the only verifier, so revoking = issuer refuses (local), and blocking revokes both and
sends the visible notice (U5 revised). Defaults DECIDED (user, 2026-10-10): ticket/QR contacts get
message+call; contacts from a group chat request (U3) start message-only; calling is upgraded
explicitly ("allow calls" sends the call token); existing `call` grants migrate to
message+call.

**U14 (user, 2026-10-10) — no per-recipient group encryption for launch; hosts are trusted members.**
Hosts are members' devices and are trusted nodes: each keeps a full copy of the group Loro doc
and may read it. Group updates are protected by QUIC (hop by hop) in transit and the device
vault (AES) at rest; the host merges plain Loro updates. A host forwarding content is treated
like any member sharing it; a misbehaving host is handled by revocation + group rotation (U12).
Per-recipient sealing (or a rotating group key) is required only if a NON-member host binary
(relay-only, own DID) is ever shipped — deferred. Resolves section 10 Q7 and closes the
"undetected rogue host" item as accepted for launch.

**U15 (user, 2026-10-10).**
- U9 threshold: any 3 flags count; they need NOT come from 3 different admins (section 11 Q1).
  Consequence: a single admin can remove a member by flagging 3 times.
- U13 token shape (G-CAPSHAPE): separate `message` and `call` tokens (section 14 Q1).
- W2-U: no count cap on pre-removal uploads — the host rotation is the only cut-off; hosts that
  learn late align to the earliest announced deadline (section 13 Q1, Q4).

**U16 (user, 2026-10-10) — writer ID = app-supplied, stable, verified by the app.** Each device
stores one random value (per account; never derived from any device or transport key) and its
writer ID = first 64 bits of hash(DID || device value). The app sets it as the Loro peer ID
(`set_peer_id`) and keeps it stable. Every post carries the device value; every receiver
recomputes the hash and refuses a post whose Loro ops use any other peer ID (U10). All replicas
use the sender's number unchanged. No claim table / claim round, no cross-host agreement.
Cross-DID copying needs ~2^64 work; same-DID copying by a phrase holder is the accepted
shared-mnemonic limitation. Resolves section 9 Q1/Q2 and section 10 Q4. Exact hash function and
domain separation to be fixed in the production spec.

**U17 (user, 2026-10-10).** (a) P9b before release: the writer value/ID is bound into the
DID-signed bytes, and the device value is tied to a stable install ID, not the device key.
(b) DID signatures become REQUIRED per contact once that contact has sent DID-signed versions
(no fallback to the device signature for them afterwards). (c) Merge phase 1 into `dev` after
P9b lands and the integration suite passes.

**U18 (user, 2026-10-10) — no backward compatibility before launch; data can be wiped.** The app
is not publicly launched. Existing installs (incl. the user's devices on 0.1.31) may be wiped, so
chat needs no compatibility with pre-P8 apps or old data. Consequences: drop the transition
machinery from P8/P9/P9b (device-key message signature, DID signature hidden in Loro commit
messages, legacy device-derived peer IDs, compat switch, per-DID "seen DID-signed" cut-off).
DID signature + writer binding become proper record fields; every op must be in a change
proven by its author's DID signature (closes the P9b squat gap without option (a)). Old apps
must fail clearly (protocol/version bump), not silently. Supersedes U17(b).

**U19 (user, 2026-10-10) — "Block" is for contacts, "Remove" is for groups.** A contact has no
separate Remove action: the contact-level action is Block (keeps the contact, signed visible
notice, refusal with the blocked reason, Unblock available). "Remove" only means removing a member
from a group (U9 flags). Today's `remove_contact` becomes Block. Answers section 15 Q1.

**U20 (user, 2026-10-10) — the blocked person's app enforces the block, and a block can request
deletion of our contact.** Two layers: (1) our shipped app on the blocked side honours the signed
notice — stops sending and calling, disables the composer and call button, no background retries;
(2) the blocker's devices still refuse anything that arrives and exclude the blocked person from
address changes (backstop for modified open-source builds). The signed block notice may also carry
a deletion request: the blocked person's app deletes our contact entry (address, tokens, device
list). Honoured by the shipped app; not enforceable against modified builds, and they still know
our DID. Open: does the deletion request also remove the chat history on their side, or only the
contact entry? Is it always sent with a block, or a separate choice in the block dialog?

**U21 (user, 2026-10-10) — group calls are relayed through one call host, never a member mesh.**
Members connect only to the selected call host (keeps U7: no member learns another member's
endpoint). The host forwards Opus packets without decoding or mixing (SFU-style); clients mix
locally. To bound host uplink, the host forwards only the 2–3 loudest speakers. The host must be
able to run on a phone when needed (CPU near zero, bounded uplink); a desktop with a stable
connection is preferred when one is available. Call hosting is a separate `host_call`
capability, not implied by chat hosting. Encryption is hop-by-hop QUIC; the host can hear the
call, accepted under U14. Launch failover: if the call host drops, the call ends and members
redial through the next host — no live hand-off. Spec and test plan: G6 (`group-call-spec.md`).

**U22 (user, 2026-10-10) — P9c confirmed.** No per-message hiding (every device reaches the same
verdict, so an unprovable op refuses the whole batch); own-device sync protocol bumped with chat
(`tinline/chat/2`, `tinline/self/2`). Merged into fixes-phase1 as `cf46f7a`.

**U23 (user, 2026-10-10) — priority: consent-based blocking first, rotation second.** The main
protection is blocking that our shipped apps honour on both sides (signed notice, the blocked
person's app stops sending/calling and can delete our contact, the blocker's devices refuse).
Endpoint rotation is the backstop for truly malicious users running modified clients; it follows
after blocking (P5) and message/call tokens (P6).

Still open after U1–U6: Q3 (history), Q4 (forks/revocation), Q5, timing (section 2), D01–D05,
the K1 restore-bug fix (section 5), and the new questions Wave 2 raises (chat-request decline
vs silent, whether a personal block hides that member's group messages locally, unblock
semantics).

## 1. Highest leverage (unblock the most work)

### Q1 — Discovery baseline (G-DISCOVERY, G-AUTHORITY)
Source: `rotation-linked-discovery-options.md` §4, §10; `scripts/test_rotation_linked_discovery_options.py`.

Option I = independent random endpoints, authenticated updates to reachable peers, explicit
recovery when both sides were offline across a rotation. Options P (shared prediction) and R
(rendezvous store) add offline discovery but: under every option a hostile holder of the
recovery phrase who reaches any still-live old address regains discovery; mnemonic-rooted
variants make the phrase a permanent discovery key; prediction hides clones; a public forward
pointer leaks new addresses to blocked contacts.

E1's 8 yes/no questions (§10). The key one: **ship option I first, with offline discovery
decided later?** Coordinator lean: yes; also answer Q6 (rule out forward pointers) = yes.

### Q2 — Author key vs transport key (G-KEYS)
Source: `rotation-author-group-adapter.md` §3.1 table; `rotation-author-key-trace.md`.

Today one device key does ~10 jobs (QUIC identity, chat batch and per-message signatures,
self-sync, Loro peer ID, ...). Options:
- (i) a separate long-lived author key per device path; transport key rotates alone.
  Rotation then touches only networking; old messages stay verifiable with one key per path;
  storage ~2 KB/contact over 5 years. Clone = long-lived Loro peer collision (G-FORK).
- (ii) author key rotates with the transport key, history retained. Every rotation strands
  unacked ops, breaks ticks unless `mc/` schema changes, grows the account-doc VV, breaks
  group-sim's `previous` chain, and needs ~0.8–2.4 MB/contact over 5 years (daily rotation).
Restore already changes the key today, so the restore fixes (stranded ops, ticks, retained
historical keys) are needed under BOTH options.
Coordinator lean: (i). Needs group-side agreement on the author/writer-ID mapping.

### Q3 — History: strict chain vs checkpoint (G-HISTORY)
Source: `rotation-history-policy-options.md`; `crates/proto/tests/rotation_history_paging.rs`.

For 1000 rotations: strict = 1001 signature checks / 1000 records / ~776 KB; checkpoint =
43 / 42 / ~32 KB. Checkpoints silently miss a fork or revoked record between the receiver's
head and the checkpoint, and **cannot satisfy the current group contract** ("full ancestry
since nominated root"). Strict needs senders to retain history back to the oldest offline
contact (unbounded). Options: strict everywhere / checkpoint everywhere (group contract
changes) / hybrid: strict for group audiences, checkpoint for private.
Coordinator lean: depends on Q2 — under (i) routing history no longer carries authorship, so
a bounded/checkpointed private history becomes much less risky. Decide Q2 first.

### Q4 — Forks and revocation (G-FORK, G-REVOCATION, G-AUTHORITY)
Source: `rotation-fork-revocation-options.md` §4–§8; `crates/proto/tests/rotation_fork_revocation.rs`.

Tested facts: forks are always detected when receivers retain per-record evidence, never
resolved by arrival order. Under the shared mnemonic **no** option excludes a hostile linked
device: it can sign successors, "recoveries", and even revoke the owner's records (DoS).
A separate recovery key (needs G-AUTHORITY change) stops hostile revoke/recover but not
hostile DID-signed successors. A checkpoint at a higher generation from a clone silently
replaces the honest one; strict history flags it.
1. On a fork: freeze the path (any mnemonic holder can then DoS it) or keep the first-accepted
   head (race)? Who signs a resolution?
2. Revocation: (a) cascade — descendants suspended until owner-signed recovery; or (b) block
   only that record's endpoint, descendants stand. Is revoking the anchor a rotation or a
   group-nomination operation? Under (a), what happens to group proofs holding revoked
   descendants (group gate D06)?
3. Keep the shared-mnemonic model and document "only a new identity truly cuts off a device",
   or open a separate-authority design? (Matches E1 Q1.)
4. Must receivers retain per-record evidence for every audience (needed to detect older forks)?

### Q5 — Wire/input bounds and ID collisions (G-HISTORY, G-FORK)
Source: `crates/proto/tests/rotation_history_adversarial.rs` (B2; analysis summarised here).

Tested facts: all delivery orders/duplicates converge; splices, malformed frames, forged and
private-on-group input are rejected with state unchanged and before signature work where
possible; a lost cursor yields explicit `RecoveryRequired` and never lowers the high-water mark.
Path-local checks cannot see replacement-ID collisions with siblings or pruned history; a
registry-level ID index catches all of them but grows one entry per rotation forever.
1. Byte cap: per-record (also caps owner/audience string length) or per-page (forces byte-aware
   paging)? Values?
2. Pending-queue drop policy: refuse new arrivals (a mnemonic clone can occupy slots first) or
   evict oldest (a clone flood evicts honest pages); or evict farthest from cursor? Bound per
   path, per DID, or global?
3. After `RecoveryRequired`: full re-verify, checkpoint, or re-nomination?
4. ID collisions: reject or flag; registry scope per (owner, audience) or per owner; forbid
   endpoint reuse across paths; how long to keep the ID index?
5. Wire encoding: sign received bytes, or require canonical encoding? (Pretty-printed JSON
   currently maps to the same signed claims.)

## 2. Timing and renewal (G-TIME, G-RENEWAL)
Source: `rotation-time-contract.md` §7; `crates/proto/tests/rotation_time_contracts.rs`.

1. Default / min / max rotation interval.
2. Interval change: FromLastRotation with floor (tested: cannot be postponed) vs FromNow
   (tested: re-saving before each deadline postpones rotation forever).
   Coordinator lean: FromLastRotation with floor.
3. Jitter: yes/no, width, persisted?
4. Overdue on resume: one successor, then ResumePlusInterval vs AlignedGrid.
5. Clock skew tolerance for signed `issued_at` (currently 0: a sender 1 unit fast is rejected);
   should a clock rollback detected on reboot force rotation?
6. Signed validity = interval + grace; how much grace? (validity < interval = periodic gaps)
7. Rotation failing past validity: fail closed (unreachable) vs a renewal record (claims change);
   show a degraded state?
8. Confirm a suspend-counting clock (Android `elapsedRealtime`/BOOTTIME) and a device
   measurement task. Wall-clock-only scheduling was shown to break invariant 11.

## 3. Lifecycle / retirement (G-RETIREMENT, G-STORAGE)
Source: `rotation-lifecycle-hooks.md` §6–7.

1. Retirement: close the old endpoint immediately vs bounded overlap. Calls, waiting calls,
   transfers, link runs and chat sessions are listed with today's behaviour in §5.
2. Android never calls `lock()` or `stop()` today and core has no resume hook. Add a
   `resumed()` entry point + Android hooks (onStartCommand, network callback, ON_RESUME)?
3. Rotating while unlocked needs a new vault "re-seal under existing data key" function;
   otherwise the remembered Keystore unlock key breaks. Accept adding it?

## 3b. Storage ordering and failure (G-STORAGE, G-RETIREMENT)
Source: `rotation-storage-failure-contract.md` §8; `crates/proto/tests/rotation_storage_contracts.rs`.

Tested: crashing before/after every sender step never reuses a generation, never revives a
retired key, never serves the old key past its deadline. Announcing before commit (two keys for
one generation) and acking before commit (lost update) are shown as broken orderings.
Source finding: a real rotation commit would span `account.json` (vault secret, device_public)
and `account.redb` (registry); no single atomic write covers both today.
1. CommitFirst vs PendingThenPromote (two-phase)? Is a small journal acceptable for cross-file
   atomicity?
2. Replacement endpoint fails to start: FailClosed (outage) vs DegradedGrace (old key serves until
   min(deadline + grace, signed expiry), shown to the user)? What grace?
3. Open sessions at retirement: Terminate vs bounded overlap. Either way a long call is cut —
   is a dropped call at rotation acceptable for v1? (Coordinator lean: yes for v1, schedule
   rotation to avoid active calls if cheap.)
4. Wipe the old secret earlier than RetireOld? What if RetireOld keeps failing?
5. Must per-peer acked generations be durable?
6. Forced transport restart acceptable when closing the old endpoint fails?

## 3c. Proving possession of the new key (G-STORAGE)
Source: `rotation-challenge-options.md`; `crates/proto/tests/rotation_challenge_contracts.rs`.

(a) durable nonce counter written before each challenge (existing model) vs (b) challenge
bound to the authenticated QUIC session, nothing persisted. Tested: (a) costs 2 disk writes per
update and lets anyone holding a captured update force writes; no quota isolates peers from
strangers. (b) costs 1 write, strangers are refused before using capacity, but freshness relies
on the boot RNG and a proof can't be answered on a different connection.
1. Persist per challenge (a), or session-bound (b)? Coordinator lean: (b).
2. Must possession ever be proven asynchronously (queued/relayed)? Only (a) allows it.
3. If (b): explicit random challenge, signature over the TLS exporter value, or iroh's
   authenticated `remote_id()` alone?
4. Admission keying: per verified DID, per transport key, or global cap?

## 4. Product semantics
Sources: `rotation-baseline-report.md` §4–5; `rotation-author-key-trace.md` §6.

1. A contact's new device key resets their `verified` flag today. Should verification survive
   an authenticated rotation (safety number is DID-only)? Otherwise every rotation clears it.
2. Contact removal currently also blocks the DID. Keep removal and block as one action, or split?
3. Where should blocked contacts be filtered from address updates (nothing filters on the
   sending side today)?

## 5. CONFIRMED bugs in the shipped product (no rotation needed)
Source: `crates/core/tests/rotation_restore_characterization.rs` (K1; 8 tests, real iroh,
`// KIND: limitation` tests assert today's wrong behaviour). Restore-from-phrase on an existing
install already changes the device key, and:
1. **Chat stall (worst):** if a contact holds our signed device list and we had an unacked
   message at restore time, the contact rejects our history ("signature does not verify") and
   NO further chat from us reaches them (pending and new messages). Root cause: the contact's
   device list for us still names only the old key and is not updated from the live session.
2. **Dead ticket:** after restore (same display name) `my_ticket()` hands out the cached ticket
   naming the old key; redeeming it times out and creates no contact.
3. **Contact's other device** rejects our pre-restore messages relayed by its sibling once it
   learned our new device list.
4. **Ticks:** pre-restore messages go Delivered → Pending after restore.
5. **Verified flag** cleared on the contact's side (safety numbers unchanged — DID-only).
Not tested: O5 (old self entry tombstoned on restore).
Decision: fix 1–4 now on `dev` as a separate bugfix task (independent of rotation design), or
defer? Coordinator lean: fix 1 and 2 now; they are small, user-visible and rotation would make
them happen constantly. 3–5 depend on G-KEYS/G-HISTORY and the verified-flag policy (§4.1).
Also: should a DID-attested key seen on a live session be accepted for verifying history when
the stored device list lacks it? (root of bug 1)
**DECIDED (user, 2026-10-10): yes** — P1 verifies history with the live session's DID-attested
key as well (fixes-phase1 `cb65ee7`, `chat/engine.rs` `chat_apply_history`).

## 6. Group chat (D01–D06)
Source: `docs/group-policy-options.md` §6 (G-B; 23 tests over all delivery orders). D06's
rotation side is Q4 above.

Core trade-off, tested: "signed frontier" (accept posts whose cited frontier predates removal)
is order-independent and keeps honest offline posts, but a removed member's backdated post is
accepted in every order. "Removal wins" blocks backdating but also drops honest delayed posts,
can drop a post after showing it, and strands later posts on that device chain. Signed epochs
with cuts block backdating but drop un-cut offline posts; competing epochs need a tie-break.
Receiver-clock expiry makes history depend on clock and arrival time.
1. Keep honest offline/concurrent posts at the cost of stale-frontier backdating, or cut off
   unseen writes from removed members at the cost of honest posts? A mixed rule?
2. Must every removal carry the remover's seen-content frontier?
3. Are posts that disappear after a partition heals acceptable? UI? Do hosts keep serving them?
4. After a rejected post: strand later posts on that device chain, allow skipping, or require a
   new chain?
5. Regrant concurrent with removal: remove-wins, add-wins or seniority? Two admins removing each
   other: both removed acceptable? Tie-break?
6. Removing an admin: cascade to members they added? (note: cascaded members can still post by
   citing a stale frontier)
7. Can the creator be removed? What if no admins remain?
8. `expires_at` at all, or only an explicit admin expiry op? Local clock for current access only?
9. Signed epochs with cuts: wanted? issuer, tie-break, format, max length?
10. Authentic ops from non-admins: keep as inert records or refuse?
From G-H (group-sim hardening; 24 → 35 tests, fixed two simulator bugs: bulk repair in ID order
overflowed the pending bound; seeded RNG ignored the seed):
1. How does a member whose host binding expired while both were offline rejoin? (tests use an
   explicit re-bootstrap fixture; same question as E1 Q2)
2. Should an envelope promoted from pending that then fails validation be reported instead of
   silently dropped?
3. Sync must send in causal order or retry on `Limit` (receivers don't keep refused envelopes) —
   accept as a protocol requirement?

From G-C (group persistence harness S01–S07, `docs/group-persistence-contract.md` §8; 15 tests):
4. S04 corrupted local store on open: FailClosed (refuse to open) or Reconstruct (quarantine and
   rebuild from verified original proofs)? If Reconstruct, tell senders about quarantined acked ids?
5. S06: persist the bounded pending queue, or keep it in memory and rely on sender retry?
6. S03: must a host's StoredAck wait for fsync of the group store and the vault?
7. S05: whole-shard eviction with an identity manifest — right granularity? Advertise availability
   or only answer on request?
8. Re-verify every stored proof on each open, or trust AEAD + an integrity check?

## 7. Wave 2 questions (W2-R, W2-Q, W2-B, W2-G — after U1–U6)

Source files: `crates/group-sim/src/{host_relay,chat_request,block_in_group}.rs`,
`crates/proto/tests/rotation_silent_block.rs`, `crates/core/tests/block_characterization.rs`.

**Findings that constrain choices (tested):**
- Hiding a blocked member's group messages must be display-only. Filtering them out of the
  replica strands other members' replies forever (W2-G test 5).
- A blocking *host* that withholds relay strands everyone's later replies, including its own;
  refusing the member is de-facto removal and is observable — conflicts with U5 (W2-G 7, 8).
- With today's single device key, every group post leaks the private endpoint to the blocked
  member (W2-G 10). U4 (separate author key) is mandatory for blocking to work.
- Silent block of a chat request is indistinguishable from offline/unanswered (W2-Q 8).
- A mnemonic clone with a higher seq replaces the honest group endpoint at host and members
  (W2-R 11) — shared-mnemonic limitation.
- Blocking today (`remove_contact`, `node.rs:984`) is NOT silent: calls get an explicit
  `Reject` within 1 s (offline: ~20 s timeout), `fetch_older_history` fails fast (offline:
  timeout), ticket redemption is refused while online. Plain chat delivery looks offline (W2-B).

**Blocking (G-BLOCK)**
1. Blocker's view of a blocked member's group messages: shown, hidden, or collapsed
   placeholder (with or without author)? Coordinator lean: collapsed, display-only.
2. May a blocking host withhold relay? Lean: no — neutral relay only.
3. If you block the group's host, keep syncing through it?
4. Unblock: re-include at next rotation, or send current endpoint immediately?
5. Linked-device sync race: a sibling that rotates before the block syncs leaks its new
   endpoint until the next rotation (measured 110 ticks at interval 100). Accept, delay
   rotation notes until sync settles, or bound sync lag?
6. A note delivered just before the block stays usable until the next periodic rotation. OK?
7. Forwarding: an unblocked contact can forward a note to a blocked one. Accept (traffic is
   dropped anyway) or recipient-bind notes?
8. How production makes a drop look offline: refuse the QUIC connection from the blocked
   key, or hold until timeout? (Receiver may learn the DID only after the handshake.)
9. Do "Unavailable" mode and ticket redemption by a blocked DID go silent too?

**Chat requests (U3)**
10. Production sealing: HPKE (RFC 9180) or a Noise pattern? (Sim uses blake3 + AES-GCM.)
11. Decline: explicit signed refusal, or silent (looks like block/offline)?
12. Two messages (requester's private endpoint inside the request — target sees it before
    deciding) or three (endpoint only after accept)? Lean: three.
13. Stale request key: drop silently, or reply with a current-key hint (reveals online)?
14. Rate limit: host, target, or both; values?
15. Replay set: expire requests by `sent_at` so it can be pruned? Answer timeout value?
16. Accept that the host learns who requested whom and when (metadata) for launch?
17. Requests dropped while blocked: may they be accepted after unblock if redelivered?

**Host relay of rotation notes (U1)**
18. Catch-up: host sends full note history or latest only; receivers accept seq gaps? (Q3)
19. Same-seq fork: keep-first (members diverge) or freeze (any phrase holder can DoS)? (Q4)
20. Key notes per (DID, audience) or per device path? Per-DID makes two honest linked
    devices look like a fork.
21. Host silently dropping notes is undetected; is a host liveness/censorship check in scope?
22. No group path left: show explicit "lost contact — re-share" in the app?

## 8. Group removal by flags (U9) — open details

1. 3 flags from 3 *distinct* admins (lean), or any 3 flags?
2. Group with fewer than 3 admins: threshold = number of admins, owner alone, or no removal?
3. Can the owner/creator be flagged or removed? Lean: no.
4. Can an admin withdraw a flag before the threshold? Do flags expire?
5. Fixed threshold of 3, or a per-group setting chosen at creation?
6. Is the removed member told (explicit), or does the group just go quiet (silent, like U5)?
7. Can a removed DID be invited back later?
8. Posts the member made before/concurrently with the 3rd flag: kept or dropped? (= D01/D02)
9. Can admins be flagged by other admins (removing their admin rights too)?

## 8b. Visible blocking (U5 revised) — open details

1. ~~Notice shape~~ DECIDED (user): one DID-signed "blocked" notice sent when blocking, and the
   same reason returned on every later attempt. The blocked party's app shows the state and
   disables composing/calling (UX reinforcement; enforcement stays on the blocker's side).
2. Does the blocked person see it immediately, or only on their next attempt to contact you?
3. Wording in the app: "X blocked you" vs neutral "You can't message X".
4. Chat requests via host from a blocked DID: refused with the same notice?
5. Unblock: notify them, or just start accepting again?
6. Peer app attestation (user asked): should peers require proof the other side runs the
   publisher-signed app? Candidate: Android hardware key attestation (cert chain includes the
   app signing-cert digest; verifiable offline by peers). Costs: excludes desktop, custom
   ROMs/unlocked bootloaders, and self-built/F-Droid builds of an open-source app; needs
   Google's revocation list. Coordinator lean: not as a gate; never rely on it for security.

## 9. W2-H questions (host-only, DID-signed group model; `crates/group-sim/src/host_only.rs`)

Tested: DID-signed posts with random writer IDs converge across two own devices; members receive
no other member's device key/transport key/endpoint; the W2-G private-endpoint leak is gone.
Changes to U1 forced by U7: members cannot forward the host's rotation note (they hold no
addresses for each other); recovery is the host retrying a member's live endpoint, or an
overlap window on the old host endpoint. Host and member both rotated while apart = re-share.
Loro silently and permanently diverges on a writer-ID collision — a collision rule is required.
1. Cross-DID writer-ID collision: first claim at the host wins, loser redraws? Tell the loser?
2. Same-DID collision: reject later, or freeze writer (any phrase holder can stall a sibling)?
3. Writer IDs let members count a DID's devices and per-device activity. Accept, or require hiding?
4. Host directory keyed per device (random slot) or per DID? (= section 7 Q20)
5. ~~Backup host~~ DECIDED (user, 2026-10-10): a group can have MULTIPLE HOSTS. Members know
   every host's group endpoint; each host knows members' group endpoints; hosts sync with each
   other. Consequences to model: member rotation notes go to all hosts; any reachable host
   repairs the others; "first claim at the host wins" (Q1) needs a deterministic cross-host
   tie-break; removal (U9) must be applied by every host and one rogue host can still serve a
   removed member; more parties see member endpoints/metadata.
   Host details (user, 2026-10-10):
   - Owner/admins appoint hosts; proof is a permit in the existing grant/delegation chain.
   - Hosts are NOT necessarily admins (relay capability only). A host may also be a separate
     (headless) binary.
   - Max number of hosts: not decided (user passed).
   - Admins can remove a host (user wrote "newer admins can remove" — confirm: any admin, or
     only admins appointed after the host?).
   - A host is one specific device of some user, preferably a desktop (not phones).
   Open: does a separate host binary hold a user's recovery phrase (a linked device), or its
   own DID with a delegated host permit? Coordinator lean: its own DID + host permit, so a
   server never holds anyone's phrase.
6. ~~Host overlap window~~ DECIDED (user, 2026-10-10): after the host rotates, the HOST connects
   to the members itself (at their current group endpoints) and hands them its new endpoint;
   members do not need to find the host. No overlap window on the host side.
   Remaining gap: host and member both rotated while apart (W2-H test 7). Coordinator
   proposal: a member keeps its old group endpoint until the host acknowledges its new note,
   so the host can always still reach it.
7. Offline write queue while host is down: unbounded, refuse new, drop oldest? Value? UI?
8. Chat-request encryption key: per-DID key published via the host (as modelled) or rotating
   request keys (W2-Q)?
9. Sync outgoing chat requests between a DID's own devices (acceptance reaches all devices)?
10. Host sees every member device endpoint, rotations and who-requested-whom: OK for launch?
Superseded existing tests (device-key signing, member mesh, silent block) are listed with
file:line in `docs/group-host-only-model.md` §4 — to be migrated, none edited yet.

## 10. W2-M questions (multi-host model; `crates/group-sim/src/multi_host.rs`, `docs/group-multi-host-model.md`)

Tested: host permits chain to an admin and are checked by members; non-admin hosts can relay but
not change membership; with ≥2 hosts, "both rotated while apart" recovers via another host with
no endpoint retention; removed hosts are refused; removed/blocked parties get verifiable signed
notices. Limitations: first-claim writer-ID rule diverges permanently across hosts; a
phrase-sharing sibling can claim the host's device slot; a removed host keeps known member
endpoints until members rotate; a rogue host can keep serving reads to a removed member.
1. Max hosts: cap? who enforces? (each host holds every member endpoint)
2. Host binary: own DID + host permit (lean), or phrase-holding linked device? (A phrase-holding
   host can post/sign as that user everywhere; an own-DID host can only relay.)
3. Host removal: any admin, or only admins appointed after the host? Owner always can?
4. Cross-host writer-ID tie-break: lowest claim after all hosts saw it (stalls new writers while a
   host is unreachable; late-claim race needs an agreement round), or another rule?
5. Member keeps old endpoint until host ack: only when every known host rotated? time limit?
6. Separate host key so a phrase-sharing sibling can't claim the host slot? (must stay out of
   content per U4 revised)
7. Rogue host serving a removed member: acceptable for launch, or require group-content
   encryption with rekey on removal?
8. ~~Early rotation after host removal~~ DECIDED (user, 2026-10-10): yes — group-level rotation.
   When a host is removed (e.g. rogue), a legitimate host sends the signed revocation to all
   members, and the group rotates: every member and remaining host rotates its group endpoint
   and sends the new note only to permitted hosts. The removed host then holds only dead
   addresses and receives no new content. (This is a group-removal trigger, distinct from
   personal blocking, which still never triggers rotation.) Limits to test: content already
   held by the rogue host stays leaked; members not yet reached keep using the rogue host
   until a legitimate host reaches them; an UNDETECTED rogue host (not yet revoked) is not
   covered.

## 11. W2-F questions (flag removal + group rotation; `crates/group-sim/src/flags.rs`, `docs/group-flag-removal-model.md`)

Tested: 3 flags from distinct admins remove the member and revoke the grant on every device, in
all 120 delivery orders and across two partitioned hosts; forged / non-admin / host-only flags
are refused; the removed member is refused by honest hosts and gets a signed notice; host
revocation rotates the group and the revoked host reaches nobody.
Findings that constrain choices:
- Withdrawing a flag "before the threshold" can undo a removal some replicas already showed.
- Flag expiry by receiver clock splits replicas permanently (like D04).
- Removing the owner leaves the owner's grants and host permits valid — needs a rule.
- Under U7, "signed frontier" doesn't save a removed member's honest offline posts: honest
  hosts refuse her upload.
- Posts sent to a revoked host before the sender heard of the revocation are lost unless members
  re-send them.
- A removed admin can still sign ops citing an old frontier (D02 backdating, for ops).
Questions (U9 details in section 8 restated with new evidence):
1. 3 distinct admin DIDs (a DID's devices count once) or any 3 flags?
2. <3 admins: owner alone / threshold = admin count / no removal? Admin count fixed at flag time?
3. Owner removable? If so, what happens to grants/host permits the owner issued?
4. Flag withdrawal allowed? May a withdrawal concurrent with the 3rd flag reverse a removal?
5. Flag expiry: none / signed time / receiver clock?
6. Threshold fixed at 3 or per group?
7. Rejoin after removal: who readmits; must readmission lift all flags?
8. Posts around the 3rd flag: keep signed-frontier posts or removal wins? Should hosts accept a
   removed member's upload of pre-removal posts?
9. Admins flagging admins? Do a removed admin's earlier flags still count?
10. After a host revocation, should members re-send their recent posts to remaining hosts?
11. Is "each device rotates once when it first applies a host removal" the trigger? Should a
    member removal also rotate the group (the model says unnecessary under U7)?

## 12. W2-D questions (device-list push; `crates/proto/tests/rotation_device_list_push.rs`, `crates/core/tests/device_list_characterization.rs`)

Verified today: our device list reaches contacts ONLY inside call hello/accept
(`node.rs:2510`, `:2754`); adding or unlinking a device reaches no contact until a call; chat
sessions never carry it. Push (U8) works in the model: every unblocked contact gets each change,
blocked contacts get the notice instead, stale/forged/wrong-DID lists are refused.
**Key finding: push alone does NOT fix the K1 restore stall** when a message was pending at
restore — the contact accepts the new list but then rejects the history snapshot that contains
the old-key-signed message ("signature does not verify"). Confirmed against the real core.
Also: a list with seq = u64::MAX from any phrase holder pins every contact forever.
1. K1 stall fix (G-HISTORY): (a) receivers keep superseded keys of the same DID for verifying
   content only (cost: an unlinked install's new content is also accepted); (b) accept history
   per message, not all-or-nothing; (c) sender re-signs/splits pre-restore content.
   Note: under U4 revised (content signed by the DID key) this whole class goes away for new
   content — only legacy device-key-signed history needs (a)/(b)/(c).
2. Concurrent list edits by two own installs (G-FORK): signature tie-break (shipped; loses one
   edit) / union (loses removals) / freeze (DoS) / re-sign after own-device sync with
   removal-wins tombstones?
3. Seq bound: cap seq at now + skew, or reject large jumps?
4. Sibling that hasn't heard of a block pushes to the blocked contact: wait for block sync, or
   accept the window (as W2-B)?
5. Each pushed list naming a new device clears the contact's `verified` flag
   (`node.rs:3122`). Keep `verified` for DID-signed list updates?
6. Must the pushed list arrive before / bundled with content from the new key, or should
   content from an unlisted key be queued?

## 13. W2-U questions (delayed host rotation after removal; `crates/group-sim/src/removal_rotation.rs`)

Tested: refusal of new posts and calls is immediate at every host and member; hosts announce
only to remaining members, rotate once at +2 days and reconnect members themselves; afterwards
the removed member reaches nobody; forged/replayed announcements are refused.
Findings: during the 2-day window a removed member can backdate posts without limit (accepted
U11 cost, ended only by rotation); a count cap cannot tell honest backlog from backdating; the
group's real cut-off is the LAST host to rotate (each host times from when it learned); members
rotating too adds cost and nothing under U7.
1. Upload cap during the window: none (rotation is the only cut-off), or a count cap?
2. Several removals in one window: keep first deadline / extend to last / one rotation each?
3. Members do not rotate after a removal (hosts only) — confirm.
4. Hosts that learn late: align to the earliest announced deadline, or own 2 days?
5. A member removed during a pending window already saw the earlier announcement's time — OK?
6. Should removal also refuse direct 1:1 calls from that person that started via a group chat
   request (U3/U13 grants)?

## 14. W2-C questions (message/call tokens; `crates/proto/tests/capability_tokens.rs`, `crates/core/tests/grant_characterization.rs`)

Verified today: ONE `call` grant gates both calls and chat (chat `Auth` is wrapped as a
`CallHello`, `chat/engine.rs:331` -> `accept_call_hello`, `node.rs:2162`). No message-only
contact is possible; only the global "unavailable" switch separates calls from chat. Grants renew
only on an answered call. Nothing in core ever writes `State.revoked`; blocking works by removing
the contact (generic "not accepted").
Findings: revoking legacy grants by id leaks (every answered call minted a new id; core keeps no
record) — a per-DID cut-off works; today's `verify_grant` hard-codes cap "call" (`lib.rs:570`,
also in the contact handshake `:900`, `:953`), so a `message` cap breaks old peers' add-contact
unless encoded compatibly; one token with a cap set makes a downgrade cut messaging until the
new token arrives (separate tokens don't); sibling devices honour a revoked/blocked holder until
the revocation set syncs.
1. Token shape (new gate G-CAPSHAPE): separate message and call tokens (lean: cleaner downgrade)
   or one token with a cap set?
2. Legacy revocation: per-DID cut-off (lean) or by id? Start recording issued token ids?
3. Renewal: message token renews on activity near expiry, or fixed TTL + explicit reissue? Call
   token keeps renew-on-answer?
4. Sibling revocation sync window: best effort OK, or must refusal be immediate on every device?
5. Wire compatibility with un-upgraded peers (keep sending a `call` grant to old peers? version bump?)
6. Show "calls not allowed" to every authenticated contact (vs today's generic refusal)?
7. Unblock: revive old tokens or issue new ones?

## 15. R1a questions (acceptance tests; fixes-phase1 `crates/core/tests/{blocking,rotation}_acceptance.rs`)

1. Remove vs block: today `remove_contact` both forgets and blocks. The design keeps a blocked
   contact with an Unblock action. Separate actions? What does a removed-but-not-blocked contact
   see? (Tests assume block keeps the contact.) Coordinator lean: separate; remove = forget,
   they see "not a contact"; block = keep + signed notice.
2. Exact reason string for calls/messages/tickets: tests use "blocked".
3. After unblock: when does "blocked you" clear; are messages sent while blocked delivered?
4. Reason a message-only contact sees when calling (not "blocked"; no ring).
5. "Contacts told N of M": M counts unblocked contacts only? (tests assume yes)
6. Grace window length and default rotation interval.
7. How "lost contact" is detected (retired endpoint looks like offline); timeout?
8. Verified flag follows the DID and survives address changes and re-share? (tests assume yes)
9. ~~UI relies on the stored notice~~ DECIDED (user): yes. The blocked person's app keeps showing
   the signed notice it already received. Principle: apps are trusted for DISPLAY only, never for
   security; all enforcement is on the receiving side (refusal + exclusion from address changes).
10. Crash before the new key is stored: it must not become current (tests assume yes).
