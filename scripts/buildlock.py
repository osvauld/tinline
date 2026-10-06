#!/usr/bin/env python3
"""Run a heavy command (cargo, gradle) only when no other agent is building and the machine has
room, so parallel agents queue instead of thrashing 8 cores and RAM with three builds at once.

    scripts/buildlock.py --who android -- ./gradlew :app:assembleDebug
    scripts/buildlock.py --status

One exclusive flock for the whole machine; the holder's name and command sit next to it so a
waiter can say who it is waiting for. Before starting, also waits for 1-minute load below the
core count and enough free memory, since a test run or emulator also takes its share.
"""
import argparse
import fcntl
import json
import os
import subprocess
import sys
import time
from pathlib import Path

LOCK = Path("/tmp/claude-1000/p2p-build.lock")
HOLDER = LOCK.with_suffix(".holder")
MIN_FREE_GIB = 4.0


def available_gib():
    for line in Path("/proc/meminfo").read_text().splitlines():
        if line.startswith("MemAvailable:"):
            return int(line.split()[1]) / 1024 / 1024
    return 0.0


def holder():
    try:
        return json.loads(HOLDER.read_text())
    except (OSError, ValueError):
        return None


def status():
    h = holder()
    load = os.getloadavg()[0]
    print(f"load {load:.1f}/{os.cpu_count()}  free {available_gib():.1f} GiB")
    print(f"held by {h['who']} since {time.strftime('%H:%M:%S', time.localtime(h['since']))}: {h['cmd']}"
          if h else "free")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--who", default=os.environ.get("USER", "?"))
    ap.add_argument("--status", action="store_true")
    ap.add_argument("cmd", nargs=argparse.REMAINDER)
    a = ap.parse_args()
    if a.status:
        return status()
    cmd = a.cmd[1:] if a.cmd[:1] == ["--"] else a.cmd
    if not cmd:
        ap.error("no command")

    LOCK.parent.mkdir(parents=True, exist_ok=True)
    fd = os.open(LOCK, os.O_CREAT | os.O_RDWR)
    waited = 0
    while True:
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            break
        except BlockingIOError:
            if waited % 60 == 0:
                h = holder() or {}
                print(f"buildlock: waiting for {h.get('who', '?')} ({h.get('cmd', '?')})", file=sys.stderr, flush=True)
            time.sleep(5)
            waited += 5
    # Held: now wait for the machine itself (tests, emulators) to calm down, bounded.
    for _ in range(60):
        if os.getloadavg()[0] < os.cpu_count() and available_gib() >= MIN_FREE_GIB:
            break
        print(f"buildlock: busy machine (load {os.getloadavg()[0]:.1f}, free {available_gib():.1f} GiB), waiting",
              file=sys.stderr, flush=True)
        time.sleep(10)
    HOLDER.write_text(json.dumps({"who": a.who, "cmd": " ".join(cmd)[:200], "since": time.time(), "pid": os.getpid()}))
    env = dict(os.environ)
    # Leave two cores for the emulator, tests and the desktop.
    env.setdefault("CARGO_BUILD_JOBS", str(max(1, os.cpu_count() - 2)))
    try:
        rc = subprocess.call(cmd, env=env)
    finally:
        HOLDER.unlink(missing_ok=True)
        fcntl.flock(fd, fcntl.LOCK_UN)
    sys.exit(rc)


if __name__ == "__main__":
    main()
