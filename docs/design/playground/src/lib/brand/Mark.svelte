<script lang="ts">
  /* The brand mark. Static (`state` slack/taut) or live (`live`): then state changes animate via
     the spring model and reduced motion is honoured. Emits fx `taut-hit` when the string goes tight. */
  import { onMount } from 'svelte';
  import { TinMark } from './tinmark.js';
  import { motion } from '../fx/reduced.svelte';
  import { emitFx } from '../fx/bus';
  import { timeScale } from '../fx/motion';

  type MarkState = 'slack' | 'ringing' | 'taut';
  let { size = 24, state = 'slack', live = false, mono = false, label }:
    { size?: number; state?: MarkState; live?: boolean; mono?: boolean; label?: string } = $props();

  let svg: SVGSVGElement;
  let api: any = null;

  onMount(() => {
    const params = { ...TinMark.tier(size), mono };
    if (!live) { TinMark.render(svg, { ...params, s: state === 'taut' ? 0 : undefined }); return; }
    api = TinMark.attach(svg, { params, reduced: motion.reduced, timeScale: timeScale() });
    api.on('hit', () => emitFx({ type: 'taut-hit' }));
    api.set(state === 'taut' ? 'taut' : 'slack');
    if (state === 'ringing') api.ring(true);
    return () => api?.destroy();
  });

  let prev: MarkState | null = null;
  $effect(() => {
    const s = state;
    if (!api) return;
    api.setReduced(motion.reduced);
    api.timeScale = timeScale();
    if (prev === null) { prev = s; return; }
    if (s === prev) return;
    if (s === 'taut') api.connect();
    else if (s === 'ringing') api.ring(true);
    else { api.ring(false); api.end(); }
    prev = s;
  });

  /** Pluck the string (taut: twang; slack: a little swing). */
  export function pluck() { api?.pluck(); }
</script>

<svg bind:this={svg} class="mark" width={size} height={size} viewBox="0 0 24 24"
  role={label ? 'img' : undefined} aria-label={label} aria-hidden={label ? undefined : 'true'}></svg>

<style>
  .mark { flex: none; overflow: visible;
    --tm-can: var(--accent); --tm-mouth: #9fd6c8; --tm-str: var(--gold); --tm-ink: currentColor; }
</style>
