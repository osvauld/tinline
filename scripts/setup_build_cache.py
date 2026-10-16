#!/usr/bin/env python3
"""Machine setup: every cargo build shares compiled crates through sccache, its cache on the
build-cache disk (all build output lives there; see AGENTS.md).

    python3 scripts/setup_build_cache.py          # writes the two configs if missing
    python3 scripts/setup_build_cache.py --check  # reports, writes nothing

Writes ~/.cargo/config.toml (rustc-wrapper = sccache) and ~/.config/sccache/config (cache dir +
size). Machine-wide, so worktrees made before scripts/buildlock.py learnt about sccache use it
too. Existing files are never overwritten: it says what to add instead. Install sccache first
(`cargo install sccache --locked`, or the distro package).
"""
import argparse
import shutil
import sys
from pathlib import Path

CACHE = Path.home() / "build-cache"
SCCACHE_DIR = CACHE / "sccache"
SIZE = 30 * 1024**3

# sccache with a stable OUT_DIR for downloaded crates (see the script).
SHIM = Path(__file__).resolve().parent / "sccache-rustc.sh"

CARGO_CONFIG = Path.home() / ".cargo" / "config.toml"
SCCACHE_CONFIG = Path.home() / ".config" / "sccache" / "config"


def cargo_text(sccache):
    return ("# Every cargo build on this machine shares compiled crates through sccache; its cache is on\n"
            f"# {CACHE} ({SCCACHE_CONFIG}). Remove this line to turn it off.\n"
            f'[build]\nrustc-wrapper = "{SHIM}"\n')


def sccache_text():
    return ("# All build caches live on the cache disk (see p2p_chat AGENTS.md).\n"
            f'[cache.disk]\ndir = "{SCCACHE_DIR}"\nsize = {SIZE}\n')


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--check", action="store_true", help="report only")
    a = ap.parse_args()
    sccache = shutil.which("sccache") or str(Path.home() / ".cargo" / "bin" / "sccache")
    if not Path(sccache).exists():
        sys.exit("sccache is not installed: cargo install sccache --locked")
    if not CACHE.is_dir():
        sys.exit(f"{CACHE} is missing: mount the build-cache disk first")
    ok = True
    for path, text, key in ((CARGO_CONFIG, cargo_text(sccache), "rustc-wrapper"),
                            (SCCACHE_CONFIG, sccache_text(), str(SCCACHE_DIR))):
        if path.exists():
            if key in path.read_text():
                print(f"ok       {path}")
            else:
                ok = False
                print(f"MISSING  {path} exists without it; add:\n{text}")
        elif a.check:
            ok = False
            print(f"MISSING  {path}")
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
            print(f"wrote    {path}")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
