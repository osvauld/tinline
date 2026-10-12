/* Call overlay <-> strip transitions (audit report-motion §5b). Durations come from fx/motion.ts;
   the two spec offsets that are not in that table yet are named here (request: move into DUR).
   Reduced motion: opacity only, crossfade length. Easing curves are the exact EASE tokens. */
import { dur, EASE, timeScale } from '../fx/motion';
import { motion } from '../fx/reduced.svelte';
import { call } from './call.svelte';

/** Spec 5b offsets (ms): strip slide starts this long after the overlay begins to minimize; End's fade starts after the string release begins. */
export const OFFSET = { stripIn: 80, endFade: 60, avatarIn: 40 } as const;
export const STRIP_H = 44;

function bezier(str: string) {
  const [x1, y1, x2, y2] = str.match(/-?[\d.]+/g)!.map(Number);
  const cx = 3 * x1, bx = 3 * (x2 - x1) - cx, ax = 1 - cx - bx;
  const cy = 3 * y1, by = 3 * (y2 - y1) - cy, ay = 1 - cy - by;
  const X = (t: number) => ((ax * t + bx) * t + cx) * t, Y = (t: number) => ((ay * t + by) * t + cy) * t;
  return (x: number) => {
    let t = x;
    for (let i = 0; i < 8; i++) { const e = X(t) - x; if (Math.abs(e) < 1e-5) break; const d = (3 * ax * t + 2 * bx) * t + cx; if (Math.abs(d) < 1e-6) break; t -= e / d; }
    return Y(Math.min(1, Math.max(0, t)));
  };
}
export const easeOut = bezier(EASE.out), easeIn = bezier(EASE.in), easeInOut = bezier(EASE.inOut);

/** Open from the call button or the strip: 240 ms ease-out, rise 16px. */
export function overlayIn(_n: Element) {
  if (motion.reduced) return { duration: dur('crossfade'), css: (t: number) => `opacity:${t}` };
  return { duration: dur('overlayOpen'), easing: easeOut, css: (t: number, u: number) => `opacity:${t};transform:translateY(${16 * u}px)` };
}
/** Leaves either by minimizing (220 ms ease-in-out, up 24px) or by ending (fade 180 ms ease-out after +60 ms, scale .98). */
export function overlayOut(_n: Element) {
  const ending = call.phase === 'ended' || call.phase === 'failed' || call.phase === 'idle';
  if (motion.reduced) return { duration: dur('crossfade'), css: (t: number) => `opacity:${t}` };
  if (ending) return { delay: OFFSET.endFade * timeScale(), duration: dur('callEnd') * 0.6, easing: easeOut, css: (t: number) => `opacity:${t};transform:scale(${0.98 + 0.02 * t})` };
  return { duration: dur('overlayClose'), easing: easeInOut, css: (t: number, u: number) => `opacity:${t};transform:translateY(${-24 * u}px)` };
}

/** In-flow strip: grows from 0 height so the app below it moves down smoothly, starting after the overlay begins to leave. */
export function stripIn(_n: Element) {
  if (motion.reduced) return { duration: dur('crossfade'), css: (t: number) => `opacity:${t}` };
  return { delay: OFFSET.stripIn * timeScale(), duration: dur('stripIn'), easing: easeOut,
    css: (t: number) => `height:${STRIP_H * t}px;opacity:${t};overflow:hidden` };
}
export function stripOut(_n: Element) {
  if (motion.reduced) return { duration: dur('crossfade'), css: (t: number) => `opacity:${t}` };
  return { duration: dur('stripIn'), easing: easeInOut, css: (t: number) => `height:${STRIP_H * t}px;opacity:${t};overflow:hidden` };
}

/** Heads-up banner / desktop card. */
export function alertIn(_n: Element) {
  if (motion.reduced) return { duration: dur('crossfade'), css: (t: number) => `opacity:${t}` };
  return { duration: dur('enter'), easing: easeOut, css: (t: number, u: number) => `opacity:${t};transform:translateY(${-20 * u}px)` };
}
export function alertOut(_n: Element) {
  if (motion.reduced) return { duration: dur('crossfade'), css: (t: number) => `opacity:${t}` };
  return { duration: dur('exit'), easing: easeIn, css: (t: number, u: number) => `opacity:${t};transform:translateY(${-12 * u}px)` };
}

/* System surfaces (outside the app). Reduced motion: opacity only. */

/** Android heads-up card: slides 14px down from the top edge and fades; leaves 10px up. */
export function headsUpIn(_n: Element) {
  if (motion.reduced) return { duration: dur('crossfade'), css: (t: number) => `opacity:${t}` };
  return { duration: dur('enter'), easing: easeOut, css: (t: number, u: number) => `opacity:${t};transform:translateY(${-14 * u}px)` };
}
export function headsUpOut(_n: Element) {
  if (motion.reduced) return { duration: dur('crossfade'), css: (t: number) => `opacity:${t}` };
  return { duration: dur('exit'), easing: easeIn, css: (t: number, u: number) => `opacity:${t};transform:translateY(${-10 * u}px)` };
}

/** Desktop toast / mini window: rises 14px from the bottom edge and fades. */
export function toastIn(_n: Element) {
  if (motion.reduced) return { duration: dur('crossfade'), css: (t: number) => `opacity:${t}` };
  return { duration: dur('enter'), easing: easeOut, css: (t: number, u: number) => `opacity:${t};transform:translateY(${14 * u}px)` };
}
export function toastOut(_n: Element) {
  if (motion.reduced) return { duration: dur('crossfade'), css: (t: number) => `opacity:${t}` };
  return { duration: dur('exit'), easing: easeIn, css: (t: number, u: number) => `opacity:${t};transform:translateY(${10 * u}px)` };
}

/** Notification shade: pulled down from the top edge with the overlay duration; leaves a little faster. */
export function shadeIn(_n: Element) {
  if (motion.reduced) return { duration: dur('crossfade'), css: (t: number) => `opacity:${t}` };
  return { duration: dur('overlayOpen'), easing: easeOut, css: (t: number, u: number) => `opacity:${Math.min(1, t * 2)};transform:translateY(${-100 * u}%)` };
}
export function shadeOut(_n: Element) {
  if (motion.reduced) return { duration: dur('crossfade'), css: (t: number) => `opacity:${t}` };
  return { duration: dur('overlayClose'), easing: easeInOut, css: (t: number, u: number) => `opacity:${Math.min(1, t * 2)};transform:translateY(${-100 * u}%)` };
}
/** The dimmed scrim behind the shade. */
export function scrimFade(_n: Element) {
  return { duration: motion.reduced ? dur('crossfade') : dur('overlayOpen'), easing: easeOut, css: (t: number) => `opacity:${t}` };
}

/** Call-waiting card on the call screen: eases down 12px from the name area and fades; leaves up 8px a little faster. */
export function waitIn(_n: Element) {
  if (motion.reduced) return { duration: dur('crossfade'), css: (t: number) => `opacity:${t}` };
  return { duration: dur('enter'), easing: easeOut, css: (t: number, u: number) => `opacity:${t};transform:translateY(${-12 * u}px)` };
}
export function waitOut(_n: Element) {
  if (motion.reduced) return { duration: dur('crossfade'), css: (t: number) => `opacity:${t}` };
  return { duration: dur('exit'), easing: easeIn, css: (t: number, u: number) => `opacity:${t};transform:translateY(${-8 * u}px)` };
}
