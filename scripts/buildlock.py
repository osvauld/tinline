#!/usr/bin/env python3
"""Queue heavy work from parallel agents so the machine stays busy without thrashing.

    scripts/buildlock.py --who ga -- cargo test -p p2pcore --lib gcall
    scripts/buildlock.py --who ga --run -- python3 scripts/regress.py ...
    scripts/buildlock.py --who ga --device ZD22257P6G -- python3 scripts/gcall_bench.py phone ...
    scripts/buildlock.py --status

Three kinds of slot:
  compile  one at a time for the whole machine (rustc/gradle saturate every core). Default for
           any command. `cargo test ...` is split: `cargo test --no-run` compiles under this
           slot, then the tests themselves run in a run slot, so the next agent compiles while
           this one's tests wait on sockets and timers.
  run      RUN_SLOTS at once: test binaries, e2e scripts, benchmarks that use already-built
           binaries (--run).
  device   one per phone/emulator serial (--device SERIAL); also takes a run slot.

Admission waits on CPU and memory pressure (/proc/pressure), not load average: the load average
counts the emulator and threads blocked on disk and stays high while the cores sit idle.
Compiling still takes the same lock file as older copies of this script, so they interoperate.

Build output lives on the cache disk (~/build-cache): each checkout's target/ is a symlink to its
own dir there, and with sccache installed every cargo/gradle build through this script shares
compiled crates via ~/build-cache/sccache (`sccache --show-stats` to check hits).
"""
import argparse
import fcntl
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

DIR = Path("/tmp/claude-1000")
LOCK = DIR / "p2p-build.lock"  # compile slot; same path as older copies
RUN_SLOTS = 4
MIN_FREE_GIB = 4.0
# Percent of the last 10 s some task waited for a CPU; above this the cores are really full.
MAX_CPU_PRESSURE = 40.0


def available_gib():
    for line in Path("/proc/meminfo").read_text().splitlines():
        if line.startswith("MemAvailable:"):
            return int(line.split()[1]) / 1024 / 1024
    return 0.0


def cpu_pressure():
    try:
        return float(Path("/proc/pressure/cpu").read_text().split()[1].split("=")[1])
    except (OSError, IndexError, ValueError):
        return 0.0


def holder_path(lock):
    return lock.with_suffix(".holder")


def read_holder(lock):
    try:
        return json.loads(holder_path(lock).read_text())
    except (OSError, ValueError):
        return None


def status():
    print(f"cpu pressure {cpu_pressure():.0f}%  load {os.getloadavg()[0]:.1f}/{os.cpu_count()}  "
          f"free {available_gib():.1f} GiB")
    locks = [LOCK] + sorted(DIR.glob("p2p-run-*.lock")) + sorted(DIR.glob("p2p-device-*.lock"))
    for lock in locks:
        h = read_holder(lock)
        name = lock.stem.removeprefix("p2p-").replace("build", "compile")
        if h:
            age = int(time.time() - h["since"])
            print(f"{name:24} {h['who']} {age // 60}m{age % 60:02}s: {h['cmd']}")
        elif lock == LOCK:
            print(f"{name:24} free")
    for w in sorted(DIR.glob("p2p-wait-*.json")):
        try:
            d = json.loads(w.read_text())
            os.kill(d["pid"], 0)
        except (OSError, ValueError, KeyError):
            w.unlink(missing_ok=True)
            continue
        print(f"waiting for {d['slot']:12} {d['who']}: {d['cmd']}")


class Slot:
    """An flock held for the life of this process, with a holder note for --status."""

    def __init__(self, lock, who, cmd):
        self.lock, self.who, self.cmd = lock, who, cmd
        lock.parent.mkdir(parents=True, exist_ok=True)
        self.fd = os.open(lock, os.O_CREAT | os.O_RDWR)

    def try_take(self):
        try:
            fcntl.flock(self.fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            return False
        holder_path(self.lock).write_text(json.dumps(
            {"who": self.who, "cmd": self.cmd, "since": time.time(), "pid": os.getpid()}))
        return True

    def release(self):
        holder_path(self.lock).unlink(missing_ok=True)
        fcntl.flock(self.fd, fcntl.LOCK_UN)
        os.close(self.fd)


def acquire(kind, locks, who, cmd, gate):
    """Take the first free lock of `locks` once `gate()` passes; announce the wait once a minute."""
    slots = [Slot(l, who, cmd) for l in locks]
    note = DIR / f"p2p-wait-{os.getpid()}.json"
    note.write_text(json.dumps({"who": who, "cmd": cmd, "slot": kind, "pid": os.getpid()}))
    waited = 0
    try:
        while True:
            reason = gate()
            if reason is None:
                for s in slots:
                    if s.try_take():
                        for o in slots:
                            if o is not s:
                                os.close(o.fd)
                        return s
                h = read_holder(locks[0]) or {}
                reason = f"{kind} busy ({h.get('who', '?')}: {h.get('cmd', '?')[:80]})"
            if waited % 60 == 0:
                print(f"buildlock: waiting, {reason}", file=sys.stderr, flush=True)
            time.sleep(2)
            waited += 2
    finally:
        note.unlink(missing_ok=True)


def machine_gate(max_pressure):
    def gate():
        if available_gib() < MIN_FREE_GIB:
            return f"low memory ({available_gib():.1f} GiB free)"
        if cpu_pressure() > max_pressure:
            return f"cpu pressure {cpu_pressure():.0f}%"
        return None
    return gate


# Build output goes to the second disk when it is there: /home filled up with one 5-15 GB target/
# per agent worktree. A checkout without target/ gets a symlink to its own dir on that disk.
CACHE = Path.home() / "build-cache"


def place_target():
    try:
        root = Path(subprocess.check_output(["git", "rev-parse", "--show-toplevel"], text=True,
                                            stderr=subprocess.DEVNULL).strip())
    except (OSError, subprocess.CalledProcessError):
        return
    t = root / "target"
    if t.exists() or t.is_symlink() or not os.path.ismount(CACHE) or not (root / "Cargo.toml").exists():
        return
    name = root.name if root.parent.name in ("worktrees", "worktree") else "main"
    real = CACHE / "p2p_chat" / name / "target"
    real.mkdir(parents=True, exist_ok=True)
    t.symlink_to(real)
    print(f"buildlock: target/ -> {real}", file=sys.stderr, flush=True)


# Compiled crates are shared between worktrees through sccache, its cache on the same disk: a fresh
# worktree otherwise rebuilds all ~600 dependencies (15+ min in release). One target/ for everyone
# would be faster still, but agents on different branches would overwrite each other's binaries.
SCCACHE_DIR = CACHE / "sccache"
SCCACHE_SIZE = "30G"


def share_compiles(env):
    """Points cargo at sccache when it is installed and the cache disk is mounted; else nothing."""
    if env.get("RUSTC_WRAPPER") or not shutil.which("sccache") or not os.path.ismount(CACHE):
        return
    env["RUSTC_WRAPPER"] = "sccache"
    env.setdefault("SCCACHE_DIR", str(SCCACHE_DIR))
    env.setdefault("SCCACHE_CACHE_SIZE", SCCACHE_SIZE)


def compile_step(cmd):
    """For a plain `cargo test ...`, the same command with --no-run and without the test args."""
    if len(cmd) < 2 or Path(cmd[0]).name != "cargo":
        return None
    sub = next((i for i, a in enumerate(cmd[1:], 1) if not a.startswith("+")), None)
    if sub is None or cmd[sub] != "test" or "--no-run" in cmd:
        return None
    args = cmd[sub + 1:]
    if "--" in args:
        args = args[:args.index("--")]
    # A positional test-name filter does not change what gets built; drop it.
    takes_value = {"-p", "--package", "--test", "--bench", "--bin", "--example", "--features", "-F",
                   "--target", "--target-dir", "--profile", "-j", "--jobs", "--manifest-path",
                   "--exclude", "--color", "--message-format", "-Z", "--config"}
    kept, i = [], 0
    while i < len(args):
        a = args[i]
        if a in takes_value and i + 1 < len(args):
            kept += [a, args[i + 1]]
            i += 2
            continue
        if a.startswith("-"):
            kept.append(a)
        i += 1
    return cmd[:sub + 1] + kept + ["--no-run"]


def run(cmd, env):
    return subprocess.call(cmd, env=env)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--who", default=os.environ.get("USER", "?"))
    ap.add_argument("--status", action="store_true")
    ap.add_argument("--run", action="store_true", help="no compiling: take a run slot only")
    ap.add_argument("--device", metavar="SERIAL", help="uses this phone/emulator: device lock + run slot")
    ap.add_argument("cmd", nargs=argparse.REMAINDER)
    a = ap.parse_args()
    if a.status:
        return status()
    cmd = a.cmd[1:] if a.cmd[:1] == ["--"] else a.cmd
    if not cmd:
        ap.error("no command")
    text = " ".join(cmd)[:200]
    env = dict(os.environ)
    # Leave two cores for the emulator, running tests and the desktop.
    env.setdefault("CARGO_BUILD_JOBS", str(max(1, os.cpu_count() - 2)))
    place_target()
    share_compiles(env)

    if not (a.run or a.device):
        pre = compile_step(cmd)
        c = acquire("compile", [LOCK], a.who, " ".join(pre or cmd)[:200], machine_gate(MAX_CPU_PRESSURE))
        try:
            rc = run(pre or cmd, env)
        finally:
            c.release()
        if pre is None or rc != 0:
            sys.exit(rc)
        # Built; the real `cargo test` now only runs the binaries.

    dev = None
    if a.device:
        safe = "".join(ch if ch.isalnum() or ch in "-_." else "_" for ch in a.device)
        dev = acquire(f"device {a.device}", [DIR / f"p2p-device-{safe}.lock"], a.who, text, lambda: None)
    slot = acquire("run", [DIR / f"p2p-run-{i}.lock" for i in range(RUN_SLOTS)], a.who, text,
                   machine_gate(MAX_CPU_PRESSURE))
    try:
        rc = run(cmd, env)
    finally:
        slot.release()
        if dev:
            dev.release()
    sys.exit(rc)


if __name__ == "__main__":
    main()
