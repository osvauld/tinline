Status: G6 call engine built and wired into group chat (GA-P1..P7, GA-I1, see §11); earlier sections are the proposal.

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
  K is every speaker (12) on a desktop host, 4 on a phone host and 2 on any host on metered
  data (Q2, decided 2026-10-14; was 3 desktop / 2 phone). K is stated in
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

Built in GA-P7 (`proto::gcall_status`, `p2pcore::gcall::rank`, `0ec04da`); this table
supersedes the original proposal and settles §9 G6–G8:

| Tier | Candidate |
|---|---|
| 0 | desktop or headless on mains or charging, unmetered |
| 1 | phone charging, unmetered |
| 2 | phone on battery ≥ 50 %, or desktop/headless on battery ≥ 30 %, unmetered |
| 3 | phone on battery 30–49 %, or desktop/headless on battery < 30 %, unmetered |
| 4 | any metered host; only if expected N ≤ `CAP_PHONE_METERED` (4) |
| (excluded) | busy, thermal ≥ Moderate, load ≥ 90, `max_participants` < expected, phone on battery < 30 % |

Tie-breaks, in order:

1. The starter's own **device** (`holder_did` + `device_slot`, G8), only if it is in the best
   non-empty tier.
2. Lower load, in 25 % buckets (G6: `load` = share of the device already used by other work;
   hosting another call is `busy`).
3. Lowest `BLAKE3(call_id ‖ 0 ‖ did ‖ 0 ‖ slot ‖ permit_digest)` — deterministic, so every
   member computes the same backup order.

Status: signed, TTL ≤ 60 s (refused above), refreshed every 30 s and on any condition change,
≤ 5 s future skew, ≤ 1 KiB. Only the latest status per device counts. Backups in each request
skip candidates that already refused or timed out. Open for tuning after the phone benchmark:
whether thermal Moderate should exclude (some phones reach it in normal use).

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
- **Measured (GA-P2, `docs/design/group-call-bench.md`, moto g52 on Wi-Fi, charging, K = 2,
  32 kbps, silence gate).** The table above is a worst case; with the gate, N = 8 measured
  383 kbps up / 155 kbps down (~240 MB/h) at the phone host, vs 988 / 532 above.
  - **CPU is not near zero on a phone:** ~0.5 ms per packet (≈ 15× a desktop) plus ~12 % of one
    core for the host's own participation. Host CPU, % of one core avg: N = 3 → 20, N = 5 → 31,
    N = 8 → 42 (forwarding only at N = 8: 29). Thermal status stayed 0; battery 34 °C.
  - **QUIC ACKs:** about one ACK-only packet per two forwarded datagrams (≈ 60–70 kbps at
    N = 8, one stream's worth) arrives at H. Budget term: forwarded pps / 2 × ~45 B.
  - **Delay:** median added delay through the phone host +11 to +22 ms vs a direct 1:1 call
    (GC-D09 passes). GC-D09's "added delay" = mouth-to-ear through H minus mouth-to-ear direct.
  - **Data warning on metered (cap 4):** ~100 MB/h (interpolated from N = 3 and 5).
  - Caps 8 (Wi-Fi) and K = 2 hold with headroom; do not raise to 12 before measuring N = 12 and
    an on-battery run. To try: fewer ACKs on the call connection (larger `max_ack_delay` / ACK
    frequency) and batched datagram sends. Still unmeasured: battery %/h on battery (GC-D02) and
    foreground service / Doze (GC-D03); both need the real app and a manual unplug.

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
2. **K (number of speakers forwarded).** **Decided 2026-10-14:** every speaker (K = 12, so
   each member hears everyone talking but themselves) on desktop hosts; K = 4 on phone hosts
   on Wi-Fi, a safety net so hosting never takes over the phone; K = 2 for any host on metered
   data, switched both ways mid-call with a new `Active`.
   `CallStart.k` may now equal `MAX_PARTICIPANTS` (12). Silent and muted members send only the
   level byte, so the cost appears only while several people talk at once. Why: nobody is cut
   off when four or more react together, and a client that lies about its level can no
   longer push real speakers out of a few slots. Selection, hysteresis, the provisional slot
   and the dominance cap still run and matter on metered hosts and phones limited to K = 1.
   Why 4 on phones: a phone host spends ~0.5 ms of CPU per forwarded packet (§5); eight people
   all talking at K = 12 would be ~2800 packets/s, more than one core. At K = 4 the worst case
   is ~1600 packets/s (4 talkers x 6 remote copies + 8 senders in, x 50/s); the thermal/battery degrade (K = 1) stays the backstop. Revisit after
   measuring a phone host at N = 8. Tests: `gcall_q2_*` (core), `gc_u02_eleven_voices_*`
   (audio), `capacity_twelve_talking_at_once_each_hears_the_other_eleven` (real nodes).
   (Was: K = 3 on desktop hosts and K = 2 on phone hosts.)
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
  audio loop (`on_mic`, `mix`). The first, the takeover trigger and the audio loop are done in
  GA-P4 (below).

**Wired on iroh (GA-P4).** Code: `crates/core/src/gcall/net.rs`, hooks in `node.rs` (ALPN
list, accept-loop branch, `push_mic`/`pull_speaker` fall-through). Test:
`cargo test -p p2pcore --test gcall` (four real nodes on loopback, no relay, tones for mics).

- **API (unstable, plain Rust on `Node`, no uniffi yet):** `gcall_configure(GcallConfig {
  dir: CallDirectory, events, host_ref, cond, note })`, `gcall_host(HostConfig) -> CallStart
  blob`, `gcall_join(start, host_addr)`, `gcall_observe_start(start)` (relayed CallStarts, for
  takeover and following `Superseded`), `gcall_leave`, `gcall_end`, `gcall_set_mic_muted`,
  `gcall_mute_participant`, `gcall_status`. Events: `Joined`, `RosterChanged`, `Reconnecting`,
  `Superseded`, `Ended`, `HostingStarted`/`HostingEnded` (carry the blobs to relay), `Notice`.
  `CallDirectory` = `CallAuthority` + `host_addr(HostRef)`; `StaticDirectory` until group chat.
- **One session per node**, both engines behind one lock; actions run synchronously, so the
  host's own user is fed engine-to-engine through `ConnId::LOCAL` with no hop. One reader and
  one writer task per connection; control frames go through the writer, datagrams out at once.
- **Audio** follows the 1:1 idiom: the app's 50 Hz threads call `push_mic`/`pull_speaker`, which
  feed the group call when no 1:1 call is active. A 20 ms ticker drives engine ticks and sends a
  level-only keepalive when the app pushes nothing, so a member is not dropped after 5 s.
- **Close codes** (QUIC application codes): 0 bye, `0x10` ended, `0x11` answered elsewhere,
  `0x12` removed, `0x13` dropped, `0x14` Join refused. A client maps `0x11` to
  `on_closed(true)`; any other close redials (§5). A host closes a connection whose Join was
  refused at once, so the client moves to the next candidate without the 5 s answer timeout.
- **Takeover:** the first `Join{host_gen + 1}` for a call this device is a backup of (the call
  its own client is in, else an observed CallStart) builds `HostEngine::takeover`; the device's
  own redialing client reaches it through `ConnId::LOCAL`. A refused first Join drops it.
- **Not yet:** dialing from the group endpoint (today the device endpoint). The rest of the
  GA-P4 gaps are closed by GA-P5 (below).

**Hardening (GA-P5).** Tests: `cargo test -p p2pcore --test gcall` (three tests, run in
parallel, ~11 s), `--test calls group_call_and_one_to_one`.

- **One call at a time.** While this device's user is in a group call (its client is not
  ended; hosting listen-only does not count) a 1:1 call cannot begin: incoming ones get the
  usual `Busy`, outgoing ones `InCall`. `gcall_host`/`gcall_join` during a 1:1 call fail with
  `InCall`. The check and the session install run under the 1:1 slot's lock.
- **`stop` / `lock`** leave the call (signed Leave) and, when hosting, send
  `End{host_leaving, next_host: first backup with a valid permit}`, then wait at most ~1.2 s
  for those frames before the endpoint closes. Known CallStarts and boards are forgotten.
  Dropping a `Node` still only closes the endpoint (members redial on the QUIC close).
- **Conditions:** `Node::gcall_set_cond(Cond)` (for the platform's battery, charging,
  thermal and metered reports) feeds `HostEngine::set_cond` of a hosted call and is the
  condition a later takeover starts with. Tested on a phone host: K 2 → 1 and a late Join
  refused at < 15 %, `End{host_leaving, next_host}` at < 8 % with everyone moving to the backup.
- **Host loss without a close** (`Node::crash_for_test`: runtime stopped, nothing sent): the
  member notices by 3 missed heartbeats, measured 2–3.5 s after the crash, and redials.
- **Glare/split sweep.** Every CallStart observed (`gcall_observe_start`) and the call hosted
  here go on a per-group `CallBoard`; when `CallBoard::superseded` names a winner the host ends
  with `End{superseded, next_host: winner}`. Members join the winner at once if they know its
  CallStart, else when it arrives within 30 s (`FOLLOW_WINDOW`). `gcall_observe_end` removes
  a relayed `End{ended}` call from the board, so a dead call does not win a later glare; a call
  whose End was never relayed still can (no freshness bound yet).

**Wired into group chat (GA-I1).** Code: `crates/core/src/group/{calldir,calls}.rs`, records
in `group/record.rs`, frames in `group/wire.rs`, `proto::gcall` (`HostRequest`,
`HostRefusal`, `GroupNote`). Tests: `cargo test -p p2pcore --test group_call` (four real nodes,
~60 s), `--lib group` (`call_hosts_need_their_own_permit_and_acceptance`), `cargo test -p
proto --lib gcall`; `--test gcall` still runs the engines over a `StaticDirectory`, now on the
group endpoint.

- **Permit (§2.1).** Not the multi-host `MGrant` leaf: two group records appended to
  `record::Body`, `CallHostPermit{host, host_dev, active}` (owner/admin, latest `(at, id)`
  wins) and `CallHostAccept{active}` (the device itself). A device hosts calls when both are
  active and its DID is a member (`GroupState::call_hosts`); chat hosting implies nothing. A
  separate accept (not `HostAccept`) because agreeing to relay chat is not agreeing to carry
  calls. `HostRef` = `{holder_did, device_slot = first 8 bytes of the group device ID BE,
  permit_digest = id of the CallHostPermit record}`: a revocation or re-issue invalidates it.
  A call-host revocation is not a rotation trigger.
- **Directory (§3.2).** `GroupCallDir`: `is_member` = member and not removed; `host_permit_ok`
  = the ref resolves to an active call host; `endpoint_ok` = the chat host's member directory,
  or the Join's DID-signed `GroupNote` (G2) naming the QUIC remote key; `host_addr` = the newest
  DID-signed `SignedHostAddr` of that device. It looks the group runtime up on every call.
- **Endpoint (U2/U7).** `tinline/gcall/1` is an ALPN of the group endpoint only (removed from
  the private one). Call hosts' addresses travel like chat hosts' (`Hosts`, `Welcome`, and
  with each CallStart), so members know chat- and call-host endpoints and nothing else
  (U7 audited in `group_call.rs`).
- **Relay (§2.2, §3.1).** `GroupMsg::{CallHostStatus, CallStart{start, hosts}, CallEnd,
  CallHostRequest, CallHostRefusal}`. Members send to their chat hosts only; a host verifies
  (signature, group, membership, permit, a member sends only what it signed) and passes
  broadcast frames to current member sessions (never removed or joining, §9.3) and other
  hosts, addressed ones (request: the device; refusal: the starter DID) towards the recipient.
  Dedupe by signature. A new member session gets the fresh statuses and live calls (< 4 h).
- **Start (§2.3).** Statuses every 30 s and on change (busy = any live call session, paused,
  or the phone policy). The starter ranks with `expected = min(members, 12)`; asking itself
  hosts at once with its user in; otherwise a signed request; a refusal or 5 s silence moves
  on; exhausted = `StartFailed`. The asked device declines when paused, busy, a phone (Q11: a
  phone hosts only calls its own user starts), or outside the policy; else hosts listen-only
  with the request's (still permitted) backups. The starter joins on the CallStart.
- **End.** `End{ended}` is relayed; a listen-only host ends a call nobody is in after 20 s;
  `group_call_leave` ends a hosted call that became empty at once.
- **Rotation (§5, Q12).** Call hosts rotate after a removal like chat hosts, but half a delay
  later when they do not host chat (they learn the chat hosts' new endpoints by `Moved` at
  their old one first; both at once would strand them). A due rotation waits while a call
  session is live, at most 1 h. A chat host relays an address that changed in a `Hello`.
- **API (UniFFI).** `group_call_start(gid) -> call_id`, `group_call_join(gid, call_id)`,
  `group_call_leave()`, `group_call_set_muted(bool)`, `group_calls(gid) -> [GroupCall]`,
  `group_call_hosting_enable(gid, bool)` (local pause, not persisted),
  `set_group_call_events(GroupCallEvents)` (`Announced`, `Ended`, `StartFailed`, `Joined`,
  `RosterChanged`, `Reconnecting`, `Left`, `HostingStarted/Ended/Limited/Unlimited`,
  `MeteredData`), `group_appoint_call_host` / `group_revoke_call_host` (admins),
  `group_accept_call_host(gid, bool)`; `GroupDetail.{am_call_host, call_hosts,
  call_host_requests, call_host_requested_here}`. Test hook:
  `group_call_hosts_advertising_for_test`. `Node::gcall_*` stays plain Rust.
- **Left:** UI (Android, desktop); `load` is always 0; the pre-join "blocked person in this
  call" warning (Q6); persisting the hosting pause. (Audio threads and condition feeders:
  GA-A1, below.)

**App audio and conditions (GA-A1).** Tests: `cargo test -p p2pcore --lib gcall`
(`group::calls::tests`), `cargo test -p desktop hostcond`, Android
`:app:testDebugUnitTest --tests com.osvauld.p2p.HostCondTest`.

- **No conditions, no advertising.** `group_call_set_conditions(GroupCallConditions{
  device_class, plugged, battery_pct, metered, metered_allowed, thermal})` (UniFFI for
  `gcall_set_cond`). Until the first one (or an explicit `gcall_configure`) a call host
  publishes no `CallHostStatus` and declines requests ("conditions not known yet"): an app
  that never reported must not pass for a desktop on mains.
- **`group_call_here() -> Option<GroupCallHere{group_id, call_id, in_call, hosting}>`.** The
  apps re-read it on every `GroupCallEvents` callback rather than trusting the event (the
  callbacks run on separate threads and can arrive out of order, e.g. a superseded call's
  `Left` after the winner's `Joined`). `in_call` (client not ended) runs the mic and speaker;
  `in_call || hosting` keeps the Android service in call mode.
- **Desktop.** `audio::Ctl::on_group_call` runs the same cpal/APM call loop as 1:1 in its own
  slot (a 1:1 call ending or the ringer never stops it); events also reach the UI channel as
  `Ev::GroupCall`. `hostcond.rs`: desktop class; `/sys/class/power_supply` (no system battery
  = mains; peripheral batteries ignored; plugged and not charging = mains), metered from
  `nmcli` (a connected device with a gateway marked metered), thermal none; every 30 s,
  pushed on change.
- **Android.** `GroupCallAudio` (the core's sink) owns a second `AudioEngine` (communication
  mode, voice focus, loudspeaker by default) and puts `CoreService` in call mode (microphone
  type, wake and Wi-Fi locks) while in or hosting a group call; `prepare()` raises the service
  from the UI before start/join (Android 14+ refuses the microphone type from the
  background). The ongoing notification's hang-up leaves the group call when no 1:1 call is
  up; a 1:1 `IDLE` keeps a group call's call mode. `HostConditions`: battery
  (`ACTION_BATTERY_CHANGED`; unknown level = 0 %), plugged (any source, or charging/full),
  thermal (`PowerManager` listener, API 29+), metered (default network capabilities,
  temporarily-not-metered counts as unmetered); phone class; pushed on change.
