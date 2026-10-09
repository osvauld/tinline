#!/usr/bin/env python3
"""Phone to phone over iroh: two emulators, each with the debug app, no desktop in the path.

    scripts/e2e_phone_phone.py [--a emulator-5554] [--b emulator-5556]

Wipes both apps, creates identities `pa` and `pb`, opens each app once (so its foreground
service may start), has `pb` redeem `pa`'s ticket, then calls a -> b and b -> a with test tones
440 (a) and 660 (b). Pass: each phone's mid-call stats show the other's tone.
"""
import argparse
import os
import re
import subprocess
import sys
import time

ADB = os.path.expanduser("~/Android/Sdk/platform-tools/adb")
PKG = "com.osvauld.tinline"


class Phone:
    def __init__(self, serial, name, tone):
        self.adb = [ADB, "-s", serial]
        self.name, self.tone = name, tone

    def sh(self, *a):
        return subprocess.run([*self.adb, *a], check=True, capture_output=True, text=True).stdout

    def dbg(self, cmd, **kw):
        args = ["shell", "am", "broadcast", "-a", f"{PKG}.DEBUG", "-n", f"{PKG}/com.osvauld.p2p.DebugReceiver", "--es", "cmd", cmd]
        for k, v in kw.items():
            args += ["--es", k, str(v)]
        self.sh(*args)

    def log(self):
        return self.sh("logcat", "-d", "-s", "P2PTEST")

    def wait(self, pat, timeout=40):
        end = time.time() + timeout
        while time.time() < end:
            m = re.findall(pat, self.log())
            if m:
                return m[-1]
            time.sleep(0.4)
        raise SystemExit(f"{self.name}: timeout waiting for {pat}")

    def fresh(self):
        self.sh("shell", "pm", "clear", PKG)
        for p in ("android.permission.RECORD_AUDIO", "android.permission.POST_NOTIFICATIONS"):
            self.sh("shell", "pm", "grant", PKG, p)
        # Android 14+ only; Android 13 and below do not know this app-op.
        subprocess.run([*self.adb, "shell", "appops", "set", PKG, "USE_FULL_SCREEN_INTENT", "allow"],
                       capture_output=True)
        self.sh("logcat", "-c")
        self.dbg("create", name=self.name)
        self.wait(r"created did=\S+")
        self.sh("shell", "am", "start", "-n", f"{PKG}/com.osvauld.p2p.MainActivity")
        time.sleep(4)
        self.sh("shell", "input", "keyevent", "KEYCODE_HOME")
        self.dbg("tone", hz=self.tone)


def call(caller, callee, ok):
    for p in (caller, callee):
        p.sh("logcat", "-c")
    caller.dbg("call", who=callee.name)
    callee.wait(r"incoming id=\S+")
    callee.dbg("answer")
    caller.wait(r"state id=\S+ state=Active")
    time.sleep(7)
    for me, other in ((caller, callee), (callee, caller)):
        stats = re.findall(r"stats secs=\d+ (direct=\S+).*freq=([\d.]+)", me.log())
        got = float(stats[-1][1]) if stats else None
        good = got is not None and abs(got - other.tone) < 10
        ok.append(good)
        print(f"{'PASS' if good else 'FAIL'} {caller.name}->{callee.name}: {me.name} heard {got} Hz "
              f"(want {other.tone}) {stats[-1][0] if stats else ''}")
    caller.dbg("hangup")
    callee.wait(r"state id=\S+ state=Ended", timeout=15)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--a", default="emulator-5554")
    ap.add_argument("--b", default="emulator-5556")
    a = ap.parse_args()
    pa, pb = Phone(a.a, "pa", 440), Phone(a.b, "pb", 660)
    pa.fresh()
    pb.fresh()
    pa.sh("logcat", "-c")
    pa.dbg("ticket")
    ticket = pa.wait(r"ticket=(\S+)")
    pb.sh("logcat", "-c")
    pb.dbg("add", ticket=ticket)
    print("pb:", pb.wait(r"(added name=\S+|error=.*)"))
    pa.wait(r"added name=pb", timeout=20)
    ok = []
    call(pa, pb, ok)
    time.sleep(2)
    call(pb, pa, ok)
    print("ALL PASS" if all(ok) else "FAILED")
    sys.exit(0 if all(ok) else 1)


if __name__ == "__main__":
    main()
