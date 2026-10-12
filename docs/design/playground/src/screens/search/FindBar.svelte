<script lang="ts">
  /* The find bar of a conversation: it covers the header (same box, so nothing moves). n/m counts messages, newest first: 1/5 is the newest.
     Up = older, Down = newer; Enter = older, Shift+Enter = newer; Esc closes. */
  import { onMount, tick } from 'svelte';
  import { fade } from 'svelte/transition';
  import Icon from '../../lib/ui/Icon.svelte';
  import { motion } from '../../lib/fx/reduced.svelte';
  import { dur } from '../../lib/fx/motion';
  import { find } from './find.svelte';

  let { count, onclose }: { count: number; onclose: () => void } = $props();
  let input = $state<HTMLInputElement>();
  const idx = $derived(Math.max(0, Math.min(find.cur, count - 1)));
  const shown = $derived(count ? count - idx : 0);
  function step(older: boolean) {
    if (!count) return;
    find.cur = older ? (idx - 1 + count) % count : (idx + 1) % count;
  }
  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); onclose(); }
    else if (e.key === 'Enter') { e.preventDefault(); step(!e.shiftKey); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); step(true); }
    else if (e.key === 'ArrowDown') { e.preventDefault(); step(false); }
  }
  onMount(() => { input?.focus(); input?.select(); });
  $effect(() => { find.focusReq; tick().then(() => { input?.focus(); input?.select(); }); });
</script>

<div class="fb" role="search" aria-label="Find in this chat" transition:fade={{ duration: motion.reduced ? 0 : dur('crossfade') }}>
  <div class="in">
    <span class="mag"><Icon name="search" size={18} /></span>
    <input bind:this={input} type="text" aria-label="Find in this chat" placeholder="Find in this chat" autocomplete="off" spellcheck="false"
      value={find.q} oninput={(e) => find.setQ(e.currentTarget.value)} {onkeydown} />
    <span class="cnt" aria-live="polite" aria-label={count ? `Match ${shown} of ${count}` : 'No matches'}>{shown}/{count}</span>
  </div>
  <button type="button" class="b press" aria-label="Older match" disabled={!count} onclick={() => step(true)}><span class="up"><Icon name="chev" size={20} /></span></button>
  <button type="button" class="b press" aria-label="Newer match" disabled={!count} onclick={() => step(false)}><Icon name="chev" size={20} /></button>
  <button type="button" class="b press" aria-label="Close find" onclick={onclose}><Icon name="x" size={20} /></button>
</div>

<style>
  .fb { position: absolute; inset: 0; z-index: 6; display: flex; align-items: center; gap: 2px; padding: 8px; background: var(--surface); }
  .in { flex: 1; min-inline-size: 0; display: flex; align-items: center; block-size: 44px; border-radius: 22px; background: var(--surface-2); box-shadow: inset 0 0 0 1px transparent;
    transition: box-shadow var(--dur-switch) var(--ease-out); }
  .in:focus-within { box-shadow: inset 0 0 0 2px var(--accent); }
  .mag { inline-size: 44px; display: grid; place-items: center; color: var(--muted); flex: none; }
  input { flex: 1; min-inline-size: 0; block-size: 100%; border: 0; background: transparent; padding: 0; font-size: var(--fs-body); color: var(--ink); }
  input:focus, input:focus-visible { outline: 0; }
  input::placeholder { color: var(--muted); opacity: 1; }
  .cnt { flex: none; inline-size: 64px; padding-inline-end: 14px; text-align: end; font-size: var(--fs-sub); color: var(--muted); font-variant-numeric: tabular-nums; white-space: nowrap; }
  .b { flex: none; inline-size: 44px; block-size: 44px; border: 0; border-radius: 50%; background: transparent; color: var(--ink); display: grid; place-items: center; cursor: pointer; }
  .b:hover:not(:disabled) { background: var(--surface-2); }
  .b:disabled { opacity: .4; cursor: default; }
  .up { display: grid; rotate: 180deg; }
</style>
