#!/usr/bin/env python3
"""Android <-> desktop e2e: phone (emulator, debug build installed) calls and is called by p2p-peer.

Usage: scripts/e2e_android.py [--peer target/release/p2p-peer] [--data DIR]
Needs: debug APK installed, emulator attached, identity created on the phone (`cmd create`).
"""
import argparse, os, re, subprocess, sys, time

ADB = os.path.expanduser("~/Android/Sdk/platform-tools/adb")


def sh(*a, **k):
    return subprocess.run(a, check=True, capture_output=True, text=True, **k).stdout


def dbg(cmd, **extras):
    args = [ADB, "shell", "am", "broadcast", "-a", "com.osvauld.p2p.DEBUG",
            "-n", "com.osvauld.p2p/.DebugReceiver", "--es", "cmd", cmd]
    for k, v in extras.items():
        args += ["--es", k, str(v)]
    sh(*args)


def logs():
    return subprocess.run([ADB, "logcat", "-d", "-s", "P2PTEST"], capture_output=True, text=True).stdout


def wait_for(pat, timeout=30):
    end = time.time() + timeout
    while time.time() < end:
        m = re.findall(pat, logs())
        if m:
            return m[-1]
        time.sleep(0.5)
    raise SystemExit(f"timeout waiting for {pat}")


def freq(text, key):
    return float(re.findall(rf"{key}=([\d.]+)", text)[-1])


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--peer", default="target/release/p2p-peer")
    ap.add_argument("--data", default="/tmp/e2e-desk")
    a = ap.parse_args()
    peer = [a.peer, "--data", a.data]
    dbg("tone", hz=440)
    dbg("ticket")
    ticket = wait_for(r"ticket=(osvc1\S+)")
    if not os.path.isdir(a.data):
        sh(*peer, "init", "desk")
    sh(*peer, "add", ticket)

    # desk -> phone
    sh(ADB, "logcat", "-c")
    p = subprocess.Popen(peer + ["call", "phone", "--tone", "660", "--secs", "12"], stdout=subprocess.PIPE, text=True, stderr=subprocess.STDOUT)
    wait_for(r"incoming id=\S+")
    dbg("answer")
    out = p.communicate()[0]
    res = re.findall(r"RESULT.*", out)[-1]
    assert abs(freq(res, "freq") - 440) < 10, res
    assert abs(freq(wait_for(r"(stats secs=8 .*)"), "freq") - 660) < 10
    print("desk->phone ok:", res)

    # phone -> desk
    sh(ADB, "logcat", "-c")
    p = subprocess.Popen(peer + ["listen", "--tone", "660", "--once", "--secs", "10"], stdout=subprocess.PIPE, text=True, stderr=subprocess.STDOUT)
    time.sleep(4)
    dbg("call", who="desk")
    out = p.communicate()[0]
    res = re.findall(r"RESULT.*", out)[-1]
    assert abs(freq(res, "freq") - 440) < 10, res
    assert abs(freq(wait_for(r"(stats secs=8 .*)"), "freq") - 660) < 10
    print("phone->desk ok:", res)


if __name__ == "__main__":
    sys.exit(main())
