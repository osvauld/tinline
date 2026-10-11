# Group-call hosting benchmark (GA-P2)

Question: can phone ZD22257P6G (moto g52, Android 13, 6 cores) host a group call of N = 3, 5, 8
participants? Measures host CPU %, thermal status, uplink/downlink against
`group-call-spec.md` §4, and the mouth-to-ear delay added by the host compared with a direct
1:1 call (GC-D09).

**Answer (runs on 2026-10-11): yes for N = 3, 5 and 8 on Wi-Fi while charging.**

- The host process used at most 42 % of one core on average, which is 7 % of the 6-core device.
  The busiest 10 s window reached 69 % of one core.
- Thermal status stayed 0 (NONE) and battery temperature stayed at or below 34 °C.
- No datagram sends failed, and receivers saw almost no loss (7 frames lost out of 196 k).
- The host adds about 11–22 ms of median mouth-to-ear delay over the direct baseline. That is
  within GC-D09's "≤ 1 hop RTT + 20 ms" (RTT p50 7.7 ms).

The on-battery run was skipped, because it needs a manual unplug.

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
  decodes plus a `mix` per 20 ms, as a phone host that is in the call would. `--no-participate`
  turns the participant off. Every 5 s it prints cumulative app and QUIC UDP counters plus its
  own CPU ticks.
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
  - `push`, `clean`, `phone`, `desktop` and `direct` sessions. `--serial` defaults to
    `$GCALL_SERIAL`, then ZD22257P6G.
  - Samples every 10 s over adb: host `/proc/<pid>/stat`, `/proc/stat` and the `wlan0`
    counters, plus `dumpsys thermalservice` (Thermal Status) and `dumpsys battery` (level,
    temperature, plugged).
  - Wire rate = QUIC UDP payload bytes + 28 B UDP/IPv4 header per datagram, from
    `Connection::stats()`.

The host is cross-compiled the same way as the app's Rust (`cargo ndk -t arm64-v8a -P 28`,
NDK 28.2):

```
L=/home/abe/p2p_chat/scripts/buildlock.py
CARGO_TARGET_DIR=/home/abe/build-cache/p2p_chat/group-audio/target $L --who ga-p2 -- cargo build --release -p gcall-bench
ANDROID_NDK_HOME=~/Android/Sdk/ndk/28.2.13676358 CARGO_TARGET_DIR=… $L --who ga-p2 -- \
    cargo ndk -t arm64-v8a -P 28 build --release -p gcall-bench --bin host
T=/home/abe/build-cache/p2p_chat/group-audio/target
scripts/gcall_bench.py push --serial ZD22257P6G --bin $T/aarch64-linux-android/release/host
for n in 3 5 8; do $L --who ga-p2-bench --run --device ZD22257P6G -- python3 scripts/gcall_bench.py phone \
    --serial ZD22257P6G --n $n --k 2 --secs 600 --members-bin $T/release/members --label phone-n$n-k2 --out results.jsonl; done
# forwarding only (the host's own participant off): … --n 8 --k 2 --secs 180 --host-args=--no-participate
scripts/gcall_bench.py clean --serial ZD22257P6G      # kills gcall-host, removes the binary
```

## Results

Setup:
- **Phone:** ZD22257P6G (moto g52) on Wi-Fi at 192.168.1.3, charging over USB. adb ran over USB,
  so polling did not share the Wi-Fi radio.
- **Members:** on the desktop at 192.168.1.25, on the same AP.
- **Path:** benchmark traffic goes desktop → AP → phone → AP → desktop. The host is the
  phone's shell binary from `/data/local/tmp`.

| Run | N | K | Host CPU % of one core: avg / peak 10 s | % of device (6 cores) | Thermal max | Battery start→end, temp max |
|---|---|---|---|---|---|---|
| phone-n3-k2, 600 s | 3 | 2 | **19.5** / 38.7 | 3.3 | 0 | 96→96 %, 34.0 °C |
| phone-n5-k2, 600 s | 5 | 2 | **30.6** / 50.7 | 5.1 | 0 | 96→97 %, 34.0 °C |
| phone-n8-k2, 600 s | 8 | 2 | **41.8** / 68.8 | 7.0 | 0 | 97→99 %, 34.0 °C |
| phone-n8-k2, forwarding only (`--no-participate`), 180 s | 8 | 2 | 29.2 / 45.8 | 4.9 | 0 | 99→99 %, 34.0 °C |
| desktop-n8-k3 (desktop host, loopback members), 300 s | 8 | 3 | 2.7 / 3.8 | 0.34 of 8 | – | – |

Bandwidth at the host. Wire rate = QUIC UDP payload + 28 B UDP/IPv4 header per datagram. §4
gives ungated worst cases at 48 kbps; the measured runs are gated at 32 kbps:

| Run | Host down kbps / pps | Host up kbps / pps | Forwarded "streams" up (pps / 50) | MB/h (in+out) | §4 ungated worst case, up / down kbps |
|---|---|---|---|---|---|
| phone-n3-k2 | 63 / 76 | 75 / 82 | 1.6 | 62 | 228 / 152 |
| phone-n5-k2 | 103 / 140 | 197 / 195 | 3.9 | 135 | 532 / 304 |
| phone-n8-k2 | 155 / 236 | 383 / 373 | 7.5 | 242 | 988 / 532 |
| desktop-n8-k3 | 184 / 272 | 451 / 437 | 8.7 | 286 | 1444 / 532 |

Latency and loss at the members. The direct 1:1 baseline is two endpoints on desktop loopback,
so its network delay is about 0:

| Run | Transit p50 / p95 / p99 ms | RTT to host p50 / p95 ms | Mouth-to-ear p50 / p95 / max ms | Added m2e vs direct (p50) | Jitter target p50 / p95 ms | Lost / received frames |
|---|---|---|---|---|---|---|
| direct-1to1 (GC-D09 baseline) | 0.4 / 0.6 / 0.6 | – | 80.1 / 80.9 / 81.3 | – | 40 / 45 | 0 / 18 k |
| phone-n3-k2 | 7.7 / 13.4 / 28.6 | 7.6 / 50.7 | 91.0 / 189 / 231 | **+11** | 45 / 65 | 0 / 33 k |
| phone-n5-k2 | 8.5 / 29.3 / 46.9 | 7.9 / 27.3 | 100.5 / 180 / 337 | **+20** | 45 / 75 | 1 / 97 k |
| phone-n8-k2 | 8.7 / 16.4 / 41.4 | 7.7 / 15.1 | 102.0 / 160 / 283 | **+22** | 50 / 80 | 7 / 196 k |
| phone-n8-k2, forwarding only | 9.3 / 27.2 / 47.0 | 7.9 / 24.7 | 111.2 / 204 / 298 | +31 | 55 / 90 | 0 / 53 k |
| desktop-n8-k3 | 0.5 / 0.7 / 1.1 | 0.2 / 0.3 | 77.4 / 114 / 260 | −3 | 40 / 50 | 0 / 114 k |

No run had a datagram send failure at the host or a late onset (> 1 s). About 1 frame was
concealed per heard spurt end. That is the designed faded-out PLC frame, not loss.

Desktop state at the start of each run (`buildlock.py --status`). After the queue change,
phone runs held only a device and a run slot, so desktop builds could run alongside them. That
affects the members' CPU (send cadence, and so transit p95 and m2e), not the phone host's CPU:

| Run | Desktop at start |
|---|---|
| phone-n3-k2 | Held the global lock: no builds; load ≈ 1 |
| phone-n5-k2 | CPU pressure 0 %, load 0.7 (a video test was just starting) |
| phone-n8-k2 | CPU pressure 39 %, load 10.7 (cargo test -p video compiling) |
| phone-n8-k2, forwarding only | CPU pressure 39 %, load 36.5 |

## Findings

1. **CPU.** A phone host is about 15× more expensive per packet than a desktop host, but it
   still fits easily.
   - Host CPU grows roughly linearly with packet rate. Between N = 3 and N = 8 it is about
     0.05 % of a core per packet/s, so about 0.5 ms of CPU per packet, plus about 12 % of a core
     fixed. The fixed part is mostly the host's own participant: Opus encode with FEC/DRED, up
     to 3 decodes and the mix. Turning the participant off saved 12.6 points at N = 8 (41.8 →
     29.2).
   - So at N = 8, QUIC forwarding through iroh costs about 29 % of one core for about
     550 packets/s. Compare 2.7 % on the desktop.
   - The ticks are counted at whatever frequency the governor picks. At this light load the
     cores probably run slowly, so the percentage overstates the energy cost.
   - Extrapolating to N = 12 at K = 2 gives about 55–60 % of one core (≈ 10 % of the device).
     That is not measured.
2. **Thermal and battery.** While charging, nothing moved:
   - Thermal status stayed 0 for 32 minutes of hosting.
   - Battery temperature went from 31 to 34 °C, and charge went up (96 → 99 %).
   - The thermal ≥ MODERATE cutoff never came near triggering. In these conditions it is a
     safety net, not a limit. Battery drain on battery was not measured.
3. **Bandwidth.** It is well under §4, because the silence gate works.
   - At N = 8 the phone host used 383 kbps up and 155 kbps down, 242 MB/h. §4's ungated worst
     case for N = 8 is 988 / 532 kbps.
   - For the metered cap of 4, interpolation gives about 100 MB/h at N = 4. N = 5 measured
     135 MB/h.
4. **QUIC ACKs are a large share of host downlink and are missing from §4.**
   - At N = 8 the members send about 55 audio, 14 level-only and 7 ping packets/s. The phone
     host still received 236 pps; the desktop host received 272 pps.
   - The difference, about 160–195 pps, is ACK-only packets: about one per 2 forwarded
     datagrams. That is about 60–70 kbps, about one audio stream.
   - Each packet also wakes the radio and the CPU, and the per-packet cost (about 0.5 ms) is
     what dominates the phone host's CPU.
5. **Latency.**
   - Through the phone host, the median mouth-to-ear delay is 11–22 ms above the loopback
     direct call. That is the second Wi-Fi hop (RTT ≈ 8 ms), plus a jitter target 5–10 ms
     higher because two Wi-Fi legs jitter more.
   - That is within GC-D09 (≤ RTT + 20 ≈ 28 ms). A real direct call between two Wi-Fi devices
     also pays one hop, so the true added delay is smaller.
   - p95 is 160–200 ms, against 81 ms direct. That gap comes from speaker selection: a new
     speaker waits for a slot when K + 1 are busy. Even the desktop host shows p95 114 ms. It
     is not network delay.
   - The forwarding-only run is the worst. Its desktop was at load 36, which delays the
     members' sends.

## Do the phone-host limits hold up?

- **Cap of 8 on unmetered Wi-Fi: yes, with large margin.** 42 % of one core, thermal 0,
  0.5 Mbps total, no loss.
  - Raising it to 12 is plausible from the extrapolation. Measure N = 12 (and a run on battery)
    before changing it.
- **Cap of 4 on metered data: fine on CPU.** Data use is about 100 MB/h at N = 4, which is
  what the user warning should quote.
  - Cellular radio cost and latency were not measured (GC-D05).
- **K = 2 on phones: holds.** The forwarded uplink averaged 7.5 streams at N = 8.
  - K = 3 would add about one copy per speaker overlap. The measured desktop K = 3 run had
    17 % more out-pps than phone K = 2, which a phone could carry (≈ +5 % of one core).
  - K = 2 is still the right default. K is not a CPU constraint on the host; members' decode
    count and mixing clarity matter more.
- **Thermal ≥ MODERATE stops advertising: keep it.** It was never reached while charging on
  Wi-Fi, so it costs nothing in the normal case.
  - This run cannot say whether hosting on battery with the screen off, or in a hot pocket,
    reaches MODERATE (GC-D03).

## Recommended spec changes

- **§4: add the ACK term and the gated numbers.**
  - Host downlink ≈ speakers × 50 pps × ≈ 150 B, plus forwarded pps / 2 × ≈ 45 B of ACKs, plus
    (N − 1) × ≈ 3 pps of level-only and keepalive traffic.
  - Give the measured gated row for N = 8, K = 2 next to the worst-case table: about 380 kbps
    up, 155 kbps down, 240 MB/h.
- **§4 host CPU:** replace "[A] near-zero" with the measured figure: about 0.5 ms of CPU per
  packet on a moto g52 (≈ 42 % of one core at N = 8 including its own participant). Packet
  rate is the cost driver, so:
  - (a) Reduce ACK frequency on the gcall connection (larger `max_ack_delay`, or the QUIC
    ACK-frequency extension) for both members and host. Worth an A/B in this harness.
  - (b) Use `send_many_datagrams` / GSO where iroh allows it.
- **GC-D09 wording:** "added delay" = m2e through the host − m2e of a direct 1:1 call. The
  measured +11–22 ms passes.
- **GC-D02:** this bench covers the CPU and thermal part at 10 min per N. The 30 min runs on
  battery (battery %/h) and GC-D03 (foreground service, Doze) still need the real app and a
  manual unplug.
- **§1.3:** the m2e p95 of 160–200 ms comes from new-speaker switch-in even with the G6-D2
  provisional slot. Hysteresis and averaging are where to look if late first words show up in
  user tests.

## Caveats

- Synthetic audio. The harmonic talker is close to voice for Opus (SILK/hybrid), but real speech
  bitrate and gate behaviour differ. Talkers are independent and do not take turns the way a
  conversation does.
- No foreground service, Doze or screen-off behaviour is measured. The host runs as a shell
  binary from `/data/local/tmp`, not inside the app's `CoreService` (GC-D03 stays open).
- Phone runs were on Wi-Fi while charging only. The host's CPU ticks do not show the radio's
  energy, and there is no battery %/h figure.
- Background traffic on the phone shows in `wlan0` (for example 327 kbps rx at N = 3 against
  63 kbps for the host), so use the host's QUIC counters, not `wlan0`, for the spec numbers.
- Members all run on one desktop. Through a phone host that means desktop → AP → phone → AP →
  desktop. A real member on another phone adds its own radio and jitter.
- `Active` goes as an unreliable datagram rather than on the control stream. Receivers fall back
  to the 200 ms no-datagram rule if one is lost.
- No signing, no membership, no roster. The control-plane CPU is not included.
- The direct baseline is loopback, so its network delay is about 0. That makes "added delay" an
  upper bound.
