#!/usr/bin/env python3
"""Build a static, VP8-only (decoder + realtime encoder) libvpx for the desktop (linux host).

    scripts/build_libvpx.py

Prints the install prefix (last stdout line); point the video crate (feature `vp8`) at it with
`VPX_DIR_<triple>=<prefix>` or `VPX_DIR=<prefix>` (its build.rs then links `<prefix>/lib/libvpx.a`
statically instead of the system libvpx). Android does not use libvpx: it codes VP8 with
MediaCodec (docs/design/video-hw-vp8.md). The desktop decodes phones' hardware-encoded VP8 and
encodes VP8 itself for peers with a hardware VP8 decoder.

Source is the GitHub mirror's archive of the official release tag TAG (commit COMMIT on
chromium.googlesource.com/webm/libvpx), pinned by SHA-256 (checked before extraction and on every
run). Everything lives under ~/build-cache/p2p_chat/libvpx (regenerable). Idempotent: a rerun only
re-checks and re-links.

The decoder parses peer-controlled bytes: the build caps frames at SIZE_LIMIT and the Rust side
rejects anything above the call's `max_pixels` before decoding.
"""
import argparse
import hashlib
import os
import shutil
import subprocess
import sys
import tarfile
import urllib.request
from pathlib import Path

VERSION = "1.17.0"
TAG = f"v{VERSION}"
COMMIT = "6df3ec34557879fff673706f4a1d9fbd0f3a6f0e"
URL = f"https://github.com/webmproject/libvpx/archive/refs/tags/{TAG}.tar.gz"
SHA256 = "1020f184046187baa2985dbde38e0691f49c44088bca7a1842b0236c6081dc0a"
SIZE_LIMIT = "4096x4096"
ROOT = Path(os.environ.get("VPX_CACHE") or Path.home() / "build-cache" / "p2p_chat" / "libvpx")


def run(cmd, **kw):
    print("+", " ".join(str(c) for c in cmd), file=sys.stderr)
    subprocess.run(cmd, check=True, stdout=sys.stderr, **kw)


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def fetch_source():
    """Verified source tree; never builds from anything but the pinned tarball."""
    tarball = ROOT / f"libvpx-{VERSION}.tar.gz"
    src = ROOT / f"src-{VERSION}-{SHA256[:12]}"
    stamp = src / ".tinline-sha256"
    if not tarball.exists():
        ROOT.mkdir(parents=True, exist_ok=True)
        part = tarball.with_suffix(".part")
        print(f"fetch {URL}", file=sys.stderr)
        with urllib.request.urlopen(URL, timeout=60) as r, open(part, "wb") as f:
            shutil.copyfileobj(r, f)
        part.rename(tarball)
    got = sha256(tarball)
    if got != SHA256:
        tarball.unlink()
        sys.exit(f"libvpx tarball sha256 mismatch: got {got}, want {SHA256} (deleted; rerun to refetch)")
    if stamp.exists() and stamp.read_text().strip() == SHA256:
        return src
    tmp = ROOT / f"extract-{os.getpid()}"
    shutil.rmtree(tmp, ignore_errors=True)
    with tarfile.open(tarball) as t:
        t.extractall(tmp, filter="data")
    shutil.rmtree(src, ignore_errors=True)
    (tmp / f"libvpx-{VERSION}").rename(src)
    tmp.rmdir()
    stamp.write_text(SHA256 + "\n")
    return src


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--target", default="host", help="only 'host' (linux desktop)")
    ap.add_argument("--jobs", default=str(os.cpu_count() or 4))
    args = ap.parse_args()
    if args.target != "host":
        sys.exit("libvpx is built for the desktop host only (Android decodes VP8 with MediaCodec)")

    src = fetch_source()
    build = ROOT / f"build-{TAG}-{args.target}"
    prefix = ROOT / f"install-{TAG}-{args.target}"
    configure = [
        str(src / "configure"),
        f"--prefix={prefix}",
        "--enable-static", "--disable-shared", "--enable-pic",
        # Only what a call needs: the VP8 decoder and encoder (VP9 stays off).
        "--disable-vp9", "--enable-vp8-encoder", "--enable-vp8-decoder",
        f"--size-limit={SIZE_LIMIT}",
        "--disable-examples", "--disable-tools", "--disable-docs", "--disable-unit-tests",
        "--disable-install-docs", "--disable-install-bins", "--disable-install-srcs",
        "--disable-webm-io", "--disable-libyuv", "--disable-postproc",
        "--disable-error-concealment",
        "--as=nasm",
    ]
    stamp = build / ".tinline-configure-args"
    if stamp.exists() and stamp.read_text() != "\n".join(configure):
        shutil.rmtree(build)  # other source tree or options: start clean
    if not stamp.exists():
        build.mkdir(parents=True, exist_ok=True)
        run(configure, cwd=build)
        stamp.write_text("\n".join(configure))
    run(["make", "-j", args.jobs], cwd=build)
    run(["make", "install"], cwd=build)
    print(prefix)


if __name__ == "__main__":
    main()
