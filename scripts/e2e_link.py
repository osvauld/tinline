#!/usr/bin/env python3
"""Device linking end to end on this machine (docs/design/device-linking.md): three headless
p2p-peer instances over iroh.

    scripts/e2e_link.py          # run
    scripts/e2e_link.py --keep   # keep the temp dir

A (a contact), E (existing install), N (new install). N shows a QR, E scans it, the codes match,
E approves, N commits. Then: N has A and the call log, an alias set on E reaches N, A's calls ring
E and (after A learned the DeviceList) both, E unlinks N and N drops the account.

Needs `target/release/p2p-peer` (cargo build --release -p peer) and network for iroh's relay.
"""
import argparse
import os
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PEER = ROOT / "target/release/p2p-peer"


class Checks:
    def __init__(self):
        self.failed = 0

    def ok(self, label, cond, detail=""):
        print(f"{'PASS' if cond else 'FAIL'} {label} {detail}".rstrip(), flush=True)
        self.failed += not cond
        return cond


class Peer:
    def __init__(self, data):
        self.data = data
        self.p = subprocess.Popen([str(PEER), "--data", str(data), "link-serve"], text=True, stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE, stderr=open(str(data) + ".err", "a"), bufsize=1)
        self.lines = []
        self.lock = threading.Lock()
        threading.Thread(target=self._read, daemon=True).start()
        assert self.wait(r"^READY", 60), "peer did not start"

    def _read(self):
        for line in self.p.stdout:
            with self.lock:
                self.lines.append(line.rstrip("\n"))

    def mark(self):
        with self.lock:
            return len(self.lines)

    def wait(self, pattern, timeout, start=0):
        end = time.time() + timeout
        while time.time() < end:
            with self.lock:
                for i in range(start, len(self.lines)):
                    if re.search(pattern, self.lines[i]):
                        return self.lines[i]
            time.sleep(0.05)
        return None

    def cmd(self, line, timeout=120):
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

    def last(self, line):
        return self.cmd(line)[-1]

    def quit(self):
        try:
            self.p.stdin.write("quit\n")
            self.p.stdin.flush()
            self.p.wait(timeout=20)
        except Exception:
            self.p.kill()


def until(fn, timeout, step=0.3):
    end = time.time() + timeout
    while time.time() < end:
        v = fn()
        if v:
            return v
        time.sleep(step)
    return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    if not PEER.exists():
        sys.exit(f"build it first: cargo build --release -p peer ({PEER})")
    tmp = Path(tempfile.mkdtemp(prefix="e2e-link-"))
    c = Checks()
    peers = []
    try:
        a, e, n = (Peer(tmp / x) for x in ("a", "e", "n"))
        peers = [a, e, n]
        a_did = a.last("init alice").split()[-1]
        e_did = e.last("init erin").split()[-1]
        c.ok("E added A", e.last(f"add {a.last('ticket').split(' ', 2)[-1]}").startswith("OK ADDED"))
        # A history record on E (A calls, E declines is not scriptable; an answered call is enough).
        e.cmd("autoanswer on")
        call = a.last(f"call {e_did}").split()[-1]
        c.ok("call answered", e.wait(r"STATE .* Active", 40) is not None)
        time.sleep(1.5)
        a.cmd(f"hangup {call}")
        e.wait(rf"STATE {call} Ended", 20)
        e.cmd("autoanswer off")

        # Link: N shows, E scans.
        m_e, m_n = e.mark(), n.mark()
        qr = n.last("link-new-show Pixel").split()[-1]
        c.ok("QR shown", qr.startswith("OSVL1:"))
        c.ok("E scans", e.last(f"link-scan {qr}").startswith("OK"))
        le = e.wait(r"^LINK code=", 40, m_e)
        ln = n.wait(r"^LINK code=", 40, m_n)
        code = lambda l: re.search(r"code=(\d{3} \d{3})", l or "") and re.search(r"code=(\d{3} \d{3})", l).group(1)
        c.ok("same code on both", le and ln and code(le) == code(ln), f"{le} / {ln}")
        c.ok("E sees N's label", le is not None and "Pixel" in le)
        c.ok("approve", e.last("approve").startswith("OK"))
        c.ok("N linked", n.wait(r"^LINK_DONE", 40, m_n) is not None)
        c.ok("N committed", n.last("commit").startswith("OK"))
        n_who = n.last("whoami").split()
        c.ok("same DID", n_who[1] == e_did)
        c.ok("devices differ", n_who[2] != e.last("whoami").split()[2])

        # Data arrives by own-device sync.
        def has_contact():
            out = n.cmd("contacts")
            return any(a_did in l for l in out)
        c.ok("N has contact A", until(has_contact, 60))
        c.ok("N has the call", until(lambda: any(call in l for l in n.cmd("recents")), 60))
        c.ok("two devices on E", until(lambda: sum(l.startswith("DEVICE") for l in e.cmd("devices")) == 2, 30))
        c.ok("two devices on N", until(lambda: sum(l.startswith("DEVICE") for l in n.cmd("devices")) == 2, 30))
        e.cmd(f"alias {a_did} Al")
        c.ok("alias reaches N", until(lambda: any('alias=Some("Al")' in l for l in n.cmd("contacts")), 60))

        # A learns N on an answered call, then both ring.
        e.cmd("autoanswer on")
        call2 = a.last(f"call {e_did}").split()[-1]
        e.wait(rf"STATE {call2} Active", 40)
        time.sleep(1.5)
        a.cmd(f"hangup {call2}")
        e.wait(rf"STATE {call2} Ended", 20)
        e.cmd("autoanswer off")
        m_e, m_n = e.mark(), n.mark()
        call3 = a.last(f"call {e_did}").split()[-1]
        c.ok("E rings", e.wait(rf"INCOMING {call3}", 40, m_e) is not None)
        c.ok("N rings", n.wait(rf"INCOMING {call3}", 40, m_n) is not None)
        a.cmd(f"hangup {call3}")

        # Unlink.
        n_dev = n_who[2]
        c.ok("unlink", e.last(f"unlink {n_dev}").startswith("OK"))
        c.ok("N drops the account", n.wait(r"^UNLINKED", 40, m_n) is not None)
    finally:
        for p in peers:
            p.quit()
        if args.keep:
            print("kept", tmp)
        else:
            shutil.rmtree(tmp, ignore_errors=True)
    print("FAILED" if c.failed else "ALL PASS")
    sys.exit(1 if c.failed else 0)


if __name__ == "__main__":
    main()
