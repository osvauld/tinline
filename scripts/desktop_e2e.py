#!/usr/bin/env python3
"""End-to-end checks for p2p-desktop against p2p-peer, with audio routed through a null sink.

  scripts/desktop_e2e.py tone   # app sends 440 Hz test tone, peer 660 Hz; both must hear the other
  scripts/desktop_e2e.py mic    # a tone played into the null source must reach the peer via the real mic path

Env: TARGET (cargo target dir with release binaries), WORK (scratch dir).
"""
import math, os, re, struct, subprocess, sys, time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TARGET = os.environ.get("TARGET", os.path.join(ROOT, "target")) + "/release"
WORK = os.environ.get("WORK", "/tmp/claude-1000/e2e")
APP, PEER = f"{TARGET}/p2p-desktop", f"{TARGET}/p2p-peer"
A, B = f"{WORK}/A", f"{WORK}/B"


def run(*a, **kw):
    return subprocess.run(a, capture_output=True, text=True, **kw)


def sh_ok(*a):
    r = run(*a)
    if r.returncode:
        sys.exit(f"{a} failed: {r.stderr}{r.stdout}")
    return r.stdout.strip()


def null_sink():
    if "p2p_test" not in run("pactl", "list", "short", "sinks").stdout:
        sh_ok("pactl", "load-module", "module-null-sink", "sink_name=p2p_test")
    # pipewire-alsa ignores the PULSE_* variables; this routes both streams to the null sink
    env = dict(os.environ, PIPEWIRE_NODE="p2p_test", PIPEWIRE_ALSA="{ stream.capture.sink=true }")
    return env


def setup(env):
    os.makedirs(WORK, exist_ok=True)
    if not os.path.exists(f"{A}/.done"):
        for d in (A, B):
            run("rm", "-rf", d)
        sh_ok(PEER, "--data", A, "init", "alice")
        sh_ok(PEER, "--data", B, "init", "bob")
        open(f"{A}/.done", "w").close()
    return env


def pair(env):
    """Both sides must be online for add; the app runs while the peer redeems its ticket."""
    if run(PEER, "--data", B, "contacts").stdout.strip() and run(PEER, "--data", A, "contacts").stdout.strip():
        return
    ticket = sh_ok(APP, "--data", A, "--print-ticket")
    app = subprocess.Popen([APP, "--data", A, "--hidden"], env=env, stderr=open(f"{WORK}/pair-app.err", "w"))
    time.sleep(4)
    print("peer add:", sh_ok(PEER, "--data", B, "add", ticket))
    time.sleep(2)
    app.terminate(); app.wait()
    print("A contacts:", run(PEER, "--data", A, "contacts").stdout.strip())
    print("B contacts:", run(PEER, "--data", B, "contacts").stdout.strip())


def call(env, peer_args, app_env):
    log = open(f"{WORK}/app.err", "w")
    app = subprocess.Popen([APP, "--data", A, "--hidden"], env=dict(env, **app_env), stderr=log)
    time.sleep(5)
    r = run(PEER, "--data", B, "call", "alice", *peer_args, "--secs", "10")
    time.sleep(1)
    app.terminate(); app.wait()
    err = open(f"{WORK}/app.err").read()
    print(r.stdout[-600:], r.stderr[-300:])
    print("\n".join(l for l in err.splitlines() if l.startswith(("STATE", "INCOMING", "STATS")))[-1800:])
    return r.stdout, err


def freq(text, pattern):
    m = re.findall(pattern + r".*?freq=([0-9.]+)", text)
    return float(m[-1]) if m else None


def check(label, got, want):
    ok = got is not None and abs(got - want) < 10
    print(f"{'PASS' if ok else 'FAIL'} {label}: {got} Hz (want {want})")
    return ok


def main():
    mode = sys.argv[1] if len(sys.argv) > 1 else "tone"
    env = setup(null_sink())
    pair(env)
    ok = True
    if mode == "tone":
        out, err = call(env, ["--tone", "660", "--record", f"{WORK}/tone.wav"], {"P2P_TEST_TONE": "440", "P2P_AUTO_ANSWER": "1"})
        ok &= check("peer heard app", freq(out, "RESULT"), 440)
        # Mid-call stats; the last line can catch the hangup's drain.
        mid = [l for l in err.splitlines() if l.startswith("STATS")][-4:-2]
        ok &= check("app heard peer", freq("\n".join(mid), "STATS"), 660)
    else:
        n = 48000 * 30
        pcm = b"".join(struct.pack("<h", int(12000 * math.sin(2 * math.pi * 880 * i / 48000))) for i in range(n))
        play = subprocess.Popen(["pacat", "-d", "p2p_test", "--format=s16le", "--rate=48000", "--channels=1"],
                                stdin=subprocess.PIPE, env=env)
        import threading
        threading.Thread(target=lambda: (play.stdin.write(pcm), play.stdin.close()), daemon=True).start()
        out, _ = call(env, ["--record", f"{WORK}/mic.wav"], {"P2P_AUTO_ANSWER": "1", **({"P2P_NO_APM": "1"} if os.environ.get("NO_APM") else {})})
        play.terminate()
        ok &= check("peer heard app's real mic", freq(out, "RESULT"), 880)
    # Leave no null sink behind.
    for line in run("pactl", "list", "short", "modules").stdout.splitlines():
        if "sink_name=p2p_test" in line:
            run("pactl", "unload-module", line.split()[0])
    sys.exit(0 if ok else 1)


main()
