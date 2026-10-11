#!/usr/bin/env python3
"""Android <-> desktop video call: the emulator's (virtual scene) camera one way, p2p-peer's
synthetic `video::Pattern` the other.

    scripts/e2e_android_video.py [--serial emulator-5554] [--desk DIR] [--secs 25] [--shot PATH]

Needs the debug APK installed on the emulator with an identity already set up (it is never wiped
here); grants CAMERA. The desk side is p2p-peer: a new identity in a temp dir that adds the phone
by ticket, or `--desk DIR` to reuse one that has the phone as a contact (named `phone`).

1. phone calls desk, desk answers with its pattern camera on; the phone turns its camera on.
   Checks: both sides negotiated video, both sent frames, both decoded them with no decode errors,
   the phone rendered frames. Saves a screenshot of the call screen (`--shot`) and the desk's last
   decoded frame of the phone's camera (next to it, `-desk.png`).
2. desk calls phone with video refused (`--no-video`): the phone sees negotiated=false and the
   call carries audio.
"""
import argparse
import os
import re
import struct
import subprocess
import sys
import tempfile
import time
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ADB = [os.path.expanduser("~/Android/Sdk/platform-tools/adb")]
PKG = "com.osvauld.tinline"


def sh(*a, **k):
    return subprocess.run(a, check=True, capture_output=True, text=True, **k).stdout


def dbg(cmd, **extras):
    args = [*ADB, "shell", "am", "broadcast", "-a", f"{PKG}.DEBUG", "-n", f"{PKG}/com.osvauld.p2p.DebugReceiver",
            "--es", "cmd", cmd]
    for k, v in extras.items():
        args += ["--es", k, str(v)]
    sh(*args)


def logs():
    return subprocess.run([*ADB, "logcat", "-d", "-s", "P2PTEST"], capture_output=True, text=True).stdout


def wait_for(pat, timeout=40):
    end = time.time() + timeout
    while time.time() < end:
        m = re.findall(pat, logs())
        if m:
            return m[-1]
        time.sleep(0.5)
    raise SystemExit(f"timeout waiting for {pat}")


def kv(line):
    return dict(re.findall(r"(\w+)=(\S+)", line))


def ppm_to_png(src, dst):
    data = Path(src).read_bytes()
    m = re.match(rb"P6\s+(\d+)\s+(\d+)\s+255\s", data)
    w, h = int(m[1]), int(m[2])
    px = data[m.end():]
    raw = b"".join(b"\0" + px[y * w * 3:(y + 1) * w * 3] for y in range(h))

    def chunk(t, d):
        return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)

    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
    Path(dst).write_bytes(png + chunk(b"IDAT", zlib.compress(raw, 6)) + chunk(b"IEND", b""))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--peer", default=str(ROOT / "target/release/p2p-peer"))
    ap.add_argument("--serial", default="emulator-5554")
    ap.add_argument("--desk", help="existing p2p-peer data dir with the phone as contact `phone`")
    ap.add_argument("--secs", type=int, default=25)
    ap.add_argument("--pattern", default="640x480")
    ap.add_argument("--shot", default=str(ROOT / "docs/video-screens/android-video-call.png"))
    a = ap.parse_args()
    ADB.extend(["-s", a.serial])
    if not a.serial.startswith("emulator-"):
        raise SystemExit("emulator only")

    sh(*ADB, "shell", "pm", "grant", PKG, "android.permission.CAMERA")
    # Foreground app: the call screen may only be started from the foreground.
    sh(*ADB, "shell", "input", "keyevent", "KEYCODE_WAKEUP")
    sh(*ADB, "shell", "am", "start", "-n", f"{PKG}/com.osvauld.p2p.MainActivity")
    time.sleep(3)
    dbg("tone", hz=440)
    dbg("video_enabled", on=1)

    desk = a.desk or tempfile.mkdtemp(prefix="p2p-e2e-video-desk-")
    peer = [a.peer, "--data", desk]
    if not a.desk:
        sh(*ADB, "logcat", "-c")
        dbg("ticket")
        ticket = wait_for(r"ticket=(\S+)")
        sh(*peer, "init", "desk")
        print(sh(*peer, "add", ticket).strip())
    contacts = sh(*peer, "contacts")
    phone = contacts.split("\t")[0] if contacts else sys.exit("desk has no contacts")
    desk_did = re.search(r"did (\S+)", sh(*peer, "whoami"))[1]

    # 1. phone -> desk, video both ways.
    sh(*ADB, "logcat", "-c")
    snap = Path(tempfile.mkdtemp(prefix="p2p-e2e-video-")) / "desk.ppm"
    p = subprocess.Popen(peer + ["listen", "--once", "--tone", "660", "--secs", str(a.secs), "--video-pattern", a.pattern,
                                 "--video-snap", str(snap)], stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    time.sleep(4)
    dbg("call", who=desk_did)
    wait_for(r"state id=\S+ state=Active")
    st = kv(wait_for(r"(video_state .*negotiated=true.*)"))
    print("phone video state:", st)
    dbg("camera", on=1)
    wait_for(r"camera started")
    time.sleep(min(10, a.secs - 8))
    Path(a.shot).parent.mkdir(parents=True, exist_ok=True)
    Path(a.shot).write_bytes(subprocess.run([*ADB, "exec-out", "screencap", "-p"], capture_output=True, check=True).stdout)
    print("screenshot:", a.shot)
    dbg("video_stats")
    time.sleep(1)
    series = re.findall(r"video_stats .*", logs())
    ph = kv(series[-1])
    print("phone:", ph)
    print("phone send size@kbps per second:", " ".join(f"{kv(l).get('send')}@{kv(l).get('kbps')}" for l in series))
    out = p.communicate(timeout=a.secs + 30)[0]
    vres = re.findall(r"VRESULT.*", out)
    res = re.findall(r"RESULT.*", out)
    if not vres:
        raise SystemExit(f"no VRESULT from the desk:\n{out[-2000:]}")
    dk = kv(vres[-1])
    print("desk:", vres[-1])
    print("desk send size@kbps / decoded per second:",
          " ".join(f"{kv(l)['send']}@{kv(l)['kbps']}/{kv(l)['decoded']}" for l in re.findall(r"VIDEO .*", out)))
    print("desk audio:", res[-1] if res else "-")
    if snap.exists():
        ppm_to_png(snap, Path(a.shot).with_name(Path(a.shot).stem + "-desk.png"))
        print("desk's view of the phone camera:", Path(a.shot).with_name(Path(a.shot).stem + "-desk.png"))
    ok = True
    for side, s, keys in (("phone", ph, ("sent", "decoded", "rendered", "pushed")), ("desk", dk, ("sent", "decoded", "pulled"))):
        for k in keys:
            if int(s.get(k, "0")) <= 0:
                print(f"FAIL {side} {k}={s.get(k)}")
                ok = False
        if int(s.get("errors", "0")) != 0:
            print(f"FAIL {side} decode errors={s['errors']}")
            ok = False
    if ph.get("remote") != "true" or dk.get("remote") != "true":
        print("FAIL remote camera state", ph.get("remote"), dk.get("remote"))
        ok = False

    # 2. desk -> phone, desk refuses video: audio only.
    time.sleep(3)
    sh(*ADB, "logcat", "-c")
    p = subprocess.Popen(peer + ["call", phone, "--tone", "660", "--secs", "10", "--no-video"],
                         stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    wait_for(r"incoming id=\S+")
    dbg("answer")
    wait_for(r"state id=\S+ state=Active")
    # The caller offered no video, so there is no video_state event: ask.
    dbg("video_stats")
    st = kv(wait_for(r"(video_stats .*)"))
    out = p.communicate(timeout=60)[0]
    res = re.findall(r"RESULT.*", out)
    print("audio-only: phone video state", st, "| desk", res[-1] if res else out[-500:])
    if st.get("negotiated") != "false" or not res or abs(float(kv(res[-1]).get("freq", 0)) - 440) > 10:
        print("FAIL audio-only fallback")
        ok = False
    print("OK" if ok else "FAILED")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
