#!/usr/bin/env python3
"""Android <-> desktop e2e: phone (emulator, debug build installed) calls and is called by p2p-peer.

Usage: scripts/e2e_android.py [--fresh] [--serial emulator-5554]
Needs the debug APK installed. --fresh wipes the app, grants its permissions and creates a new
phone identity; otherwise the phone's existing identity is used. The desktop side is always a
fresh p2p-peer identity in a temp dir.
"""
import argparse, os, re, subprocess, sys, tempfile, time

ADB = [os.path.expanduser("~/Android/Sdk/platform-tools/adb")]
PKG = "com.osvauld.p2p"


def sh(*a, **k):
    return subprocess.run(a, check=True, capture_output=True, text=True, **k).stdout


def dbg(cmd, **extras):
    args = [*ADB, "shell", "am", "broadcast", "-a", "com.osvauld.p2p.DEBUG",
            "-n", "com.osvauld.p2p/.DebugReceiver", "--es", "cmd", cmd]
    for k, v in extras.items():
        args += ["--es", k, str(v)]
    sh(*args)


def logs():
    return subprocess.run([*ADB, "logcat", "-d", "-s", "P2PTEST"], capture_output=True, text=True).stdout


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
    ap.add_argument("--serial", default="emulator-5554")
    ap.add_argument("--fresh", action="store_true")
    a = ap.parse_args()
    ADB.extend(["-s", a.serial])
    data = tempfile.mkdtemp(prefix="p2p-e2e-desk-")
    peer = [a.peer, "--data", data]
    if a.fresh:
        sh(*ADB, "shell", "pm", "clear", PKG)
        for perm in ["android.permission.RECORD_AUDIO", "android.permission.POST_NOTIFICATIONS"]:
            sh(*ADB, "shell", "pm", "grant", PKG, perm)
        sh(*ADB, "shell", "appops", "set", PKG, "USE_FULL_SCREEN_INTENT", "allow")
        sh(*ADB, "logcat", "-c")
        dbg("create", name="phone")
        wait_for(r"created did=\S+")
    # Open the app like a user would: a foreground service may only start from the foreground
    # (or with the battery-optimisation exemption the setup card asks for). A debug broadcast
    # alone leaves the node in a cached process that Android freezes, unreachable.
    sh(*ADB, "shell", "am", "start", "-n", f"{PKG}/.MainActivity")
    time.sleep(4)
    sh(*ADB, "shell", "input", "keyevent", "KEYCODE_HOME")
    if "isForeground=true" not in sh(*ADB, "shell", "dumpsys", "activity", "services", PKG):
        raise SystemExit("CoreService is not a foreground service")
    dbg("tone", hz=440)
    sh(*ADB, "logcat", "-c")
    dbg("ticket")
    ticket = wait_for(r"ticket=(\S+)")
    sh(*peer, "init", "desk")
    print(sh(*peer, "add", ticket).strip())

    # desk -> phone
    sh(*ADB, "logcat", "-c")
    p = subprocess.Popen(peer + ["call", "phone", "--tone", "660", "--secs", "12"], stdout=subprocess.PIPE, text=True, stderr=subprocess.STDOUT)
    wait_for(r"incoming id=\S+")
    dbg("answer")
    out = p.communicate()[0]
    res = re.findall(r"RESULT.*", out)[-1]
    assert abs(freq(res, "freq") - 440) < 10, res
    assert abs(freq(wait_for(r"(stats secs=8 .*)"), "freq") - 660) < 10
    print("desk->phone ok:", res)

    # phone -> desk
    sh(*ADB, "logcat", "-c")
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
