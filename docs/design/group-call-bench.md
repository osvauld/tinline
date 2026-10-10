# Group-call hosting benchmark (GA-P2)

Question: can phone ZD22257P6G (moto g52, Android 13, 6 cores) host a group call of N = 3, 5, 8
participants? Measures host CPU %, thermal status, uplink/downlink against
`group-call-spec.md` §4, and the mouth-to-ear delay added by the host compared with a direct
1:1 call (GC-D09).

**Status (2026-10-10, 23:55): only the desktop baselines have been run.** The phone dropped
off USB and Wi-Fi at about 23:05, before the first phone run (no adb device, no ping to
192.168.1.3 for 50 min). The phone rows below are still open. Nothing was pushed to the phone.

## Harness

`crates/gcall-bench` (measurement harness, not product code; it does not use the GA-P1 engine):

- **`host`**: an iroh endpoint (`presets::Minimal`, no relay, no discovery) on UDP port 7700,
  ALPN `tinline/gcall-bench/1`, with participant keys hard-coded per slot. It does not decode.
  It forwards each `[0x01][seq][level][opus]` datagram as `[0x02][slot][…]` to every other
  participant while the sender is in S. S is chosen by the spec §1.3 rule: 300 ms average of the
  level byte, threshold `SPEECH_LEVEL` 50, K loudest, and 6 dB / 200 ms hysteresis. The host also
  applies the G6-D2 provisional K+1 slot (200 ms). It sends `Active{gen, slots}` on every change.
  In the bench `Active` is a datagram; in the spec it goes on the control stream. The host also
  runs **its own participant** (slot 0): one `encode_group` and up to K+1 `Receiver::new_group`
  decodes plus a `mix` per 20 ms, as a phone host that is in the call would. Every 5 s it prints
  cumulative app and QUIC UDP counters plus its own CPU ticks.
- **`members`**: N−1 members in one process, so they share one clock. Each member has its own
  endpoint and connection to the host. Each sends 32 kbps Opus (`audio::Sender`, unchanged
  settings: FEC, DRED, complexity 6) through the G6-D1 gate (`encode_group`), plus `[0x03][level]`
  every 500 ms while gated and a ping each second. Each member receives through one
  `Receiver::new_group` per slot, applies `Active` via `set_active`, and mixes.
  - **Talkers**: synthetic but speech-like. A harmonic voice (f0 100–250 Hz, 12 harmonics,
    pitch drift, 4 Hz syllable envelope, breath noise) talks in spurts of 0.6–3 s. Between
    spurts there is a −64 dBov noise floor, so the gate closes. Each talker is independent and
    talks 1/N of the time, so about one speaker is active at a time, with random overlaps.
  - **transit**: from a datagram's send to its arrival at another member through the host.
  - **mouth-to-ear (m2e)**: from the first sample of a talk spurt (one frame before its send
    tick) to the start of the first audible frame (RMS > 200) pulled from that slot at a
    receiver. It covers capture framing, network, host and jitter buffer. It does not include
    device audio I/O, which is the same for host and direct calls. An onset more than 1 s late
    counts as a late switch-in and is excluded from the percentiles.
  - `members --direct`: GC-D09 baseline. Two endpoints on loopback run today's 1:1 path:
    continuous `encode` and `Receiver::new()`.
- **`scripts/gcall_bench.py`**:
  - `push`, `clean`, `phone`, `desktop` and `direct` sessions.
  - Samples every 10 s: host `/proc/<pid>/stat`, `/proc/stat` and the `wlan0` counters, plus
    `dumpsys thermalservice` (Thermal Status) and `dumpsys battery` (level, temperature, plugged).
  - Wire rate = QUIC UDP payload bytes + 28 B UDP/IPv4 header per datagram, from
    `Connection::stats()`.
  - Each session runs inside `scripts/buildlock.py --who ga-p2-bench`, so no build runs during
    a measurement.

The host is cross-compiled the same way as the app's Rust (`cargo ndk -t arm64-v8a -P 28`,
NDK 28.2):

```
CARGO_TARGET_DIR=/home/abe/build-cache/p2p_chat/group-audio/target scripts/buildlock.py --who ga-p2 -- cargo build --release -p gcall-bench
ANDROID_NDK_HOME=~/Android/Sdk/ndk/28.2.13676358 CARGO_TARGET_DIR=… scripts/buildlock.py --who ga-p2 -- \
    cargo ndk -t arm64-v8a -P 28 build --release -p gcall-bench --bin host
T=/home/abe/build-cache/p2p_chat/group-audio/target
scripts/gcall_bench.py push --serial ZD22257P6G --bin $T/aarch64-linux-android/release/host
for n in 3 5 8; do scripts/buildlock.py --who ga-p2-bench -- scripts/gcall_bench.py phone --serial ZD22257P6G \
    --n $n --k 2 --secs 600 --members-bin $T/release/members --label phone-n$n-k2 --out results.jsonl; done
scripts/gcall_bench.py clean --serial ZD22257P6G      # kills gcall-host, removes the binary
```

## Results

| Run | Host | N | K | Host CPU (% of one core) avg / peak 10 s | Host down kbps / pps | Host up kbps / pps | Transit p50 / p95 / p99 ms | m2e p50 / p95 ms | Jitter target p50 | Loss |
|---|---|---|---|---|---|---|---|---|---|---|
| direct-1to1 (GC-D09 baseline) | – (loopback) | 2 | – | – | – | – | 0.4 / 0.6 / 0.6 | **80.1 / 80.9** | 40 ms | 0 |
| desktop-n8-k3 | desktop (loopback members) | 8 | 3 | 2.7 / 3.8 (0.34 % of 8 cores) | 184 / 272 | 451 / 437 | 0.5 / 0.7 / 1.1 | **77.4 / 113.9** (max 260) | 40 ms | 0 (0 late onsets) |
| phone-n3-k2 | ZD22257P6G, Wi-Fi, charging | 3 | 2 | *not run* | | | | | | |
| phone-n5-k2 | ZD22257P6G, Wi-Fi, charging | 5 | 2 | *not run* | | | | | | |
| phone-n8-k2 | ZD22257P6G, Wi-Fi, charging | 8 | 2 | *not run* | | | | | | |
| phone-n5-k2 on battery | ZD22257P6G | 5 | 2 | *not run*. The cable cannot be pulled remotely; `dumpsys battery unplug` only fakes the state and charging continues. | | | | | | |

The phone columns still to fill: thermal max, battery % and temperature start→end, and
`wlan0` kbps.

Notes on the desktop runs:

- **desktop-n8-k3**, 300 s, 155 talk spurts, 930 onsets measured (each heard by 6 receivers):
  - Selection: about 75 changes to the forwarded set per minute (spurt starts, ends and
    provisional slots). No datagram sends failed.
  - Data: 286 MB/h at the host (in + out).
  - Concealment: 1217 concealed frames, about 1 per heard spurt end. This is the designed
    faded-out PLC frame at each spurt end, not loss.
- **m2e p50 is the same through the host as direct** (77 vs 80 ms, all on loopback). The
  G6-D1 receiver resumes at its learned target (40 ms) and adds no latency at speaker
  switches.
- **The p95 of 114 ms (max 260 ms) is speaker selection, not the pipeline.** When K+1 slots
  are busy, a new speaker's onset waits for a slot (hysteresis, provisional slot taken).
  Direct 1:1 has no selection, and its p95 equals its p50.

## Findings so far

1. **Host CPU for forwarding is small, at least on a desktop.** At N = 8, about 710 UDP
   packets/s through the host cost 2.7 % of one desktop core. That figure includes the host's
   own participant: an encode and up to 4 decodes plus a mix every 20 ms. That is about
   40 µs per packet, everything included. An A55-class phone core is perhaps 3–6× slower, which
   would put N = 8 at roughly 10–20 % of one core. This is an extrapolation only; the phone run
   has to confirm it.
2. **QUIC ACKs are a large part of the host's downlink, and §4 does not count them.** At
   N = 8, members send about 55 audio, 14 level-only and 7 ping packets/s to the host. The host
   nevertheless received 272 pps. The other ~195 pps are ACK-only packets for the ~437 pps it
   forwards, about one ACK per 2 datagrams (QUIC's default ACK frequency). At about 40–50 B
   each, that is about 70 kbps, comparable to one audio stream. The 1:1 path pays this too, but
   only for one stream.
3. **The gated, one-speaker uplink matches §9 correction 6.** On average the host forwarded
   437 pps ≈ 8.7 "streams" of 50 pps at N = 8 and K = 3. That is about 1.2 simultaneous
   speakers × (N − 2) copies, plus overlaps and the provisional slot. Wire cost per forwarded
   stream is about 60 kbps at 32 kbps Opus (≈ 150 B/packet), which confirms §9 correction 2.

## Recommended spec changes (pending the phone numbers)

- §4: add the ACK term. Host downlink ≈ (active speakers × 50 pps of ≈ 150 B) + (forwarded pps
  / 2 × ≈ 45 B) + (N − 1) × 3 pps of level/keepalive. At N = 8 with one speaker that is about
  180 kbps, not the 60 kbps that "1 stream" implies. Packet rate rather than bytes is what
  costs radio wake-ups on a phone. Lowering the receivers' ACK frequency (QUIC
  ACK-frequency extension, or a larger `max_ack_delay` on the gcall connection) is worth trying
  if the phone run shows radio or CPU cost.
- GC-D09 wording: measure "added delay" as m2e through the host − m2e of a direct 1:1 call.
  On loopback the pipeline adds nothing. On Wi-Fi the extra is expected to be the second hop
  (≈ RTT/2 to the host) plus any jitter-target increase from two Wi-Fi hops; the phone run
  shows which.
- Caps 8/4 and K = 2 on phones: **no evidence yet either way.** Keep them provisional until the
  phone runs above are filled in.

## Caveats

- Synthetic audio. The harmonic talker is close to voice for Opus (SILK/hybrid), but real speech
  bitrate and gate behaviour differ. Talkers are independent and do not take turns the way a
  conversation does.
- No foreground service, Doze or screen-off behaviour is measured. The host runs as a shell
  binary from `/data/local/tmp`, not inside the app's `CoreService` (GC-D03 stays open).
- Members all run on one desktop. Through a phone host that means desktop → AP → phone → AP →
  desktop. A real member on another phone adds its own radio and jitter.
- `Active` goes as an unreliable datagram rather than on the control stream. Receivers fall back
  to the 200 ms no-datagram rule if one is lost.
- No signing, no membership, no roster. The control-plane CPU is not included.
