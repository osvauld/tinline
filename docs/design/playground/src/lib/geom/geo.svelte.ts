/* Shared geometry state: the corner-smoothing amount (live-switchable from the drawer, persisted) and the radius tokens
   read once from tokens.css, so the CSS fallback and the Bézier shapes always use the same numbers. */
import type { Radii } from './shape';

const KEY = 'pg.smooth';
const css = (name: string, fallback: number) => {
  if (typeof document === 'undefined') return fallback;
  const v = parseFloat(getComputedStyle(document.documentElement).getPropertyValue(name));
  return Number.isFinite(v) ? v : fallback;
};
const saved = (() => { try { const v = parseFloat(localStorage.getItem(KEY) ?? ''); return Number.isFinite(v) ? v : null; } catch { return null; } })();

export const geo = $state({ smooth: saved ?? css('--smooth', 0.6) });

export function setSmooth(v: number) {
  geo.smooth = Math.min(1, Math.max(0, v));
  document.documentElement.style.setProperty('--smooth', String(geo.smooth));
  try { localStorage.setItem(KEY, String(geo.smooth)); } catch { /* private mode */ }
}
if (typeof document !== 'undefined') document.documentElement.style.setProperty('--smooth', String(geo.smooth));

let cache: Record<'bub' | 'tight' | 'quote' | 'ctl' | 'field' | 'sheet', number> | undefined;
/* Radius tokens in px (tokens.css: --r-bub, --r-bub-tight, --r-quote, --r-ctl, --r-field, --r-sheet). */
export function R() {
  return (cache ??= {
    bub: css('--r-bub', 18), tight: css('--r-bub-tight', 6), quote: css('--r-quote', 12),
    ctl: css('--r-ctl', 14), field: css('--r-field', 22), sheet: css('--r-sheet', 24),
  });
}
export const uniform = (r: number): Radii => [r, r, r, r];
if (typeof window !== 'undefined') (window as unknown as { __geo: unknown }).__geo = { geo, setSmooth };   // test hook
