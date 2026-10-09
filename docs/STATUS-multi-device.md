# Status: multi-account + device linking (2026-10-09)

## On `dev` (merged, verified: Rust tests, clippy, e2e_callwait/chat/link, Android build)
- 34a core: `accounts/<id>/` layout + migration, sealed contacts/history (S3), account switch/new/remove, device label, `commit_identity` (S1 core), S4, S6.
- 34b proto: DeviceList, Cancel, link QR/handshake, self-sync auth.
- 34c core: ring all of a contact's devices, first answer wins, global decline, answered/declined elsewhere.
- 34e Android + desktop: account switcher, device name, per-account remembered keys, S1 UI; desktop S2 (no keyring => passphrase required).
- 34f core: storage keys from the recovery phrase; restore in place keeps data; `AccountExists(did)`, `did_of_phrase`.
- 34d core: linking (`tinline/link/1`), own-device sync (`tinline/self/1`: registry, contacts, redeemed, revoked, history), unlink.
- 35: S5 voice parser hardened. Security review S1–S6 marked fixed.
- 34h-desktop: desktop linking UI (desktop 32 tests + clippy re-run by lead after merge; linked against headless peer both ways).
- e2e scripts now always rebuild the release peer.
- 34h-android: Android linking UI (link a device, linked devices, unlink, new-device onboarding path, unlinked screen). Verified on the emulator against headless peers with `scripts/e2e_android_link.py`: phone as existing device (same code, wrong passphrase + retry, data arrives, linked devices list, unlink the other device, cancel) and as new device (onboarding link path, contacts + call history arrive, unlinked screen). Core fix found there: a link QR now waits up to 8 s for the relay (a fresh endpoint's QR had none, leaving the scanner to DNS discovery: intermittent "Couldn't reach the other device").
- Fix (lead): unlinking a device from itself now reaches the other devices. Its own tombstone coming back in a batch removed the account before the push (`SelfSync::leaving`), and `wait_pushed` now waits for the peer's ack (a `Hello` after each applied batch), not just a stream write.

## Stopped mid-task by the user (NOT merged, unverified)
- 34g chat + "You" sync between own devices: WIP commit 3083577 on branch `task-34g-chatsync` (worktree `.claude/worktree/task-34g-chatsync`).

## Next
1. Resume 34g (chats + You across own devices), verify, merge.
2. Desktop ↔ Android link on the emulator (only peer ↔ app so far). Android/desktop: "You" chat entry once 34g lands; real-device test of linking (never the user's Moto g52 without asking).
3. Release checklist in docs/security-review.md (audits, signed-artifact checks, physical-device tests); re-check docs/play/privacy.md wording vs sealed storage.
