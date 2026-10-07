# p2p_chat

Peer-to-peer voice calls over [iroh](https://iroh.computer), with osvauld identities. No server
of ours: devices dial each other by key, directly when hole punching works and through iroh's
public relays when it doesn't. A native Android app and a native desktop app share one Rust core.

```
crates/
  identity, cryptography   copied from osvauld2 @ abcae3d: mnemonic -> did:key, signing
  proto                    contact tickets, device attestations, call grants, handshake checks
  audio                    Opus 1.6 (FEC + DRED + deep PLC), adaptive jitter buffer, tone tools
  core      (p2pcore)      the iroh endpoint, contacts, call state machine; UniFFI API
  peer      (p2p-peer)     headless desktop peer for tests: tone/WAV in, WAV out
  desktop   (p2p-desktop)  iced app, cpal audio + WebRTC echo cancellation, tray
android/                   Kotlin + Compose app over p2pcore (cargo-ndk + UniFFI)
docs/                      protocol.md, android-app.md
scripts/                   build + end-to-end tests (Python)
```

## How it works

- **Identity**: a 24-word phrase derives the DID (osvauld `identity`). Each install also has a
  random **device key**, which is its iroh endpoint id; the DID signs an attestation that the
  device is its own.
- **Adding a contact**: show your ticket (QR / text). The other side dials your device, proves
  its DID and device, and the two exchange **grants** — tokens signed by each DID saying "you may
  call me". Tickets are single-use. See `docs/protocol.md`.
- **Calling**: dial the contact's device (stored relay + DNS discovery), present the grant over
  one QUIC control stream; voice is 20 ms Opus frames as QUIC datagrams. Answering renews the
  caller's grant; removing a contact blocks their DID.
- **Always on (Android)**: a foreground service keeps the endpoint up (started on app open, on
  boot, after updates; restarted by the system if killed). Incoming calls ring with a full-screen
  notification over the lock screen. The battery-optimisation exemption keeps it reliable in doze.

## Build and run

```sh
cargo build --release -p desktop -p peer         # desktop app + test peer
./target/release/p2p-desktop                     # tray app; --data DIR for a separate profile
python3 scripts/build_android.py --install --abis x86_64        # emulator build
python3 scripts/build_android.py --install                      # + arm64 for phones
```

Heavy builds queue through `scripts/buildlock.py` (one at a time on this machine).

## Tests

| script | what it proves |
|---|---|
| `cargo test -p proto -p audio` | handshake security checks; codec + jitter buffer under loss/jitter/drift |
| `scripts/e2e_desktop.py` | two peers: add contact, call both ways, direct **and** relay-only |
| `scripts/desktop_e2e.py tone\|mic` | desktop app vs peer; `mic` = real cpal capture path |
| `scripts/e2e_android.py --fresh` | emulator app vs desktop peer, both directions |
| `scripts/e2e_phone_phone.py` | two emulators call each other |
| `scripts/audio_compare.py` | speech in vs speech out: delay + envelope correlation |

Verified by hand on the emulator as well: screen off, forced doze, `kill -9` (service restarts
in ~1 s), reboot (auto-start, call rings over the lock screen). Desktop speech over iroh:
66 ms delay, envelope correlation 0.97.

## Known gaps

- The phone's real mic/speaker content was not verified on an emulator: the headless emulator
  binary has no PulseAudio backend and the windowed one hung here. AudioEngine is exercised for
  init/timing; content needs a real phone.
- One device per DID is dialled at a time (newest first); a phrase restored on a second phone
  gets a new device key, but nothing yet announces it to contacts until it calls them.
- Secrets (phrase, device key) are sealed under a passphrase (Argon2id, see `docs/vault.md`); the
  platform UIs for it and the Keystore-wrapped unlock key are still being built. No Bluetooth
  audio routing, no QR scanning on desktop.

## License

GPL-3.0-or-later (see `LICENSE`). Free and open source, no ads, no tracking: anyone who
distributes a modified version must publish its source under the same terms. `crates/identity`
and `crates/cryptography` come from osvauld2 and are relicensed here by their author.
