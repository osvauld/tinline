#!/usr/bin/env python3
"""Build the Android app (Gradle runs cargo-ndk + uniffi-bindgen), optionally install and launch.

    scripts/build_android.py [--install] [--serial emulator-5554 ...] [--abis x86_64] [--debug-rust] [--release]
    scripts/build_android.py --bundle     # signed release AAB for Play, copied to ~/tinline-builds

Goes through scripts/buildlock.py itself, so it waits its turn behind other heavy builds.
--serial may repeat to install on several devices; without it, adb picks the only device.
"""
import argparse
import os
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
APK = ROOT / "android/app/build/outputs/apk/debug/app-debug.apk"
RELEASE_APK = ROOT / "android/app/build/outputs/apk/release/app-release.apk"
AAB = ROOT / "android/app/build/outputs/bundle/release/app-release.aab"
OUT = Path.home() / "tinline-builds"


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--install", action="store_true")
    ap.add_argument("--serial", action="append", default=[])
    ap.add_argument("--abis", help="comma list, e.g. x86_64 or x86_64,arm64-v8a")
    ap.add_argument("--release", action="store_true",
                    help="minified release APK (signed with the debug key unless RELEASE_* gradle properties are set; "
                         "has no DebugReceiver, so the e2e scripts cannot drive it)")
    ap.add_argument("--bundle", action="store_true",
                    help="release AAB for Play; needs the RELEASE_* gradle properties (refuses the debug key); "
                         "versionCode = commit count, so every upload from a later commit is higher")
    ap.add_argument("--debug-rust", action="store_true", help="cargo debug profile (faster build, slow audio)")
    a = ap.parse_args()

    env = dict(os.environ)
    sdk = Path(env.setdefault("ANDROID_HOME", str(Path.home() / "Android/Sdk")))
    env["PATH"] = os.pathsep.join([str(Path.home() / ".cargo/bin"), str(sdk / "platform-tools"), env["PATH"]])
    if Path("/usr/lib/jvm/java-21-openjdk").is_dir():
        env["JAVA_HOME"] = "/usr/lib/jvm/java-21-openjdk"

    task = ":app:bundleRelease" if a.bundle else ":app:assembleRelease" if a.release else ":app:assembleDebug"
    gradle = ["./gradlew", "--console=plain", task]
    if a.bundle:
        props = Path.home() / ".gradle/gradle.properties"
        if "RELEASE_STORE_FILE=" not in (props.read_text() if props.exists() else ""):
            sys.exit("--bundle needs RELEASE_* signing properties in ~/.gradle/gradle.properties")
        code = subprocess.run(["git", "rev-list", "--count", "HEAD"], cwd=ROOT, capture_output=True, text=True,
                              check=True).stdout.strip()
        gradle.append(f"-PversionCode={code}")
    if a.abis:
        gradle.append(f"-Pabis={a.abis}")
    if a.debug_rust:
        gradle.append("-PrustRelease=false")
    lock = [sys.executable, str(ROOT / "scripts/buildlock.py"), "--who", "build_android", "--"]
    # Static libaom (video encoder) per ABI from the pinned, verified source; gradle points cargo at
    # these install dirs. Idempotent: an up-to-date build only re-checks the tarball hash.
    env.setdefault("ANDROID_NDK_HOME", str(sdk / "ndk/28.2.13676358"))
    triples = {"arm64-v8a": "aarch64-linux-android", "x86_64": "x86_64-linux-android"}
    for abi in (a.abis or "x86_64,arm64-v8a").split(","):
        subprocess.run(lock + [sys.executable, str(ROOT / "scripts/build_libaom.py"), "--target", triples[abi.strip()]],
                       cwd=ROOT, env=env, check=True, stdout=subprocess.DEVNULL)
    subprocess.run(lock + gradle, cwd=ROOT / "android", env=env, check=True)
    if a.bundle:
        OUT.mkdir(exist_ok=True)
        name = subprocess.run(["git", "describe", "--always", "--dirty"], cwd=ROOT, capture_output=True, text=True,
                              check=True).stdout.strip()
        dest = OUT / f"tinline-{code}-{name}.aab"
        shutil.copy2(AAB, dest)
        print(f"AAB: {dest}  (versionCode {code})")
        return
    apk = RELEASE_APK if a.release else APK
    if a.release:
        with (ROOT / "Cargo.toml").open("rb") as manifest:
            version = tomllib.load(manifest)["workspace"]["package"]["version"]
        OUT.mkdir(exist_ok=True)
        dest = OUT / f"tinline-{version}-android.apk"
        shutil.copy2(apk, dest)
        apk = dest
    print(f"APK: {apk}")

    if a.install:
        adb = shutil.which("adb", path=env["PATH"]) or "adb"
        for serial in a.serial or [None]:
            target = [adb] + (["-s", serial] if serial else [])
            subprocess.run(target + ["install", "-r", str(apk)], check=True)
            subprocess.run(target + ["shell", "am", "start", "-n", "com.osvauld.tinline/com.osvauld.p2p.MainActivity"], check=True)


if __name__ == "__main__":
    main()
