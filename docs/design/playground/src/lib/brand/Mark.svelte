<script lang="ts">
  /* The brand mark. Static (`state` slack/taut) or live (`live`): then state changes animate via
     the spring model and reduced motion is honoured. Emits fx `taut-hit` when the string goes tight. */
  import { onMount } from 'svelte';
  import { TinMark } from './tinmark.js';
  import { tierParams } from './tiers.js';
  import { motion } from '../fx/reduced.svelte';
  import { emitFx, onFx } from '../fx/bus';
  import { timeScale } from '../fx/motion';

  type MarkState = 'slack' | 'ringing' | 'taut';
  let { size = 24, state = 'slack', live = false, mono = false, label, sway, dim = false }:
    { size?: number; state?: MarkState; live?: boolean; mono?: boolean; label?: string;
      /** Ringing sway: period (s) and amplitude (deg). Lock the period to the cue that's playing. */
      sway?: { period: number; amp: number }; dim?: boolean } = $props();

  let svg: SVGSVGElement;
  let api: any = null;

  onMount(() => {
    // optical-size tier for the rendered size (hinted small tiers: see MARK-MATH.md section 4)
    const params = { ...tierParams(size), mono };
    if (!live) {
      const taut = state === 'taut';
      if (!taut) TinMark.render(svg, params);
      else if (params.sagOnly) TinMark.render(svg, { ...params, s: params.sT });
      else TinMark.render(svg, { ...params, s: 0, delta: TinMark.resolve(params).DT });
      return;
    }
    api = TinMark.attach(svg, { params: { ...params, ...(sway ? { T: sway.period, phi0: sway.amp } : {}) }, reduced: motion.reduced, timeScale: timeScale() });
    api.on('hit', () => emitFx({ type: 'taut-hit' }));
    api.set(state === 'taut' ? 'taut' : 'slack');
    if (state === 'ringing') api.ring(true);
    // silent send pluck: only a visible, live mark answers; reduced motion does nothing (tinmark ignores it)
    const off = onFx((e) => { if (e.type === 'sent' && svg.isConnected && svg.getClientRects().length && getComputedStyle(svg).visibility !== 'hidden') api?.pluck(); });
    return () => { off(); api?.destroy(); };
  });

  let prev: MarkState | null = null;
  $effect(() => {
    const sw = sway;
    if (api && sw) api.setParams({ T: sw.period, phi0: sw.amp });
  });
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

<svg bind:this={svg} class="mark" class:dim width={size} height={size} viewBox="0 0 24 24"
  role={label ? 'img' : undefined} aria-label={label} aria-hidden={label ? undefined : 'true'}></svg>

<style>
  .mark { flex: none; overflow: visible; transition: opacity var(--dur-crossfade) var(--ease-out);
    --tm-can: var(--accent); --tm-mouth: #9fd6c8; --tm-str: var(--gold); --tm-ink: currentColor; }
  .mark.dim { opacity: .45; --tm-str: var(--muted); }
</style>
