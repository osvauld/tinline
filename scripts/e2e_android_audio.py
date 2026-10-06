#!/usr/bin/env python3
"""Real audio through the phone: AudioRecord/AudioTrack, not the core's test tone.

Needs an emulator started with its audio on two private null sinks (never the real speakers):
    pactl load-module module-null-sink sink_name=p2p_emu_out   # emulator speaker -> here
    pactl load-module module-null-sink sink_name=p2p_emu_in    # we play here -> emulator mic
    PULSE_SINK=p2p_emu_out PULSE_SOURCE=p2p_emu_in.monitor \\
        emulator -avd p2p-audio -port 5556 -no-window -allow-host-audio
and the app on it set up with a contact `desk` (scripts/e2e_android.py --fresh --serial ...).

One call, both directions at once, different speech each way:
  desk -> phone speaker: p2p-peer sends clip A; we record p2p_emu_out.monitor
  phone mic -> desk:     we play clip B into p2p_emu_in; p2p-peer records what it heard
Each recording is aligned to its source and scored by scripts/audio_compare.py.

    .venv/bin/python scripts/e2e_android_audio.py --a A.wav --b B.wav [--serial emulator-5556]
"""
import argparse
import os
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ADB = os.path.expanduser("~/Android/Sdk/platform-tools/adb")
PKG = "com.osvauld.p2p"


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--a", required=True, help="speech the desk sends (48 kHz mono s16)")
    ap.add_argument("--b", required=True, help="speech played into the phone's mic")
    ap.add_argument("--serial", default="emulator-5556")
    ap.add_argument("--secs", type=int, default=20)
    ap.add_argument("--min-env", type=float, default=0.7)
    a = ap.parse_args()
    adb = [ADB, "-s", a.serial]
    sinks = subprocess.run(["pactl", "list", "short", "sinks"], capture_output=True, text=True).stdout
    for s in ("p2p_emu_out", "p2p_emu_in"):
        if s not in sinks:
            sys.exit(f"null sink {s} missing; see the docstring")

    def dbg(cmd, **kw):
        args = [*adb, "shell", "am", "broadcast", "-a", f"{PKG}.DEBUG", "-n", f"{PKG}/.DebugReceiver",
                "--es", "cmd", cmd]
        for k, v in kw.items():
            args += ["--es", k, str(v)]
        subprocess.run(args, check=True, capture_output=True)

    def log():
        return subprocess.run([*adb, "logcat", "-d", "-s", "P2PTEST"], capture_output=True, text=True).stdout

    def wait_for(pat, timeout=40):
        end = time.time() + timeout
        while time.time() < end:
            m = re.findall(pat, log())
            if m:
                return m[-1]
            time.sleep(0.3)
        sys.exit(f"timeout waiting for {pat}")

    work = Path(tempfile.mkdtemp(prefix="p2p-audio-"))
    # The desk identity: reuse the newest one e2e_android.py made for this phone.
    desks = sorted(Path("/tmp").glob("p2p-e2e-desk-*"), key=lambda p: p.stat().st_mtime)
    desk = None
    for d in reversed(desks):
        out = subprocess.run([str(ROOT / "target/release/p2p-peer"), "--data", str(d), "contacts"],
                             capture_output=True, text=True).stdout
        if out.startswith("phone"):
            desk = d
            break
    if not desk:
        sys.exit("no desk identity with a 'phone' contact; run e2e_android.py --fresh first")

    dbg("tone", hz=0)  # real mic path, not the core's tone
    subprocess.run([*adb, "logcat", "-c"], check=True)
    phone_heard = work / "phone_heard.wav"
    rec = subprocess.Popen(["parecord", "--device=p2p_emu_out.monitor", "--channels=1", "--rate=48000",
                            "--format=s16le", "--file-format=wav", str(phone_heard)])
    desk_heard = work / "desk_heard.wav"
    peer = subprocess.Popen([str(ROOT / "target/release/p2p-peer"), "--data", str(desk), "call", "phone",
                             "--wav", a.a, "--record", str(desk_heard), "--secs", str(a.secs)],
                            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
    wait_for(r"incoming id=\S+")
    dbg("answer")
    wait_for(r"state id=\S+ state=Active")
    play = subprocess.Popen(["paplay", "--device=p2p_emu_in", a.b])
    out = peer.communicate(timeout=a.secs + 60)[0]
    play.wait(timeout=30)
    time.sleep(1)
    rec.terminate()
    rec.wait()
    print(re.findall(r"RESULT.*", out)[-1] if "RESULT" in out else out[-500:])
    streams = subprocess.run(["pactl", "list", "short", "source-outputs"], capture_output=True, text=True).stdout
    print("mic/speaker init:", [l for l in log().splitlines() if "audio" in l and "unavailable" in l] or "ok")

    cmp = [str(ROOT / ".venv/bin/python"), str(ROOT / "scripts/audio_compare.py"), "--max-lag", "30",
           "--min-env", str(a.min_env)]
    r1 = subprocess.run([*cmp, a.a, str(phone_heard)], capture_output=True, text=True)
    print("desk -> phone speaker:", r1.stdout.strip())
    r2 = subprocess.run([*cmp, a.b, str(desk_heard)], capture_output=True, text=True)
    print("phone mic -> desk:    ", r2.stdout.strip())
    print(f"recordings in {work}")
    sys.exit(r1.returncode or r2.returncode)


if __name__ == "__main__":
    main()
