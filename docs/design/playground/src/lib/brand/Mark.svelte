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
  let { size = 24, state = 'slack', live = false, mono = false, label, sway, dim = false, spring, ontug, lineColor = false }:
    { size?: number; /** Spring overrides for one moment (e.g. Done's longer pull); calls use the defaults. */ spring?: Record<string, number>; /** Sign-up marks: the slack line is a faded gold that warms when it pulls tight. */ lineColor?: boolean; /** Each tug of the string going tight (1, 2, …). */ ontug?: (n: number) => void; state?: MarkState; live?: boolean; mono?: boolean; label?: string;
      /** Ringing sway: period (s) and amplitude (deg). Lock the period to the cue that's playing. */
      sway?: { period: number; amp: number }; dim?: boolean } = $props();

  let svg: SVGSVGElement;
  let api: any = null;
  // the line has gone tight (colour); set on the element directly: the `state` prop shadows the $state rune here
  const setTaut = (on: boolean) => svg?.classList.toggle('taut', on);

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
    api = TinMark.attach(svg, { params: { ...params, ...(spring || {}), ...(sway ? { T: sway.period, phi0: sway.amp } : {}) }, reduced: motion.reduced, timeScale: timeScale() });
    api.on('hit', () => emitFx({ type: 'taut-hit' }));
    api.on('tug', (e: { n: number }) => { setTaut(true); ontug?.(e.n); });
    api.set(state === 'taut' ? 'taut' : 'slack'); setTaut(state === 'taut');
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
    else { api.ring(false); api.end(); setTaut(false); }
    prev = s;
  });

  /** Pluck the string (taut: twang; slack: a little swing). */
  export function pluck() { api?.pluck(); }
</script>

<svg bind:this={svg} class="mark" class:dim class:live-line={lineColor} width={size} height={size} viewBox="0 0 24 24"
  role={label ? 'img' : undefined} aria-label={label} aria-hidden={label ? undefined : 'true'}></svg>

<style>
  .mark { flex: none; overflow: visible; transition: opacity var(--dur-crossfade) var(--ease-out);
    --tm-can: var(--accent); --tm-mouth: #9fd6c8; --tm-str: var(--gold); --tm-ink: currentColor; }
  .mark.dim { opacity: .45; --tm-str: var(--muted); }
  /* Line colour: a slack line is a soft, faded gold; it warms to full gold the moment it pulls tight. */
  .mark.live-line { --tm-str: color-mix(in oklab, var(--gold) 55%, var(--line)); }
  .mark.live-line > :global(path) { transition: stroke var(--dur-switch, 240ms) var(--ease-out); }
  .mark.live-line:global(.taut) { --tm-str: var(--gold); }
</style>
