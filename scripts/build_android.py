#!/usr/bin/env python3
"""Build the Android app (Gradle runs cargo-ndk + uniffi-bindgen), optionally install and launch.

    scripts/build_android.py [--install] [--serial emulator-5554 ...] [--abis x86_64] [--debug-rust] [--release]

Goes through scripts/buildlock.py itself, so it waits its turn behind other heavy builds.
--serial may repeat to install on several devices; without it, adb picks the only device.
"""
import argparse
import os
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
APK = ROOT / "android/app/build/outputs/apk/debug/app-debug.apk"
RELEASE_APK = ROOT / "android/app/build/outputs/apk/release/app-release.apk"


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--install", action="store_true")
    ap.add_argument("--serial", action="append", default=[])
    ap.add_argument("--abis", help="comma list, e.g. x86_64 or x86_64,arm64-v8a")
    ap.add_argument("--release", action="store_true",
                    help="minified release APK (signed with the debug key unless RELEASE_* gradle properties are set; "
                         "has no DebugReceiver, so the e2e scripts cannot drive it)")
    ap.add_argument("--debug-rust", action="store_true", help="cargo debug profile (faster build, slow audio)")
    a = ap.parse_args()

    env = dict(os.environ)
    sdk = Path(env.setdefault("ANDROID_HOME", str(Path.home() / "Android/Sdk")))
    env["PATH"] = os.pathsep.join([str(Path.home() / ".cargo/bin"), str(sdk / "platform-tools"), env["PATH"]])
    if Path("/usr/lib/jvm/java-21-openjdk").is_dir():
        env["JAVA_HOME"] = "/usr/lib/jvm/java-21-openjdk"

    gradle = ["./gradlew", "--console=plain", ":app:assembleRelease" if a.release else ":app:assembleDebug"]
    if a.abis:
        gradle.append(f"-Pabis={a.abis}")
    if a.debug_rust:
        gradle.append("-PrustRelease=false")
    lock = [sys.executable, str(ROOT / "scripts/buildlock.py"), "--who", "build_android", "--"]
    subprocess.run(lock + gradle, cwd=ROOT / "android", env=env, check=True)
    apk = RELEASE_APK if a.release else APK
    print(f"APK: {apk}")

    if a.install:
        adb = shutil.which("adb", path=env["PATH"]) or "adb"
        for serial in a.serial or [None]:
            target = [adb] + (["-s", serial] if serial else [])
            subprocess.run(target + ["install", "-r", str(apk)], check=True)
            subprocess.run(target + ["shell", "am", "start", "-n", "com.osvauld.p2p/.MainActivity"], check=True)


if __name__ == "__main__":
    main()
