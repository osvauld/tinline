<script lang="ts">
  /* Reads any profile value: text and numbers as text, lists as lists, nested objects as indented key/value rows.
     Text only (never markup, never fetched). Deep structures stop at a depth so a hostile value can't grow the page. */
  import Self from './Value.svelte';
  import { humanize } from './info';
  let { v, depth = 0 }: { v: unknown; depth?: number } = $props();
  const MAX_DEPTH = 4;
  const prim = (x: unknown) => x === null || ['string', 'number', 'boolean'].includes(typeof x);
  const show = (x: unknown) => (x === null ? '—' : typeof x === 'boolean' ? (x ? 'Yes' : 'No') : String(x));
  const arr = $derived(Array.isArray(v) ? v : null);
  const flat = $derived(arr?.every(prim) ?? false);
  const obj = $derived(!arr && v && typeof v === 'object' ? Object.entries(v as Record<string, unknown>) : null);
</script>

{#if depth > MAX_DEPTH}
  <span class="v muted">…</span>
{:else if arr && flat}
  <span class="v" dir="auto">{arr.map(show).join(', ') || '—'}</span>
{:else if arr}
  <ul class="kv nest">{#each arr as item, i (i)}<li><Self v={item} depth={depth + 1} /></li>{/each}</ul>
{:else if obj}
  <dl class="kv nest">
    {#each obj as [k, val] (k)}
      <div class="pair"><dt>{humanize(k)}</dt><dd><Self v={val} depth={depth + 1} /></dd></div>
    {/each}
  </dl>
{:else}
  <span class="v" dir="auto">{show(v)}</span>
{/if}

<style>
  .v { overflow-wrap: anywhere; white-space: pre-wrap; }
  .muted { color: var(--muted); }
  .kv { display: grid; gap: 8px; margin: 0; padding: 0; list-style: none; }
  .kv.nest { margin-block-start: 6px; padding-inline-start: 12px; border-inline-start: 2px solid var(--hairline); }
  .pair { display: grid; gap: 1px; }
  dt { font-size: var(--fs-sub); line-height: 18px; color: var(--muted); }
  dd { margin: 0; line-height: 21px; }
</style>
