#!/usr/bin/env python3
"""Screen-record the Play foreground-service demo on a USB phone: the always-on line ringing a
locked phone (specialUse) and a call carrying on with the app in the background (microphone).

    scripts/record_play_demo.py [--serial ZD22257P6G] [--out ~/tinline-builds/play-fgs-demo.mp4]

The script drives the phone with adb and stops only where a person must act: start the call from
the other device, answer, unlock, hang up. Touches are NOT shown (that would reveal the PIN).
screenrecord stops at 3 minutes; the whole run takes about one and a half.
"""
import argparse
import subprocess
import sys
import time
from pathlib import Path

PKG = "com.osvauld.tinline"


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--serial", default="ZD22257P6G")
    ap.add_argument("--out", default=str(Path.home() / "tinline-builds/play-fgs-demo.mp4"))
    a = ap.parse_args()
    adb = ["timeout", "30", "adb", "-s", a.serial]

    def sh(*cmd):
        return subprocess.run(adb + ["shell", *cmd], capture_output=True, text=True).stdout.strip()

    def step(msg, secs=0.0):
        print(f"  {msg}")
        time.sleep(secs)

    def wait_enter(msg):
        input(f"\n>>> {msg}\n    press Enter when done ")

    if sh("echo", "ok") != "ok":
        sys.exit(f"phone {a.serial} not reachable over adb")
    if PKG not in sh("pm", "list", "packages", PKG):
        sys.exit(f"{PKG} is not installed on {a.serial}")

    print("Before starting: the phone is UNLOCKED and on its home screen, and the other device")
    print("(desktop app) has the phone as a contact and is ready to call it.")
    wait_enter("Ready?")

    remote = "/sdcard/tinline-fgs-demo.mp4"
    sh("rm", "-f", remote)
    rec = subprocess.Popen(["adb", "-s", a.serial, "shell", "screenrecord", "--time-limit", "180",
                            "--bit-rate", "6000000", remote])
    time.sleep(2)
    try:
        step("1. open Tinline", 0)
        sh("am", "start", "-n", f"{PKG}/com.osvauld.p2p.MainActivity")
        time.sleep(4)
        step("2. show the always-on notification", 0)
        sh("cmd", "statusbar", "expand-notifications")
        time.sleep(5)
        sh("cmd", "statusbar", "collapse")
        step("3. home, then lock the phone", 1)
        sh("input", "keyevent", "KEYCODE_HOME")
        time.sleep(2)
        sh("input", "keyevent", "KEYCODE_SLEEP")
        time.sleep(2)
        sh("input", "keyevent", "KEYCODE_WAKEUP")  # the lock screen, so the video shows it
        time.sleep(2)
        wait_enter("4. CALL the phone from the desktop now. When it rings full-screen, ANSWER on the\n"
                   "    phone and say a few words")
        wait_enter("5. UNLOCK the phone (PIN/fingerprint) so the call screen shows")
        step("6. leave the app: the call keeps going in the background", 0)
        sh("input", "keyevent", "KEYCODE_HOME")
        time.sleep(3)
        sh("cmd", "statusbar", "expand-notifications")
        print("  keep talking for ~10 s: the ongoing-call notification is on screen")
        time.sleep(10)
        sh("cmd", "statusbar", "collapse")
        time.sleep(2)
        wait_enter("7. HANG UP (from the desktop or the phone)")
        time.sleep(3)
    finally:
        sh("pkill", "-INT", "screenrecord")
        rec.wait(timeout=20)
        time.sleep(2)
        out = Path(a.out).expanduser()
        out.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run(["timeout", "120", "adb", "-s", a.serial, "pull", remote, str(out)], check=False)
        sh("rm", "-f", remote)
        print(f"\nvideo: {out}" if out.exists() else "\nno video pulled")


if __name__ == "__main__":
    main()
