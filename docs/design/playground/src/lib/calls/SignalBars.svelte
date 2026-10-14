<script lang="ts">
  /* Signal: four rising bars with round ends, the shape everyone already reads as "how good is the line".
     Lit bars are ink; the rest stay as faint ghosts so the glyph never changes size. No signal yet (still
     connecting) = all ghosts; reconnecting = a slow wave runs through the ghosts, nothing lit.
     Port: a 24×24 box; bars 3 wide at x 3.5 / 8.5 / 13.5 / 18.5, heights 6 / 10 / 14 / 18, bottoms on y 20,
     radius 1.5; ghost alpha .28; lit changes fade over 300 ms (one bar at a time reads as calm). */
  import type { Strength } from './link.svelte';
  let { level = 0, wave = false, size = 24 }: { level?: Strength | 0; wave?: boolean; size?: number } = $props();
  const H = [6, 10, 14, 18];
</script>

<svg class="sb" class:wave width={size} height={size} viewBox="0 0 24 24" aria-hidden="true" focusable="false">
  {#each H as h, i (i)}
    <rect x={3.5 + i * 5} y={20 - h} width="3" height={h} rx="1.5" class:on={!wave && i < level} style="--i:{i}" />
  {/each}
</svg>

<style>
  .sb { flex: none; display: block; }
  rect { fill: currentColor; fill-opacity: .28; transition: fill-opacity 300ms var(--ease-out); }
  rect.on { fill-opacity: 1; }
  .wave rect { animation: sbw 1.6s var(--ease-in-out, ease-in-out) infinite; animation-delay: calc(var(--i) * 160ms); }
  @keyframes sbw { 0%, 60%, 100% { fill-opacity: .28; } 25% { fill-opacity: .7; } }
  :global(:root[data-motion='reduce']) .wave rect { animation: none; }
</style>
