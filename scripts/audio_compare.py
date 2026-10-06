#!/usr/bin/env python3
"""Did the far end hear what we sent? Aligns `heard` to `sent` by cross-correlation and scores it.

    .venv/bin/python scripts/audio_compare.py sent.wav heard.wav [--min-env 0.8]

Prints the delay, the waveform correlation (high only through a clean codec path), and the
correlation of 20 ms loudness envelopes — the pass metric, since a phone's noise suppression,
AGC and echo canceller reshape the waveform but keep the syllable rhythm. Exit 1 below
--min-env. Needs numpy (scripts use the repo's .venv).
"""
import argparse
import sys
import wave

import numpy as np

RATE = 48_000
WIN = RATE // 50  # 20 ms


def load(path):
    with wave.open(path) as w:
        if w.getframerate() != RATE or w.getsampwidth() != 2:
            sys.exit(f"{path}: need 48 kHz 16-bit")
        x = np.frombuffer(w.readframes(w.getnframes()), dtype="<i2").astype(np.float64)
        if w.getnchannels() > 1:
            x = x.reshape(-1, w.getnchannels()).mean(axis=1)
    return x


def envelope(x):
    n = len(x) // WIN
    return np.sqrt((x[: n * WIN].reshape(n, WIN) ** 2).mean(axis=1) + 1e-9)


def corr(a, b):
    n = min(len(a), len(b))
    a, b = a[:n] - a[:n].mean(), b[:n] - b[:n].mean()
    d = np.sqrt((a * a).sum() * (b * b).sum())
    return float((a * b).sum() / d) if d else 0.0


def best_lag(sent, heard, max_lag_s):
    """Lag (samples) at which `heard` best matches `sent`, searched on envelopes then refined."""
    es, eh = np.log(envelope(sent)), np.log(envelope(heard))
    max_lag = int(max_lag_s * 50)
    scores = [corr(es, eh[lag:]) for lag in range(0, min(max_lag, len(eh) - 10))]
    coarse = int(np.argmax(scores)) * WIN
    lo, hi = max(0, coarse - WIN), coarse + WIN
    n = min(len(sent), len(heard) - hi) if len(heard) > hi else 0
    if n <= 0:
        return coarse
    seg = sent[:n]
    fine = [corr(seg, heard[l : l + n]) for l in range(lo, hi)]
    return lo + int(np.argmax(fine))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("sent")
    ap.add_argument("heard")
    ap.add_argument("--max-lag", type=float, default=5.0, help="seconds to search")
    ap.add_argument("--min-env", type=float, default=0.8)
    a = ap.parse_args()
    sent, heard = load(a.sent), load(a.heard)
    lag = best_lag(sent, heard, a.max_lag)
    h = heard[lag:]
    n = min(len(sent), len(h))
    wave_c = corr(sent[:n], h[:n])
    env_c = corr(np.log(envelope(sent[:n])), np.log(envelope(h[:n])))
    level = 20 * np.log10((np.sqrt((h[:n] ** 2).mean()) + 1e-9) / (np.sqrt((sent[:n] ** 2).mean()) + 1e-9))
    ok = env_c >= a.min_env
    print(f"{'PASS' if ok else 'FAIL'} delay={lag / RATE * 1000:.0f}ms envelope_corr={env_c:.3f} "
          f"waveform_corr={wave_c:.3f} level={level:+.1f}dB compared={n / RATE:.1f}s")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
