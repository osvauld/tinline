/* Shared Svelte transitions. Durations come from motion.ts; reduced motion makes them instant. */
import { cubicOut, cubicIn } from 'svelte/easing';
import { dur } from './motion';
import { motion } from './reduced.svelte';

/** Fade-through, incoming half: waits for the outgoing half, then fades in from 98 % scale.
    For peer destinations (tabs) where a slide would imply a direction that isn't there. */
export function fadeThroughIn(_n: Element) {
  if (motion.reduced) return { duration: 0 };
  return { delay: dur('tabOut'), duration: dur('tabIn'), easing: cubicOut,
    css: (t: number) => `opacity:${t};transform:scale(${0.98 + 0.02 * t})` };
}
/** Fade-through, outgoing half: a quick fade, no movement. */
export function fadeThroughOut(_n: Element) {
  if (motion.reduced) return { duration: 0 };
  return { duration: dur('tabOut'), easing: cubicIn, css: (t: number) => `opacity:${t}` };
}
