/* Motion tokens: the ONLY place durations and easings live. Mirrored to CSS custom properties
   (--dur-*, --ease-*) by applyMotionVars(), so CSS and JS can never drift apart.
   Spec: audit report-motion §5b. */

export const EASE = {
  out: 'cubic-bezier(.2,.8,.2,1)',
  in: 'cubic-bezier(.4,0,1,1)',
  inOut: 'cubic-bezier(.4,0,.2,1)',
} as const;

export const DUR = {
  press: 100,          // scale .97 in
  release: 140,        // back to 1
  switch: 120,
  crossfade: 120,      // icon / status text swaps
  enter: 200,          // dialogs, sheets, menus in
  exit: 140,           // dialogs, sheets, menus out
  overlayOpen: 240,    // call overlay from button/strip
  overlayClose: 220,   // minimize
  stripIn: 160,
  callEnd: 300,        // total release on End (string release + fade)
  bubbleIn: 160,
  /** Measured: the mark's string reaches taut this long after connect() (verify.js check e). */
  markHit: 217,
} as const;

export type DurName = keyof typeof DUR;

/** Slow-motion multiplier from the Simulation drawer (1 = normal, 4 = judge jank). */
let scale = 1;
export function setTimeScale(s: number) { scale = s; applyMotionVars(); }
export function timeScale() { return scale; }
export const dur = (n: DurName) => DUR[n] * scale;

export function applyMotionVars(root: HTMLElement = document.documentElement) {
  for (const [k, v] of Object.entries(DUR)) root.style.setProperty(`--dur-${k}`, `${v * scale}ms`);
  for (const [k, v] of Object.entries(EASE)) root.style.setProperty(`--ease-${k}`, v);
}
