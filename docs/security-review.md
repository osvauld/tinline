# Pre-release security review

Reviewed source at `0c32480` on branch `dev`. This is a source/documentation review,
not an independent audit or release certification. Concurrent worktrees (including chat)
are outside this snapshot; review again after merging them.

## Release decision

**Not ready for an unrestricted public release.** Correct public claims, address the
key-persistence failure paths, settle the desktop plaintext-key policy, and complete
artifact/dependency/device verification before publishing. Restricted testing is not
proof of production readiness.

## Findings

### S1 — High: no-passphrase identity can lose its only unlock key

Status: FIXED in tasks 34a (core `commit_identity`: a no-passphrase identity is written only after the platform saved its key), 34e-android and 34e-desktop (save result checked, read back on Android, Retry / Add a passphrase on failure). Not exercised on a device: a real Keystore/keyring failure.

Evidence: `Node::set_identity` saves the sealed profile before platform key persistence.
Android `P2pApp.rememberKey` ignores the Boolean returned by `UnlockStore.save`;
`identityReady` continues to start the service. Desktop `keystore::sync` returns no result
and only logs a save failure; onboarding in `app.rs` continues. With no passphrase,
no recoverable DEK remains on disk if key persistence fails and the process exits.
Recovery requires the mnemonic; users may not have saved it.

Required: return and surface persistence failures, do not report successful setup until
key storage succeeds, offer retry or adding a passphrase while still unlocked. Test
Keystore/keyring failures, disk-full/read-only failures, and process death between the
profile write and key write. Design crash recovery for this two-store transaction.

### S2 — High for file-copy threat model: desktop plaintext DEK fallback

Status: FIXED in task 34e-desktop: with no secure keyring the passphrase is required at setup and nothing is written in plaintext; an existing `unlock.key` install keeps working with a Settings warning, and adding a passphrase deletes the file. Windows ACLs on legacy files remain unverified.

Evidence: `crates/desktop/src/keystore.rs::save/write_file` falls back to `unlock.key`
in the same data directory as `profile.json`. Unix uses 0600; the non-Unix branch uses
`std::fs::write` without explicit ACL hardening. Anyone who obtains both files can decrypt
the identity without guessing a passphrase. File permissions are not encryption.
Settings discloses the fallback, but that does not establish informed onboarding consent.

Required: prefer refusing no-passphrase setup when a secure keyring is unavailable, with
an explicit passphrase alternative. If retained, require informed opt-in and accurately
state the protection; verify Windows ACLs. Do not claim protection against copying the
whole data directory in this mode. Desktop keyring entries are per canonical data directory;
that earlier collision fix is present.

### S3 — High publication blocker: privacy policy overstates local encryption

Status: FIXED in task 34a: contacts, grants, blocks, redeemed nonces, the ticket, call history and the device label are sealed in `accounts/<id>/account.redb` (keys from the recovery phrase since 34f). Still in the clear: the account display name, DID and device public key in `account.json` (lock screen), and directory names. Re-check `docs/play/privacy.md` wording against this.

Evidence: `store.rs` serializes `state.json` and `calls.json` as ordinary JSON. State
contains contact DIDs/names, devices, grants, block lists and the bearer contact ticket.
Public profile fields and platform settings are also not vault-encrypted. The mnemonic
is stored inside the encrypted vault and can be shown again, not merely shown once.
`docs/play/privacy.md` previously claimed all those records were encrypted and the phrase
was not stored. Corrected during this review; encryption of metadata remains unimplemented.

Required: use the corrected policy on the website/store. Decide whether plaintext local
metadata is an accepted threat-model limitation or needs DEK sealing before release.

### S4 — Medium: incoming device updates do not reset verification

Status: FIXED in tasks 34a/34c: `note_device` and incoming DeviceLists reset `verified` for any device not seen before, persist, and emit contacts-changed; tests `tests/devices.rs`, `tests/fanout.rs`.

Evidence: `node.rs::save_contact` clears `verified` for an unfamiliar device, but
`note_device` (called after authenticated incoming calls) inserts a new device without
clearing it or notifying the UI of contact changes. `docs/protocol.md` and `Contact`
comments promise a reset on device change.

This is not an unauthenticated identity takeover: the new device must have a valid
DID-signed attestation and grant. It is inconsistent verification UI/policy, relevant
when a phrase or device is compromised.

Required: choose DID-only verification or device-sensitive verification consistently.
For the documented policy, reset on every unfamiliar device and emit contact changes.
Add an integration test using a second device of the same DID.

### S5 — Medium, gate before chat exposure: voice file parser is unbounded/permissive

Status: FIXED in task 35 (`crates/core/src/voice.rs`): 16 MiB file cap, 100k packets, 30 min, 7650-byte audio packets (64 KiB tags); Ogg version/flags/BOS/EOS/CRC/serial/sequence/continuation/granule checks, strict OpusHead (mono, family 0, major 0, pre-skip bound), unfinished/truncated files rejected; randomized mutation test.

Evidence: `VoiceDecoder::open` reads the whole file and `read_ogg` copies packets into
memory. It checks framing bounds but not CRC, stream serial/sequence, continuation rules,
Ogg version, or an unfinished final packet. OpusHead validation only checks prefix/length,
not the supported channel/mapping/version fields. Large hostile files can amplify memory
use. No arbitrary-code-execution finding was established.

Required: enforce file/packet/count/duration limits before allocation; validate supported
Ogg Opus format; fuzz malformed/truncated/continued pages and native Opus calls. Review
plaintext recording/playback temporary-file cleanup before advertising encrypted chat.
The current main workspace has voice helpers, not shipped chat transport.

### S6 — Medium reliability: contact acceptance is not one durable transaction

Status: FIXED in task 34a: contact insertion, nonce redemption and ticket drop commit in one sealed-store transaction before Welcome; a failed commit sends Reject.

Evidence: incoming `ContactHello` spends and persists the nonce, sends Welcome, then
calls `save_contact`. A crash/write failure between these steps can leave the joiner
with a contact/grant while the owner has no saved contact and an already-spent ticket.
This does not bypass authentication, but breaks pairing and recovery.

Required: persist contact insertion and ticket redemption together before Welcome;
notify UI after commit. Test failure/crash boundaries and retry UX.

## Previously reported protections: source check

`TASKS.md` records reviews/fixes in tasks 10 and 15–18, but is not a full findings ledger
or reproducible audit report. The following protections are present in this snapshot:

- Strict Ed25519 verification (`cryptography/src/signature.rs::verify_strict`).
- Domain-separated signatures; attestation bound to QUIC-authenticated device; ticket
  field matching, expiry, joiner binding and grant holder/issuer/capability checks.
- Current-ticket-only redemption and nonce consumption under a shared lock.
- 64 KiB control frame bound, 15 s initial hello deadline, 24 general + 8 reserved
  incoming slots; generic unauthenticated refusal messages.
- Local DID blocks; re-adding by scanning lifts the block. Individual grant/device
  revocation is NOT implemented; attestations have no expiry.
- AES-256-GCM vault envelope encryption and bounded Argon2id parameters. Nonempty
  passphrases have no minimum strength; recommend long unique passphrases.
- Unix private directory/file permissions and fsync/rename store writes. Windows
  permission/crash behavior has not been established by this review.
- Android `allowBackup=false`, non-exported service/call activities/boot receiver,
  secure-window helper and debug-only receiver source separation.
- Android Keystore AES wrapping has no user-auth requirement for background startup;
  hardware backing is device-dependent, not guaranteed by the current key specification.
- Desktop automation knobs are gated by the `test-hooks` feature (but a release profile
  can still be built WITH that feature; artifact checks must reject it).

## Claims and threat model to retain

- Calls use peer-authenticated iroh QUIC/TLS encryption, including over relays. There is
  no separate application media cipher or ratchet. Cached iroh 1.3 source configures TLS
  1.3; do not equate the lack of an application ratchet with absence of TLS forward secrecy.
  This review did not independently verify TLS session/key-compromise behavior.
- No server operated by this app is needed for calls, but public relay and discovery
  infrastructure is used. This is not an infrastructure-free or anonymous service.
- Direct peers can learn each other's network addresses; relay/discovery providers see
  metadata. Generic Reject hides the reason, not reachability/timing or all contact inference.
- Mnemonic compromise grants identity control; there is no global device revocation.
  Local blocks cannot remotely erase another person's data.
- Remembered Android keys let the app auto-unlock even with a passphrase configured.
  A passphrase is not a screen-lock gate on every startup. Desktop drops remembered
  keys once a passphrase is configured. Neither protects an already compromised process.
- Contact tickets are bearer invitations: possession permits redemption. Reset leaked
  tickets; compare safety numbers via a trusted channel.

## Release acceptance checklist (not yet completed here)

- [x] Resolve S1; decide/implement S2 policy; resolve S4 and S6; gate S5 before voice/chat (2026-10-09, tasks 34a–35).
- [ ] Run `cargo test --locked -p identity -p cryptography -p proto -p audio -p p2pcore -p desktop`
  through `scripts/buildlock.py`; rerun direct/relay/call-waiting E2E on the release commit.
- [ ] Run RustSec/dependency/license checks for Rust AND Gradle/native dependencies, triage
  results, and retain the reports/SBOM. `cargo-audit`/`cargo-deny` were not installed here.
- [ ] Verify the actual signed AAB/APK: no debug receiver/gallery, debuggable false, expected
  exported components/permissions, no test automation. Gradle release currently falls back
  to debug signing without credentials; `build_android.py --bundle` checks for signing
  configuration, but the final signing certificate must still be verified.
- [ ] Test physical Android devices: mic/speaker, lock screen, reboot, Doze, permission
  denial/revocation, key loss, interrupted onboarding and restore, API 28–36/OEM restrictions.
- [ ] Freeze reviewed source, use locked builds, publish signed artifacts/checksums and
  matching GPL source; add release CI and a vulnerability reporting channel.
- [ ] Validate live privacy/terms/source/download URLs and Play declarations against the
  exact release. Recheck Google Data Safety definitions for third-party metadata rather
  than assuming end-to-end encryption exempts all network processing.
- [ ] Independently review chat/authorship/blob authorization after merge. `docs/chat.md`
  describes planned behavior; it is not evidence of implemented security.

## Validation performed

Read protocol/vault/Android/Play/chat docs and relevant core/proto/crypto/platform source.
No files containing credentials were needed. No exploits or production network probes
were run. Earlier test invocation waited behind another build and was aborted; tests,
dependency audits and release artifact inspection are outstanding. No security-code fixes
were made as part of this documentation pass.
