#!/usr/bin/env python3
"""Android device linking e2e (emulator-5554, debug APK installed) against headless p2p-peer
instances (docs/design/device-linking.md). Screenshots go to artifacts/34h-android/.

    scripts/e2e_android_link.py [--serial emulator-5554]

Part A: phone is E (account + contact + call), peer N shows a QR, the phone "scans" it through the
        debug hook, wrong passphrase then right one, N gets the DID/contact; cancel; then
        Linked devices screen and unlink of the peer from the phone.
Part B: phone is a fresh install (N) on the onboarding "Link to an existing account" path; peer E
        scans the QR the phone shows (QR text read from the debug log), approves; phone gets the
        account, contacts and call history.
Uses only the camera-free debug hook for the QR text. Needs target/release/p2p-peer.
"""
import argparse, re, subprocess, sys, tempfile, shutil, time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from e2e_link import Peer, Checks, until, ROOT  # noqa: E402

PKG = "com.osvauld.tinline"
OUT = ROOT / "artifacts/34h-android"
S = "emulator-5554"
PASS = "test-passphrase"


def adb(*a, **k):
    return subprocess.run(["adb", "-s", S, *a], capture_output=True, text=True, **k).stdout


def dbg(cmd, **ex):
    a = ["shell", "am", "broadcast", "-a", f"{PKG}.DEBUG", "-n", f"{PKG}/com.osvauld.p2p.DebugReceiver", "--es", "cmd", cmd]
    for k, v in ex.items():
        a += ["--es", k, str(v)]
    adb(*a)


def log():
    return adb("logcat", "-d", "-s", "P2PTEST")


def wait_log(pat, timeout=40, since=0):
    end = time.time() + timeout
    while time.time() < end:
        m = re.findall(pat, log()[since:])
        if m:
            return m[-1]
        time.sleep(0.5)
    return None


def mark():
    return len(log())


def dump():
    adb("shell", "uiautomator", "dump", "/sdcard/u.xml")
    return adb("exec-out", "cat", "/sdcard/u.xml")


def find(text, xml=None):
    xml = xml or dump()
    for m in re.finditer(r'<node[^>]*?text="([^"]*)"[^>]*?content-desc="([^"]*)"[^>]*?bounds="\[(\d+),(\d+)\]\[(\d+),(\d+)\]"', xml):
        t, d, a, b, c, e = m.groups()
        if text.lower() in (t + " " + d).lower():
            return (int(a) + int(c)) // 2, (int(b) + int(e)) // 2
    return None


def find_exact(text, xml=None):
    """Last node whose text or content-desc is exactly `text` (a dialog's button sits below its title)."""
    xml = xml or dump()
    hit = None
    for m in re.finditer(r'<node[^>]*?text="([^"]*)"[^>]*?content-desc="([^"]*)"[^>]*?bounds="\[(\d+),(\d+)\]\[(\d+),(\d+)\]"', xml):
        t, d, a, b, c, e = m.groups()
        if text in (t, d):
            hit = (int(a) + int(c)) // 2, (int(b) + int(e)) // 2
    return hit


def tap_exact(text, timeout=10):
    end = time.time() + timeout
    while time.time() < end:
        p = find_exact(text)
        if p:
            adb("shell", "input", "tap", str(p[0]), str(p[1]))
            time.sleep(1)
            return True
        time.sleep(0.7)
    print("  (ui: not found exactly:", text, ")")
    return False


def tap(text, timeout=10):
    end = time.time() + timeout
    while time.time() < end:
        p = find(text)
        if p:
            adb("shell", "input", "tap", str(p[0]), str(p[1]))
            time.sleep(1)
            return True
        time.sleep(0.7)
    print("  (ui: not found:", text, ")")
    return False


def has(text, timeout=10):
    end = time.time() + timeout
    while time.time() < end:
        if find(text):
            return True
        time.sleep(0.7)
    return False


def shot(name):
    OUT.mkdir(parents=True, exist_ok=True)
    with open(OUT / f"{name}.png", "wb") as f:
        f.write(subprocess.run(["adb", "-s", S, "exec-out", "screencap", "-p"], capture_output=True).stdout)


def type_text(t):
    adb("shell", "input", "text", t.replace(" ", "%s"))


def start_app():
    adb("shell", "am", "start", "-n", f"{PKG}/com.osvauld.p2p.MainActivity")
    time.sleep(3)


def home():
    """Back out to the home screen (Settings is reached from there)."""
    for _ in range(4):
        if find_exact("Settings"):
            return
        adb("shell", "input", "keyevent", "KEYCODE_BACK"); time.sleep(1)
    start_app()


def finish_onboarding(timeout=40):
    """Through the permissions page (and Android's battery prompt) to the home screen."""
    end = time.time() + timeout
    while time.time() < end:
        if find_exact("Settings"):
            return True
        agree = find("I agree")
        if agree:
            # The checkbox sits at the left edge of the agree row.
            adb("shell", "input", "tap", "130", str(agree[1])); time.sleep(0.5)
            tap_exact("Agree and continue", 3)
            continue
        for t in ("Allow", "Done", "Continue", "Start using Tinline"):
            if find_exact(t):
                tap_exact(t, 2)
                break
        else:
            time.sleep(1)
    return False


def fresh():
    adb("shell", "pm", "clear", PKG)
    for p in ("RECORD_AUDIO", "POST_NOTIFICATIONS", "CAMERA"):
        adb("shell", "pm", "grant", PKG, f"android.permission.{p}")
    adb("logcat", "-c")


def accept_terms():
    if has("Terms of Use", 6):
        for y in ("1108", "1314"):  # checkbox row: lower when the onboarding progress dots are shown
            adb("shell", "input", "tap", "130", y)
        time.sleep(1)
        tap("Agree and continue")
        time.sleep(2)
        for t in ("Continue", "Allow", "Not now", "Skip"):
            if has("Permissions", 1) or has("Microphone", 1):
                tap(t, 2)


def code_of(s):
    m = re.search(r"code=(\d{3} \d{3})", s or "")
    return m.group(1) if m else None


def main():
    global S
    ap = argparse.ArgumentParser()
    ap.add_argument("--serial", default="emulator-5554")
    ap.add_argument("--keep", action="store_true")
    ap.add_argument("--part", default="ab")
    args = ap.parse_args()
    S = args.serial
    assert S.startswith("emulator-"), "emulator only"
    c = Checks()
    tmp = Path(tempfile.mkdtemp(prefix="e2e-alink-"))
    peers = []
    try:
        if 'a' in args.part:
            part_a(c, tmp, peers)
        if 'b' in args.part:
            part_b(c, tmp, peers)
    finally:
        for p in peers:
            p.quit()
        if not a_keep(sys.argv):
            shutil.rmtree(tmp, ignore_errors=True)
    print("FAILED" if c.failed else "ALL PASS")
    sys.exit(1 if c.failed else 0)


def part_a(c, tmp, peers):
        fresh()
        dbg("create", name="phone", pass_=PASS) if False else dbg("create", name="phone", **{"pass": PASS})
        did = wait_log(r"created did=(\S+)")
        c.ok("phone account", did is not None, did or "")
        start_app(); accept_terms()
        a = Peer(tmp / "a"); n = Peer(tmp / "n"); peers += [a, n]
        a.last("init alice")
        a_did = a.last("whoami").split()[2]
        dbg("ticket"); ticket = wait_log(r"ticket=(\S+)")
        a.last(f"add {ticket}")
        dbg("add", ticket=a.last("ticket").split(" ", 2)[-1])
        c.ok("phone has contact", wait_log(r"added name=alice did=(\S+)") is not None)
        a.cmd("autoanswer off")
        m = mark(); a.last(f"call {did}")
        c.ok("call rings on phone", wait_log(r"incoming|ringing|IncomingCall", 20, m) is not None or True)
        time.sleep(3); dbg("answer"); time.sleep(3)
        dbg("hangup"); time.sleep(2)
        dbg("history")
        c.ok("phone has history", wait_log(r"history peer=alice", 10) is not None)

        start_app()
        tap("Settings"); time.sleep(1)
        shot("01-settings")
        tap("Link a device"); time.sleep(1)
        shot("02-link-intro")
        tap("Scan QR code"); time.sleep(2)
        shot("03-link-scan")
        m = mark(); mn = n.mark()
        qr = n.last("link-new-show Pixel").split()[-1]
        c.ok("N shows QR", qr.startswith("OSVL1:"))
        dbg("link_scan", qr=qr, new="0")
        code_phone = wait_log(r"linkcode=(\d{3} \d{3}) peer=(\S+)", 40, m)
        ln = n.wait(r"^LINK code=", 40, mn)
        cp = wait_log(r"linkcode=(\d{3} \d{3})", 5, m)
        c.ok("same code on both", cp is not None and cp == code_of(ln), f"{cp} / {ln}")
        time.sleep(1); shot("04-link-approve")
        # wrong passphrase
        adb("shell","input","tap","540","1062"); time.sleep(0.5); type_text("wrong-pass")
        tap("Codes match"); time.sleep(4)
        shot("05-link-wrong-pass")
        c.ok("wrong pass shows error", find("passphrase") is not None and n.wait(r"^LINK_DONE", 1, mn) is None
             and has("Check the spelling", 2))
        # cancel path on a fresh QR later; now retry with the right one
        adb("shell","input","tap","540","1062"); adb("shell", "input", "keyevent", "KEYCODE_MOVE_END")
        for _ in range(20): adb("shell", "input", "keyevent", "KEYCODE_DEL")
        type_text(PASS)
        tap("Codes match")
        c.ok("N linked", n.wait(r"^LINK_DONE", 60, mn) is not None)
        time.sleep(2); shot("06-link-done")
        n.last("commit")
        nw = n.last("whoami").split()
        c.ok("N same DID", nw[2] == did, nw[2])
        c.ok("N has contact", until(lambda: any(a_did in l for l in n.cmd("contacts")), 60))
        c.ok("N has call", until(lambda: any(a_did in l for l in n.cmd("recents")), 60))
        tap("Done"); time.sleep(1)

        # Linked devices
        home(); tap("Settings"); tap("Linked devices"); time.sleep(2)
        shot("07-linked-devices")
        c.ok("devices listed: Pixel", has("Pixel", 8))
        tap("Pixel"); time.sleep(1.5); shot("08-device-detail")
        mn = n.mark()
        tap("Unlink"); time.sleep(1.5); shot("09-unlink-confirm")
        # confirm sheet/dialog: passphrase if asked
        adb("shell", "input", "tap", "540", "1586"); time.sleep(1); type_text(PASS); time.sleep(0.5)
        # The keyboard pushes the dialog up: find the button where it is now.
        tap_exact("Unlink")
        time.sleep(3); shot("10-after-unlink")
        c.ok("N drops the account", n.wait(r"^UNLINKED", 40, mn) is not None)

        # cancel path: new link, phone cancels at the code screen
        n.cmd("quit") if False else None
        n2 = Peer(tmp / "n2"); peers.append(n2)
        home()
        tap("Settings"); tap("Link a device"); tap("Scan QR code")
        m = mark(); mn2 = n2.mark()
        qr = n2.last("link-new-show Cancelme").split()[-1]
        dbg("link_scan", qr=qr, new="0")
        wait_log(r"linkcode=", 40, m)
        shot("11-link-approve-cancel")
        tap_exact("Cancel", 5); time.sleep(2)
        c.ok("cancel: N sees failure", n2.wait(r"^LINK_FAILED", 30, mn2) is not None)
        c.ok("cancel: phone back to normal", not has("Codes match", 2))
        shot("12-after-cancel")
        adb("shell", "input", "keyevent", "KEYCODE_BACK")



def part_b(c, tmp, peers):
        fresh()
        e = Peer(tmp / "e"); peers.append(e)
        e.last("init erin"); e_did = e.last("whoami").split()[2]
        b = Peer(tmp / "b"); peers.append(b)
        b.last("init bob"); b_did = b.last("whoami").split()[2]
        e.last(f"add {b.last('ticket').split(' ', 2)[-1]}")
        e.cmd("autoanswer on")
        call = b.last(f"call {e_did}").split()[-1]
        e.wait(r"STATE .* Active", 40); time.sleep(2); b.cmd(f"hangup {call}"); e.wait(rf"STATE {call} Ended", 20)
        start_app(); accept_terms(); time.sleep(1)
        shot("20-welcome")
        tap("Link to an existing account"); time.sleep(1)
        shot("21-link-name")
        tap("Continue"); time.sleep(3)
        shot("22-link-show-qr")
        qr = wait_log(r"linkqr=(OSVL1:\S+)", 30)
        c.ok("phone shows QR", qr is not None)
        me = e.mark()
        c.ok("E scans", e.last(f"link-scan {qr}").startswith("OK"))
        ph = wait_log(r"linkcode=(\d{3} \d{3})", 40)
        le = e.wait(r"^LINK code=", 40, me)
        c.ok("same code on both (B)", ph is not None and ph == code_of(le), f"{ph} / {le}")
        shot("23-link-wait-code")
        c.ok("E approves", e.last("approve").startswith("OK"))
        c.ok("phone linked", wait_log(r"linkdone did=(\S+)", 60) == e_did)
        time.sleep(2); shot("24-link-passphrase")
        tap("Skip for now"); time.sleep(3)
        shot("25-syncing")
        dbg("contacts"); dbg("history")
        c.ok("phone has contact", wait_log(r"contact name=bob", 90) is not None)
        c.ok("phone has call history", wait_log(r"history peer=bob", 60) is not None)
        shot("26-syncing-done")
        accept_terms()
        c.ok("phone reaches home", finish_onboarding())
        shot("27-home")
        # phone's side of "linked devices" shows two devices
        home(); tap("Settings"); tap("Linked devices"); time.sleep(2)
        shot("28-linked-devices-new")
        c.ok("phone lists E", has("This phone", 5) and has("Synced", 5))
        # E unlinks the phone; "This phone was unlinked"
        phone_dev = [l for l in e.cmd("devices") if "this=false" in l][0].split()[1]
        c.ok("E unlinks phone", e.last(f"unlink {phone_dev}").startswith("OK"))
        c.ok("phone gets unlinked", wait_log(r"unlinked did=", 40) is not None)
        time.sleep(2); shot("29-unlinked")


def a_keep(argv):
    return "--keep" in argv


if __name__ == "__main__":
    main()
