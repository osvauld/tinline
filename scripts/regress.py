#!/usr/bin/env python3
"""Everything, in order: unit + integration tests, desktop e2e, desktop app e2e, Android e2e on
every attached emulator, and phone-to-phone if two are attached.

    scripts/regress.py [--skip-build] [--no-android]

Builds go through scripts/buildlock.py. Prints one PASS/FAIL line per step and a summary.
"""
import argparse
import os
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PY = sys.executable
LOCK = [PY, str(ROOT / "scripts/buildlock.py"), "--who", "regress", "--"]
ADB = os.path.expanduser("~/Android/Sdk/platform-tools/adb")


def step(name, cmd, timeout=1800):
    t = time.time()
    r = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True, timeout=timeout)
    ok = r.returncode == 0
    print(f"{'PASS' if ok else 'FAIL'} {name} ({time.time() - t:.0f}s)", flush=True)
    if not ok:
        print("    " + "\n    ".join((r.stdout + r.stderr).strip().splitlines()[-15:]), flush=True)
    return ok


def emulators():
    out = subprocess.run([ADB, "devices"], capture_output=True, text=True).stdout
    return [l.split()[0] for l in out.splitlines()[1:] if l.strip().endswith("device") and l.startswith("emulator-")]


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--skip-build", action="store_true")
    ap.add_argument("--no-android", action="store_true")
    a = ap.parse_args()
    results = []
    if not a.skip_build:
        results.append(step("build desktop (test-hooks) + peer", LOCK + ["cargo", "build", "--release", "-q", "-p", "desktop", "--features", "desktop/test-hooks", "-p", "peer"]))
    results.append(step("unit tests (identity, proto, audio)", LOCK + ["cargo", "test", "--release", "-q", "-p", "identity", "-p", "proto", "-p", "audio"]))
    results.append(step("core call-state tests", LOCK + ["cargo", "test", "--release", "-q", "-p", "p2pcore", "--test", "calls"]))
    results.append(step("e2e desktop peers (direct + relay)", [PY, "scripts/e2e_desktop.py"]))
    results.append(step("desktop app tone", [PY, "scripts/desktop_e2e.py", "tone"]))
    results.append(step("desktop app real mic", [PY, "scripts/desktop_e2e.py", "mic"]))
    emus = [] if a.no_android else emulators()
    if emus:
        if not a.skip_build:
            results.append(step("build android", [PY, "scripts/build_android.py", "--abis", "x86_64"]))
        apk = ROOT / "android/app/build/outputs/apk/debug/app-debug.apk"
        for e in emus:
            results.append(step(f"install {e}", [ADB, "-s", e, "install", "-r", str(apk)]))
            results.append(step(f"e2e android {e}", [PY, "scripts/e2e_android.py", "--fresh", "--serial", e]))
        if len(emus) >= 2:
            results.append(step("e2e phone<->phone", [PY, "scripts/e2e_phone_phone.py", "--a", emus[0], "--b", emus[1]]))
    print(f"{sum(results)}/{len(results)} passed")
    sys.exit(0 if all(results) else 1)


if __name__ == "__main__":
    main()
