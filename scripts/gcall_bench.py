#!/usr/bin/env python3
"""Group-call hosting benchmark (GA-P2): run the gcall-bench `host` on the phone (or desktop)
and N-1 simulated `members` on this machine, sample host CPU / thermal / battery / bytes.

    scripts/gcall_bench.py push  --serial ZD22257P6G --bin <target>/aarch64-linux-android/release/host
    scripts/gcall_bench.py phone --serial ZD22257P6G --n 5 --k 2 --secs 600 --members-bin <...>/members
    scripts/gcall_bench.py desktop --n 8 --k 3 --secs 300 --host-bin <...>/host --members-bin <...>/members
    scripts/gcall_bench.py direct --secs 180 --members-bin <...>/members
    scripts/gcall_bench.py clean --serial ZD22257P6G

Run measurement sessions inside scripts/buildlock.py (--who ga-p2-bench) so no build competes
for CPU. Prints one JSON result line (prefix RESULT) and appends it to --out.
"""
import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import threading
import time
from pathlib import Path

ADB = shutil.which("adb") or str(Path.home() / "Android/Sdk/platform-tools/adb")
REMOTE = "/data/local/tmp/gcall-host"
PORT = 7700
CLK_TCK = 100


def adb(serial, *args, check=True):
    return subprocess.run([ADB, "-s", serial, *args], capture_output=True, text=True, check=check).stdout


def kv(line):
    return {k: (float(v) if re.fullmatch(r"-?[0-9.]+", v) else v) for k, v in re.findall(r"(\w+)=(\S+)", line)}


def proc_stat_total(text):
    f = [int(x) for x in text.split()[1:]]
    idle = f[3] + f[4]
    return sum(f), idle


def pid_ticks(stat):
    rest = stat.rsplit(")", 1)[1].split()
    return int(rest[11]) + int(rest[12])


def wlan_bytes(netdev):
    for line in netdev.splitlines():
        if "wlan0:" in line:
            f = line.split(":", 1)[1].split()
            return int(f[0]), int(f[1]), int(f[8]), int(f[9])  # rx bytes, rx pkts, tx bytes, tx pkts
    return 0, 0, 0, 0


PHONE_SAMPLE = ("cat /proc/{pid}/stat; echo ===; head -1 /proc/stat; echo ===; cat /proc/net/dev; echo ===; "
                "dumpsys thermalservice | grep -E '^Thermal Status'; dumpsys battery | grep -E "
                "'level:|temperature:|USB powered|AC powered|status:'")


def sample_phone(serial, pid):
    out = adb(serial, "shell", PHONE_SAMPLE.format(pid=pid), check=False)
    parts = out.split("===")
    if len(parts) < 4:
        return None
    s = {"t": time.time(), "ticks": pid_ticks(parts[0])}
    s["sys_total"], s["sys_idle"] = proc_stat_total(parts[1])
    s["wlan"] = wlan_bytes(parts[2])
    m = re.search(r"Thermal Status: (\d+)", parts[3])
    s["thermal"] = int(m.group(1)) if m else -1
    m = re.search(r"level: (\d+)", parts[3])
    s["battery"] = int(m.group(1)) if m else -1
    m = re.search(r"temperature: (\d+)", parts[3])
    s["batt_temp"] = int(m.group(1)) / 10 if m else -1
    s["usb"] = "USB powered: true" in parts[3] or "AC powered: true" in parts[3]
    return s


def sample_local(pid):
    s = {"t": time.time(), "ticks": pid_ticks(Path(f"/proc/{pid}/stat").read_text())}
    s["sys_total"], s["sys_idle"] = proc_stat_total(Path("/proc/stat").read_text().splitlines()[0])
    return s


def run_session(a, host_cmd, host_addr, sampler, cores):
    host = subprocess.Popen(host_cmd, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
    ready = host.stdout.readline()
    if not ready.startswith("READY"):
        host.kill()
        sys.exit(f"host failed: {ready!r}")
    pid = int(kv(ready)["pid"])
    stats = []
    threading.Thread(target=lambda: [stats.append(kv(l)) for l in host.stdout if l.startswith("STAT")],
                     daemon=True).start()
    members = subprocess.Popen([a.members_bin, "--host", host_addr, "--members", str(a.n - 1),
                                "--participants", str(a.n), "--secs", str(a.secs), "--label", a.label],
                               stdout=subprocess.PIPE, text=True)
    time.sleep(5)  # connections up
    samples = [sampler(pid)]
    stat0 = stat1 = stats[-1] if stats else None
    while members.poll() is None:
        time.sleep(10)
        s = sampler(pid)
        if s and members.poll() is None:
            samples.append(s)
            stat1 = stats[-1] if stats else None
    out = members.stdout.read()
    summary = next((kv(l) for l in out.splitlines() if l.startswith("SUMMARY")), {})
    return host, pid, samples, stat0, stat1, summary


def analyse(a, samples, stat0, stat1, summary, cores):
    s0, s1 = samples[0], samples[-1]
    dt = s1["t"] - s0["t"]
    r = {"label": a.label, "mode": a.cmd, "n": a.n, "k": a.k, "secs": a.secs, "sample_secs": round(dt),
         "host_args": a.host_args}
    r["host_cpu_pct_of_core"] = round(100 * (s1["ticks"] - s0["ticks"]) / CLK_TCK / dt, 2)
    r["host_cpu_pct_of_device"] = round(r["host_cpu_pct_of_core"] / cores, 2)
    busy = lambda s: s["sys_total"] - s["sys_idle"]
    r["system_busy_pct"] = round(100 * (busy(s1) - busy(s0)) / max(1, s1["sys_total"] - s0["sys_total"]), 1)
    peak = 0.0
    for p, q in zip(samples, samples[1:]):
        peak = max(peak, 100 * (q["ticks"] - p["ticks"]) / CLK_TCK / (q["t"] - p["t"]))
    r["host_cpu_peak_10s_pct_of_core"] = round(peak, 1)
    if "thermal" in s0:
        r["thermal_max"] = max(s["thermal"] for s in samples)
        r["battery_start"], r["battery_end"] = s0["battery"], s1["battery"]
        r["batt_temp_start"], r["batt_temp_max"] = s0["batt_temp"], max(s["batt_temp"] for s in samples)
        r["charging"] = s0["usb"]
        w0, w1 = s0["wlan"], s1["wlan"]
        r["wlan_rx_kbps"] = round((w1[0] - w0[0]) * 8 / dt / 1000, 1)
        r["wlan_tx_kbps"] = round((w1[2] - w0[2]) * 8 / dt / 1000, 1)
        r["wlan_rx_pps"] = round((w1[1] - w0[1]) / dt, 1)
        r["wlan_tx_pps"] = round((w1[3] - w0[3]) / dt, 1)
    if stat0 and stat1:
        st = (stat1["t_ms"] - stat0["t_ms"]) / 1000
        d = lambda k: (stat1[k] - stat0[k]) / st
        # QUIC UDP payload bytes + 28 B UDP/IPv4 header per datagram = IPv4 wire rate.
        r["host_down_kbps_wire"] = round((d("udp_rx") + 28 * d("udp_rx_pkts")) * 8 / 1000, 1)
        r["host_up_kbps_wire"] = round((d("udp_tx") + 28 * d("udp_tx_pkts")) * 8 / 1000, 1)
        r["host_in_pps"] = round(d("udp_rx_pkts"), 1)
        r["host_out_pps"] = round(d("udp_tx_pkts"), 1)
        r["host_up_streams_avg"] = round(d("udp_tx_pkts") / 50, 2)
        r["host_down_streams_avg"] = round(d("udp_rx_pkts") / 50, 2)
        r["host_tx_fail"] = stat1["tx_fail"] - stat0["tx_fail"]
        r["sel_changes_per_min"] = round(d("sel_changes") * 60, 1)
        r["host_mb_per_h"] = round((d("udp_rx") + d("udp_tx") + 28 * (d("udp_rx_pkts") + d("udp_tx_pkts"))) * 3600 / 1e6, 1)
    r["members"] = summary
    return r


def emit(a, r):
    line = json.dumps(r)
    print("RESULT " + line, flush=True)
    if a.out:
        with open(a.out, "a") as f:
            f.write(line + "\n")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("cmd", choices=["push", "phone", "desktop", "direct", "clean"])
    ap.add_argument("--serial", default=os.environ.get("GCALL_SERIAL", "ZD22257P6G"))
    ap.add_argument("--bin", help="host binary to push")
    ap.add_argument("--host-bin")
    ap.add_argument("--members-bin")
    ap.add_argument("--n", type=int, default=3)
    ap.add_argument("--k", type=int, default=2)
    ap.add_argument("--secs", type=int, default=600)
    ap.add_argument("--label", default="run")
    ap.add_argument("--out")
    ap.add_argument("--host-args", default="", help="extra host flags, e.g. '--no-participate'")
    a = ap.parse_args()

    if a.cmd == "push":
        adb(a.serial, "push", a.bin, REMOTE)
        adb(a.serial, "shell", f"chmod 755 {REMOTE}")
        return
    if a.cmd == "clean":
        adb(a.serial, "shell", "pkill -f gcall-host", check=False)
        adb(a.serial, "shell", f"rm -f {REMOTE}", check=False)
        print(adb(a.serial, "shell", "ls /data/local/tmp", check=False))
        return
    if a.cmd == "direct":
        out = subprocess.run([a.members_bin, "--direct", "--secs", str(a.secs), "--label", a.label],
                             capture_output=True, text=True, check=True).stdout
        summary = next((kv(l) for l in out.splitlines() if l.startswith("SUMMARY")), {})
        emit(a, {"label": a.label, "mode": "direct", "secs": a.secs, "members": summary})
        return
    if a.cmd == "phone":
        ip = re.search(r"inet (\d+\.\d+\.\d+\.\d+)", adb(a.serial, "shell", "ip -4 addr show wlan0")).group(1)
        cores = int(adb(a.serial, "shell", "nproc").strip())
        host_cmd = [ADB, "-s", a.serial, "shell", f"{REMOTE} --port {PORT} --k {a.k} --secs {a.secs + 30} {a.host_args}"]
        host, pid, samples, stat0, stat1, summary = run_session(
            a, host_cmd, f"{ip}:{PORT}", lambda p: sample_phone(a.serial, p), cores)
        adb(a.serial, "shell", f"kill {pid}", check=False)
    else:
        cores = os.cpu_count()
        host_cmd = [a.host_bin, "--port", str(PORT), "--k", str(a.k), "--secs", str(a.secs + 30), *a.host_args.split()]
        host, pid, samples, stat0, stat1, summary = run_session(a, host_cmd, f"127.0.0.1:{PORT}", sample_local, cores)
    host.kill()
    emit(a, analyse(a, samples, stat0, stat1, summary, cores))


if __name__ == "__main__":
    main()
