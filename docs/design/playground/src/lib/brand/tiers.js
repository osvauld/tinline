// @ts-nocheck
/* Tinline mark D: optical-size tiers + static SVG builders.
   Everything is derived from TinMark.markPaths (tinmark.js); nothing here is a drawing.
   A tier is designed in PIXELS of its own box B (1 model unit = 1 px for B <= 28, so the numbers below are
   literally pixel coordinates: flat edges sit on whole pixels, rims/mouth on half pixels where it matters).
   For B >= 32 the base 24-grid design is scaled by u = B/24 and snapped (b to whole px, the rest to 0.5 px).
   Used by: generate.mjs (all icon files), Mark.svelte (in-app logo), the contact sheet. */
import { TinMark } from './tinmark.js';

/* ---- palettes ------------------------------------------------------------------------------------------ */
export const PAL = {
  teal: '#0B6B5B', mint: '#9FD6C8', gold: '#C98A1E', amber: '#E8B04A', cream: '#F5F6F3', deep: '#14433A', ink: '#101614',
};
export const VARIANTS = {
  // transparent, for light surfaces
  color: { can: PAL.teal, rim: PAL.teal, mouth: PAL.mint, str: PAL.gold },
  // transparent, for dark surfaces
  dark: { can: PAL.cream, rim: PAL.cream, mouth: PAL.deep, str: PAL.amber },
  // on the app tile (teal ground): the mouth IS the ground, so it reads as a hole
  tile: { can: PAL.cream, rim: PAL.cream, mouth: PAL.teal, str: PAL.amber },
  mono: { mono: true, ink: PAL.ink },
  white: { mono: true, ink: '#FFFFFF', solid: true },   // solid silhouette, no mouth cutout
  monowhite: { mono: true, ink: '#FFFFFF' },            // white ink with the mouth cut out (status bar, dark tray)
};

/* ---- small tiers: designed in px (box B x B, origin top-left, y down) ------------------------------------
   c  = x of the mouth centre (left can), measured from the left edge     a = rim half-width   b = rim half-height
   L  = body length behind the rim   mu = mouth ratio (0 = no mouth)       sw = string stroke
   s  = rest sag    sT = sag when 'taut' (small tiers: sag only, the cans never move; null = full model taut)
   Left extreme of the glyph = c - L - a. */
export const SMALL = {
  // <= 20 px: the SAME oval silhouette as 24+, pixel-hinted: rounded rim + rounded back, plain 2-colour mouth (flat),
  // string starts on the rim beside the mouth (ko) so no gold blob fills the hole. Mouth sits ~1/3 of the can width from the axis side, as at 24+.
  12: { c: 3.5, a: 1.5, b: 4, L: 1.5, mu: 0.5, noCap: false, flat: true, ko: 0.75, sw: 1.5, s: 2.5, sT: 0.5 },
  14: { c: 4.5, a: 1.5, b: 5, L: 2, mu: 0.5, noCap: false, flat: true, ko: 0.75, sw: 1.5, s: 3, sT: 0.5 },
  16: { c: 5, a: 2, b: 5, L: 2, mu: 0.5, noCap: false, flat: true, ko: 1.25, sw: 1.75, s: 3, sT: 0.5 },
  20: { c: 6, a: 2.5, b: 6, L: 2.5, mu: 0.5, noCap: false, flat: true, ko: 1.25, sw: 1.75, s: 3.5, sT: 0.75 },
  // 24/28: same proportions as the base (a ~ .5 b, L ~ .5 b, mouth at the rim centre), mint mouth returns
  24: { c: 7.5, a: 3, b: 6, L: 3, mu: 0.6, noCap: false, sw: 1.75, s: 3.5, sT: null },
  28: { c: 8.5, a: 3.5, b: 7, L: 3.5, mu: 0.6, noCap: false, sw: 1.75, s: 4, sT: null },
};
export const TIER_SIZES = [12, 14, 16, 20, 24, 28, 32, 40, 48, 64, 96, 128, 256, 512, 1024];

const half = (v) => Math.round(v * 2) / 2;

/** px-level spec of the tier for box size B. */
export function tierPx(B) {
  if (SMALL[B]) return { B, ...SMALL[B], u: 1, designed: true };
  const u = B / 24, base = TinMark.DEFAULTS;
  const exact = B >= 96;                                    // snapping is below the noise at this size
  const sn = (v, f) => (exact ? v : f(v));
  const b = sn(base.b * u, Math.round), aRaw = base.b * Math.sin(base.theta * Math.PI / 180) * u;
  return { B, u, designed: true,
    c: sn(base.c * u, half), a: sn(aRaw, half), b, L: sn(base.L * u, half), mu: base.mu, noCap: false,
    sw: Math.max(1.5, sn(base.strokeW * u, half)), s: base.s0 * u, sT: null };
}

/** model-unit params for markPaths / TinMark.attach, plus the viewBox that crops model space to the box. */
export function tierModel(B) {
  const t = tierPx(B), u = t.u, half0 = B / (2 * u);
  const m = (v) => v / u;
  const c = 12 - (B / 2 - t.c) / u;
  const p = { c, ko: m(t.ko || 0), a: m(t.a), b: m(t.b), L: m(t.L), mu: t.mu || 0.0001, noCap: t.noCap, strokeW: m(t.sw), s0: m(t.s) };
  const vb = [12 - half0, 12 - half0, B / u, B / u];
  return { t, p, vb, u, sagOnly: t.sT != null, sT: t.sT != null ? m(t.sT) : null, hasMouth: !!t.mu, flat: !!t.flat };
}

/** Params for the in-app Mark (TinMark.tier is wired to this). */
export function tierParams(px) {
  const B = pickTier(px), M = tierModel(B);
  return { ...M.p, viewBox: M.vb.join(' '), B, sagOnly: M.sagOnly, sT: M.sT, hasMouth: M.hasMouth };
}
/** Nearest designed tier for a rendered size. */
export function pickTier(px) {
  const all = TIER_SIZES;
  let best = all[0];
  for (const t of all) if (Math.abs(t - px) < Math.abs(best - px) || (Math.abs(t - px) === Math.abs(best - px) && t < best)) best = t;
  return px > 1024 ? 1024 : best;
}

/* ---- static SVG ---------------------------------------------------------------------------------------- */
const f = (v) => { const r = Math.round(v * 1000) / 1000; return String(r === 0 ? 0 : r); };

/** Geometry for one tier & state: {M, d (markPaths), vb, delta}. state: 'slack' | 'taut' */
export function geometry(B, state = 'slack') {
  const M = tierModel(B), P = { ...M.p };
  let delta = 0;
  if (state === 'taut') {
    if (M.sagOnly) P.s = M.sT;
    else { const q = TinMark.resolve(P); delta = q.DT; P.s = 0; }
  }
  const d = TinMark.markPaths({ ...P, delta });
  return { M, d, delta, P };
}

/** Inner markup (no <svg>) of the mark in model coordinates. variant keys of VARIANTS. */
export function markInner(B, variant = 'color', state = 'slack', ids = {}) {
  const { M, d } = geometry(B, state);
  let V = VARIANTS[variant];
  if (M.flat && !V.mono) V = { ...V, mouth: variant === 'dark' ? PAL.deep : (variant === 'tile' ? PAL.teal : PAL.cream) };   // <= 20 px: mouth is a plain 2-colour hole
  const MIR = 'translate(24 0) scale(-1 1)';
  const str = `<path d="${d.string}" fill="none" stroke="${V.mono ? V.ink : V.str}" stroke-width="${f(M.p.strokeW)}" stroke-linecap="round"/>`;
  let can;
  if (V.mono) {
    const dd = (V.solid || !M.hasMouth) ? d.silhouette : d.mono;
    const ev = V.solid || !M.hasMouth ? '' : ' fill-rule="evenodd"';
    can = (tf) => `<path${tf}${ev} fill="${V.ink}" d="${dd}"/>`;
  } else {
    can = (tf) => `<g${tf}><path fill="${V.can}" d="${d.canLeft}"/><path fill="${V.rim}" d="${d.rim}"/>` +
      (M.hasMouth ? `<path fill="${V.mouth}" d="${d.mouth}"/>` : '') + '</g>';
  }
  return can('') + can(` transform="${MIR}"`) + str;
}

const NSA = 'xmlns="http://www.w3.org/2000/svg"';
/** Standalone bare-mark SVG, width/height = B px. */
export function markSvg(B, variant = 'color', state = 'slack', extra = '') {
  const { M } = geometry(B, state);
  return `<svg ${NSA} width="${B}" height="${B}" viewBox="${M.vb.map(f).join(' ')}"${extra}>${markInner(B, variant, state)}</svg>`;
}

/* ---- tiles (app icons) --------------------------------------------------------------------------------- */
/** Glyph box for a tile of size S (the bare tier used inside it). */
export function tileBox(S, frac = null) {
  const fixed = { 16: 12, 20: 16, 22: 16, 24: 20, 28: 24 };
  if (frac == null && fixed[S]) return fixed[S];
  let B = Math.round(S * (frac ?? 0.72) / 2) * 2;        // even, so the pad is whole px
  return B;
}
export function tileRadius(S, shape = 'rounded') {
  if (shape === 'square') return 0;
  if (shape === 'circle') return S / 2;
  const r = S * 0.2237;
  return S <= 64 ? Math.round(r) : r;
}
/** SVG of the app tile: teal ground + cream cans. shape: rounded|square|circle. B = glyph box override. */
export function tileSvg(S, { shape = 'rounded', B = null, state = 'slack', ground = PAL.teal } = {}) {
  B = B || tileBox(S);
  const pad = (S - B) / 2, r = tileRadius(S, shape);
  const { M } = geometry(B, state);
  const bg = shape === 'square' ? `<rect width="${S}" height="${S}" fill="${ground}"/>`
    : `<rect width="${S}" height="${S}" rx="${f(r)}" fill="${ground}"/>`;
  return `<svg ${NSA} width="${S}" height="${S}" viewBox="0 0 ${S} ${S}">${bg}` +
    `<svg x="${f(pad)}" y="${f(pad)}" width="${B}" height="${B}" viewBox="${M.vb.map(f).join(' ')}">${markInner(B, 'tile', state)}</svg></svg>`;
}
/** Bare mark centred in a larger canvas (tray 22 etc). */
export function paddedSvg(S, B, variant, state = 'slack') {
  const pad = (S - B) / 2, { M } = geometry(B, state);
  return `<svg ${NSA} width="${S}" height="${S}" viewBox="0 0 ${S} ${S}"><svg x="${f(pad)}" y="${f(pad)}" width="${B}" height="${B}" viewBox="${M.vb.map(f).join(' ')}">${markInner(B, variant, state)}</svg></svg>`;
}
