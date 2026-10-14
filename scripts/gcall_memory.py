"""Linux RSS/HWM observations; allocator/cache growth is not proof of a leak.

VmHWM is the process lifetime resident highwater as observed at sample time;
it can miss growth after the final sample. VmRSS is not proportional/unique
memory and includes shared resident pages. Units are KiB (Linux status kB).
"""
import re


def parse_memory(status):
    def field(name):
        m = re.search(rf'^{name}:\s+(\d+)\s+kB$', status, re.MULTILINE)
        return int(m[1]) if m else None
    return {'rss_kib': field('VmRSS'), 'hwm_kib': field('VmHWM')}


def summarise_memory(samples):
    rss = [(s['t'], s['memory']['rss_kib']) for s in samples
           if s.get('memory', {}).get('rss_kib') is not None]
    hwm = [s['memory']['hwm_kib'] for s in samples
           if s.get('memory', {}).get('hwm_kib') is not None]
    dt = rss[-1][0] - rss[0][0] if len(rss) >= 2 else 0
    delta = rss[-1][1] - rss[0][1] if rss else None
    return {'rss_first_kib': rss[0][1] if rss else None,
            'rss_last_kib': rss[-1][1] if rss else None,
            'rss_peak_sampled_kib': max(v for _, v in rss) if rss else None,
            'hwm_peak_observed_kib': max(hwm) if hwm else None,
            'rss_delta_kib': delta,
            'rss_endpoint_slope_kib_per_s': round(delta / dt, 3) if dt > 0 else None,
            'sample_span_s': round(dt, 3), 'valid_rss_samples': len(rss),
            'leak_established': False,
            'limits': 'sampled Linux RSS and observed HWM, not unique memory or proof of steady state/leak'}
