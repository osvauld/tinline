#!/usr/bin/env python3
"""Link a phone to a desktop account and prove contacts, chats and calls arrive on both (34g).
WIPES the phone app. The desktop runs from its own data dir (WORK/desk), not the user's profile.

    scripts/e2e_link_desktop_phone.py setup --serial ZD22257P6G   # desktop account + phone linked
    target/release/p2p-desktop --data WORK/desk                     # (test-hooks build, P2P_PASSPHRASE)
    scripts/e2e_link_desktop_phone.py after --serial ZD22257P6G   # new messages both ways

setup: account "Abe" (desk) gets contacts bob and carol, a chat with bob (both directions) and a
       missed call from carol; then the phone (fresh) links to it as a new device through the
       onboarding "Link to an existing account" path, and must show all of it.
after: bob sends to Abe, the phone sends to bob: both must reach phone and desktop.
Screenshots of the phone go to artifacts/link-desk-phone/.
"""
import argparse, re, shutil, subprocess, sys, time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import e2e_android_link as ui  # noqa: E402
from e2e_link import Peer, Checks, until, ROOT  # noqa: E402

WORK = Path("/tmp/claude-1000/link-desk-phone")
ui.OUT = ROOT / "artifacts/link-desk-phone"


def phone(cmd, pat, timeout=20, **ex):
    m = ui.mark()
    ui.dbg(cmd, **ex)
    return ui.wait_log(pat, timeout, m)


def phone_lines(cmd, pat, done=None, **ex):
    m = ui.mark()
    ui.dbg(cmd, **ex)
    time.sleep(3)
    return re.findall(pat, ui.log()[m:])


def open_chat(name, shot):
    ui.start_app()
    ui.tap_exact("Chats", 3)
    ui.tap_exact(name, 8)
    time.sleep(2)
    ui.shot(shot)
    ui.adb("shell", "input", "keyevent", "KEYCODE_BACK")


def setup(c):
    shutil.rmtree(WORK, ignore_errors=True)
    WORK.mkdir(parents=True)
    d, bob, carol = Peer(WORK / "desk"), Peer(WORK / "bob"), Peer(WORK / "carol")
    try:
        d.last("init Abe"); bob.last("init bob"); carol.last("init carol")
        abe = d.last("whoami").split()[2]
        for p in (bob, carol):
            p.last("add " + d.last("ticket").split(" ", 2)[-1])
            d.last("add " + p.last("ticket").split(" ", 2)[-1])
        c.ok("desk has bob and carol", until(lambda: sum(n in " ".join(d.cmd("contacts")) for n in ("bob", "carol")) == 2, 30))
        d.last("send bob hi-bob-from-desktop-before-link")
        bob.last(f"send {abe} hey-abe-from-bob-before-link")
        both = ("hi-bob-from-desktop-before-link", "hey-abe-from-bob-before-link")
        c.ok("desk chat has both messages", until(lambda: all(t in " ".join(d.cmd("msgs bob")) for t in both), 40))
        c.ok("bob has both messages", until(lambda: all(t in " ".join(bob.cmd(f"msgs {abe}")) for t in both), 40))
        cid = carol.last(f"call {abe}").split()[-1]
        time.sleep(5)
        carol.last(f"hangup {cid}")
        c.ok("desk has carol's call", until(lambda: any(cid in l for l in d.cmd("recents")), 30))

        # --- the phone links to the desk as a new device ---------------------------------
        ui.fresh()
        ui.start_app(); ui.accept_terms(); time.sleep(1)
        ui.shot("01-welcome")
        ui.tap("Link to an existing account"); time.sleep(1)
        ui.tap("Continue"); time.sleep(3)
        ui.shot("02-phone-shows-qr")
        qr = ui.wait_log(r"linkqr=(OSVL1:\S+)", 30)
        c.ok("phone shows QR", qr is not None)
        me = d.mark()
        c.ok("desk scans", d.last(f"link-scan {qr}").startswith("OK"))
        ph = ui.wait_log(r"linkcode=(\d{3} \d{3})", 40)
        c.ok("same code on both", ph is not None and ph in (d.wait(r"^LINK code=", 40, me) or ""), ph or "")
        c.ok("desk approves", d.last("approve").startswith("OK"))
        c.ok("phone linked to Abe", ui.wait_log(r"linkdone did=(\S+)", 60) == abe)
        time.sleep(2); ui.tap("Skip for now"); time.sleep(3)
        ui.accept_terms()
        c.ok("phone reaches home", ui.finish_onboarding())
        ui.shot("03-home")
        c.ok("phone: contacts bob + carol", until(lambda: {"bob", "carol"} <= set(phone_lines("contacts", r"contact name=(\S+)")), 60))
        c.ok("phone: carol's call", until(lambda: phone("history", r"(history peer=carol)", 5), 60) is not None)
        c.ok("phone: chat with bob from before the link", until(lambda: {"hi-bob-from-desktop-before-link", "hey-abe-from-bob-before-link"}
                                                               <= set(phone_lines("chat_msgs", r"msg id=\S+ out=\S+ delivery=\S+ text=(\S+)", who="bob")), 90))
        open_chat("bob", "04-phone-chat-bob")
        ui.start_app(); ui.tap_exact("Calls", 3); time.sleep(2); ui.shot("05-phone-calls")
        ui.start_app(); ui.tap_exact("Contacts", 3); time.sleep(2); ui.shot("06-phone-contacts")
        print("abe", abe)
    finally:
        for p in (d, bob, carol):
            p.quit()


def after(c):
    bob = Peer(WORK / "bob")
    try:
        abe = [l for l in bob.cmd("contacts") if "Abe" in l][0]
        abe = re.search(r"did:key:\S+", abe).group(0)
        bob.last(f"send {abe} after-link-from-bob")
        phone("chat_send", r"chat_sent id=(\S+)", who="bob", text="after-link-from-phone")
        c.ok("phone: has bob's new message", until(lambda: "after-link-from-bob" in phone_lines("chat_msgs", r"text=(\S+)", who="bob"), 60))
        c.ok("bob got the phone's message", until(lambda: "after-link-from-phone" in " ".join(bob.cmd(f"msgs {abe}")), 60))
        c.ok("phone: two ticks", until(lambda: "DELIVERED" in phone_lines("chat_msgs", r"delivery=(\S+) text=after-link-from-phone", who="bob"), 60))
        open_chat("bob", "07-phone-chat-bob-after")
    finally:
        bob.quit()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("phase", choices=["setup", "after"])
    ap.add_argument("--serial", required=True)
    a = ap.parse_args()
    ui.S = a.serial
    c = Checks()
    (setup if a.phase == "setup" else after)(c)
    print(f"{'ALL PASS' if not c.failed else f'{c.failed} FAILED'}; phone screenshots in {ui.OUT}; data in {WORK}")
    sys.exit(1 if c.failed else 0)


if __name__ == "__main__":
    main()
