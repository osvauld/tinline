// Renders the tray icons (3 states x light/dark panel x 16/22/24/32 px) from the brand tiers (docs/design/brand).
// Needs rsvg-convert:  node crates/desktop/assets/gen_tray.mjs   (light panel = black ink, dark panel = white ink;
// available = taut (online, as the app header), unavailable = taut at 45% opacity, in a call = taut plus a small
// accent dot centred under the string (option 1, user 2026-10-13). The output is checked in.
import { paddedSvg, markSvg } from '../../../docs/design/playground/src/lib/brand/tiers.js';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
const out = new URL('./tray', import.meta.url).pathname;
const art = (S, v, st) => (S === 22 ? paddedSvg(22, 20, v, st) : markSvg(S, v, st));
// The dot sits on the mark's centre line, below the string, in the mark's own viewBox (the first one after the outer svg
// for the padded 22 px art). Accent: brand teal on a light panel, mint on a dark one.
const dot = (svg, fill) => {
  const vb = [...svg.matchAll(/viewBox="([^"]+)"/g)].pop()[1].split(' ').map(Number);
  const [x, y, w, h] = vb, c = `<circle cx="${x + w / 2}" cy="${y + h * 0.84}" r="${w * 0.09}" fill="${fill}"/>`;
  return svg.replace('</svg>', c + '</svg>');
};
for (const [theme, v] of [['light', 'mono'], ['dark', 'monowhite']]) for (const S of [16, 22, 24, 32]) {
  const put = (name, svg) => execFileSync('rsvg-convert', ['-w', S, '-h', S, '-o', `${out}/${name}-${theme}-${S}.png`], { input: svg });
  put('available', art(S, v, 'taut'));
  put('incall', dot(art(S, v, 'taut'), theme === 'light' ? '#0B6B5B' : '#6FD3BD'));
  put('unavailable', art(S, v, 'taut').replace(/<svg ([^>]*)>/, '<svg $1><g opacity="0.45">').replace(/<\/svg>$/, '</g></svg>'));
}
