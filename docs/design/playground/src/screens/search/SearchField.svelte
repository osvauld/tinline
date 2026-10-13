<script lang="ts">
  /* The search field (one at a time: the desktop list pane, or the Android search screen). Reads and writes `ui.q`.
     The clear button stays in the layout (fades in), so typing never moves the field or its text. */
  import { onDestroy, onMount } from 'svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import { ui } from './state.svelte';

  let { placeholder = 'Search', size = 44, onescape, listId = 'sr-list', activeId = '' }:
    { placeholder?: string; size?: number; onescape?: () => void; listId?: string; activeId?: string } = $props();

  let input = $state<HTMLInputElement>();
  onMount(() => { ui.el = input ?? null; });
  onDestroy(() => { if (ui.el === input) { ui.el = null; ui.focused = false; } });

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') { e.preventDefault(); onescape?.(); return; }
    if (e.isComposing) return;
    if (ui.keys?.(e)) e.preventDefault();
  }
</script>

<div class="sf" class:has={ui.q !== ''} style:--h="{size}px">
  <span class="mag"><Icon name="search" size={20} /></span>
  <input
    bind:this={input} bind:value={ui.q} type="text" role="combobox" aria-label="Search" aria-autocomplete="list" aria-expanded={ui.q.trim() !== ''}
    aria-controls={listId} aria-activedescendant={activeId || undefined}
    {placeholder} autocomplete="off" autocapitalize="off" spellcheck="false" enterkeyhint="search"
    onfocus={() => (ui.focused = true)} onblur={() => (ui.focused = false)} {onkeydown}
  />
  <button type="button" class="clr" aria-label="Clear search" tabindex={ui.q ? 0 : -1} aria-hidden={ui.q ? undefined : 'true'}
    onmousedown={(e) => e.preventDefault()} onclick={() => { ui.clear(); input?.focus(); }}><Icon name="x" size={18} /></button>
</div>

<style>
  .sf { position: relative; display: flex; align-items: center; block-size: var(--h); border-radius: calc(var(--h) / 2); background: var(--surface-2);
    box-shadow: inset 0 0 0 1px transparent; transition: box-shadow var(--dur-switch) var(--ease-out), background-color var(--dur-switch) var(--ease-out); }
  .sf:focus-within { box-shadow: inset 0 0 0 2px var(--accent); background: var(--surface); }
  .mag { display: grid; place-items: center; inline-size: 44px; flex: none; color: var(--muted); }
  input { flex: 1; min-inline-size: 0; block-size: 100%; border: 0; background: transparent; padding: 0; font-size: var(--fs-body); color: var(--ink); text-overflow: ellipsis; }
  input:focus, input:focus-visible { outline: 0; }
  input::placeholder { color: var(--muted); opacity: 1; }
  .clr { position: relative; flex: none; inline-size: 36px; block-size: 36px; margin-inline: 2px 4px; border: 0; border-radius: 50%; background: transparent; color: var(--muted); display: grid; place-items: center;
    cursor: pointer; opacity: 0; pointer-events: none; transition: opacity var(--dur-crossfade) var(--ease-out), background-color var(--dur-switch) var(--ease-out); }
  .clr::after { content: ''; position: absolute; inset: -4px; }   /* 44px hit area */
  .has .clr { opacity: 1; pointer-events: auto; }
  .clr:hover { background: var(--hairline); color: var(--ink); }
</style>
