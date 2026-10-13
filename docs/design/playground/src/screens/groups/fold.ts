/* Height + opacity transition for list inserts and removals. Durations from the motion table;
   instant under reduced motion. Block-level elements only (li, section, div). */
import { cubicIn, cubicOut } from 'svelte/easing';
import { dur } from '../../lib/fx/motion';
import { motion } from '../../lib/fx/reduced.svelte';

export function fold(node: Element, { out = false }: { out?: boolean } = {}) {
  if (motion.reduced) return { duration: 0 };
  const h = (node as HTMLElement).offsetHeight;
  return {
    duration: dur(out ? 'exit' : 'enter'),
    easing: out ? cubicIn : cubicOut,
    css: (t: number) => `overflow:hidden;height:${(t * h).toFixed(2)}px;opacity:${t}`,
  };
}
export const foldOut = (node: Element) => fold(node, { out: true });

/** Flip duration for keyed lists that re-order (newest activity first). */
export const flipMs = () => (motion.reduced ? 0 : dur('enter'));
