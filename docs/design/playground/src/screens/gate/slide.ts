/* Step transition: forward slides one way, back the other, in inline direction (RTL-aware). Durations
   from motion.ts only; reduced motion makes it a plain cut (zero duration). */
import { dur } from '../../lib/fx/motion';
import { motion } from '../../lib/fx/reduced.svelte';

const DIST = 36; // px travelled; the rest is opacity

export function stepSlide(_node: Element, p: { dir: 1 | -1; out?: boolean }) {
  const rtl = getComputedStyle(document.documentElement).direction === 'rtl';
  const sign = (rtl ? -1 : 1) * p.dir;
  return {
    duration: motion.reduced ? 0 : dur(p.out ? 'exit' : 'enter'),
    easing: (t: number) => 1 - Math.pow(1 - t, 3),
    css: (t: number, u: number) => `opacity:${t};transform:translateX(${(p.out ? -sign : sign) * u * DIST}px)`,
  };
}
