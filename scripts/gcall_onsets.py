#!/usr/bin/env python3
"""Offline opportunity ledger for opt-in member traces (times are microseconds).

Onset->first arrival includes source gate/host selection/network/scheduling; it
cannot isolate host selection time. Arrival->audible is a threshold proxy, not
verified decoded-frame lineage or hardware latency. Host slot 0 is excluded.
Deadline is an experiment variable, not a product SLA. Offline trace memory is
not claimed to be a bounded production resource.
"""
import argparse
from bisect import bisect_left
from collections import Counter, defaultdict
import json
from pathlib import Path


def analyse(events, deadline_us, host_events=()):
    if deadline_us <= 0:
        raise ValueError('deadline must be positive')
    events = sorted(events, key=lambda e: e[4])
    ready, finish = {}, {}
    onsets, ends, sends, audible = (defaultdict(list) for _ in range(4))
    arrivals = {}
    for kind, source, receiver, seq, at in events:
        if kind == 'ready':
            ready[source] = at
        elif kind == 'finish':
            finish[source] = at
        elif kind == 'onset':
            onsets[source].append(at)
        elif kind == 'speech_end':
            ends[source].append(at)
        elif kind == 'send':
            sends[source].append((at, seq))
        elif kind == 'receive':
            arrivals.setdefault((source, receiver, seq), at)
        elif kind == 'audible':
            audible[source, receiver].append(at)
        else:
            raise ValueError(f'unknown event {kind}')
    if not ready or ready.keys() != finish.keys():
        raise ValueError('missing ready/finish: incomplete trace')
    host_receives, host_admits = {}, {}
    for kind, source, _, seq, at in sorted(host_events, key=lambda e: e[4]):
        if kind == 'host_receive':
            host_receives.setdefault((source, seq), at)
        elif kind == 'forward_admit':
            host_admits.setdefault((source, seq), at)
        else:
            raise ValueError(f'unknown host event {kind}')
    opportunities = []
    for source, starts in onsets.items():
        if source == 0 or source not in finish:
            continue
        source_sends = sends[source]
        send_times = [t for t, _ in source_sends]
        for i, onset in enumerate(starts):
            next_onset = starts[i + 1] if i + 1 < len(starts) else finish[source]
            end = next((t for t in ends[source] if t >= onset), finish[source])
            end = min(end, next_onset)
            seqs = [seq for _, seq in source_sends[bisect_left(send_times, onset):bisect_left(send_times, end)]]
            host_seen = [host_receives[source, seq] for seq in seqs if (source, seq) in host_receives]
            host_forwarded = [host_admits[source, seq] for seq in seqs if (source, seq) in host_admits]
            # These two timestamps share only the host clock; never subtract a member timestamp.
            host_wait = min(host_forwarded) - min(host_seen) if host_seen and host_forwarded else None
            if host_wait is not None and host_wait < 0:
                raise ValueError('host admission precedes receipt: corrupt trace')
            for receiver in ready:
                if receiver == source or not ready[receiver] <= onset < finish[receiver]:
                    continue
                observed_until = min(finish[source], finish[receiver])
                candidates = [arrivals[source, receiver, seq] for seq in seqs
                              if (source, receiver, seq) in arrivals
                              and onset <= arrivals[source, receiver, seq] <= observed_until]
                first = min(candidates) if candidates else None
                deadline = onset + deadline_us
                if first is not None:
                    status = 'arrived_by_deadline' if first <= deadline else 'late_arrival'
                else:
                    status = 'tail_censored' if observed_until < deadline else 'not_received_by_deadline'
                # Stop at next onset: do not credit a later spurt's RMS transition.
                transitions = audible[source, receiver]
                ix = bisect_left(transitions, first) if first is not None else len(transitions)
                proxy = transitions[ix] if ix < len(transitions) and transitions[ix] < min(next_onset, observed_until) else None
                opportunities.append({
                    'source': source, 'receiver': receiver, 'onset_us': onset,
                    'status': status, 'observed_until_us': observed_until,
                    'onset_to_first_arrival_us': first - onset if first is not None else None,
                    'arrival_to_audible_proxy_us': proxy - first if proxy is not None else None,
                    'host_observed_admission_wait_us': host_wait,
                    'audible_frame_attribution_verified': False,
                })
    counts = Counter(p['status'] for p in opportunities)
    counts['opportunities'] = len(opportunities)
    # Late arrivals remain deadline misses, never discarded from the denominator.
    counts['not_received_by_deadline'] += counts['late_arrival']
    return {'deadline_us': deadline_us, 'counts': dict(counts), 'opportunities': opportunities,
            'limits': 'member pairs only; arrival, host admission wait and audible proxy, not verified playback; clocks not synchronized; no hardware or capacity verdict'}


def read_trace(path):
    events = []
    for line in path.read_text().splitlines():
        kind, source, receiver, seq, at = line.split()
        events.append((kind, int(source), int(receiver), int(seq), int(at)))
    return events


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('trace', type=Path)
    p.add_argument('--deadline-ms', type=int, default=1000)
    p.add_argument('--host-trace', type=Path)
    a = p.parse_args()
    print(json.dumps(analyse(read_trace(a.trace), a.deadline_ms * 1000,
                             read_trace(a.host_trace) if a.host_trace else ()), indent=2))


if __name__ == '__main__':
    main()
