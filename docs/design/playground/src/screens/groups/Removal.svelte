<script lang="ts">
  /* The removed person's view (U5: visible). Who flagged, when, and what it means. */
  import Avatar from '../../lib/ui/Avatar.svelte';
  import Btn from './Btn.svelte';
  import { firstName } from '../../lib/model/store.svelte';
  import { nav } from '../../lib/nav/router.svelte';
  import { FLAGS_TO_REMOVE, removalDetails, whenLabel } from '../../lib/model/actions/groups';
  import type { Group } from '../../lib/model/types';
  import { fullName, hueOf } from './util';

  let { g }: { g: Group } = $props();
  const d = $derived(removalDetails(g));
  const ord = ['First flag', 'Second flag', 'Third flag'];
</script>

<div class="rm">
  <h2 class="h">Removed after {FLAGS_TO_REMOVE} admin flags</h2>
  {#if d.removedAt}<p class="sub">{whenLabel(d.removedAt)}</p>{/if}

  <h3 class="sec">Who flagged you</h3>
  <ul class="flags" role="list">
    {#each d.flags as f, i (i)}
      <li class="row">
        <Avatar name={fullName(f.id)} hue={hueOf(f.id)} size={40} />
        <span class="tx"><b>{fullName(f.id)}</b><span class="meta">{(ord[i] ?? 'Flag') + (f.at ? ' · ' + whenLabel(f.at) : '')}</span></span>
      </li>
    {/each}
  </ul>
  <p class="foot">Each flag was made by an admin of the group, and this device checked them before showing this.</p>

  <h3 class="sec">What this means</h3>
  <p class="body">You keep the posts already on this device, and you can read them. You can’t post, and the group’s hosts no longer accept anything from you. Removal is final: to come back, an admin has to invite you again.</p>
  <div class="acts"><Btn variant="outline" onclick={() => nav.back()}>Read the posts</Btn></div>
</div>

<style>
  .rm { padding: 8px 20px 32px; }
  .h { margin: 8px 0 2px; font-size: 22px; line-height: 28px; font-weight: 650; }
  .sub { margin: 0; color: var(--muted); font-size: var(--fs-sub); line-height: 19px; }
  .sec { margin: 24px 0 6px; font-size: 12px; font-weight: 650; letter-spacing: .06em; text-transform: uppercase; color: var(--muted); line-height: 16px; }
  .flags { list-style: none; margin: 0; padding: 0; }
  .row { display: flex; align-items: center; gap: 12px; min-block-size: 60px; border-block-end: 1px solid var(--hairline); }
  .row:last-child { border-block-end: 0; }
  .tx { display: grid; gap: 1px; min-inline-size: 0; }
  .tx b { font-weight: 600; }
  .meta { font-size: var(--fs-sub); color: var(--muted); line-height: 18px; font-variant-numeric: tabular-nums; }
  .foot { margin: 6px 0 0; color: var(--muted); font-size: var(--fs-sub); line-height: 19px; }
  .body { margin: 0; line-height: 22px; }
  .acts { margin-block-start: 20px; }
</style>
