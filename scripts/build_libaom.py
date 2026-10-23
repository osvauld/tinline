#!/usr/bin/env python3
"""Build a static, encoder-only, realtime-only libaom for one target.

    scripts/build_libaom.py --target host
    scripts/build_libaom.py --target aarch64-linux-android
    scripts/build_libaom.py --target aarch64-apple-ios      (macOS with Xcode; also aarch64-apple-ios-sim)

Prints the install prefix (last stdout line); point the video crate at it with
`AOM_DIR_<triple>=<prefix>` or `AOM_DIR=<prefix>` (its build.rs then links `<prefix>/lib/libaom.a`
statically instead of the system libaom).

Source is the official release tarball, pinned by SHA-256 (checked before extraction and on every
run); it is byte-identical to the git tag's tree (commit COMMIT). Everything lives under
~/build-cache/p2p_chat/libaom (regenerable). Idempotent: a rerun only re-checks and re-links.
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

VERSION = "3.15.2"
TAG = f"v{VERSION}"
COMMIT = "af3dc9aadc793c1b00edc003e3fc19a44fdeb574"
URL = f"https://storage.googleapis.com/aom-releases/libaom-{VERSION}.tar.gz"
SHA256 = "67bb54b245f33ed98600e08269e6139986e48114e14042474ddd8885801dfddc"
ROOT = Path(os.environ.get("AOM_CACHE") or Path.home() / "build-cache" / "p2p_chat" / "libaom")
ANDROID_ABI = {
    "aarch64-linux-android": ("arm64-v8a", "arm64"),
    "x86_64-linux-android": ("x86_64", "x86_64"),
}
MIN_SDK = "28"
# Rust iOS triple -> Xcode SDK. Keep IOS_MIN in step with ios/project.yml deploymentTarget.
IOS_SDK = {
    "aarch64-apple-ios": "iphoneos",
    "aarch64-apple-ios-sim": "iphonesimulator",
}
IOS_MIN = "17.0"


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
    tarball = ROOT / f"libaom-{VERSION}.tar.gz"
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
        sys.exit(f"libaom tarball sha256 mismatch: got {got}, want {SHA256} (deleted; rerun to refetch)")
    if stamp.exists() and stamp.read_text().strip() == SHA256:
        return src
    tmp = ROOT / f"extract-{os.getpid()}"
    shutil.rmtree(tmp, ignore_errors=True)
    with tarfile.open(tarball) as t:
        t.extractall(tmp, filter="data")
    shutil.rmtree(src, ignore_errors=True)
    (tmp / f"libaom-{VERSION}").rename(src)
    tmp.rmdir()
    stamp.write_text(SHA256 + "\n")
    return src


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--target", default="host", help="host, or a Rust Android or iOS target triple")
    ap.add_argument("--jobs", default=str(os.cpu_count() or 4))
    args = ap.parse_args()
    if args.target != "host" and args.target not in ANDROID_ABI and args.target not in IOS_SDK:
        sys.exit(f"unknown target {args.target}")

    src = fetch_source()
    build = ROOT / f"build-{TAG}-{args.target}"
    prefix = ROOT / f"install-{TAG}-{args.target}"
    cache = build / "CMakeCache.txt"
    if cache.exists() and f"CMAKE_HOME_DIRECTORY:INTERNAL={src}\n" not in cache.read_text():
        shutil.rmtree(build)  # configured from another (older/unverified) source tree
    cmake = [
        "cmake", "-S", str(src), "-B", str(build), "-G", "Ninja",
        "-DCMAKE_BUILD_TYPE=Release",
        f"-DCMAKE_INSTALL_PREFIX={prefix}",
        "-DCMAKE_POSITION_INDEPENDENT_CODE=ON",
        "-DBUILD_SHARED_LIBS=OFF",
        # Only what a call needs: the realtime AV1 encoder (rav1d decodes).
        "-DCONFIG_REALTIME_ONLY=1",
        "-DCONFIG_AV1_DECODER=0",
        "-DCONFIG_AV1_HIGHBITDEPTH=0",
        "-DENABLE_DOCS=0", "-DENABLE_EXAMPLES=0", "-DENABLE_TESTS=0",
        "-DENABLE_TESTDATA=0", "-DENABLE_TOOLS=0",
    ]
    if args.target in IOS_SDK:
        # CMake's own iOS support; untested until the first Mac build.
        cmake += [
            "-DCMAKE_SYSTEM_NAME=iOS",
            f"-DCMAKE_OSX_SYSROOT={IOS_SDK[args.target]}",
            "-DCMAKE_OSX_ARCHITECTURES=arm64",
            f"-DCMAKE_OSX_DEPLOYMENT_TARGET={IOS_MIN}",
            "-DAOM_TARGET_CPU=arm64",
            "-DCONFIG_RUNTIME_CPU_DETECT=0",
        ]
    elif args.target != "host":
        abi, cpu = ANDROID_ABI[args.target]
        ndk = os.environ.get("ANDROID_NDK_HOME") or os.environ.get("ANDROID_NDK_ROOT")
        if not ndk:
            sys.exit("set ANDROID_NDK_HOME")
        cmake += [
            f"-DCMAKE_TOOLCHAIN_FILE={ndk}/build/cmake/android.toolchain.cmake",
            f"-DANDROID_ABI={abi}",
            f"-DANDROID_PLATFORM=android-{MIN_SDK}",
            f"-DAOM_TARGET_CPU={cpu}",
        ]
    # Reconfiguring regenerates the rtcd headers and so rebuilds everything: only when args change.
    args_stamp = build / ".tinline-cmake-args"
    if not (build / "build.ninja").exists() or not args_stamp.exists() or args_stamp.read_text() != "\n".join(cmake):
        run(cmake)
        args_stamp.write_text("\n".join(cmake))
    run(["cmake", "--build", str(build), "-j", args.jobs])
    run(["cmake", "--install", str(build)])
    print(prefix)


if __name__ == "__main__":
    main()
