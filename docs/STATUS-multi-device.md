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

## Stopped mid-task by the user (NOT merged, unverified)
- 34g chat + "You" sync between own devices: WIP commit 3083577 on branch `task-34g-chatsync` (worktree `.claude/worktree/task-34g-chatsync`).
- 34h-android linking UI: commit a995fa5 on branch `task-34h-android` (worktree `.claude/worktree/task-34h-android`); agent stopped before its emulator verification finished. Needs: build_android.py, two-emulator link test, review, merge.

## Next
1. Finish/verify 34h-android, merge.
2. Resume 34g (chats + You across own devices), verify, merge.
3. Android/desktop: "You" chat entry once 34g lands; real-device test of linking (never the user's Moto g52 without asking).
4. Release checklist in docs/security-review.md (audits, signed-artifact checks, physical-device tests); re-check docs/play/privacy.md wording vs sealed storage.
