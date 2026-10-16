# Rotation + group: decisions

Status: shrunk 2026-10-16. Section 0 records what the user decided (U1–U25 from 2026-10-10,
D11–D12 from 2026-10-16; some U-items are marked superseded). The full 2026-10-10 digest, with
every question and its evidence (sections 1–15), is
`docs/archive/rotation/rotation-open-decisions-2026-10-10.md`; the option studies it cites are in
`docs/archive/rotation/`. Current rotation behaviour: `docs/rotation.md`.

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
removed member cannot keep injecting backdated posts indefinitely? → answered by D12 (local
2-day window).

**U12 (user, 2026-10-10) — SUPERSEDED by D11 (rotation is periodic only) and D12 (upload
window). Kept for history.** Hosts rotate after a member removal, announced, not immediate.
After a member is removed, every host announces an upcoming rotation of its group endpoint
(signed, to remaining members only) and performs it after a delay. After the rotation the
removed member holds only dead host addresses. The host reconnects members itself (section 9
Q6), so members need not do anything. Coordinator note: the delay doubles as the U11 upload
cut-off — the removed member's pre-removal posts are accepted until the hosts rotate, then it
cannot reach them. Open: delay length; batch several removals into one rotation; does a
member removal also rotate members' own group endpoints (W2-F says unnecessary under U7)?

**U12 detail (user; rotation part superseded by D11):** host rotation delay after a removal = 2 days. Refusal is immediate: from
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
like any member sharing it; a misbehaving host is handled by revocation (rotation is periodic, D11).
Per-recipient sealing (or a rotating group key) is required only if a NON-member host binary
(relay-only, own DID) is ever shipped — deferred. Resolves section 10 Q7 and closes the
"undetected rogue host" item as accepted for launch.

**U15 (user, 2026-10-10).**
- U9 threshold: any 3 flags count; they need NOT come from 3 different admins (section 11 Q1).
  Consequence: a single admin can remove a member by flagging 3 times.
- U13 token shape (G-CAPSHAPE): separate `message` and `call` tokens (section 14 Q1).
- W2-U: no count cap on pre-removal uploads. ~~The host rotation is the only cut-off~~ — the
  cut-off is now D12's local window (2 days from when each host learned of the removal).

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
our DID. DECIDED (user): the deletion request removes the contact entry only; their chat history
with us stays (read-only). DECIDED (user): always sent with every block (no checkbox). Consequence: after an unblock the other
person must re-add us (scan QR / ticket) because their contact entry for us is gone; the unblock
notice only clears their "blocked you" state on the kept chat history.

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

**U24 (user, 2026-10-10) — chats are never deleted; media can be.** No "Delete chat" action:
chat history is kept (blocking keeps it too). Users may delete media/files to free space. The
now-unused `chat_purge` (P5) should be removed or limited to media. Messages written before a
block are delivered after an unblock + re-add (accepted). The "tried to message you N times"
counter comes later with the UI wiring.

**U25 (user, 2026-10-11) — group calls trust the call host.** "Trusting the host is the model
here. It's live and it's a group, they have to trust the host." The call host forwards the K
loudest Opus streams unchanged and labels them with roster slots; members mix locally. No
per-packet sender signatures: members cannot verify which DID a slot's audio came from, and the
host can hear the call (as U14/U21). The app discloses "Hosted by <name>'s <device> — the host
can hear this call".

**D11 (user, 2026-10-16) — endpoint rotation is periodic only.** A hosting device rotates its
group endpoint every interval (+ jitter), counted from its last rotation; removals, flags, host
revocations and blocking never trigger or reschedule it. Defaults (final): interval 7 days,
jitter up to 1 day (persisted per key), announce window = interval, no overlap (old endpoint
closes after the promote), a running call defers it ≤ 1 h. The policy is per device and
settable (`set_group_rotation_policy`). Answers §2 Q1–Q4 for the group endpoint, §3 Q1, §3b Q1
(two-phase: stage → bind → promote → close), §11 Q11, §13 Q2–Q5, §15 Q10.

**D12 (user, 2026-10-16) — removed member's upload window is local.** A host persists when it
first derives a member's removal (`rm_seen`, local clock) and refuses that DID's pre-removal
upload once 2 days (`REMOVED_UPLOAD_WINDOW_MS`) have passed. Never a record's `at`. Holds even
when the removed DID shares another group with the same host. Replaces U12/U15's "rotation is
the cut-off"; answers U11's open part and §13 Q1, Q4.

## Still open

Rotation:
- `RotationPolicy::MIN_INTERVAL_MS` (provisional 1 day) and any maximum.
- Private (1:1) endpoint rotation — next packet: grace window (§3b Q2, §15 Q6), the verified
  flag surviving a key change (§4 Q1, §12 Q5, §15 Q8), "lost contact" detection (§15 Q7),
  rotating while unlocked (§3 Q3), Android resume hooks (§3 Q2), clocks (§2 Q5–Q8).
  Backlog tests: `docs/design/rotation-acceptance-checklist.md`.
- Rotation notes: forks and history (§1 Q3–Q5, §7 Q18–Q22, §12 Q2–Q3).

Groups and blocking (unchanged by this shrink; numbering as in the archived digest):
- Removal details: §8 / §11 Q1–Q9 (distinct admins, < 3 admins, owner, withdrawal, expiry,
  rejoin, admins flagging admins), §11 Q10, §13 Q6.
- Group chat D01–D06 and persistence S03–S07: §6.
- Blocking and chat requests: §7 Q1–Q17, §8b Q2–Q6, §15 Q1–Q5.
- Device lists: §12 Q4, Q6. Tokens: §14. Host-only / multi-host model questions: §9, §10.
