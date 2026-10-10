#!/usr/bin/env python3
"""Build a static, encoder-only, realtime-only libaom for one target.

    scripts/build_libaom.py --target host
    scripts/build_libaom.py --target aarch64-linux-android

Prints the install prefix; point the video crate at it with
`AOM_DIR=<prefix> cargo build ...` (its build.rs then links `<prefix>/lib/libaom.a`
statically instead of the system libaom).

Source and builds live under ~/build-cache/p2p_chat/libaom (regenerable).
"""
import argparse
import os
import subprocess
import sys
from pathlib import Path

TAG = "v3.14.1"
REPO = "https://aomedia.googlesource.com/aom"
ROOT = Path.home() / "build-cache" / "p2p_chat" / "libaom"
ANDROID_ABI = {
    "aarch64-linux-android": ("arm64-v8a", "arm64"),
    "x86_64-linux-android": ("x86_64", "x86_64"),
}
MIN_SDK = "28"


def run(cmd, **kw):
    print("+", " ".join(str(c) for c in cmd), file=sys.stderr)
    subprocess.run(cmd, check=True, **kw)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--target", default="host", help="host or a Rust Android target triple")
    ap.add_argument("--jobs", default=str(os.cpu_count() or 4))
    args = ap.parse_args()

    src = ROOT / f"src-{TAG}"
    if not src.exists():
        ROOT.mkdir(parents=True, exist_ok=True)
        run(["git", "clone", "--depth", "1", "--branch", TAG, REPO, str(src)])

    build = ROOT / f"build-{TAG}-{args.target}"
    prefix = ROOT / f"install-{TAG}-{args.target}"
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
    if args.target != "host":
        if args.target not in ANDROID_ABI:
            sys.exit(f"unknown target {args.target}")
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
    run(cmake)
    run(["cmake", "--build", str(build), "-j", args.jobs])
    run(["cmake", "--install", str(build)])
    print(prefix)


if __name__ == "__main__":
    main()
