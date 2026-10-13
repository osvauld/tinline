<script lang="ts">
  /* "Call in progress · 3 joined · Join": a group call is running and I may join it. Shown in the conversation (under the
     header, over the top of the messages) and, as `row`, inside the Groups list row. A call never rings: this is where
     it is found. The wording and the button width are fixed, so a person joining only changes a number. */
  import Avatar from '../../lib/ui/Avatar.svelte';
  import { gcall } from '../../lib/calls/gcall.svelte';
  import { fmtDuration } from '../../lib/calls/call.svelte';
  import { person } from '../../lib/model/store.svelte';
  import { hueOf, fullName } from './util';

  let { gid }: { gid: string } = $props();
  const lc = $derived(gcall.liveFor(gid));
  const mine = $derived(gcall.inCall(gid));
  const parts = $derived(lc?.parts ?? []);
  const shown = $derived(parts.slice(0, 3));
  const sub = $derived(mine ? (gcall.phase === 'active' ? fmtDuration(gcall.elapsedS) : gcall.statusText) : `${parts.length} joined`);
</script>

{#if lc}
  <div class="oc" role="status" aria-label="A call is in progress, {parts.length} joined" data-gbanner={gid}>
    <span class="stack" aria-hidden="true">
      {#each shown as id (id)}<span class="a"><Avatar name={fullName(id)} hue={hueOf(id)} person={id} size={28} /></span>{/each}
    </span>
    <span class="tx"><b>{mine ? 'You’re in the call' : 'Call in progress'}</b><small class="n">{sub}</small></span>
    {#if mine}
      <button type="button" class="j alt press" onclick={() => gcall.restore()}>Open</button>
    {:else}
      <button type="button" class="j press" onclick={() => gcall.join(gid)}>Join</button>
    {/if}
  </div>
{/if}

<style>
  .oc { display: flex; align-items: center; gap: 12px; padding: 8px 12px 8px 14px; min-block-size: 56px; background: var(--accent-soft); color: var(--accent-ink); border-block-end: 1px solid var(--hairline); }
  .stack { display: inline-flex; flex: none; }
  .a { display: inline-flex; border-radius: 50%; box-shadow: 0 0 0 2px var(--accent-soft); }
  .a :global(.t-avatar) { box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent-ink) 24%, transparent); }
  .a + .a { margin-inline-start: -8px; }
  .tx { flex: 1; min-inline-size: 0; display: grid; }
  .tx b { font-weight: 650; font-size: var(--fs-body); line-height: 20px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tx small { font-size: var(--fs-sub); line-height: 17px; opacity: .85; font-variant-numeric: tabular-nums; }
  .j { flex: none; min-inline-size: 72px; min-block-size: 36px; padding-inline: 18px; border: 0; border-radius: 18px; background: var(--accent); color: var(--on-accent); font-weight: 700; font-size: var(--fs-sub); cursor: pointer; position: relative; }
  .j::after { content: ''; position: absolute; inset: -4px -2px; }
  .j.alt { background: transparent; color: var(--accent-ink); box-shadow: inset 0 0 0 1.5px currentColor; }
  .j:hover { filter: brightness(1.08); }
  .j:focus-visible { outline: 2px solid var(--accent-ink); outline-offset: 2px; }
  :global(:root[data-theme='dark']) .oc { background: color-mix(in srgb, var(--accent) 24%, var(--surface)); color: var(--ink); }
  :global(:root[data-theme='dark']) .a { box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 24%, var(--surface)); }
  @media (prefers-color-scheme: dark) {
    :global(:root:not([data-theme='light'])) .oc { background: color-mix(in srgb, var(--accent) 24%, var(--surface)); color: var(--ink); }
    :global(:root:not([data-theme='light'])) .a { box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 24%, var(--surface)); }
  }
</style>
