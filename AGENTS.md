# Repository instructions

This repository contains Tinline: Rust core/protocol/media crates, an Iced desktop
app, and a Kotlin/Compose Android app. The Svelte playground under
`docs/design/playground/` is a design reference, not the shipped application.

## Shared checkout safety

- Verify the working directory, branch, HEAD and `git status --short` before work.
  The integration checkout normally uses `dev`; do not switch branches or reset it.
- Other agents may be editing or building concurrently. Coordinate exact file
  ownership before parallel edits; do not overwrite another agent's changes.
- Do not modify or remove other worktrees. Do not delete build outputs while a
  build/test or device session may be using them.
- Do not stage, commit, merge, push, install on the user's phone, or wipe app data
  without explicit authorization. Never use `git add .` to collect unrelated work.
- Keep generated screenshots, test logs and local build artifacts out of Git.
  Preserve intentional visual baselines and required runtime/package assets.

## Builds and tests

- Heavy local builds/tests must use the shared build queue:
  `python3 scripts/buildlock.py --who <unique-agent-name> -- <command>`.
- All build output and caches live on the cache disk `/home/abe/build-cache`: buildlock
  symlinks each checkout's `target/`, `android/build` and `android/app/build` there, runs
  Gradle with `GRADLE_USER_HOME=/home/abe/build-cache/gradle`, and every cargo build shares compiled crates
  through sccache (`/home/abe/build-cache/sccache`; machine-wide via `~/.cargo/config.toml`,
  set up by `scripts/setup_build_cache.py`). Don't override `RUSTC_WRAPPER`/`CARGO_TARGET_DIR`/`GRADLE_USER_HOME`
  or point several worktrees at one `target/` (branches overwrite each other's binaries).
- Inspect `python3 scripts/buildlock.py --status` before starting work. Do not
  bypass admission, launch duplicate jobs after a timeout, or terminate another
  agent's processes. Use the existing device lock for emulator/phone operations.
- Run targeted checks for owned changes; report exact commands, results, ignored
  tests and limitations. Do not claim models prove production integration.
- Timing-sensitive video tests need an appropriate release profile. Separate
  network/device-dependent validation from deterministic offline tests.
- Format only owned files, not the entire shared workspace.

## Protocol and product constraints

- Read the relevant current protocol/subsystem docs before changing behavior.
  Important references: `docs/protocol.md`, `docs/chat.md`, `docs/group-core.md`,
  `docs/vault.md`, `docs/video.md`, and `docs/design/rotation-open-decisions.md`.
- The account DID is stable; device IDs are owner-replaceable. Endpoint rotation
  is periodic/configurable, not triggered by blocking. Enforce verified-DID policy.
- Personal blocking, group delegation/membership and device succession/revocation
  are separate policies; do not implicitly change one through another.
- No anonymity promise. Shared account mnemonics do not provide strong
  cryptographic revocation of a hostile linked device.
- Do not invent offline-discovery guarantees or resolve open security/product
  decisions silently. Propose alternatives and acceptance tests.
- Preserve author proofs and user data. Use synthetic identities in tests.

Follow more specific `AGENTS.md` instructions within subdirectories. Current UI
integration is active: coordinate before removing its design references or previews.
