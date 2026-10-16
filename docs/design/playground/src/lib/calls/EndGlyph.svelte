<script lang="ts" module>
  /* The End button's glyph, drawn from the logo (lab: `?lab=endcall`). The apps' `callend` icon ("cans apart,
     strings dropped") reads as a headset, so these are the candidates (user, 2026-10-16: "the logo"):
       slack  the logo in white, its line tight while the call is up; pressing End lets the line go slack
       cut    the logo with its line cut in the middle; pressing End opens the cut
       slash  the logo with the shared 45° off-slash (mic off, camera off)
       phone  today's playground handset, tipped down (reference)
     `?endicon=` picks one for the whole playground; without it the call screen keeps `phone`. */
  export type EndVariant = 'slack' | 'cut' | 'slash' | 'phone';
  export const END_VARIANTS: EndVariant[] = ['slack', 'cut', 'slash', 'phone'];
  const q = new URLSearchParams(location.search).get('endicon');
  export const endVariant: EndVariant = (END_VARIANTS as string[]).includes(q ?? '') ? (q as EndVariant) : 'phone';
</script>

<script lang="ts">
  import Mark from '../brand/Mark.svelte';
  import Icon from '../ui/Icon.svelte';
  import { TinMark } from '../brand/tinmark.js';
  import { tierParams } from '../brand/tiers.js';

  let { variant = endVariant, size = 34, pressed = false }:
    { variant?: EndVariant; size?: number; /** End was pressed: play the line going down. */ pressed?: boolean } = $props();

  /* Cut and slash draw the logo's cans from its own geometry in the tight pose (cans apart, as in A while the call
     is up), so the line runs straight between them. Cut: the line broken in the middle, each end dipping; pressing End
     opens the gap and lets the ends fall. Slash: the whole line, with the off-slash over it. */
  const tp = $derived(tierParams(size));
  const DT = $derived(TinMark.resolve(tp).DT as number);
  const cans = $derived(TinMark.markPaths({ ...tp, s: 0, delta: DT }).mono as string);
  const mx = $derived((tp.c as number) - DT);
  const r = (n: number) => Math.round(n * 100) / 100;
  /** One end of the cut line: straight out of the can's mouth, then dipping `d` at the gap (half-width `g`). */
  function end(right: boolean, g: number, d: number) {
    const f = (x: number) => (right ? 24 - x : x), ex = 12 - g;
    return `M${r(f(mx))} 12Q${r(f(ex - 1.6))} 12 ${r(f(ex))} ${r(12 + d)}`;
  }
  const full = $derived(`M${r(mx)} 12H${r(24 - mx)}`);
  const sw = $derived(Math.max(1.6, (tp.strokeW as number) * 1.15));
</script>

{#if variant === 'phone'}
  <Icon name="phone" size={size - 2} class="hang" />
{:else}
  <span class="eg {variant}" class:pressed style:--s="{size}px">
    {#if variant === 'slack'}
      <Mark {size} live mono state={pressed ? 'slack' : 'taut'} />
    {:else}
      <svg width={size} height={size} viewBox={tp.viewBox} aria-hidden="true" style:--sw={sw}>
        <path class="cans" d={cans} /><path class="cans" d={cans} transform="matrix(-1 0 0 1 24 0)" />
        {#if variant === 'cut'}
          <path class="str" style:d="path('{end(false, pressed ? 2.6 : 1.6, pressed ? 4.4 : 2.1)}')" />
          <path class="str" style:d="path('{end(true, pressed ? 2.6 : 1.6, pressed ? 4.4 : 2.1)}')" />
        {:else}
          <path class="str" d={full} />
          <path class="slash gap" d="M4 3.5L20 20.5" /><path class="slash" d="M4 3.5L20 20.5" />
        {/if}
      </svg>
    {/if}
  </span>
{/if}

<style>
  .eg { position: relative; display: grid; place-items: center; inline-size: var(--s); block-size: var(--s); color: inherit; }
  .eg :global(.mark) { --tm-ink: currentColor; }
  .eg svg { overflow: visible; }
  .cans { fill: currentColor; fill-rule: evenodd; }
  .str, .slash { fill: none; stroke: currentColor; stroke-width: var(--sw); stroke-linecap: round; }
  .str { transition: d var(--dur-switch, 240ms) var(--ease-out); }
  .slash.gap { stroke: var(--danger-btn); stroke-width: calc(var(--sw) * 2.6); }
</style>
