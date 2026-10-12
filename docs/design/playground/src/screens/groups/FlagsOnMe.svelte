<script lang="ts">
  /* Flags on me while I'm still a member (visible, like the removal view): who flagged, when, and how many are left. */
  import Avatar from '../../lib/ui/Avatar.svelte';
  import { FLAGS_TO_REMOVE, removalDetails, whenLabel } from '../../lib/model/actions/groups';
  import type { Group } from '../../lib/model/types';
  import { fold, foldOut } from './fold';
  import { fullName, hueOf } from './util';

  let { g }: { g: Group } = $props();
  const d = $derived(removalDetails(g));
  const n = $derived(d.flags.length);
  const left = $derived(FLAGS_TO_REMOVE - n);
</script>

<section class="sec" aria-labelledby="flag-h">
  <h2 id="flag-h">Flags on you <span class="n">{n} of {FLAGS_TO_REMOVE}</span></h2>
  <div class="meter" role="img" aria-label="{n} of {FLAGS_TO_REMOVE} flags">
    {#each Array(FLAGS_TO_REMOVE) as _, i (i)}<i class:on={i < n}></i>{/each}
  </div>
  <ul class="flags" role="list">
    {#each d.flags as f, i (i)}
      <li class="row" in:fold out:foldOut>
        <Avatar name={fullName(f.id)} hue={hueOf(f.id)} size={40} />
        <span class="tx"><b>{fullName(f.id)}</b><span class="meta">Flagged you{f.at ? ' · ' + whenLabel(f.at) : ''}</span></span>
      </li>
    {/each}
  </ul>
  <p class="note">{left === 1 ? 'One more flag' : `${left} more flags`} from any admin removes you from the group. Flags can’t be taken back. You can still read and post until then.</p>
</section>

<style>
  .sec { padding: 0 16px; margin-block-start: 24px; }
  h2 { margin: 0 4px 10px; font-size: 12px; font-weight: 650; letter-spacing: .06em; text-transform: uppercase; color: var(--muted); line-height: 16px; }
  h2 .n { margin-inline-start: 4px; color: var(--danger); font-variant-numeric: tabular-nums; }
  h2 .n::before { content: '·'; margin-inline-end: 6px; color: var(--muted); }
  .meter { display: flex; gap: 6px; margin: 0 4px 4px; }
  .meter i { flex: 1; block-size: 6px; border-radius: 3px; background: var(--line); transition: background-color var(--dur-enter) var(--ease-out); }
  .meter i.on { background: var(--danger-btn); }
  .flags { list-style: none; margin: 0; padding: 0 4px; }
  .row { display: flex; align-items: center; gap: 12px; min-block-size: 60px; border-block-end: 1px solid var(--hairline); }
  .row:last-child { border-block-end: 0; }
  .tx { display: grid; gap: 1px; min-inline-size: 0; }
  .tx b { font-weight: 600; }
  .meta { font-size: var(--fs-sub); color: var(--muted); line-height: 18px; font-variant-numeric: tabular-nums; }
  .note { margin: 6px 4px 0; color: var(--muted); font-size: var(--fs-sub); line-height: 19px; }
</style>
