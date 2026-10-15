#!/usr/bin/env python3
"""Codebase size metrics for the refactor plan: file sizes, inline Rust test blocks, long fns."""
import json, re, subprocess, sys
from collections import defaultdict

repo = sys.argv[1]
out = sys.argv[2]
files = subprocess.run(["git", "-C", repo, "ls-files"], capture_output=True, text=True).stdout.split()
files = [f for f in files if re.search(r"\.(rs|kt|py)$", f) and "playground" not in f]

def is_test_file(f):
    return bool(re.search(r"(^|/)tests?/|_tests?\.rs$|/tests\.rs$|src/test/|Test\.kt$|/test_[^/]*\.py$|/e2e_", f))

def area(f):
    p = f.split("/")
    if p[0] == "crates": return "crates/" + p[1]
    if p[0] == "android": return "android"
    return p[0]

FN = re.compile(r"^\s*(pub(\([^)]*\))?\s+)?(async\s+)?(unsafe\s+)?(const\s+)?fn\s+(\w+)|^\s*(private |internal |override |suspend |inline |public )*fun\s+(<[^>]*>\s*)?([\w.`]+)|^\s*(async\s+)?def\s+(\w+)")

rows = []
for f in files:
    try:
        lines = open(f"{repo}/{f}", encoding="utf-8", errors="replace").read().split("\n")
    except FileNotFoundError:
        continue
    n = len(lines)
    # inline #[cfg(test)] module blocks (Rust), brace matched
    test_inline = 0
    i = 0
    while i < n:
        if lines[i].strip().startswith("#[cfg(test)]"):
            j = i + 1
            while j < n and lines[j].strip().startswith("#["):
                j += 1
            if j < n and re.match(r"\s*(pub(\(\w+\))?\s+)?mod\s+\w+\s*\{", lines[j]):
                depth, k = 0, j
                while k < n:
                    depth += lines[k].count("{") - lines[k].count("}")
                    if depth <= 0 and k > j:
                        break
                    k += 1
                test_inline += k - i + 1
                i = k
        i += 1
    # function lengths (indent/brace heuristic)
    fns = []
    for idx, l in enumerate(lines):
        m = FN.match(l)
        if not m:
            continue
        name = m.group(6) or m.group(9) or m.group(11)
        if f.endswith(".py"):
            ind = len(l) - len(l.lstrip())
            k = idx + 1
            while k < n and (not lines[k].strip() or len(lines[k]) - len(lines[k].lstrip()) > ind):
                k += 1
            fns.append((k - idx, name, idx + 1))
        else:
            depth, started, k = 0, False, idx
            while k < n and k < idx + 3000:
                depth += lines[k].count("{") - lines[k].count("}")
                if "{" in lines[k]:
                    started = True
                if started and depth <= 0:
                    break
                if not started and (lines[k].rstrip().endswith(";") or (f.endswith(".kt") and "=" in lines[k] and k > idx + 2)):
                    break
                k += 1
            if started:
                fns.append((k - idx + 1, name, idx + 1))
    rows.append(dict(file=f, area=area(f), lines=n, test_file=is_test_file(f), test_inline=test_inline,
                     fns=len(fns), long_fns=sorted([x for x in fns if x[0] >= 80], reverse=True)))

json.dump(rows, open(out, "w"), indent=1)

def pr(*a): print(*a)
B = [(0, 300), (300, 500), (500, 800), (800, 1200), (1200, 10**9)]
pr("== File size buckets (all .rs/.kt/.py, raw lines) ==")
for lo, hi in B:
    sel = [r for r in rows if lo <= r["lines"] < hi]
    pr(f"  {lo:>5}-{'' if hi > 10**8 else hi:<5} files={len(sel):>4}  lines={sum(r['lines'] for r in sel):>7}")
tot = sum(r["lines"] for r in rows)
big = [r for r in rows if r["lines"] >= 500]
pr(f"  total files={len(rows)} lines={tot}; files>=500: {len(big)} holding {sum(r['lines'] for r in big)} lines ({100*sum(r['lines'] for r in big)//tot}%)")

pr("\n== Per area: prod lines / test-file lines / inline-test lines / files>=500 / largest ==")
agg = defaultdict(lambda: [0, 0, 0, 0, 0, ""])
for r in rows:
    a = agg[r["area"]]
    if r["test_file"]: a[1] += r["lines"]
    else: a[0] += r["lines"] - r["test_inline"]
    a[2] += r["test_inline"]
    if r["lines"] >= 500: a[3] += 1
    if r["lines"] > a[4]: a[4], a[5] = r["lines"], r["file"].split("/")[-1]
pr(f"  {'area':<22}{'prod':>8}{'testfile':>9}{'inline':>8}{'test%':>7}{'>=500':>7}  largest")
for k, (p, t, il, b, mx, mf) in sorted(agg.items(), key=lambda x: -(x[1][0] + x[1][1] + x[1][2])):
    pct = 100 * (t + il) // max(1, p + t + il)
    pr(f"  {k:<22}{p:>8}{t:>9}{il:>8}{pct:>6}%{b:>7}  {mf} ({mx})")
P = sum(v[0] for v in agg.values()); T = sum(v[1] for v in agg.values()); I = sum(v[2] for v in agg.values())
pr(f"  {'TOTAL':<22}{P:>8}{T:>9}{I:>8}{100*(T+I)//(P+T+I):>6}%")

pr("\n== Files with inline #[cfg(test)] blocks >= 100 lines (should move to own test file) ==")
inl = sorted([r for r in rows if r["test_inline"] >= 100], key=lambda r: -r["test_inline"])
for r in inl:
    pr(f"  {r['test_inline']:>5} of {r['lines']:>5}  {r['file']}")
pr(f"  -> {len(inl)} files, {sum(r['test_inline'] for r in inl)} lines; all inline blocks: {sum(1 for r in rows if r['test_inline'])} files, {I} lines")

pr("\n== Largest 40 files ==")
for r in sorted(rows, key=lambda r: -r["lines"])[:40]:
    pr(f"  {r['lines']:>5}  {'T ' if r['test_file'] else '  '}{r['file']}")

pr("\n== Functions >= 80 lines ==")
lf = [(x[0], r["file"], x[1], x[2]) for r in rows for x in r["long_fns"]]
for lo, hi in [(80, 150), (150, 300), (300, 10**9)]:
    pr(f"  {lo}-{'' if hi > 10**8 else hi}: {sum(1 for x in lf if lo <= x[0] < hi)}")
pr("  top 30:")
for n, f, name, ln in sorted(lf, reverse=True)[:30]:
    pr(f"  {n:>5}  {f}:{ln}  {name}")
