#!/usr/bin/env python3
"""A second incoming call during a call (TASK 29), with four headless p2p-peer instances.

    scripts/e2e_callwait.py

B listens and answers A's call. While it is up, C calls B, and B reacts (p2p-peer
`listen --second ACTION`):
  decline -> C ends `busy`, the A-B call continues to its end (B's RESULT ended_by=us/hangup by A)
  answer  -> A's call ends, B moves on to C's call and hears C's tone
  ignore  -> C ends `no_answer` after the shortened ring (P2P_WAITING_RING_SECS), B logs a missed call
"""
import os
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PEER = ROOT / "target/release/p2p-peer"
WAIT_SECS = 5


def peer(data, *args, env, wait=True, out=None):
    cmd = [str(PEER), "--data", str(data), *map(str, args)]
    if not wait:
        return subprocess.Popen(cmd, env=env, stdout=out, stderr=subprocess.DEVNULL, text=True)
    return subprocess.run(cmd, env=env, capture_output=True, text=True, timeout=120)


class Checks:
    def __init__(self):
        self.failed = 0

    def ok(self, label, cond, detail=""):
        print(f"{'PASS' if cond else 'FAIL'} {label} {detail}".rstrip(), flush=True)
        self.failed += not cond


def setup(tmp, env):
    for n in "abc":
        peer(tmp / n, "init", n, env=env)
    # b has a and c as contacts, and they have b: each of a, c adds b's ticket.
    for who in "ac":
        ticket = peer(tmp / "b", "ticket", env=env).stdout.strip()
        listener = peer(tmp / "b", "listen", "--once", "--secs", 5, env=env, wait=False, out=subprocess.DEVNULL)
        time.sleep(3)
        peer(tmp / who, "add", ticket, env=env)
        listener.kill()
        listener.wait()


def scenario(c, tmp, env, action):
    print(f"== second call: {action} ==", flush=True)
    log = tmp / f"b-{action}.out"
    with open(log, "w") as out:
        b = peer(tmp / "b", "listen", "--once", "--second", action, "--tone", 550, "--secs", 14,
                 env=env, wait=False, out=out)
        time.sleep(3)
        a = peer(tmp / "a", "call", "b", "--tone", 440, "--secs", 12, env=env, wait=False, out=subprocess.PIPE)
        time.sleep(5)  # a-b is active
        cres = peer(tmp / "c", "call", "b", "--tone", 660, "--secs", 6, env=env)
        a_out = a.communicate(timeout=60)[0]
        b.wait(timeout=60)
    b_out = log.read_text()
    c_out = cres.stdout
    c.ok(f"{action}: B saw a second call waiting", "SECOND_CALL" in b_out and "waiting=true" in b_out, b_out[-300:])
    if action == "decline":
        c.ok("decline: C sees busy", "reason=\"busy\"" in c_out or "reason=busy" in c_out, c_out[-200:])
        c.ok("decline: the A-B call continued", "ended_by=us" in b_out or "ended_by=hangup" in b_out, b_out[-200:])
        m = re.search(r"RESULT .*?freq=([0-9.]+)", a_out or "")
        c.ok("decline: A still heard B", bool(m) and abs(float(m.group(1)) - 550) < 10, a_out[-200:] if a_out else "")
    elif action == "ignore":
        c.ok("ignore: C sees no_answer", "no_answer" in c_out, c_out[-200:])
        rec = peer(tmp / "b", "recents", env=env).stdout
        c.ok("ignore: B logged a missed call from c",
             re.search(r"peer=c incoming=true reason=no_answer missed=true", rec) is not None, rec[-300:])
        m = re.search(r"RESULT .*?freq=([0-9.]+)", a_out or "")
        c.ok("ignore: A-B call undisturbed", bool(m) and abs(float(m.group(1)) - 550) < 10, a_out[-200:] if a_out else "")
    else:
        c.ok("answer: A's call ended by B", "reason=hangup_remote" in (a_out or "") or "ended_by=hangup_remote" in (a_out or ""), (a_out or "")[-200:])
        m = re.search(r"RESULT .*", c_out)
        f = re.search(r"freq=([0-9.]+)", m.group(0)) if m else None
        c.ok("answer: C was answered and heard B", bool(f) and abs(float(f.group(1)) - 550) < 10, c_out[-200:])
        c.ok("answer: B ran a second call", b_out.count("RESULT") >= 2, b_out[-300:])


def main():
    if not PEER.exists():
        subprocess.run(["cargo", "build", "--release", "-q", "-p", "peer"], cwd=ROOT, check=True)
    env = dict(os.environ, P2P_WAITING_RING_SECS=str(WAIT_SECS))
    env.pop("P2P_RELAY_ONLY", None)
    c = Checks()
    with tempfile.TemporaryDirectory(prefix="p2p-callwait-") as d:
        tmp = Path(d)
        setup(tmp, env)
        for action in ("decline", "ignore", "answer"):
            scenario(c, tmp, env, action)
    print("ALL PASS" if not c.failed else f"{c.failed} FAILED")
    sys.exit(1 if c.failed else 0)


if __name__ == "__main__":
    main()
