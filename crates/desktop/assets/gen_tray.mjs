// Renders the tray icons (3 states x light/dark panel x 16/22/24/32 px) from the brand tiers (docs/design/brand).
// Needs rsvg-convert:  node crates/desktop/assets/gen_tray.mjs   (light panel = black ink, dark panel = white ink;
// in a call = taut pose, unavailable = 45% opacity). The output is checked in.
import { paddedSvg, markSvg } from '../../../docs/design/playground/src/lib/brand/tiers.js';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
const out = new URL('./tray', import.meta.url).pathname;
const art = (S, v, st) => (S === 22 ? paddedSvg(22, 20, v, st) : markSvg(S, v, st));
for (const [theme, v] of [['light', 'mono'], ['dark', 'monowhite']]) for (const S of [16, 22, 24, 32]) {
  const put = (name, svg) => execFileSync('rsvg-convert', ['-w', S, '-h', S, '-o', `${out}/${name}-${theme}-${S}.png`], { input: svg });
  put('available', art(S, v, 'slack'));
  put('incall', art(S, v, 'taut'));
  put('unavailable', art(S, v, 'slack').replace(/<svg ([^>]*)>/, '<svg $1><g opacity="0.45">').replace(/<\/svg>$/, '</g></svg>'));
}
