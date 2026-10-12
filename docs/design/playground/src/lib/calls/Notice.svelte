<script lang="ts">
  /* One notification card as the OS draws it. The card itself is neutral OS chrome; only the small
     Tinline icon and the app name are ours, so our notifications read as ours at a glance. */
  import type { Snippet } from 'svelte';
  import Mark from '../brand/Mark.svelte';
  import Avatar from '../ui/Avatar.svelte';
  import { displayName, person as getPerson } from '../model/store.svelte';

  /* `person`: the caller. Their picture (or initials) sits at the card's end, like an OS large icon. */
  let { title, text, when = '', tabular = false, label, onopen, actions, person }:
    { title: string; text: string; when?: string; tabular?: boolean; label?: string; onopen?: () => void; actions?: Snippet; person?: string } = $props();
</script>

<article class="notice" class:withlg={!!person} aria-label={label ?? title}>
  <button type="button" class="main" onclick={() => onopen?.()} disabled={!onopen}>
    <span class="hd">
      <span class="ico" aria-hidden="true"><Mark size={13} mono /></span>
      <span class="app">Tinline</span>{#if when}<span class="sep" aria-hidden="true">·</span><time>{when}</time>{/if}
    </span>
    <b class="t" dir="auto">{title}</b>
    <span class="x" class:tab={tabular}>{text}</span>
  </button>
  {#if person}<span class="lg"><Avatar name={displayName(person)} hue={getPerson(person)?.hue ?? 0} {person} size={40} /></span>{/if}
  {#if actions}<div class="acts">{@render actions()}</div>{/if}
</article>

<style>
  .notice { background: var(--os-card); color: var(--os-ink); border-radius: 22px; border: 1px solid var(--os-line); padding: 12px 14px 6px; display: grid; gap: 2px; position: relative; }
  .withlg .main { padding-inline-end: 52px; }
  .lg { position: absolute; inset-block-start: 14px; inset-inline-end: 14px; pointer-events: none; }
  .main { display: grid; gap: 2px; text-align: start; border: 0; background: transparent; color: inherit; font: inherit; padding: 0; cursor: pointer; min-inline-size: 0; border-radius: 10px; }
  .main:disabled { cursor: default; }
  .hd { display: flex; align-items: center; gap: 6px; font-size: var(--fs-meta); line-height: 16px; color: var(--os-sub); margin-block-end: 4px; }
  .ico { inline-size: 18px; block-size: 18px; border-radius: 50%; background: var(--accent); color: var(--on-accent); display: grid; place-items: center; flex: none; }
  .app { font-weight: 600; letter-spacing: .01em; }
  .sep { opacity: .7; }
  .t { font-size: var(--fs-body); line-height: 20px; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .x { font-size: var(--fs-sub); line-height: 18px; color: var(--os-sub); }
  .x.tab { font-variant-numeric: tabular-nums; letter-spacing: .02em; }
  .acts { display: flex; flex-wrap: wrap; gap: 2px 4px; margin: 4px -12px 0; }
  .notice :global(.act) { min-block-size: 40px; padding-inline: 12px; border: 0; border-radius: 20px; background: transparent; color: var(--accent-ink); font-weight: 650; font-size: var(--fs-sub); cursor: pointer; }
  .notice :global(.act:hover) { background: color-mix(in srgb, var(--os-ink) 8%, transparent); }
  .notice :global(.act.bad) { color: var(--danger); }
  .notice :global(.act:focus-visible), .main:focus-visible { outline: 2px solid var(--os-ink); outline-offset: -2px; }
</style>
