Status: proposal (G6). Docs only. No code implements it yet.

# Group calls through one call host (G6)

This spec turns U21 (`docs/design/rotation-open-decisions.md` §0) into a wire, control and
test plan. It builds on U7 (only hosts know members' group endpoints), U12 (host rotation
after a removal; refusal is immediate), U13 (separate message and call tokens), U14 (hosts are
trusted members; hop-by-hop QUIC only) and U20 (the blocked person's app enforces the block).

Markers used below:

- **[V]** means verified in code at the cited `file:line`, on branch `g6/call-spec` at `345a401`.
- **[A]** means assumed or estimated, and must be measured or confirmed.
- **[P]** means proposed here and not yet decided.

## 0. What exists today (1:1 calls) and what is reused

| Piece | Where | Reuse in G6 |
|---|---|---|
| Live audio codec: Opus, 48 kHz mono, 20 ms frames, VBR, complexity 6, in-band FEC, DRED, DTX off | `crates/audio/src/lib.rs:14-15, 292-307` [V] | Unchanged on every client. |
| Call bitrate is **48 kbps** | `crates/core/src/node.rs:2388` (`audio::Sender::new(48_000)`) [V] | This is the budget baseline in §4. |
| Datagram `[seq u32 BE][level u8][opus]`; `level` is an RFC 6464 −dBov value computed by the sender | `crates/audio/src/lib.rs:7-8, 55-63, 316-317` [V] | Carried unchanged inside the group datagram. The level byte is what drives speaker selection. |
| Adaptive jitter buffer (64 slots, 40–200 ms target, PLC and FEC, resync after a 1 s jump) | `crates/audio/src/lib.rs:21-36, 354-560` [V] | One `Receiver` per forwarded speaker slot on each client. |
| Media sent as QUIC datagrams on the call connection | `node.rs:3009` (send), `node.rs:2814-2830` (read loop) [V] | Same mechanism on a new ALPN (§1.1). |
| Control frames: u32 BE length + JSON, `MAX_FRAME` 64 KiB | `crates/proto/src/lib.rs:35, 644-660` [V] | Same framing for a new `GroupCallMsg` enum. |
| DID-signed blobs with domain separation | `proto/src/lib.rs:145` (`sign_blob`), `:156` (`open_blob`) [V] | All call control (§3) is signed this way. |
| Call grant `cap = "call"`, 1-year TTL, checked in `accept_call_hello` | `proto/src/lib.rs:34, 542-580, 985-1015`; `node.rs:2150-2177` [V] | **Not** used for group calls (§3.3). |
| QUIC keep-alive 5 s, idle timeout 20 s | `node.rs:1775-1779` [V] | Too slow to detect a lost host; §5 adds a 1 s control heartbeat. |
| Android `CoreService` foreground service, type `specialUse\|microphone` | `android/app/src/main/AndroidManifest.xml:65-72` [V] | Covers a phone host while its own user is in the call (§2.4). |

`crates/core/src/voice.rs` is for **voice messages**: Ogg Opus at 16 kHz and 24 kbps
(`voice.rs:1-3, 15-17`) [V]. It is not part of the call path and G6 does not use it.

## 1. Topology and wire

```
 member A ──QUIC──┐
 member B ──QUIC──┤  call host H (member device with a host_call permit)
 member C ──QUIC──┘  receives N-1 streams, forwards the top-K streams to each member
```

- Each participant has exactly one QUIC connection, to H. Participants dial from their
  **group** endpoint (U2) and never from their private endpoint. H's group endpoint comes from
  its signed host record (`docs/group-multi-host-model.md` §1). No member learns another
  member's endpoint or device ID, which keeps U7. [P]
- H never decodes, mixes or re-encodes. It forwards each Opus datagram's bytes unchanged,
  apart from adding a slot byte (§1.2). [P, U21]
- If H's own user is in the call, H is also a participant. Its local frames go into the
  forwarding path in memory, without a network hop.

### 1.1 Connection layout [P]

ALPN `tinline/gcall/1`, a separate connection per call. It is not multiplexed on the group
chat connection: chat presence already uses datagrams on chat connections (`b"tp1"`,
`docs/protocol.md:284`), and a separate connection keeps media datagrams from being confused
with chat datagrams. Each connection carries:

| Channel | Direction | Content |
|---|---|---|
| Bidirectional stream 0 | both | `GroupCallMsg` control frames (§3), framed as in `proto` |
| Datagrams | member → H | `[0x01 type][audio datagram]`, or `[0x03 type][level u8]` while gated (§1.4; never forwarded) |
| Datagrams | H → member | `[0x02 type][slot u8][audio datagram]` |

`audio datagram` is exactly the 1:1 wire `[seq u32][level u8][opus]`. Each sender keeps its own
`seq`, and H forwards it unchanged. The group framing therefore adds 1 byte upstream and
2 bytes downstream.

### 1.2 Sender binding

- **Upstream, at H.** The QUIC connection authenticates the member's group endpoint. At `Join`,
  H maps that endpoint to a DID through its directory of DID-signed group endpoint notes
  (U7). From then on, every datagram on the connection belongs to that DID. [P]
- **Downstream, at members.** `slot` is a 1-byte index into the host-signed `Roster` (§3).
  Each roster entry embeds the participant's own DID-signed `Join`. A member can therefore
  check that every listed DID really asked to join this `call_id` and `host_gen`.
- **What members cannot check.** Members cannot verify that the audio in slot *s* came from
  the DID in slot *s*. H can misattribute or inject audio. Per-packet sender signatures would
  cost one Ed25519 signature every 20 ms per sender and would not stop H from listening, so
  they are not proposed for launch. This is accepted under U14 (§6).

### 1.3 Audio level and speaker selection

- **H cannot measure loudness.** Measuring needs decoding, which U21 rules out (phones need
  near-zero CPU). The level therefore has to come from the **sender**, as the RFC 6464 byte
  the 1:1 sender already writes (`audio/src/lib.rs:317`) [V].
- **Selection rule [P].** H keeps a 300 ms moving average of each sender's level, as −dBov
  where lower means louder. Its active set S is the K loudest senders whose average is below
  a speech threshold (e.g. 50 dBov). Hysteresis: a new sender replaces a member of S only if
  it is louder by at least 6 dB for at least 200 ms. Each member receives S without itself.
  The defaults are K = 3 on a desktop host and K = 2 on a phone host (Q2). K is stated in
  `CallStart`.
- **Abuse.** The level is self-reported and unverifiable. A modified client that always
  reports level 0 holds a slot permanently and can push out real speakers. Mitigations and
  the decision are in §6 and Q5.

### 1.4 Clients: jitter buffer and local mixing [P]

- Each client keeps one `audio::Receiver` per roster slot that is being forwarded, and creates
  it lazily on the first datagram for that slot. Each slot needs its own Opus decoder, because
  decoder state belongs to one sender.
- Every 20 ms the client pulls one frame from each live receiver, sums them in i32, and applies
  soft limiting before converting back to i16. At most K streams are mixed.
- **Forwarding gaps — measured (GC-U01, `21bfd7a`) [V].** On today's `Receiver` every gap
  ratchets latency: the underrun branch in `pull()` adds 20 ms to the base target per gap and
  decays only 5 ms per 3 s (`update_target` merely applies it), so speaker switching pins the
  target at 200 ms. A pause with continuous `seq` inflates the jitter estimate by gap/16. Short
  `seq` jumps (< 1 s) are waited on and played as lost frames, adding the gap to latency. After
  `MAX_CONCEAL` PLC frames output drops to hard zero (click), and resume clicks mid-frame.
  Resetting the receiver after 100 ms alone does not fix the clicks.
- **Decided (G6-D1, user 2026-10-10): H signals slot changes explicitly; receivers do not
  guess from missing packets.**
  - H sends `Active { gen: u32, slots: [u8] }` on the control stream (§3) whenever S changes.
    Not signed: H already controls the media (U14). `gen` increases; stale ones are ignored.
  - On a slot leaving `slots`: that slot's receiver plays one faded-out PLC frame, then goes
    idle (no pulls, no underrun accounting) and is reset. Fallback if the control message is
    late: a slot with no datagram for 200 ms is treated the same way.
  - On a slot entering `slots`: the receiver starts fresh at its last learned network-jitter
    target (not the gap-inflated one), and the first frame fades in. The gap never counts as
    an underrun or as jitter.
  - The 1:1 receiver behaviour (continuous stream, underrun adaptation) is unchanged; the
    idle/resume path is only entered by the group mixer.
- **Sender silence gate (G6-D1, answers Q4 = yes, group calls only).** A client stops sending
  Opus after 300 ms below the speech threshold, and resumes at the first frame above it. `seq`
  keeps counting one per 20 ms frame of wall time while gated, so a resumed stream shows a jump
  equal to the pause. While gated it sends a level-only datagram `[0x03 type][level u8]` every
  500 ms. H uses these only for speaker selection and **never forwards them**. This cuts H's
  downlink to about the number of active speakers and saves battery on every phone. New
  behaviour: today DTX is off and the sender sends continuously (`audio/src/lib.rs:304`) [V].
- **Scope (user, 2026-10-10): G6 is audio only.** Video (the `video-spike` AV1 work: 0.5–2 Mbps
  per stream vs ~76 kbps audio) would go through the same call host but is a later phase that
  needs a desktop host; a phone host cannot forward more than one or two video streams.

## 2. Call host selection

### 2.1 Who can be a call host [P]

- Only a device that holds a valid **`host_call`** permit can host. This is a capability
  separate from chat hosting (`docs/group-chat.md` §4 capability list; U21).
- The permit is the multi-host model's host-permit leaf with the `Host` capability narrowed
  to `CallHost` (`multi_host.rs` `MGrant`; `docs/group-multi-host-model.md` §1). Its holder
  must also have enabled call hosting locally.
- The permit is required because hosting exposes the host's group endpoint to every
  participant. Only permitted hosts have an endpoint that members are allowed to know (U7).
  For the same reason, an ordinary member device can never host ad hoc.
- Chat-host status does not imply `host_call` (`docs/group-chat-tests.md` R05).

### 2.2 Advertised conditions [P]

Each call-host candidate sends a DID-signed `CallHostStatus`. It travels as a group `Ephemeral`
message through the chat hosts (`docs/group-chat.md` §7) and lives for 60 s. The candidate
re-sends it every 30 s while eligible, and immediately when its conditions change.

```
CallHostStatus { v, gid, host_ref, class: desktop|headless|phone,
                 power: mains|battery(pct), network: unmetered|metered,
                 thermal_ok: bool, max_participants: u8, busy: bool, issued_ms, ttl_ms }
host_ref = { holder_did, device_slot, permit_digest }   // no transport key (U4 revised)
```

These fields are self-reported and advisory. A host can lie about them, and the only result of
a lie is worse quality. [A]

### 2.3 Ranking, picker, tie-breaks [P]

The **starter's** device (the member who taps "Start call") picks. It ranks the candidates
that have a fresh status, are not busy, and have `max_participants` at least the expected
size:

| Tier | Candidate |
|---|---|
| 0 | desktop or headless, on mains, unmetered |
| 1 | phone, charging, unmetered |
| 2 | phone, battery ≥ 50 %, unmetered |
| 3 | phone on metered/mobile data (any power); only if N ≤ the metered cap (Q1) |
| (excluded) | `thermal_ok = false`, or a phone on battery below 30 % |

Tie-breaks, in order:

1. The starter itself, if it is in the best tier. This saves one hop for the starter.
2. Lower `busy`/load.
3. Lowest `BLAKE3(call_id || host_ref)`. This is deterministic, so every member computes the
   same order for the backup list.

After ranking:

- The starter sends `CallHostRequest` to the first choice through the chat host. The candidate
  re-checks its local consent and conditions, then either signs `CallStart` or refuses.
- If there is no answer within 5 s, the starter tries the next candidate.
- `CallStart` carries `backups` (at most 2 `host_ref`s), taken from the same ranking. These are
  used for redial after a host loss (§5).

### 2.4 Phone-host constraints [P]

- **Participant caps.** At most 8 participants on unmetered Wi-Fi and at most 4 on metered data
  (Q1). Desktop and headless hosts allow up to 12 (the launch cap).
- **Foreground service.** A phone hosts only while its own user is in the call. The existing
  `CoreService` microphone foreground service then covers it (`AndroidManifest.xml:65-68`)
  [V for the declaration; A that it stays up for a hosting load].
  - Hosting with no participant on the phone would need `specialUse` without the microphone,
    which is a Play-policy risk. Phones therefore never act as listen-only hosts (Q11).
- **Inputs** [A, Android APIs]:
  - `PowerManager.addThermalStatusListener` (API 29+);
  - `BatteryManager` level and plugged state;
  - `NetworkCapabilities.NET_CAPABILITY_NOT_METERED`.
- **Cutoffs while hosting:**

| Condition | Action |
|---|---|
| Thermal ≥ MODERATE, or battery < 30 % and unplugged, or metered without user override | Stop advertising (`busy` / no status). |
| Thermal SEVERE, or battery < 15 % and unplugged | Drop K to 1, refuse new `Join`s, show "hosting limited". |
| Thermal CRITICAL, or battery < 8 % and unplugged, or user ends hosting | Send `End{reason: host_leaving, next_host: backups[0]}`. Members redial at once (§5). |
| Wi-Fi to metered mid-call | Continue. Refuse joins beyond the metered cap and warn the host user about data use. |

## 3. Signed call control

### 3.1 Messages [P]

All messages are DID-signed blobs with domain `tinline/gcall/v1\0` (`sign_blob` pattern). They
contain no device or transport keys (U4 revised).

| Message | Signed by | Fields (besides `v`, `gid`, `call_id` (16 random bytes)) |
|---|---|---|
| `CallStart` | host DID | `host_ref`, `host_gen` u32, `starter_did`, `started_ms`, `k`, `max_participants`, `backups[≤2]`, `meta_frontier` digest |
| `Join` | joiner DID | `host_gen`, `issued_ms`, `nonce` |
| `Roster` | host DID | `host_gen`, `epoch` u32 (increments on each change), `entries: [(slot u8, did, Join)]` |
| `Leave` | member DID | `host_gen`, `issued_ms` |
| `End` | host DID | `host_gen`, `reason` (`ended`\|`host_leaving`\|`host_rotating`\|`removed`\|`superseded`), `next_host` optional |
| `Heartbeat` | — (unsigned, on the authenticated stream) | `epoch`, once per second from H |

How the control messages fit together:

- **Announcement.** `CallStart` is relayed as a group `Ephemeral` message through the chat
  hosts. Members see "Call in progress" with a Join button.
- **Joining.** A member opens `tinline/gcall/1` to `host_ref`'s endpoint and sends `Join`.
- **Generation.** `host_gen` starts at 1 and increments with each redial to a new host for the
  same `call_id`. Every peer rejects control for a `call_id` whose `host_gen` is lower than
  the highest it has seen. This prevents stale takeover (`docs/group-chat.md` §9).
- **Glare** (two `CallStart`s for one group at the same time). The lower
  `BLAKE3(call_id)` wins. The losing host sends `End{superseded, next_host: winner}` and its
  participants redial. This mirrors 1:1 glare (`docs/protocol.md:143`). One live call per
  group. [P]

### 3.2 Checks, applied by H on Join and by every member on CallStart, Roster and End

1. Signature, domain, `gid` and size bounds are valid. `call_id` and `host_gen` are current.
2. The signer is a member of `gid` at the locally validated meta frontier and is **not
   removed** (U9, U12).
   - Refusal is immediate from the moment a peer learns of the removal (U12 detail).
   - A participant who is removed mid-call is dropped at once by an honest H, with
     `End{removed}` sent to that participant only and a new `Roster` to the rest.
3. For `CallStart`, `Roster` and `End`: the signer holds an unrevoked `host_call` permit bound
   to `host_ref.device_slot`. If a permit is revoked mid-call, members leave and redial the
   next backup.
4. For `Join`: the connecting endpoint equals the joiner's current group endpoint in H's
   directory. The device slot is not on the DID's signed device list as unlinked.
5. One device per DID per call. A newer `Join` from the same DID replaces the older one, like
   "answered elsewhere". [P]

### 3.3 Relation to U13 call tokens [P, Q8]

**Group calls do not use per-contact call tokens.** The authority is group membership plus the
host's `host_call` permit.

- Group members need not be contacts, and a token is issued by one contact to another
  (`proto/src/lib.rs:542`) [V]. There is no single issuer who could verify one for a group.
- Nobody is "called" in a group call. A `CallStart` is a non-ringing group notice, and joining
  is the user's choice (Q7).
- Consequence: a contact that holds only a message token from you can still be in a group call
  with you. Your protection is not to join.
- U13 tokens keep gating direct 1:1 calls, including 1:1 calls between group members that
  began through a U3 chat request.

### 3.4 Blocking (U5 revised, U19, U20)

- Removal (U9/U12) is enforced as described in §3.2.
- A **personal block** does not change group membership (U5 revised), so H cannot refuse a
  blocked person's `Join` on behalf of the blocker. Doing so would also tell H about the block.
- Proposed rule [P, Q6]:
  - The blocker's client mutes the blocked DID locally, by not mixing that slot, and shows
    "1 blocked participant".
  - Before joining, the blocker sees "X is in this call", where X is the blocked person.
  - The blocker's own audio still reaches the blocked person through H.
  - The blocked side's app (U20 layer 1) does nothing extra in group calls. U20 covers direct
    calls and messages.
- **Unclear, so flagged in Q6:** U20 says the blocked person's app "stops sending and calling".
  It is not specified whether that should keep the blocked person out of group calls the
  blocker is in.

## 4. Bandwidth and CPU budget

**Per-stream wire cost [A, estimate]:**

| Component | Bytes |
|---|---|
| Opus at 48 kbps, 20 ms frames | ≈ 120 (VBR average) |
| DRED/FEC overhead (`audio/src/lib.rs:300-302` says "~10 bytes/packet" [V]) | ≈ 5 (approx.) |
| `audio` header | 5 |
| Group framing | 2 |
| QUIC short header, packet number, DATAGRAM frame, AEAD tag | ≈ 30 |
| UDP + IPv4 | 28 |
| **Total** | **≈ 190** |

190 bytes × 50 packets/s ≈ **76 kbps per stream** on a direct IPv4 path. IPv6 adds 20 bytes
per packet. A path through an iroh relay adds more, and must be measured (GC-D07).

**Formulas.** These assume a host that is also a participant, the worst case where every
member sends continuously (no silence gate), and N participants including H:

- H downlink = N − 1 streams.
- H uplink = (N − 2)·K + 1 streams (worst case: H itself is in S). For N ≤ K it is (N − 1)².
- A dedicated non-participant host has downlink N and uplink N·K − K.
- Each member: uplink 1 stream (76 kbps), downlink ≤ K streams, ≤ K Opus decodes.

| N | K | H up streams | H up kbps | H down kbps | H out / in pkt/s | H data MB/h | member down kbps |
|---|---|---|---|---|---|---|---|
| 3 | 2 | 3 | 228 | 152 | 150 / 100 | 171 | 152 |
| 3 | 3 | 4 | 304 | 152 | 200 / 100 | 205 | 152 (only 2 others) |
| 5 | 2 | 7 | 532 | 304 | 350 / 200 | 376 | 152 |
| 5 | 3 | 10 | 760 | 304 | 500 / 200 | 479 | 228 |
| 8 | 2 | 13 | 988 | 532 | 650 / 350 | 684 | 152 |
| 8 | 3 | 19 | 1444 | 532 | 950 / 350 | 889 | 228 |
| 12 | 2 | 21 | 1596 | 836 | 1050 / 550 | 1094 | 152 |
| 12 | 3 | 31 | 2356 | 836 | 1550 / 550 | 1436 | 228 |

With the silence gate and one active speaker, H downlink drops to about 1 stream, and H uplink
is about N − 1 streams (for N = 8: about 532 kbps up and 76 kbps down).

At 32 kbps Opus, a stream costs about 145 bytes, or about 58 kbps: roughly 24 % less across
the whole table (Q3).

**Host CPU [A].**

- Forwarding: no codec work. Per packet, H does one QUIC decrypt and one encrypt per copy on
  about 130-byte payloads. The worst row above is 1550 + 550 ≈ 2100 packets/s.
- As a participant, H also does what every participant does: 1 encode and ≤ K decodes per
  20 ms.
- On a phone, the cost is expected to come mostly from the radio (uplink) rather than the CPU.
  No measurement exists yet. GC-D02 and GC-D03 measure CPU, battery %/h and thermal state
  against this table.

## 5. Failure handling [P]

| Event | Detection | Behaviour |
|---|---|---|
| **Host drop** (crash, network, killed) | 3 missed 1 s `Heartbeat`s, or the QUIC connection closes | The call ends locally with "Reconnecting…". Each client opens `tinline/gcall/1` to `backups[0]` and sends `Join{host_gen + 1}`. That backup signs `CallStart{host_gen + 1}` on the first valid Join. If it is unreachable within 5 s, clients try `backups[1]`. If both fail, the call ends with "Call ended — host lost" and a Rejoin button. There is no live hand-off (U21). |
| **Split after redial** (some clients reached backup 0, others backup 1) | The two `CallStart`s for the same `call_id` and `host_gen` are relayed via the chat hosts, and members carry both | The backup with the lower index wins. The other sends `End{superseded, next_host}`. |
| **Planned host exit** (degrade or user) | `End{host_leaving, next_host}` | The same redial, without waiting for the heartbeat timeout. |
| **Member drop** | H sees 5 s with no control or datagrams | H removes the slot and sends a new `Roster`. A rejoin is a fresh `Join` and may get a new slot. |
| **U12 host rotation due during the call** | The host knows its own deadline | The host defers rotation until the call ends, by at most 1 h (Q12). Then it sends `End{host_rotating, next_host: self}`. Its new record has already been pushed (multi-host `rotate_host`), and members redial its new endpoint with `host_gen + 1`. Join refusal of the removed member was already immediate, so the deferral only lengthens the U11 backdated-upload window by ≤ 1 h. |
| **Periodic group-endpoint rotation by H** | Local | Deferred while hosting. Same 1 h cap and the same end-and-redial if the cap is reached. |
| **Member group-endpoint rotation** | Local | Deferred while in a call. If it is forced, the member sends its rotation note to the chat hosts, `Leave`s, and re-`Join`s from the new endpoint. Rotation of the private endpoint does not affect the call (U2). |
| **Path change** (Wi-Fi to mobile) | QUIC/iroh path migration [A] | No control action. On a phone host, the §2.4 metered rule applies. |
| **`host_call` permit revoked, or host removed (`RemoveHost`)** | Members verify the op | Members leave and redial the next backup that is not revoked. |

## 6. Threat model and limits

**What the host can do (accepted under U14 and U21).** H receives every member's media in
clear inside QUIC, so it can:

- listen and record;
- drop or delay any sender (censorship);
- inject or misattribute audio in any slot;
- lie about its conditions.

Members can detect only control-level lies, because `Roster` entries must carry valid signed
`Join`s. They cannot detect media tampering. The UI should say "Hosted by <name>'s <device>"
(Q13). A rogue host can also keep a removed member in the call. That is the same limitation as
`multi_host.rs` test :2551.

**Level spoofing.** This is §1.3: the level byte cannot be checked without decoding.
Mitigations [P]:

- hysteresis;
- a per-sender cap, where a sender holding a slot for more than 90 % of a rolling 60 s while
  K−1 others are also above threshold is demoted to the lowest priority for 10 s;
- every client can mute a participant locally;
- the host's user can "lower priority" for a DID;
- admins can remove the member (U9).

[A, measure in GC-S07 and GC-D09] Opus VBR packet size grows with activity even with DTX off.
H could use it as a cheap plausibility check (claimed loud plus tiny packets means suspect)
without decoding.

**Rate and size limits at H [P].**

| Item | Limit |
|---|---|
| Upstream datagrams | ≤ 60/s per member, burst 10, ≤ 1200 bytes; excess dropped |
| Control frames | ≤ 10/s, ≤ 4 KiB (a `Roster` up to 12 entries fits) |
| `Join` | ≤ 1 per 2 s per DID |
| Participants | ≤ `max_participants` |
| Calls hosted at once per device | 1 |

**Limits at clients.** Clients drop datagrams from slots that are not in the current `Roster`,
and accept media only on the host connection. The `Receiver`'s existing bounds apply:
`MAX_PACKET` 1500 and 64 slots (`audio/src/lib.rs:20-22`) [V].

**Metadata.** H learns who joined, when, from which group endpoint, and how much each person
spoke. The chat hosts see `CallStart` and status notices. Relays see traffic volume.

**Shared mnemonic.** A phrase-holding sibling can join as the same DID and replace the user's
device (one device per DID). It can also claim a `host_call` device slot (multi-host limitation,
test :2088).

## 7. Test plan

**Simulation tests.** These are group-sim style (proposed `crates/group-sim/src/group_call.rs`,
reusing `multi_host.rs` fixtures). Each refusal case asserts byte-identical state.

| ID | Case | Expected |
|---|---|---|
| GC-S01 | `Join` from a non-member, a removed member (U12), a forged signature, a wrong `gid`, a stale `host_gen`, or a replay | Refused by H; roster unchanged |
| GC-S02 | `CallStart` from a chat host without `host_call`, or with a revoked permit (R05) | Members refuse; no call shown |
| GC-S03 | Stale `End`/`CallStart` with a lower `host_gen` after a redial | Ignored everywhere |
| GC-S04 | Same status set at the starter and at all members | Identical ranking and backups; tie-break by hash |
| GC-S05 | Two members start a call concurrently | One call survives; the loser ends `superseded`; participants converge |
| GC-S06 | Speaker selection, N = 3/5/8/12, K = 2/3 | Forwarded counts equal the §4 formulas; no self-echo; hysteresis prevents flapping |
| GC-S07 | A sender always reports level 0 | Limitation shown; the dominance cap and local mute bound it |
| GC-S08 | Host dropped; `backups[0]` up, or down | Redial to b0 with `host_gen + 1`, or else b1; the split case resolves to b0 |
| GC-S09 | Member removed mid-call (honest H, then rogue H) | Honest H ejects at once and re-rosters; rogue H keeps them (limitation) |
| GC-S10 | Personal block between two participants | Membership unchanged; the blocker's mix excludes the blocked slot; H learns nothing |
| GC-S11 | U12 deadline during a call | Rotation deferred ≤ cap; `End{host_rotating}`; members redial the new endpoint |
| GC-S12 | Member rotation during a call, deferred and forced | Deferred, or a clean Leave and re-Join; the call continues |
| GC-S13 | Flood: oversize datagrams, > 60 pps, Join spam | Bounded; other members unaffected |
| GC-S14 | U7 audit over the whole call lifecycle | No member ever holds another member's endpoint or device key; the roster holds DIDs and slots only |
| GC-S15 | `host_call` revoked mid-call | Members leave and redial the next valid backup |
| GC-S16 | Phone degrade thresholds | K reduced, joins refused, `End{host_leaving}` with the next host |
| GC-S17 | Same DID joins from two linked devices | Newer replaces older; one slot per DID |

**Unit tests (`audio`).**

| ID | Case |
|---|---|
| GC-U01 | Per-slot `Receiver` with forwarding gaps of 100 ms to 2 s: latency target does not ratchet up; no clicks; driven through the G6-D1 idle/resume path (§1.4) |
| GC-U02 | Mixer with K = 3 tones (`mix`, Goertzel `amplitude_at` in `group_call_tests.rs`): all frequencies present; no clipping |

**On-device tests.** Use `scripts/buildlock.py` for builds. The phone is **ZD22257P6G**.

| ID | Setup | Measure / pass |
|---|---|---|
| GC-D01 | Desktop host; phone ZD22257P6G plus 2 desktop/emulator members; 10 min | Host CPU, uplink vs §4 (N = 3); audio both ways |
| GC-D02 | **ZD22257P6G as host**, Wi-Fi, charging; N = 3, then N = 5; 30 min each | CPU %, battery %/h, thermal status, uplink vs §4 |
| GC-D03 | ZD22257P6G host on battery, screen off, doze | The call survives 30 min; the foreground service stays alive |
| GC-D04 | ZD22257P6G host with `adb shell cmd thermalservice override-status 4` and `dumpsys battery unplug; dumpsys battery set level 7` | `End{host_leaving}`; members on the desktop backup within 5 s |
| GC-D05 | Phone host switches from Wi-Fi to mobile data mid-call | The call continues; metered cap enforced; data warning |
| GC-D06 | Kill the host (desktop `pkill`, phone `am force-stop`) | Members reconnect via the backup within 3 s detection plus 5 s redial |
| GC-D07 | UDP blocked so the path goes through the iroh relay | Loss, latency and achievable N through the relay |
| GC-D08 | Remove a member mid-call (3 flags) | Ejected at once; cannot rejoin |
| GC-D09 | Tone-based mouth-to-ear latency through the host vs a 1:1 call | Added delay ≤ 1 hop RTT + 20 ms |

## 8. Open questions for the user

1. **Participant caps.**
   *Recommended:* desktop or headless host 12; phone host on Wi-Fi 8; phone host on mobile
   data 4, at about 270 MB/h with K = 2.
2. **K (number of speakers forwarded).**
   *Recommended:* K = 3 on desktop hosts and K = 2 on phone hosts. The host announces K in
   `CallStart`.
3. **Group-call bitrate.** 1:1 calls use 48 kbps.
   *Recommended:* 32 kbps for group calls. That is about 24 % less traffic at the host, and
   voice quality is still good.
4. **Sender silence gate.** Clients would stop sending while quiet. Today DTX is off.
   *Recommended:* yes, for group calls only. It cuts the host's downlink to about the number
   of active speakers and saves battery on every phone.
5. **Audio-level spoofing.** A modified app can always claim to be loudest.
   *Recommended:* accept it for launch, with hysteresis, the dominance cap, local mute, the
   host's "lower priority" control and admin removal. No host-side decoding.
6. **Personal block inside a group call.**
   *Recommended:* the blocker's app mutes the blocked person locally and warns before joining.
   Membership is unchanged and the host is not told. The blocked person's app does nothing
   extra. Should U20 also keep the blocked person out of calls the blocker is in? Recommended:
   no, because it would reveal the blocker's presence and would need host enforcement.
7. **Ringing.**
   *Recommended:* starting a group call sends a non-ringing notice ("Call started in G"),
   following the group's mute setting. Nobody's phone rings for a group call.
8. **Call tokens vs membership.**
   *Recommended:* group calls are authorized by membership plus the `host_call` permit. U13
   tokens apply only to 1:1 calls, so a message-only contact can share a group call with you.
9. **Redial after host loss.**
   *Recommended:* automatic redial to the backups for up to 10 s, then "Call ended" with a
   Rejoin button.
10. **Who may start a group call.**
    *Recommended:* any member, limited to one live call per group, with a start rate limit.
11. **Listen-only hosts.**
    *Recommended:* desktop and headless devices may host without taking part. Phones host
    only while their user is in the call (foreground-service and Play-policy reasons).
12. **U12 rotation during a call.**
    *Recommended:* defer it until the call ends, by at most 1 h, then end the call and redial
    the rotated host.
13. **Disclosure.**
    *Recommended:* the call screen always shows "Hosted by <name>'s <device> — the host can
    hear this call" (U14). Is this wording acceptable?
14. **Public relay load.** A call host behind NAT may push about 1–2 Mbps through n0's public
    relays.
    *Recommended:* allow it for launch, prefer hosts with direct paths, and measure with
    GC-D07. Revisit when the user's own relay lands.

## 9. Findings from the test-first pass (2026-10-10)

Tests: `crates/audio/src/group_call_tests.rs` (GC-U01/U02, 29 audio tests pass, `1737dfc`) and
`crates/group-sim/src/group_call.rs` (GC-S01..S17, 19 pass, 2 ignored, `0c5b56d`).

**Built (GA-F1, `1737dfc`).** `Receiver::new_group()` / `set_active(bool)` / `is_idle()` with a
200 ms no-datagram fallback (`SLOT_FALLBACK_MS`), faded spurt end and fade-in on resume; in the
group path a gap only counts as an underrun if audio resumes before the spurt ends.
`Sender::encode_group` returns `Opus` or `Gated { level }` (gate after 300 ms at or above
`SPEECH_LEVEL = 50` −dBov, `seq` keeps counting, encoder reset on reopen). 1:1 `Receiver::new()`
and `Sender::encode` are unchanged. Measured: target 60 ms and latency equal to gap-free in every
gap case (before: up to 200 ms target, 250 ms latency).

**Corrections to this spec** (not yet folded into the sections above):
1. A 12-entry `Roster` exceeds the 4 KiB `MAX_CONTROL` in both JSON and `sign_blob` shape (§6
   claim is wrong). Raise the limit or encode entries compactly. **Fixed by compact entries
   (§11).**
2. 32 kbps saves about 21 % (150 B / 60 kbps per stream on the wire), not 24 % (§4).
3. Removed members must not be sent a `CallStart`: its call-host record carries H's (possibly
   rotated) group endpoint. Chat hosts skip removed members when relaying it.
4. A forced member rotation mid-call leaves ≥ 2 s without audio (Join rate limit 1 per 2 s).
5. A new speaker is forwarded only ~380 ms after starting when S is full (300 ms average +
   200 ms hysteresis); the first words are lost. Proposed: forward a new speaker at once in a
   temporary extra slot (K+1), let hysteresis decide after 200 ms. **Decided G6-D2 (§10),
   built (§11).**
6. §4's one-speaker gated numbers mix two cases: a remote speaker costs 1 stream down and N−2
   up at H; N−1 up only when H's own user is the speaker.

**Gaps the simulation had to fill** (`ASSUMPTIONS` in `group_call.rs`; to be decided here):
G1 `CallStart` ships next to a call-host record (note + permit); G2 `Join` carries the joiner's
signed group note (a call host that is not a chat host has no endpoint directory); G3 average
uses only datagrams inside the 300 ms window; G4 dominance cap counts only with ≥ K other
eligible senders; G5 "newer Join" = larger signed `issued_ms`; G6 the busy/load tie-break is a
no-op (no load field); G7 no tier for a phone at 30–49 % on battery or a desktop on battery or
metered (ranked last); G8 "starter itself" cannot distinguish two devices of one DID; G9 a
takeover `CallStart` lists the previous backups minus the new host; G10 members reject a
`Roster` listing a removed DID. Also: the pre-join "blocked person is in this call" warning
(Q6) has no roster to read, since non-participants never receive one.

**Still ignored:** GC-S01b (no signed device-list fixture), GC-S07b (packet-size plausibility
needs a real encoder; GC-D09). On-device GC-D01..D09 wait for the call layer.

## 10. Decisions after the test-first pass (user, 2026-10-10)

- **G6-D2 — temporary extra slot for new speakers.** When S is full and a non-selected sender
  crosses the speech threshold, H forwards it at once as a provisional slot K+1 (announced in
  `Active`). After 200 ms hysteresis decides: it replaces a member of S or is dropped. At most
  one provisional slot at a time; the §4 uplink bound becomes N×(K+1) for short bursts.
- **G6-D3 — §8 Q1–Q14 taken as recommended** (lead's reading of the user's "1 yes"; correct
  here if wrong). The values are the named constants in `group_call.rs` and `group_call_tests.rs`.
- The §9 gaps G1–G10 are settled by the lead in the call-engine packet (GA-P1) as modelled,
  unless one changes user-visible behaviour.

## 11. Settled in the call engine (GA-P1, 2026-10-10)

Code: `crates/proto/src/gcall.rs` (wire), `crates/core/src/gcall/` (sans-IO `HostEngine`,
`ClientEngine`, `CallBoard`, `Forwarder`; not wired into `node.rs` yet). Tests: `cargo test -p
proto gcall`, `cargo test -p p2pcore --lib gcall` (GC-S01/03/05/06/06b/08/13/16/17 and G6-D2 on
the real engines), `cargo test -p group-sim group_call` (model updated for G6-D2).

- **§9.1 fixed without raising the limit.** `MAX_CONTROL` stays 4 KiB. A Roster entry carries
  `(slot, did, issued_ms, nonce, Join signature)`; the signed Join payload is rebuilt from the
  Roster's `v`/`gid`/`call_id`/`host_gen`, so members still verify every Join for exactly this
  call and generation. A 12-entry Roster with a 64-char gid is 3770 B.
- **Signing.** One domain `tinline/gcall/v1\0`; the signed payload carries a kind tag, so a
  signed Join cannot be replayed as a Leave (their fields overlap).
- **New frame `Start`.** After accepting a Join the host sends its current `CallStart` on the
  call connection. A takeover host's `CallStart{gen+1}` (new backups, G9) thus reaches its
  members without waiting for the chat relay.
- **Forwarding is per datagram**, not per 20 ms batch: audio from a forwarded slot goes out on
  arrival; selection runs once per 20 ms frame.
- **G6-D2 details.** Trigger: the first audio datagram with level < 50 from a sender outside S
  that is not lowered/demoted, while no provisional slot exists (also when S has room, until
  the 300 ms average admits it). `Active` is emitted before that datagram is forwarded. After
  200 ms the provisional sender replaces S's weakest member if the mean level of its spurt so
  far is ≥ 6 dB louder than that member's average (or joins S if S has room), else it is
  dropped and gets no new provisional slot until its average goes back above the threshold.
  Off while a phone host is limited (K = 1). Consequence: a loud 200 ms burst does displace a
  speaker (it meets the "6 dB for 200 ms" rule); without D2 it never did.
- **Redial.** A backup that cannot be reached is skipped at once; one that does not answer is
  given `ANSWER_TIMEOUT_MS` (5 s). `End{host_leaving|host_rotating}` tries `next_host` first,
  then the other backups.
- **Small rules.** A replacement Join from the same DID keeps its slot; a Leave older than the
  participant's Join is refused; the control rate (10/s) counts every frame on a connection,
  refused ones too; a host whose own permit is revoked stops without an `End`; a limited phone
  host returns to the announced K when conditions recover.
- **Left for wiring:** the `tinline/gcall/1` accept/dial loop and `ConnId` mapping (the host's
  own user is `ConnId::LOCAL`), a `CallAuthority` over real group membership, permits and
  group endpoint notes, the chat-relay `Ephemeral` for `CallStart`/`End` with the call-host
  record (G1), `CallHostStatus` and ranking (§2.2–2.3, still model-only), the takeover trigger
  (`CallBoard::may_take`) and glare/split settling (`CallBoard::superseded`), and the 20 ms
  audio loop (`on_mic`, `mix`).
