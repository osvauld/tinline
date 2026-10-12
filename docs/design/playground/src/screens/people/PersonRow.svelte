<script lang="ts">
  /* One person in any list (Contacts, New chat, Blocked, Who can call me). The row is a button when it goes somewhere;
     `end` is an optional trailing control (a Switch, an Unblock button) that is NOT inside the button. Height is fixed
     (64 px) so a changing subtitle never moves the list. */
  import type { Snippet } from 'svelte';
  import Avatar from '../../lib/ui/Avatar.svelte';
  import { displayName } from '../../lib/model/store.svelte';
  import type { Person } from '../../lib/model/types';

  let { p, sub = '', onclick, selected = false, dim = false, end, pad = 8 }: {
    p: Person; sub?: string; onclick?: () => void; selected?: boolean; dim?: boolean; end?: Snippet; pad?: number;
  } = $props();
</script>

<div class="pr" class:sel={selected} class:dim style:padding-inline={pad + 'px'} data-person-row={p.id}>
  {#if onclick}
    <button type="button" class="main press" onclick={onclick}>
      <Avatar name={p.name} hue={p.hue} person={p.id} size={44} />
      <span class="t"><b dir="auto">{displayName(p.id)}</b><small dir="auto">{sub}</small></span>
    </button>
  {:else}
    <div class="main">
      <Avatar name={p.name} hue={p.hue} person={p.id} size={44} />
      <span class="t"><b dir="auto">{displayName(p.id)}</b><small dir="auto">{sub}</small></span>
    </div>
  {/if}
  {#if end}<span class="end">{@render end()}</span>{/if}
</div>

<style>
  .pr { display: flex; align-items: center; gap: 8px; min-block-size: 64px; border-radius: var(--r-ctl); }
  .pr:has(button.main:hover) { background: var(--surface-2); }
  .pr.sel { background: var(--accent-soft); }
  .main { flex: 1; min-inline-size: 0; display: flex; align-items: center; gap: 12px; min-block-size: 64px; border: 0; background: none; padding: 0; text-align: start; font: inherit; color: inherit; }
  button.main { cursor: pointer; border-radius: var(--r-ctl); }
  .t { flex: 1; min-inline-size: 0; display: grid; gap: 1px; }
  .t b { font-weight: 600; line-height: 21px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .t small { font-size: var(--fs-sub); line-height: 18px; min-block-size: 18px; color: var(--muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .dim .t b, .dim :global(.t-avatar) { opacity: .6; }
  .end { flex: none; display: inline-flex; align-items: center; }
  .end:empty { display: none; }
</style>
