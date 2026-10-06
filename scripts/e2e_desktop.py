#!/usr/bin/env python3
"""Two p2p-peer instances on this machine, over iroh: add a contact, call both ways, and check
each side heard the other's tone.

    scripts/e2e_desktop.py                 # direct, then relay-only
    scripts/e2e_desktop.py --mode relay    # one mode

Relay mode sets P2P_RELAY_ONLY=1, so no UDP of our own: every packet crosses the relay.
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
PEER = ROOT / "target/release/p2p-peer"


def peer(data, *args, env, wait=True, out=None):
    cmd = [str(PEER), "--data", str(data), *map(str, args)]
    if not wait:
        return subprocess.Popen(cmd, env=env, stdout=out or subprocess.DEVNULL,
                                stderr=subprocess.DEVNULL, text=True)
    return subprocess.run(cmd, env=env, capture_output=True, text=True, timeout=120)


def heard(text):
    m = re.search(r"RESULT .*?freq=([0-9.]+)", text or "")
    return float(m.group(1)) if m else None


class Checks:
    def __init__(self):
        self.failed = 0

    def ok(self, label, cond, detail=""):
        print(f"{'PASS' if cond else 'FAIL'} {label} {detail}".rstrip(), flush=True)
        self.failed += not cond

    def tone(self, label, text, want):
        f = heard(text)
        self.ok(label, f is not None and abs(f - want) < 10, f"heard {f} Hz, want {want}")


def call_pair(c, mode, env, tmp, caller, callee, caller_name, callee_name, tx, rx):
    """`callee` listens sending `rx` Hz; `caller` calls sending `tx` Hz."""
    log = tmp / f"{callee_name}.out"
    with open(log, "w") as out:
        listener = peer(tmp / callee_name, "listen", "--once", "--tone", rx, "--secs", 8,
                        env=env, wait=False, out=out)
        time.sleep(3)
        r = peer(tmp / caller_name, "call", callee_name, "--tone", tx, "--secs", 8, env=env)
        listener.wait(timeout=60)
    c.tone(f"{mode}: {caller_name} heard {callee_name}", r.stdout, rx)
    c.tone(f"{mode}: {callee_name} heard {caller_name}", log.read_text(), tx)


def run_mode(c, mode):
    print(f"== {mode} ==", flush=True)
    env = dict(os.environ)
    if mode == "relay":
        env["P2P_RELAY_ONLY"] = "1"
    else:
        env.pop("P2P_RELAY_ONLY", None)
    with tempfile.TemporaryDirectory(prefix="p2p-e2e-") as d:
        tmp = Path(d)
        peer(tmp / "alice", "init", "alice", env=env)
        peer(tmp / "bob", "init", "bob", env=env)
        ticket = peer(tmp / "alice", "ticket", env=env).stdout.strip()
        c.ok(f"{mode}: ticket", ticket.startswith("OSVC2:"))
        listener = peer(tmp / "alice", "listen", "--once", "--secs", 5, env=env, wait=False)
        time.sleep(3)
        added = peer(tmp / "bob", "add", ticket, env=env)
        listener.kill()
        listener.wait()
        c.ok(f"{mode}: contact added", "added alice" in added.stdout, added.stderr[-300:])
        call_pair(c, mode, env, tmp, tmp / "bob", tmp / "alice", "bob", "alice", 660, 440)
        # Reverse: uses the grant bob gave alice during the add.
        call_pair(c, mode, env, tmp, tmp / "alice", tmp / "bob", "alice", "bob", 770, 550)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--mode", choices=["direct", "relay"], action="append")
    args = ap.parse_args()
    subprocess.run(["cargo", "build", "--release", "-q", "-p", "peer"], cwd=ROOT, check=True)
    c = Checks()
    for mode in args.mode or ["direct", "relay"]:
        run_mode(c, mode)
    print("ALL PASS" if not c.failed else f"{c.failed} FAILED")
    sys.exit(1 if c.failed else 0)


if __name__ == "__main__":
    main()
