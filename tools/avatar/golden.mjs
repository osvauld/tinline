// Checks the playground fingerprint port against the Rust golden file.
//   node tools/avatar/golden.mjs   (exit 1 on any mismatch)
import { readFileSync } from 'node:fs';
import { artFromFingerprint, fingerprint, blake3Hash } from '../../docs/design/playground/src/lib/ui/fingerprint.ts';

// BLAKE3 test vector: hash("") = af1349b9...
const empty = Buffer.from(blake3Hash([])).toString('hex');
if (!empty.startsWith('af1349b9f5f9a1a6')) { console.error('blake3 empty vector wrong', empty); process.exit(1); }

const lines = readFileSync(new URL('../../crates/core/src/avatar_golden.txt', import.meta.url), 'utf8').trimEnd().split('\n');
let bad = 0, cases = 0, strokes = 0, i = 0;
const fail = (m) => { if (bad++ < 10) console.error(m); };
while (i < lines.length) {
  const [, did, , small, , sw] = lines[i++].split(' ');
  const fp = lines[i++].split(' ')[1];
  const col = lines[i++].split(' ');
  const mine = fingerprint(did);
  if (!mine || Buffer.from(mine).toString('hex') !== fp) { fail(`fingerprint mismatch ${did}`); while (lines[i]?.startsWith('s ')) i++; continue; }
  const art = artFromFingerprint(mine, small === 'true');
  cases++;
  const hex = (n) => n.toString(16).padStart(8, '0');
  const got = ['light', hex(art.light.bgArgb), hex(art.light.fgArgb), 'dark', hex(art.dark.bgArgb), hex(art.dark.fgArgb)].join(' ');
  if (got !== col.join(' ')) fail(`colours ${did}: ${got} vs ${col.join(' ')}`);
  if (String(art.strokeWidth) !== sw) fail(`stroke width ${did}`);
  let k = 0;
  while (lines[i]?.startsWith('s ')) {
    const f = lines[i++].split(' '), s = art.strokes[k++];
    strokes++;
    if (!s || +f[1] !== +s.closed || +f[2] !== +s.gold || +f[3] !== s.alpha) { fail(`stroke flags ${did} #${k}`); continue; }
    const pts = f.slice(4).map(Number);
    if (pts.length !== s.points.length) { fail(`stroke length ${did} #${k}`); continue; }
    pts.forEach((v, j) => { if (Math.abs(v - s.points[j]) > 0.0021) fail(`point ${did} #${k}[${j}] ${v} vs ${s.points[j]}`); });
  }
  if (k !== art.strokes.length) fail(`stroke count ${did}: ${k} vs ${art.strokes.length}`);
}
console.log(`${cases} cases, ${strokes} strokes, ${bad} mismatches`);
process.exit(bad ? 1 : 0);
