<script lang="ts">
  /* A screen not ported yet. On desktop the detail pane has no shell bar, so it carries its own Back. */
  import IconButton from '../lib/ui/IconButton.svelte';
  import { nav } from '../lib/nav/router.svelte';
  import { prefs } from '../lib/model/prefs.svelte';
  let { title, step }: { title: string; step: string } = $props();
</script>
{#if prefs.form === 'desktop' && nav.stack.length > 1}<div class="bk"><IconButton icon="back" label="Back" onclick={() => nav.back()} /></div>{/if}
<div class="ph"><h2>{title}</h2><p>Not ported yet — PLAN.md step {step}.</p></div>
<style>
  .bk { position: absolute; inset-block-start: 8px; inset-inline-start: 8px; z-index: 1; }
  :global([dir='rtl']) .bk :global(svg) { transform: scaleX(-1); }
  .ph { display: grid; place-content: center; text-align: center; height: 100%; color: var(--muted); padding: 24px; }
  h2 { color: var(--ink); font-size: var(--fs-title); margin: 0 0 4px; }
  p { margin: 0; font-size: var(--fs-sub); }
</style>
