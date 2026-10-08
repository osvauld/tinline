#!/usr/bin/env python3
"""Chat end to end on this machine: three p2p-peer instances over iroh (docs/chat.md section 6).

    scripts/e2e_chat.py            # all scenarios
    scripts/e2e_chat.py --keep     # keep the temp dir

Covers C1-C7 (C8 is Android), plus: a 50 MB file round trip byte for byte, a voice-message blob
round trip, a contact that is not a party to the conversation cannot fetch the blob, a forged
batch is rejected (unit test run at the end), restart keeps everything, offline catch-up, and
concurrent edits converge.

Needs `target/release/p2p-peer` (cargo build --release -p peer) and network for iroh's relay.
Clock knob: P2P_CHAT_CLOCK_MS_OFFSET shifts the chat clock of one peer (for the day-shard tests).
"""
import argparse
import hashlib
import os
import queue
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PEER = ROOT / "target/release/p2p-peer"
DAY_MS = 86_400_000


class Checks:
    def __init__(self):
        self.failed = 0

    def ok(self, label, cond, detail=""):
        print(f"{'PASS' if cond else 'FAIL'} {label} {detail}".rstrip(), flush=True)
        self.failed += not cond
        return cond


def run(data, *args, env=None, timeout=120):
    cmd = [str(PEER), "--data", str(data), *map(str, args)]
    return subprocess.run(cmd, env=env, capture_output=True, text=True, timeout=timeout)


class Serve:
    """A long-lived `chat-serve` peer: commands on stdin, lines on stdout."""

    def __init__(self, data, env=None):
        e = dict(os.environ)
        e.update(env or {})
        self.p = subprocess.Popen([str(PEER), "--data", str(data), "chat-serve"], env=e, text=True,
                                  stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=open(str(data) + ".err", "a"), bufsize=1)
        self.lines = []
        self.lock = threading.Lock()
        threading.Thread(target=self._read, daemon=True).start()
        self.wait_line(r"^READY", 60)

    def _read(self):
        for line in self.p.stdout:
            with self.lock:
                self.lines.append(line.rstrip("\n"))

    def mark(self):
        with self.lock:
            return len(self.lines)

    def wait_line(self, pattern, timeout, start=0):
        end = time.time() + timeout
        while time.time() < end:
            with self.lock:
                for i in range(start, len(self.lines)):
                    if re.search(pattern, self.lines[i]):
                        return self.lines[i]
            time.sleep(0.05)
        return None

    def cmd(self, line, timeout=180):
        start = self.mark()
        self.p.stdin.write(line + "\n")
        self.p.stdin.flush()
        end = time.time() + timeout
        while time.time() < end:
            with self.lock:
                for i in range(start, len(self.lines)):
                    if self.lines[i].startswith(("OK ", "ERR ")):
                        return self.lines[start:i + 1]
            time.sleep(0.02)
        raise TimeoutError(line)

    def send(self, who, text):
        out = self.cmd(f"send {who} {text}")
        return out[-1].split()[-1] if out[-1].startswith("OK SENT") else None

    def listing(self, who, day=None):
        out = self.cmd(f"list {who}" + (f" {day}" if day else ""))
        msgs = []
        info = {}
        for l in out:
            if l.startswith("DAY "):
                m = re.match(r"DAY (\S+) older=(.*)", l)
                info = {"day": m.group(1), "older": m.group(2)}
            elif l.startswith("MSG "):
                d = dict(re.findall(r"(\w+)=(\"[^\"]*\"|\S+)", l[4:]))
                d["text"] = re.search(r'text="((?:[^"\\]|\\.)*)"', l).group(1) if 'text="' in l else ""
                msgs.append(d)
        return info, msgs

    def texts(self, who, day=None):
        return [m["text"] for m in self.listing(who, day)[1]]

    def quit(self):
        try:
            self.p.stdin.write("quit\n")
            self.p.stdin.flush()
            self.p.wait(timeout=20)
        except Exception:
            self.p.kill()

    def kill(self):
        self.p.kill()


def until(fn, timeout, step=0.25):
    end = time.time() + timeout
    while time.time() < end:
        v = fn()
        if v:
            return v
        time.sleep(step)
    return fn()


def day_noon_offset(days_back):
    """Offset (ms) that moves the chat clock to noon UTC `days_back` days ago."""
    now = datetime.now(timezone.utc)
    target = now.replace(hour=12, minute=0, second=0, microsecond=0).timestamp() * 1000 - days_back * DAY_MS
    return int(target - now.timestamp() * 1000)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--keep", action="store_true")
    ap.add_argument("--skip-unit", action="store_true")
    args = ap.parse_args()
    c = Checks()
    tmp = Path(tempfile.mkdtemp(prefix="tl-chat-e2e-"))
    env = dict(os.environ)
    env.pop("P2P_RELAY_ONLY", None)
    live = []
    try:
        for n in ("alice", "bob", "carol"):
            run(tmp / n, "init", n, env=env)
        t = run(tmp / "alice", "ticket", env=env).stdout.strip()
        c.ok("alice ticket", t.startswith("OSVC2:"))
        for joiner in ("bob", "carol"):
            # The ticket owner has to be online while the joiner redeems it.
            lis = subprocess.Popen([str(PEER), "--data", str(tmp / "alice"), "listen", "--for", "40"], env=env,
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            time.sleep(3)
            c.ok(f"{joiner} adds alice", "added" in run(tmp / joiner, "add", t, env=env).stdout)
            lis.kill()
            lis.wait()
            t = run(tmp / "alice", "ticket", env=env).stdout.strip()

        alice = Serve(tmp / "alice")
        bob = Serve(tmp / "bob")
        live += [alice, bob]
        time.sleep(5)  # both dial each other at start

        # ---- C1: three messages, in order, within 2 s, two ticks -------------------------------
        t0 = time.time()
        ids = [alice.send("bob", f"c1-{i}") for i in range(3)]
        got = until(lambda: bob.texts("alice") == ["c1-0", "c1-1", "c1-2"], 2.5)
        c.ok("C1 bob has 3 messages in order within 2 s", bool(got), f"{time.time() - t0:.2f}s {bob.texts('alice')}")
        ticks = until(lambda: all(m["delivery"] == "Delivered" for m in alice.listing("bob")[1]), 3)
        c.ok("C1 alice sees two ticks (Delivered)", bool(ticks))

        # ---- C4: edit and delete ------------------------------------------------------------------
        alice.cmd(f"edit bob {ids[1]} c1-1-edited")
        alice.cmd(f"delete bob {ids[2]}")
        ok = until(lambda: bob.texts("alice") == ["c1-0", "c1-1-edited", ""], 5)
        c.ok("C4 bob shows the edit", bool(ok), str(bob.texts("alice")))
        dm = [m for m in bob.listing("alice")[1] if m["id"] == ids[2]]
        c.ok("C4 bob shows the deletion", bool(dm) and dm[0]["deleted"] == "true")

        # Replies and unread
        rid = bob.send("alice", "reply from bob")
        until(lambda: "reply from bob" in alice.texts("bob"), 5)
        chats = alice.cmd("chats")
        c.ok("unread counted for alice", any("unread=1" in l for l in chats), str(chats))
        alice.cmd("markread bob")
        c.ok("mark_read zeroes it", any("unread=0" in l for l in alice.cmd("chats")))

        # ---- C2: offline catch-up -----------------------------------------------------------------
        bob.quit()
        live.remove(bob)
        time.sleep(1)
        for i in range(5):
            alice.send("bob", f"c2-{i}")
        bob = Serve(tmp / "bob")
        live.append(bob)
        want = [f"c2-{i}" for i in range(5)]
        ok = until(lambda: [t for t in bob.texts("alice") if t.startswith("c2-")] == want, 40)
        c.ok("C2 bob catches up without opening the chat", bool(ok), str(bob.texts("alice")))
        ok = until(lambda: all(m["delivery"] == "Delivered" for m in alice.listing("bob")[1] if m["text"].startswith("c2-")), 10)
        c.ok("C2 alice sees them delivered", bool(ok))

        # ---- C3: both write while apart, then converge; concurrent edit -------------------------------
        a_first = alice.send("bob", "c3-a-first")
        until(lambda: "c3-a-first" in bob.texts("alice"), 8)
        alice.quit(); bob.quit()
        live.clear()
        r1 = run(tmp / "alice", "chat-send", "bob", "c3-from-alice", "--wait", "0", env=env)
        r2 = run(tmp / "bob", "chat-send", "alice", "c3-from-bob", "--wait", "0", env=env)
        c.ok("C3 both wrote while apart", "SENT" in r1.stdout and "SENT" in r2.stdout)
        alice = Serve(tmp / "alice")
        bob = Serve(tmp / "bob")
        live += [alice, bob]
        alice.cmd(f"edit bob {a_first} c3-a-first-edited")
        want = lambda s: sorted(t for t in s if t.startswith("c3-"))
        ok = until(lambda: want(alice.texts("bob")) == want(bob.texts("alice")) == sorted(["c3-a-first-edited", "c3-from-alice", "c3-from-bob"]), 40)
        c.ok("C3 both show the same merge", bool(ok), f"{alice.texts('bob')[-4:]} / {bob.texts('alice')[-4:]}")
        order_a = [m["id"] for m in alice.listing("bob")[1]]
        order_b = [m["id"] for m in bob.listing("alice")[1]]
        c.ok("C3 same order on both sides", order_a == order_b)

        # ---- file: 50 MB round trip --------------------------------------------------------------------
        big = tmp / "big.bin"
        with open(big, "wb") as f:
            for i in range(50):
                f.write(hashlib.sha256(str(i).encode()).digest() * 32768)
        sha = hashlib.sha256(big.read_bytes()).hexdigest()
        t0 = time.time()
        mark = bob.mark()
        out = alice.cmd(f"file bob {big}", timeout=300)
        fid = out[-1].split()[-1]
        got = bob.wait_line(rf"CHAT added id={fid}.*state=(Remote|Downloading)", 30, mark)
        c.ok("50 MB file is offered, not auto-downloaded", got is not None, str(got)[:120])
        saved = tmp / "big.out"
        r = bob.cmd(f"get alice {fid} {saved}", timeout=300)
        same = saved.exists() and hashlib.sha256(saved.read_bytes()).hexdigest() == sha
        c.ok("50 MB file round trip byte-identical", same, f"{time.time() - t0:.1f}s {r[-1][:80]}")
        small = tmp / "small.txt"
        small.write_text("hello file " * 100)
        sid = alice.cmd(f"file bob {small}")[-1].split()[-1]
        ok = until(lambda: any(m["id"] == sid and m.get("state") == "Ready" for m in bob.listing("alice")[1]), 15)
        c.ok("small file auto-downloads", bool(ok))
        bob.cmd(f"get alice {sid} {tmp / 'small.out'}")
        c.ok("small file identical", (tmp / "small.out").read_text() == small.read_text())

        # ---- voice message blob -----------------------------------------------------------------------------
        voice = tmp / "voice.ogg"
        voice.write_bytes(b"OggS" + os.urandom(20000))
        vid = alice.cmd(f"voice bob {voice} 4200")[-1].split()[-1]
        ok = until(lambda: any(m["id"] == vid and m.get("kind") == "Voice" and m.get("state") == "Ready" for m in bob.listing("alice")[1]), 15)
        c.ok("voice message arrives as Voice and downloads", bool(ok))
        bob.cmd(f"get alice {vid} {tmp / 'voice.out'}")
        c.ok("voice blob identical", (tmp / "voice.out").read_bytes() == voice.read_bytes())

        # ---- a contact that is not a party cannot fetch the blob ------------------------------------------------
        lst = alice.listing("bob")[1]
        fmsg = [m for m in lst if m["id"] == fid][0]
        h = fmsg["hash"]
        carol = run(tmp / "carol", "blob-fetch", "alice", h, 50 * 1024 * 1024, env=env)
        c.ok("non-party contact cannot fetch the blob", carol.returncode != 0 or "RESULT fetched=false" in carol.stdout,
             (carol.stdout + carol.stderr).strip()[-160:])

        # ---- restart keeps everything ---------------------------------------------------------------------------
        before_a = alice.texts("bob")
        before_b = bob.texts("alice")
        alice.quit(); bob.quit()
        live.clear()
        alice = Serve(tmp / "alice")
        bob = Serve(tmp / "bob")
        live += [alice, bob]
        c.ok("restart keeps alice's conversation", alice.texts("bob") == before_a)
        c.ok("restart keeps bob's conversation", bob.texts("alice") == before_b)
        bob.cmd(f"get alice {sid} {tmp / 'small2.out'}")
        c.ok("restart keeps downloaded blobs readable", (tmp / "small2.out").read_text() == small.read_text())

        # ---- C6: day shards; old history fetched on demand ------------------------------------------------------------
        alice.quit(); live.remove(alice)
        old = Serve(tmp / "alice", env={"P2P_CHAT_CLOCK_MS_OFFSET": str(day_noon_offset(1))})
        old.send("bob", "c6-yesterday")
        old.quit()
        older = Serve(tmp / "alice", env={"P2P_CHAT_CLOCK_MS_OFFSET": str(day_noon_offset(10))})
        older.send("bob", "c6-ten-days-ago")
        older.quit()
        alice = Serve(tmp / "alice")
        live.append(alice)
        alice.send("bob", "c6-today")
        ok = until(lambda: "c6-today" in bob.texts("alice"), 15)
        info, msgs = bob.listing("alice")
        c.ok("C6 today's shard has today's message only", bool(ok) and [m["text"] for m in msgs if m["text"].startswith("c6-")] == ["c6-today"], str(info))
        yday = datetime.fromtimestamp(time.time() + day_noon_offset(1) / 1000, timezone.utc).strftime("%Y-%m-%d")
        ok = until(lambda: "c6-yesterday" in bob.texts("alice", yday), 20)
        c.ok("C6 yesterday's shard synced as its own day", bool(ok))
        # A fresh install of bob (chat data gone): the last 7 days sync, older ones are asked for.
        bob.quit(); live.remove(bob)
        (tmp / "bob" / "chat.redb").unlink()
        shutil.rmtree(tmp / "bob" / "blobs", ignore_errors=True)
        bob = Serve(tmp / "bob")
        live.append(bob)
        ok = until(lambda: "c6-yesterday" in bob.texts("alice", yday) and "c6-today" in bob.texts("alice"), 30)
        c.ok("C6 fresh install pulls today and yesterday by itself", bool(ok))
        tenth = datetime.fromtimestamp(time.time() + day_noon_offset(10) / 1000, timezone.utc).strftime("%Y-%m-%d")
        c.ok("C6 ten-day-old shard not pulled yet", "c6-ten-days-ago" not in bob.texts("alice", tenth))
        r = bob.cmd(f"history alice {yday}")
        n = int(r[-1].split()[-1]) if r[-1].startswith("OK HISTORY") else -1
        c.ok("C6 scrolling back pulls the older shard", n >= 1 and "c6-ten-days-ago" in bob.texts("alice", tenth), r[-1])

        # ---- C7: removal ---------------------------------------------------------------------------------------------------
        alice.cmd("remove bob")
        c.ok("C7 alice's copy of the conversation is gone", any(l.startswith("ERR") for l in alice.cmd("list bob")) and not any("bob" in l for l in alice.cmd("chats") if l.startswith("CHATROW")))
        mid = bob.send("alice", "c7-after-removal")
        time.sleep(8)
        pending = [m for m in bob.listing("alice")[1] if m["id"] == mid]
        c.ok("C7 bob's later message is refused at alice (never delivered)", bool(pending) and pending[0]["delivery"] == "Pending")
        c.ok("C7 alice stored nothing from it", any(l.startswith("ERR") for l in alice.cmd("list bob")))
    finally:
        for s in live:
            s.kill()
        if not args.keep:
            shutil.rmtree(tmp, ignore_errors=True)
        else:
            print("kept", tmp)

    if not args.skip_unit:
        # C5: the forged-batch rules are unit tests (peer id, author, third-device signature, ...).
        r = subprocess.run([sys.executable, str(ROOT / "scripts/buildlock.py"), "--who", "chat", "--", "cargo", "test", "--release", "-p", "p2pcore",
                            "--", "forged", "batch_signatures"], cwd=ROOT, capture_output=True, text=True)
        c.ok("C5 forged batches rejected and not stored (unit tests)", r.returncode == 0, r.stdout[-200:] if r.returncode else "")
    print("FAILED" if c.failed else "ALL PASSED", c.failed)
    return 1 if c.failed else 0


if __name__ == "__main__":
    sys.exit(main())
