# Refactor wave 1: common brief for every agent

Read first: `docs/refactor/PLAN.md` (the "Structure standards" and "Wave 1" sections) and the audit report for
your area in `docs/refactor/audit/`. Also read `AGENTS.md`.

## Where you work
- Only in your own worktree (absolute path is in your task). Branch `rf/<name>`, created from `refactor/main`.
- Edit ONLY the paths your task says you own. If a change is needed elsewhere, write it in your final report; do not make it.
- Never touch `crates/desktop`, `android/`, `dev`, the main checkout `/home/abe/p2p_chat`, other worktrees, or the stash.
- `target` is a symlink to a target dir shared by all refactor agents. Never delete or clean it.

## Builds and tests
- Every cargo command goes through the queue, by absolute path:
  `python3 /home/abe/p2p_chat/scripts/buildlock.py --who rf-<name> -- cargo test -p <crate> ...`
  Check `--status` first. Don't loop cargo inside `sh -c`. Never `touch` sources. Don't start a duplicate job after a timeout.
- Run only what your change touches: `-p <crate>`, `--lib`, `--test <file>`. Run the crate's full suite once at the end.
  Many core integration tests need a real relay or network. If one fails, check whether it also fails on the
  base commit (698bcb1) before you blame your change, and report it.
- `cargo fmt` only on files you changed (`rustfmt --edition 2024 <files>`), never the whole workspace.
- Before you finish: `cargo clippy -p <crate> --all-targets` must show no NEW warnings in files you touched.

## How to change code
- Behaviour-preserving. Moves and splits must not change logic, wire bytes, persisted data, or public API, unless your
  task explicitly says so. Keep `pub` paths stable with `pub use` re-exports when you move items between modules.
- Size: production files ≤600 lines (aim for 400); functions ≤120 lines (aim for 60) where a split is natural.
- Unit tests live in their own file: `foo.rs` has `#[cfg(test)] mod tests;` and the tests go in `foo/tests.rs`
  (or `foo/tests/<topic>.rs`). For `mod.rs` modules, use `tests.rs` beside it.
- Test count: record `git grep -c -E '#\[(tokio::)?test' -- crates/<crate>` before and after. It must not drop.
  If you merge duplicate tests on purpose, list each one removed and the test that covers it now.
- Match the surrounding style and comment density. Keep module doc comments; update them if they become wrong.
- Leave open PROVISIONAL / spec decisions as they are. Do not resolve product or security questions yourself.

## Commits
- Commit small verified steps on your branch (`git add <paths>`, never `git add .` or `-a`). Message: short
  imperative summary + why, ending with `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`.
- Do not merge, rebase onto other branches, or push.

## Final report (reply, ≤40 lines)
Commits (sha + one line), files before/after (largest file sizes), test count before/after, exact test commands
and results, any pre-existing failures, anything you could not do, and suggestions for wave 2.
