#!/usr/bin/env python3
"""Interpret legacy benchmark results conservatively; preserve raw observations.

No reconstructed end-to-end percentiles: never-heard/late durations and frame
attribution cannot be recovered from SUMMARY. Nominal duty is generator intent,
not measured talk time (startup, minimum gaps and short runs alter it).
"""
import argparse
import json
from pathlib import Path


def legacy_match(onsets, now, last_matched):
    """Reference characterization of members.rs matching, not a valid oracle."""
    mouth = next((m for m in reversed(onsets) if m <= now), None)
    return mouth if mouth is not None and mouth > last_matched else None


def audit_result(result):
    m = result.get('members', {})
    samples = int(m.get('m2e_n', 0))
    late = int(m.get('late_onsets', 0))
    matched = samples + late
    n = int(result.get('n', 0))
    pattern = result.get('talk_pattern', 'random')
    host_duty = 0 if result.get('controlled_host_silent') else 0.2
    if pattern == 'turn-taking' and n > 1:
        member_duty = 5 / (6 * (n - 1))
    elif pattern == 'overlap':
        member_duty = 5 / 6
    else:
        member_duty = 1 / n if n else None
    warnings = [
        'Legacy percentiles are conditional on heard onsets <=1000ms, not all offered speech.',
        'No receiver-pair opportunity ledger: never-heard speech and fairness cannot be scored.',
        'Latest-prior-onset matching has no decoded-frame origin attribution.',
        'Host speech is excluded from onset timing and host receiver is not in the member summary.',
        'No audio hardware I/O; clients share the host laptop; external contention must be reviewed.',
        'Zero received-packet loss does not prove every offered talk spurt was forwarded or heard.',
    ]
    return {
        'n': n,
        'conditional_p95_ms': m.get('m2e_p95') if samples else None,
        'matched_onset_observations': matched,
        'late_onset_observations': late,
        'percentiles_exclude_late': late > 0,
        'late_fraction_of_matched': late / matched if matched else None,
        'never_heard_fraction': None,
        'talk_pattern': pattern,
        'host_duty_nominal': host_duty,
        'member_duty_nominal': member_duty,
        'expected_speakers_nominal': host_duty + (n - 1) * member_duty if member_duty is not None else None,
        'service_capacity_established': False,
        'warnings': warnings,
    }


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('results', type=Path)
    a = p.parse_args()
    for line in a.results.read_text().splitlines():
        if line.strip():
            print(json.dumps(audit_result(json.loads(line))))


if __name__ == '__main__':
    main()
