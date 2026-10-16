#!/bin/bash
# rustc wrapper (cargo's rustc-wrapper / RUSTC_WRAPPER): sccache, with a stable OUT_DIR for
# downloaded crates so their cache keys don't depend on the worktree.
#
# Cargo's OUT_DIR is an absolute path inside each worktree's target/. sccache hashes it, and
# `include!(concat!(env!("OUT_DIR"), ..))` bakes it into the rlib, so every dependent misses too:
# a second worktree hit 0 of 79 crates. Pointing OUT_DIR at a symlink named by crate and cargo
# metadata hash (the same in every worktree for the same crate version and features) gave 79/79.
#
# Only for registry and git crates: their build-script output is the same in every worktree.
# Workspace crates can differ between branches with the same metadata hash, and two worktrees
# compiling one at once would race on the shared link, so they keep their own OUT_DIR.
STABLE_DIR=/home/abe/build-cache/sccache-outdirs
if [ -n "$OUT_DIR" ] && [[ "$CARGO_MANIFEST_DIR" == */.cargo/registry/* || "$CARGO_MANIFEST_DIR" == */.cargo/git/* ]]; then
  name= meta= prev=
  for a in "$@"; do
    [ "$prev" = "--crate-name" ] && name=$a
    case "$a" in metadata=*) meta=${a#metadata=} ;; esac
    prev=$a
  done
  if [ -n "$name" ] && [ -n "$meta" ] && mkdir -p "$STABLE_DIR"; then
    link=$STABLE_DIR/$name-$meta
    ln -sfn "$OUT_DIR" "$link.$$" && mv -T "$link.$$" "$link" && export OUT_DIR=$link
  fi
fi
exec sccache "$@"
