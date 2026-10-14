#!/usr/bin/env python3
"""Sequential synthetic audio capacity sweep; never modifies production caps.

Reuse dev's release host/members (record their hashes). Wait for an idle queue,
record queue snapshots during runs, and retain even contaminated results.
--allow-busy is for explicitly labelled smoke tests only, not capacity claims.
"""
import argparse
import hashlib
import json
import re
import subprocess
import sys
import time
from pathlib import Path

from gcall_metrics import audit_result

LOCK = Path('/home/abe/p2p_chat/scripts/buildlock.py')
ROOT = Path(__file__).resolve().parent.parent


def validate_counts(value):
    counts = [int(x) for x in value.split(',') if x]
    if not counts or len(set(counts)) != len(counts) or any(n < 2 or n > 64 for n in counts):
        raise ValueError('unique participant counts must be in 2..64')
    return counts


def busy_reasons(text):
    reasons = []
    pressure = re.search(r'cpu pressure (\d+)%', text)
    load = re.search(r'load ([\d.]+)/', text)
    if pressure and int(pressure[1]) > 2:
        reasons.append('cpu pressure >2%')
    if load and float(load[1]) > 2:
        reasons.append('load >2')
    for line in text.splitlines():
        if line.startswith(('compile ', 'run-', 'waiting ')):
            if not line.rstrip().endswith('free') and 'capacity-measure' not in line:
                reasons.append(line)
    return reasons


def status():
    return subprocess.run([sys.executable, str(LOCK), '--status'], check=True,
                          capture_output=True, text=True).stdout


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--counts', default='12,16,24,32')
    p.add_argument('--secs', type=int, default=300)
    p.add_argument('--talk-pattern', choices=['random', 'turn-taking', 'overlap'], default='random')
    p.add_argument('--k', type=int, default=3)
    p.add_argument('--wait-secs', type=int, default=1800)
    p.add_argument('--allow-busy', action='store_true')
    p.add_argument('--out', type=Path, required=True)
    p.add_argument('--bin-dir', type=Path, default=Path('/home/abe/p2p_chat/target/release'))
    a = p.parse_args()
    counts = validate_counts(a.counts)
    if a.secs < 30 or a.wait_secs < 0 or not 1 <= a.k < min(counts):
        p.error('secs must be >=30, wait-secs >=0 and k in 1..min(counts)-1')
    a.out.mkdir(parents=True, exist_ok=True)
    binaries = {n: {'path': str((a.bin_dir / n).resolve()),
                    'sha256': hashlib.sha256((a.bin_dir / n).read_bytes()).hexdigest()}
                for n in ('host', 'members')}
    manifest = {'counts': counts, 'secs': a.secs, 'k': a.k, 'talk_pattern': a.talk_pattern, 'binaries': binaries,
                'smoke_allow_busy': a.allow_busy,
                'limits': 'synthetic Opus/QUIC loopback, no membership/auth/audio hardware; host CPU only; clients share laptop',
                'runs': []}
    dest = a.out / 'manifest.json'
    for n in counts:
        deadline = time.monotonic() + a.wait_secs
        while True:
            snapshot = status()
            reasons = busy_reasons(snapshot)
            if not reasons or a.allow_busy:
                break
            if time.monotonic() >= deadline:
                manifest['stopped'] = {'reason': 'idle wait expired', 'queue': snapshot}
                dest.write_text(json.dumps(manifest, indent=2))
                return 2
            print('Waiting for idle queue:', '; '.join(reasons), flush=True)
            time.sleep(15)
        cmd = [sys.executable, str(LOCK), '--who', 'capacity-measure', '--run', '--',
               sys.executable, str(ROOT / 'scripts/gcall_bench.py'), 'desktop',
               '--n', str(n), '--k', str(a.k), '--secs', str(a.secs), '--talk-pattern', a.talk_pattern,
               '--host-bin', str(a.bin_dir / 'host'), '--members-bin', str(a.bin_dir / 'members'),
               '--label', f'laptop-n{n}-k{a.k}-{a.talk_pattern}', '--out', str(a.out / 'results.jsonl')]
        run = {'n': n, 'command': cmd, 'queue': [{'at': time.time(), 'text': snapshot}],
               'busy_at_start': reasons}
        print(f'Starting N={n}, {a.secs}s; smoke={a.allow_busy}', flush=True)
        with (a.out / f'n{n}.log').open('w') as log:
            child = subprocess.Popen(cmd, stdout=log, stderr=subprocess.STDOUT)
            while child.poll() is None:
                time.sleep(5)
                run['queue'].append({'at': time.time(), 'text': status()})
        run['exit_code'] = child.returncode
        raw_results = a.out / 'results.jsonl'
        if raw_results.exists():
            matched_results = [json.loads(line) for line in raw_results.read_text().splitlines() if line.strip()]
            run['metric_audits'] = [audit_result(r) for r in matched_results if int(r.get('n', 0)) == n]
        run['other_jobs_observed'] = any(
            any(r.startswith(('compile ', 'run-', 'waiting ')) for r in busy_reasons(s['text']))
            for s in run['queue'])
        manifest['runs'].append(run)
        dest.write_text(json.dumps(manifest, indent=2))
        if child.returncode:
            return child.returncode
    return 0


if __name__ == '__main__':
    sys.exit(main())
