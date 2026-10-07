#!/usr/bin/env python3
"""Measure idle network activity (bursts ~ radio wakeups) of com.osvauld.p2p on an emulator/device.

Usage: scripts/measure_idle.py --serial emulator-5554 --minutes 30 [--doze] [--force-stop]

Per-UID counters: what works where (found on the Android 15 google_apis userdebug emulator):
  * /proc/net/xt_qtaguid/stats and /proc/uid_stat do not exist (removed since Android 9/10).
  * `dumpsys netstats detail|--uid` works but only has 1-2 h buckets: totals, not a time series.
  * the eBPF maps (/sys/fs/bpf/netd_shared) are root-only and need a bpf() tool; none on image.
  * WORKS (needs `adb root`, i.e. userdebug/emulator): an iptables/ip6tables `-m owner --uid-owner`
    counting rule in a private chain hooked into OUTPUT gives exact tx packets/bytes for the UID
    (all protocols); rx is counted with INPUT rules on the local UDP ports owned by the UID
    (/proc/net/udp{,6}, refreshed every 15 s) plus `ss -tie` per-socket segs_in/bytes_received for
    the UID's TCP sockets (relay connection). Sampled once per second.
On a production phone without root only the dumpsys netstats totals are available.
"""
import argparse, collections, json, re, statistics, subprocess, sys, threading, time

PKG = "com.osvauld.p2p"
ADB = ["adb"]


def adb(*a, check=False, timeout=60):
    r = subprocess.run([*ADB, *a], capture_output=True, text=True, timeout=timeout)
    return r.stdout + (r.stderr if not r.stdout else "")


def sh(cmd, **k):
    return adb("shell", cmd, **k)


def app_uid():
    m = re.search(rf"package:{re.escape(PKG)} uid:(\d+)", sh(f"pm list packages -U {PKG}"))
    if not m:
        sys.exit(f"{PKG} not installed")
    return int(m.group(1))


def ensure_fg_service():
    sh(f"am start -n {PKG}/.MainActivity")
    time.sleep(4)
    sh("input keyevent KEYCODE_HOME")
    time.sleep(1)
    return "isForeground=true" in sh(f"dumpsys activity services {PKG}")


def ipt_setup(uid):
    for t in ("iptables", "ip6tables"):
        for ch in ("P2POUT", "P2PIN"):
            sh(f"{t} -w -N {ch} 2>/dev/null; {t} -w -F {ch}")
        sh(f"{t} -w -A P2POUT -m owner --uid-owner {uid}")
        sh(f"{t} -w -I OUTPUT 1 -j P2POUT; {t} -w -I INPUT 1 -j P2PIN")


def ipt_teardown():
    for t in ("iptables", "ip6tables"):
        sh(f"{t} -w -D OUTPUT -j P2POUT; {t} -w -D INPUT -j P2PIN; "
           f"{t} -w -F P2POUT; {t} -w -X P2POUT; {t} -w -F P2PIN; {t} -w -X P2PIN")


def udp_ports(uid):
    ports = set()
    for f in ("udp", "udp6"):
        for line in sh(f"cat /proc/net/{f}").splitlines()[1:]:
            p = line.split()
            if len(p) > 7 and p[7] == str(uid):
                ports.add(int(p[1].split(":")[1], 16))
    return ports


def ipt_add_ports(ports, known):
    for p in sorted(ports - known):
        for t in ("iptables", "ip6tables"):
            sh(f"{t} -w -A P2PIN -p udp --dport {p}")
        known.add(p)


SAMPLE_CMD = ("echo =O4; iptables -w -nvxL P2POUT; echo =O6; ip6tables -w -nvxL P2POUT; "
              "echo =I4; iptables -w -nvxL P2PIN; echo =I6; ip6tables -w -nvxL P2PIN; "
              "echo =SS; ss -tie; echo =END")


def parse_rules(txt, uid=None):
    """sum pkts/bytes of rule lines (skip headers)"""
    pk = by = 0
    for line in txt.splitlines():
        p = line.split()
        if len(p) >= 2 and p[0].isdigit() and p[1].isdigit():
            pk += int(p[0]); by += int(p[1])
    return pk, by


def sample(uid):
    out = sh(SAMPLE_CMD)
    secs = dict(re.findall(r"=(O4|O6|I4|I6|SS|END)\n(.*?)(?=\n=(?:O4|O6|I4|I6|SS|END)\n|\Z)", out + "\n", re.S))
    txp = txb = rxp = rxb = 0
    for k in ("O4", "O6"):
        a, b = parse_rules(secs.get(k, "")); txp += a; txb += b
    for k in ("I4", "I6"):
        a, b = parse_rules(secs.get(k, "")); rxp += a; rxb += b
    tcp = {}
    lines = secs.get("SS", "").splitlines()
    for i, l in enumerate(lines):
        if f"uid:{uid} " in l and i + 1 < len(lines):
            sk = re.search(r"sk:(\w+)", l)
            d = lines[i + 1]
            si = re.search(r"segs_in:(\d+)", d); br = re.search(r"bytes_received:(\d+)", d)
            if sk and si and br:
                tcp[sk.group(1)] = (int(si.group(1)), int(br.group(1)))
    return dict(t=time.time(), txp=txp, txb=txb, urxp=rxp, urxb=rxb, tcp=tcp)


def deltas(samples):
    """per-interval (t, pkts, bytes, tx_pkts) from consecutive samples."""
    res = []
    for a, b in zip(samples, samples[1:]):
        tcp_p = tcp_b = 0
        for sk, (sp, sb) in b["tcp"].items():
            op, ob = a["tcp"].get(sk, (sp, sb))  # new socket: no delta
            tcp_p += max(0, sp - op); tcp_b += max(0, sb - ob)
        rxp = max(0, b["urxp"] - a["urxp"]) + tcp_p
        rxb = max(0, b["urxb"] - a["urxb"]) + tcp_b
        txp = max(0, b["txp"] - a["txp"]); txb = max(0, b["txb"] - a["txb"])
        res.append(dict(t=b["t"], pk=txp + rxp, by=txb + rxb, txp=txp, rxp=rxp, txb=txb, rxb=rxb))
    return res


def bursts(dl, gap=2.0):
    """merge active intervals separated by <= gap seconds idle."""
    out = []
    for d in dl:
        if d["pk"] <= 0:
            continue
        if out and d["t"] - out[-1]["end"] <= gap + 1.0:  # +1: sampling interval
            out[-1]["end"] = d["t"]; out[-1]["pk"] += d["pk"]; out[-1]["by"] += d["by"]
        else:
            out.append(dict(start=d["t"], end=d["t"], pk=d["pk"], by=d["by"]))
    return out


def summarize(samples, gap):
    dl = deltas(samples)
    dur = samples[-1]["t"] - samples[0]["t"]
    bs = bursts(dl, gap)
    starts = [b["start"] for b in bs]
    gaps = [b - a for a, b in zip(starts, starts[1:])]
    pk = sum(d["pk"] for d in dl); by = sum(d["by"] for d in dl)
    h = 3600 / dur
    s = dict(duration_s=round(dur), samples=len(samples), bursts=len(bs), bursts_per_h=round(len(bs) * h, 1),
             packets_per_h=round(pk * h), bytes_per_h=round(by * h),
             tx_pk=sum(d["txp"] for d in dl), rx_pk=sum(d["rxp"] for d in dl),
             tx_b=sum(d["txb"] for d in dl), rx_b=sum(d["rxb"] for d in dl),
             median_gap_s=round(statistics.median(gaps), 1) if gaps else None)
    c = collections.Counter(round(g) for g in gaps)
    s["gap_modes_s"] = c.most_common(8)
    s["median_burst_pk"] = statistics.median([b["pk"] for b in bs]) if bs else None
    return s, bs


def logcat_reader(proc, lines):
    for l in proc.stdout:
        lines.append(l.rstrip("\n"))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--serial", default="emulator-5554")
    ap.add_argument("--minutes", type=float, default=30)
    ap.add_argument("--interval", type=float, default=1.0)
    ap.add_argument("--doze", action="store_true")
    ap.add_argument("--force-stop", action="store_true", help="baseline: app not running")
    ap.add_argument("--gap", type=float, default=2.0)
    ap.add_argument("--out", default=None, help="write json (samples/bursts/log) here")
    a = ap.parse_args()
    ADB.extend(["-s", a.serial])
    adb("root"); time.sleep(2); adb("wait-for-device")
    uid = app_uid()
    print("uid", uid, flush=True)
    sh("input keyevent KEYCODE_WAKEUP")
    if a.force_stop:
        sh(f"am force-stop {PKG}")
        time.sleep(2)
    elif not ensure_fg_service():
        sys.exit("CoreService is not foreground")
    print("pids:", sh(f"pidof {PKG}").strip(), flush=True)
    adb("logcat", "-c")
    lc = subprocess.Popen([*ADB, "logcat", "-v", "epoch,uid"], stdout=subprocess.PIPE, text=True, errors="replace")
    loglines = []
    threading.Thread(target=logcat_reader, args=(lc, loglines), daemon=True).start()
    samples = []
    known = set()
    try:
        sh("input keyevent KEYCODE_SLEEP")
        sh("dumpsys battery unplug")
        sh("dumpsys batterystats --reset")
        if a.doze:
            print(sh("dumpsys deviceidle force-idle").strip(), flush=True)
        ipt_setup(uid)
        ipt_add_ports(udp_ports(uid), known)
        end = time.time() + a.minutes * 60
        last_ports = time.time()
        nxt = time.time()
        while time.time() < end:
            if time.time() - last_ports > 15:
                ipt_add_ports(udp_ports(uid), known); last_ports = time.time()
            samples.append(sample(uid))
            nxt += a.interval
            time.sleep(max(0, nxt - time.time()))
        stats = sh(f"dumpsys batterystats --charged {PKG}")
        print("=== batterystats (filtered) ===")
        for l in stats.splitlines():
            if re.search(r"Wake|wake|packets|Mobile|radio|Wifi|Network|Uid u0a|Foreground|Cpu|Idle", l):
                print(l.strip()[:200])
    finally:
        ipt_teardown()
        sh("dumpsys battery reset")
        sh("dumpsys deviceidle unforce")
        sh("input keyevent KEYCODE_WAKEUP")
        lc.terminate()
    s, bs = summarize(samples, a.gap)
    print("=== summary ===")
    print(json.dumps(s))
    print("udp ports seen:", sorted(known))
    # logcat correlation
    mine = [l for l in loglines if re.search(rf"\b(u0_a{uid - 10000}|{uid})\b", l)]
    tags = collections.Counter(re.sub(r"\s+", " ", l).split(" ")[5] if len(l.split()) > 5 else "?" for l in mine)
    print("app log lines:", len(mine), "top tags:", tags.most_common(8))
    for l in mine[:40]:
        print("LOG", l[:200])
    # nearest log line after each of first few bursts
    print("=== log lines near bursts ===")
    parsed = []
    for l in mine:
        m = re.match(r"\s*(\d+\.\d+)", l)
        if m:
            parsed.append((float(m.group(1)), l))
    for b in bs[:15]:
        near = [l for t, l in parsed if b["start"] - 1.5 <= t <= b["end"] + 0.5]
        print(f"burst @{b['start'] - samples[0]['t']:.0f}s pk={b['pk']} by={b['by']}: {len(near)} log lines")
        for l in near[:3]:
            print("   ", l[:180])
    print("burst start offsets (s):", [round(b["start"] - samples[0]["t"]) for b in bs[:80]])
    if a.out:
        json.dump(dict(summary=s, bursts=bs, t0=samples[0]["t"], log=mine), open(a.out, "w"))


if __name__ == "__main__":
    main()
