/* Enter/exit for menus, sheets and dialogs. Durations come ONLY from the motion table (dur()),
   and collapse to 0 under reduced motion. */
import { cubicIn, cubicOut } from 'svelte/easing';
import { dur } from '../../lib/fx/motion';
import { motion } from '../../lib/fx/reduced.svelte';

type Kind = 'sheet' | 'pop' | 'dialog' | 'scrim';
const d = (n: 'enter' | 'exit') => (motion.reduced ? 0 : dur(n));

export function layerIn(_node: Element, { kind }: { kind: Kind }) {
  return {
    duration: d('enter'), easing: cubicOut,
    css: (t: number, u: number) =>
      kind === 'scrim' || kind === 'pop' ? `opacity:${t}`
        : kind === 'sheet' ? `transform:translateY(${u * 24}px);opacity:${0.4 + 0.6 * t}`
          : `transform:translateY(${u * 8}px) scale(${0.97 + 0.03 * t});opacity:${t}`,
  };
}

export function layerOut(_node: Element, { kind }: { kind: Kind }) {
  return {
    duration: d('exit'), easing: cubicIn,
    css: (t: number, u: number) =>
      kind === 'scrim' || kind === 'pop' ? `opacity:${t}`
        : kind === 'sheet' ? `transform:translateY(${u * 24}px);opacity:${t}`
          : `transform:scale(${0.97 + 0.03 * t});opacity:${t}`,
  };
}
