#!/usr/bin/env python3
"""Video call between two p2p-desktop instances on this machine, both sending the synthetic
camera pattern (TINLINE_FAKE_CAMERA=pattern): alice calls bob, bob answers, both turn the camera
on; checks that each side decoded and rendered the other's frames, and saves a screenshot of
both windows (side by side). Runs inside a headless sway (nothing appears on your screen) with audio routed to
a null sink.

    scripts/desktop_video.py [--secs 20] [--shots docs/video-screens]

Needs: cargo build -p desktop --features test-hooks -p peer (debug or --release; TARGET =
cargo target dir, PROFILE = debug|release). Env WORK = scratch dir.
"""
import argparse
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TARGET = os.environ.get("TARGET", os.environ.get("CARGO_TARGET_DIR", os.path.join(ROOT, "target")))
PROFILE = os.environ.get("PROFILE", "debug")
APP, PEER = f"{TARGET}/{PROFILE}/p2p-desktop", f"{TARGET}/{PROFILE}/p2p-peer"


def run(*a, **kw):
    return subprocess.run(a, capture_output=True, text=True, **kw)


def ok(*a, **kw):
    r = run(*a, **kw)
    if r.returncode:
        sys.exit(f"{a} failed: {r.stderr}{r.stdout}")
    return r.stdout.strip()


def null_sink():
    if "p2p_test" not in run("pactl", "list", "short", "sinks").stdout:
        ok("pactl", "load-module", "module-null-sink", "sink_name=p2p_test")
    return {"PIPEWIRE_NODE": "p2p_test", "PIPEWIRE_ALSA": "{ stream.capture.sink=true }"}


def drop_sink():
    for line in run("pactl", "list", "short", "modules").stdout.splitlines():
        if "sink_name=p2p_test" in line:
            run("pactl", "unload-module", line.split()[0])


def headless_sway(work):
    """A private compositor; returns (process, WAYLAND_DISPLAY)."""
    rt = os.path.join(work, "xdg")
    os.makedirs(rt, mode=0o700, exist_ok=True)
    cfg = os.path.join(work, "sway.cfg")
    with open(cfg, "w") as f:
        f.write("output HEADLESS-1 resolution 2400x800\ndefault_border none\n")
    env = dict(os.environ, WLR_BACKENDS="headless", WLR_LIBINPUT_NO_DEVICES="1", XDG_RUNTIME_DIR=rt)
    for k in ("WAYLAND_DISPLAY", "SWAYSOCK", "DISPLAY"):
        env.pop(k, None)
    p = subprocess.Popen(["sway", "-c", cfg], env=env, stdout=subprocess.DEVNULL, stderr=open(f"{work}/sway.err", "w"))
    for _ in range(50):
        socks = [s for s in os.listdir(rt) if s.startswith("wayland-") and not s.endswith(".lock")]
        if socks:
            return p, rt, socks[0]
        time.sleep(0.1)
    p.kill()
    sys.exit(f"headless sway did not start: {open(f'{work}/sway.err').read()[-500:]}")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--secs", type=int, default=20, help="how long the apps run")
    ap.add_argument("--shots", default=None, help="directory for the window screenshots")
    a = ap.parse_args()
    if b"p2p-desktop-test-hooks-enabled" not in open(APP, "rb").read():
        sys.exit(f"{APP} was built without test hooks")
    work = os.environ.get("WORK") or tempfile.mkdtemp(prefix="tl-video-")
    os.makedirs(work, exist_ok=True)
    A, B = f"{work}/alice", f"{work}/bob"
    for d in (A, B):
        shutil.rmtree(d, ignore_errors=True)
    base = dict(os.environ, P2P_PASSPHRASE="test-passphrase", P2P_KEYRING="none")
    ok(PEER, "--data", A, "init", "alice", env=base)
    ok(PEER, "--data", B, "init", "bob", env=base)
    ticket = ok(PEER, "--data", A, "ticket", env=base)
    listener = subprocess.Popen([PEER, "--data", A, "listen", "--once", "--secs", "5"], env=base,
                                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(3)
    print(ok(PEER, "--data", B, "add", ticket, env=base))
    listener.kill()
    listener.wait()

    sway, rt, disp = headless_sway(work)
    env = dict(base, **null_sink(), XDG_RUNTIME_DIR=rt, WAYLAND_DISPLAY=disp, TINLINE_FAKE_CAMERA="pattern",
               P2P_AUTO_CAMERA="1")
    for k in ("SWAYSOCK", "DISPLAY"):
        env.pop(k, None)
    shots = os.path.abspath(a.shots) if a.shots else work
    os.makedirs(shots, exist_ok=True)
    procs = {}
    try:
        for name, d, extra in (("bob", B, {"P2P_AUTO_ANSWER": "1"}), ("alice", A, {"P2P_AUTO_CALL": "bob"})):
            procs[name] = subprocess.Popen([APP, "--data", d], env=dict(env, **extra), stderr=open(f"{work}/{name}.err", "w"))
            time.sleep(2)
        time.sleep(a.secs)
        # Both windows tile side by side on the one output.
        shot = f"{shots}/desktop-call.png"
        r = run("grim", shot, env=env)
        print(f"screenshot {shot}" if r.returncode == 0 else f"grim failed: {r.stderr.strip()}")
    finally:
        for p in procs.values():
            if p.poll() is None:
                p.terminate()
                p.wait(timeout=10)
        sway.terminate()
        sway.wait()
        drop_sink()

    failed = 0
    for name in procs:
        err = open(f"{work}/{name}.err").read()
        vs = [l for l in err.splitlines() if l.startswith("VSTATS")]
        last = vs[-1] if vs else ""
        num = lambda k: int(m.group(1)) if (m := re.search(rf"\b{k}=(\d+)", last)) else 0
        rend = [l for l in err.splitlines() if l.startswith(("VRENDER", "CAMERA", "VIDEO", "STATE"))]
        print(f"--- {name}\n" + "\n".join(rend[-6:]) + f"\n{last}")
        good = num("sent") >= 60 and num("decoded") >= 60 and num("errors") == 0
        print(f"{'PASS' if good else 'FAIL'} {name}: sent {num('sent')} decoded {num('decoded')} errors {num('errors')}")
        failed += not good
    print(f"logs and data in {work}")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
