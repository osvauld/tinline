<script lang="ts">
  /* The search screen (route 'search'). Android: full screen, field at the bottom (thumb reach) or the top, results between.
     Desktop never shows this as a screen: search lives in the list pane, so a deep link just hands over to it. */
  import { onMount, tick } from 'svelte';
  import IconButton from '../../lib/ui/IconButton.svelte';
  import SearchField from './SearchField.svelte';
  import SearchResults from './SearchResults.svelte';
  import { ui } from './state.svelte';
  import { nav } from '../../lib/nav/router.svelte';
  import { prefs } from '../../lib/model/prefs.svelte';

  let { q = '' }: { q?: string } = $props();
  let active = $state('');
  const bottom = $derived(ui.position === 'bottom');

  onMount(() => {
    if (q) ui.set(q);
    if (prefs.form === 'desktop') { nav.setTab(nav.tab); tick().then(() => ui.focus()); return; }
    tick().then(() => ui.el?.focus());
  });
  const close = () => { ui.el?.blur(); nav.back(); };
</script>

<div class="scr" class:bottom data-search-screen data-pos={ui.position}>
  {#if !bottom}
    <header class="bar top"><IconButton icon="back" label="Back" onclick={close} /><div class="fld"><SearchField placeholder="Search" size={48} onescape={close} activeId={active} /></div></header>
  {/if}
  <div class="body"><SearchResults onactive={(id) => (active = id)} /></div>
  {#if bottom}
    <footer class="bar dock"><IconButton icon="back" label="Back" onclick={close} /><div class="fld"><SearchField placeholder="Search" size={48} onescape={close} activeId={active} /></div></footer>
  {/if}
  {#if ui.kbd}<div class="kbd" aria-hidden="true"><span>Soft keyboard (mock)</span></div>{/if}
</div>

<style>
  .scr { display: flex; flex-direction: column; block-size: 100%; min-block-size: 0; background: var(--bg); }
  .body { flex: 1; min-block-size: 0; }
  /* With the field at the bottom, the results start at the top edge: give them the status-bar breathing room. */
  .scr.bottom .body { padding-block-start: 12px; }
  .bar { display: flex; align-items: center; gap: 4px; flex: none; padding: 8px 12px 8px 4px; }
  .top { padding-block-start: 8px; }
  .dock { padding-block-end: calc(8px + env(safe-area-inset-bottom)); border-block-start: 1px solid var(--hairline); background: var(--surface); }
  .fld { flex: 1; min-inline-size: 0; }
  .kbd { flex: none; block-size: 272px; display: grid; place-items: center; background: var(--surface-2); color: var(--muted); font-size: var(--fs-sub); border-block-start: 1px solid var(--hairline); }
</style>
