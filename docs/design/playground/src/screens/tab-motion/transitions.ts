import { cubicOut } from 'svelte/easing';
import type { TransitionConfig } from 'svelte/transition';
import { dur } from '../../lib/fx/motion';
import { motion } from '../../lib/fx/reduced.svelte';
import { fadeThroughIn, fadeThroughOut } from '../../lib/fx/transitions';

export type Treatment = 'through' | 'crossfade' | 'slide';
type Params = { treatment: Treatment; direction: number };

export function panelIn(node: Element, { treatment, direction }: Params): TransitionConfig {
  if (motion.reduced) return { duration: 0 };
  if (treatment === 'through') return fadeThroughIn(node);
  return {
    duration: dur(treatment === 'crossfade' ? 'crossfade' : 'tabIn'),
    easing: cubicOut,
    css: (t) => `opacity:${t};transform:translateX(${treatment === 'slide' ? direction * 16 * (1 - t) : 0}px)`,
  };
}

export function panelOut(node: Element, { treatment, direction }: Params): TransitionConfig {
  if (motion.reduced) return { duration: 0 };
  if (treatment === 'through') return fadeThroughOut(node);
  return {
    duration: dur(treatment === 'crossfade' ? 'crossfade' : 'tabOut'),
    easing: cubicOut,
    css: (t) => `opacity:${t};transform:translateX(${treatment === 'slide' ? -direction * 16 * (1 - t) : 0}px)`,
  };
}
