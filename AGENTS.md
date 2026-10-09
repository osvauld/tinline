# Endpoint rotation worktree — session/subagent entry point

This worktree is dedicated to endpoint rotation and DID blocking.

**Read first:** `docs/design/rotation-agent-handoff.md`, then the documents it
lists. Task packets and ownership are in `docs/design/rotation-agent-tasks.md`.
Update `docs/design/rotation-agent-status.md` when claiming or finishing work.

## Non-negotiable coordination rules

- Work in this checkout, branch `endpoint-rotation-blocking`. Do not reset,
  clean, switch branches, overwrite another agent's work or modify other worktrees.
- Existing rotation tests/docs are uncommitted/untracked at handoff. They are
  intentional work, not disposable scratch files. Preserve them.
- User wants test-first, deterministic peer/device simulation before production
  integration. Existing passing models do not establish production correctness.
- Device IDs are owner-replaceable. Earlier permanent-slot assumptions are
  superseded; do not freeze them into the protocol.
- The DID is stable; endpoint rotation is periodic/configurable, not triggered by
  blocking. Exclude blocked contacts from updates; enforce verified-DID policy.
- Group delegation and device succession are separate. Group membership policy
  and device revocation must not be implicitly changed by a personal block.
- No anonymity promise. Shared account mnemonic means unlinking is not strong
  cryptographic revocation of a hostile linked device.
- No independent future-endpoint derivation scheme is selected. Do not invent one
  or claim signatures/permits alone solve offline discovery.
- Use `scripts/buildlock.py` for heavy builds. Report exact commands and results.
- Coordinate file ownership BEFORE parallel edits. On this shared worktree only
  the coordinating agent integrates shared files or stages/commits changes, and
  only when requested. Do not use `git add .` to scoop up unrelated work.
- Stop at unresolved policy gates; propose alternatives and tests instead of
  silently deciding security/product semantics.

Scope: this file governs the rotation worktree, not sibling group-chat work.
