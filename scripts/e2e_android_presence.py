#!/usr/bin/env python3
"""Presence and honest ticks on a real phone (34g): WIPES the app's data on the phone.

    scripts/e2e_android_presence.py --serial ZD22257P6G [--keep]

Identities:
  X  = the phone (fresh account) + headless peer X2 linked to it (our "other device")
  Y  = headless peer "bob", the contact; quit cleanly, killed hard, and brought back

Checks, phone UI checked from the conversation header ("Connected · direct" / "Not connected"):
  P1 Y online -> phone, X2 see it online; Y sees the phone online
  P2 Y quits cleanly -> offline on the phone and X2 (time printed)
  P3 Y killed with SIGKILL (no goodbye) -> offline within the presence timeout (time printed)
  T1 Y away: the phone sends -> one tick on the phone AND on X2
  T2 Y away: X2 sends -> one tick on X2 AND on the phone
  T3 Y back -> Y has both, two ticks everywhere
  C1 Y calls the phone after linking -> the call is in X2's call log
  L0/L1 contact carol + her call on the phone BEFORE linking -> both reach X2 when linked
  L2 X2 renames carol -> the phone shows the alias
  L3 X2 removes carol -> she and her calls are gone on the phone
  L4-L6 dave added on X2 after linking -> on the phone; the phone chats with him; his call on both
  L7 X2 renames itself -> the phone shows the new name
Screenshots go to artifacts/presence-phone/. Needs the debug APK installed and target/release/p2p-peer.
"""
import argparse, re, shutil, sys, tempfile, time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import e2e_android_link as ui  # noqa: E402
from e2e_link import Peer, Checks, until, ROOT  # noqa: E402

PASS = ui.PASS
ui.OUT = ROOT / "artifacts/presence-phone"


def phone(cmd, pat, timeout=20, **ex):
    """Runs a debug command on the phone, returns the first log match after it (group 1 if any)."""
    m = ui.mark()
    ui.dbg(cmd, **ex)
    return ui.wait_log(pat, timeout, m)


def phone_online(who):
    return phone("online", r"online=(true|false)", who=who) == "true"


def phone_msgs(who):
    """{text: delivery} for the conversation with `who` on the phone."""
    m = ui.mark()
    ui.dbg("chat_msgs", who=who)
    ui.wait_log(r"chat_msgs done", 20, m)
    return {t: d for d, t in re.findall(r"msg id=\S+ out=\S+ delivery=(\S+) text=(.*)", ui.log()[m:])}


def peer_online(p, who):
    return p.last(f"online {who}") == "OK ONLINE true"


def peer_msgs(p, who):
    out = {}
    for l in p.cmd(f"msgs {who}"):
        m = re.match(r'MSG \S+ out=\S+ delivery=(\S+) text="(.*)" att=', l)
        if m:
            out[m.group(2)] = m.group(1)
    return out


def timed(fn, timeout):
    t0 = time.time()
    ok = until(fn, timeout, 1)
    return bool(ok), time.time() - t0


def header(want, name):
    """Opens the conversation with bob on the phone and checks the header text."""
    ui.start_app()
    ui.tap_exact("bob", 8)
    time.sleep(2)
    got = ui.has(want, 15) if want != "Not connected" else until(lambda: ui.find_exact("Not connected"), 45, 1)
    ui.shot(name)
    ui.adb("shell", "input", "keyevent", "KEYCODE_BACK")
    return bool(got)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--serial", required=True)
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()
    ui.S = a.serial
    c = Checks()
    tmp = Path(tempfile.mkdtemp(prefix="e2e-presence-"))
    peers = []
    try:
        # --- identities -------------------------------------------------------------------
        ui.fresh()
        did = phone("create", r"created did=(\S+)", 30, name="me", **{"pass": PASS})
        c.ok("phone account", did is not None, did or "")
        ui.start_app(); ui.accept_terms(); ui.finish_onboarding()

        # --- data from before the link: contact carol and a call from her -------------------
        z = Peer(tmp / "z"); peers.append(z)
        z.last("init carol")
        carol = z.last("whoami").split()[2]
        z.last("add " + phone("ticket", r"ticket=(\S+)"))
        c.ok("L0 phone added carol", phone("add", r"added name=(\S+)", ticket=z.last("ticket").split(" ", 2)[-1]) == "carol")
        pre_call = z.last(f"call {did}").split()[-1]
        time.sleep(5)
        z.last(f"hangup {pre_call}")
        c.ok("L0 phone has carol's call", until(lambda: phone("history", r"(history peer=carol)", 5), 30) is not None)

        x2 = Peer(tmp / "x2"); peers.append(x2)
        mx = x2.mark()
        qr = x2.last("link-new-show Laptop").split()[-1]
        code = phone("link_scan", r"linkcode=(\d{3} \d{3})", 40, qr=qr, new="0")
        c.ok("link code on phone", code is not None, code or "")
        ui.dbg("link_approve", **{"pass": PASS})
        c.ok("X2 linked", x2.wait(r"^LINK_DONE", 60, mx) is not None)
        x2.last("commit")
        c.ok("X2 same DID", x2.last("whoami").split()[2] == did)
        c.ok("L1 X2 got carol from before the link", until(lambda: any(carol in l for l in x2.cmd("contacts")), 60))
        c.ok("L1 X2 got carol's call from before the link", until(lambda: any(l.startswith(f"REC {pre_call}") for l in x2.cmd("recents")), 60))
        ui.start_app()

        y = Peer(tmp / "y"); peers.append(y)
        y.last("init bob")
        ticket = phone("ticket", r"ticket=(\S+)")
        y.last(f"add {ticket}")
        c.ok("phone added bob", phone("add", r"added name=(\S+)", ticket=y.last("ticket").split(" ", 2)[-1]) == "bob")
        c.ok("X2 has bob", until(lambda: any("bob" in l for l in x2.cmd("contacts")), 60))

        # --- contact edits from X2 reach the phone ------------------------------------------
        x2.last(f"alias {carol} Caroline")
        c.ok("L2 X2's rename reaches the phone", until(lambda: phone("contacts", rf"did={carol} alias=(\S+)", 5) == "Caroline", 60))
        x2.last(f"remove {carol}")
        c.ok("L3 X2's removal reaches the phone", until(lambda: phone("contacts", r"(contact name=bob)", 5) and not phone("contacts", rf"(did={carol})", 3), 60))
        c.ok("L3 carol's calls gone on the phone", until(lambda: not phone("history", r"(history peer=(?:carol|Caroline))", 3), 60))
        z.quit(); peers.remove(z)

        # --- a contact added on X2 after the link: on the phone, chat and calls from there --
        dv = Peer(tmp / "dave"); peers.append(dv)
        dv.last("init dave")
        dave = dv.last("whoami").split()[2]
        x2.last("add " + dv.last("ticket").split(" ", 2)[-1])
        c.ok("L4 dave (added on X2) is on the phone", until(lambda: phone("contacts", rf"(did={dave})", 5), 60) is not None)
        c.ok("L4 dave has us", until(lambda: any(did in l for l in dv.cmd("contacts")), 60))
        phone("chat_send", r"chat_sent id=(\S+)", who="dave", text="hi-dave-from-phone")
        c.ok("L5 dave got the phone's message", until(lambda: "hi-dave-from-phone" in peer_msgs(dv, did), 60))
        dv.last(f"send {did} hi-back-from-dave")
        c.ok("L5 the phone has dave's reply", until(lambda: "hi-back-from-dave" in phone_msgs("dave"), 60))
        c.ok("L5 X2 has both", until(lambda: {"hi-dave-from-phone", "hi-back-from-dave"} <= set(peer_msgs(x2, dave)), 60))
        dcall = dv.last(f"call {did}").split()[-1]
        time.sleep(5)
        dv.last(f"hangup {dcall}")
        c.ok("L6 dave's call on the phone", until(lambda: phone("history", r"(history peer=dave)", 5), 60) is not None)
        c.ok("L6 dave's call on X2", until(lambda: any(l.startswith(f"REC {dcall}") for l in x2.cmd("recents")), 60))
        x2.last("label Work-laptop")
        c.ok("L7 X2's new name on the phone", until(lambda: phone("devices", r"(device label=Work-laptop)", 5), 60) is not None)
        dv.quit(); peers.remove(dv)

        # --- presence ---------------------------------------------------------------------
        ok, s = timed(lambda: phone_online("bob"), 60)
        c.ok("P1 phone sees bob online", ok, f"{s:.0f}s")
        ok, s = timed(lambda: peer_online(x2, "bob"), 60)
        c.ok("P1 X2 sees bob online", ok, f"{s:.0f}s")
        ok, s = timed(lambda: peer_online(y, did), 60)
        c.ok("P1 bob sees us online", ok, f"{s:.0f}s")
        ui.dbg("chat_send", who="bob", text="hello bob")  # so bob is in the chat list to tap
        c.ok("P1 header Connected", header("Connected ·", "p1-online"))

        # --- a call after linking reaches X2's call log (calls doc) -----------------------
        cid = y.last(f"call {did}").split()[-1]
        time.sleep(5)
        y.last(f"hangup {cid}")
        ok, s = timed(lambda: phone("history", r"(history peer=bob)", 5) is not None, 30)
        c.ok("C1 phone has bob's call", ok, f"{s:.0f}s")
        ok, s = timed(lambda: any(l.startswith(f"REC {cid}") for l in x2.cmd("recents")), 60)
        c.ok("C1 X2 has bob's call", ok, f"{s:.0f}s")

        y.quit(); peers.remove(y)
        ok, s = timed(lambda: not phone_online("bob"), 60)
        c.ok("P2 clean quit -> phone offline", ok, f"{s:.0f}s")
        ok, s = timed(lambda: not peer_online(x2, "bob"), 60)
        c.ok("P2 clean quit -> X2 offline", ok, f"{s:.0f}s")
        c.ok("P2 header Not connected", header("Not connected", "p2-quit"))

        y = Peer(tmp / "y"); peers.append(y)
        ok, s = timed(lambda: phone_online("bob"), 90)
        c.ok("P1' bob back -> phone online", ok, f"{s:.0f}s")
        y.p.kill(); y.p.wait(); peers.remove(y)
        ok, s = timed(lambda: not phone_online("bob"), 90)
        c.ok("P3 SIGKILL -> phone offline", ok, f"{s:.0f}s")
        ok, s = timed(lambda: not peer_online(x2, "bob"), 90)
        c.ok("P3 SIGKILL -> X2 offline", ok, f"{s:.0f}s")

        # --- ticks while bob is away ------------------------------------------------------
        phone("chat_send", r"chat_sent id=(\S+)", who="bob", text="from-phone-while-away")
        x2.last("send bob from-x2-while-away")
        c.ok("T1/T2 X2 has the phone's message", until(lambda: "from-phone-while-away" in peer_msgs(x2, "bob"), 60))
        c.ok("T1/T2 phone has X2's message", until(lambda: "from-x2-while-away" in phone_msgs("bob"), 60))
        time.sleep(3)
        pm, xm = phone_msgs("bob"), peer_msgs(x2, "bob")
        print("  phone:", pm); print("  X2:   ", xm)
        c.ok("T1 phone-sent: one tick on phone", pm.get("from-phone-while-away") == "PENDING")
        c.ok("T1 phone-sent: one tick on X2", xm.get("from-phone-while-away") == "Pending")
        c.ok("T2 X2-sent: one tick on X2", xm.get("from-x2-while-away") == "Pending")
        c.ok("T2 X2-sent: one tick on phone", pm.get("from-x2-while-away") == "PENDING")
        ui.start_app(); ui.tap_exact("bob", 8); time.sleep(2); ui.shot("t1-away-one-tick")
        ui.adb("shell", "input", "keyevent", "KEYCODE_BACK")

        # --- bob comes back ---------------------------------------------------------------
        y = Peer(tmp / "y"); peers.append(y)
        c.ok("T3 bob got both", until(lambda: {"from-phone-while-away", "from-x2-while-away"} <= set(peer_msgs(y, did)), 90))
        c.ok("T3 two ticks on phone", until(lambda: all(v == "DELIVERED" for v in phone_msgs("bob").values()), 60), str(phone_msgs("bob")))
        c.ok("T3 two ticks on X2", until(lambda: all(v == "Delivered" for k, v in peer_msgs(x2, "bob").items()), 60), str(peer_msgs(x2, "bob")))
        c.ok("T3 header Connected", header("Connected ·", "t3-back"))
        ui.start_app(); ui.tap_exact("bob", 8); time.sleep(2); ui.shot("t3-two-ticks")
    finally:
        for p in peers:
            p.quit()
        if not a.keep:
            shutil.rmtree(tmp, ignore_errors=True)
        else:
            print("kept", tmp)
    print(f"{'ALL PASS' if not c.failed else f'{c.failed} FAILED'}; screenshots in {ui.OUT}")
    sys.exit(1 if c.failed else 0)


if __name__ == "__main__":
    main()
