<script lang="ts">
  /* Desktop sidebar foot: the open name (whole nickname, never the first word), and above it the other names on this
     computer, one click each, with their news. Only drawn when there is more than one name. */
  import Avatar from '../../lib/ui/Avatar.svelte';
  import { db, unreadOf } from '../../lib/model/store.svelte';
  import { switchTo } from '../../lib/model/actions/names.svelte';
  import { nav } from '../../lib/nav/router.svelte';
</script>

<div class="names" data-side-names>
  {#if db.others.length}
    <p class="h">Other names</p>
    {#each db.others as o, i (o.id)}
      <button type="button" class="o press" class:off={!o.online} onclick={() => switchTo(i)} title={o.online ? `Open ${o.name}` : `${o.name} is offline on this computer`}>
        <Avatar name={o.name} hue={o.hue} person={o.id} size={28} />
        <span class="n">{o.name}</span>
        {#if unreadOf(o)}<span class="t-badge">{unreadOf(o)}</span>{:else if !o.online}<small>Offline</small>{/if}
      </button>
    {/each}
  {/if}
  <button type="button" class="me press" onclick={() => nav.open({ name: 'settings' })}>
    <Avatar name={db.account.name} hue={db.account.hue} person="me" size={36} />
    <span><b>{db.account.name}</b><small>{db.others.length ? 'Open now' : 'Your account'}</small></span>
  </button>
</div>

<style>
  .names { margin-top: auto; display: grid; gap: 2px; }
  .h { margin: 0 8px 2px; font-size: 12px; font-weight: 600; color: var(--muted); }
  .o { display: flex; align-items: center; gap: 10px; min-height: 40px; padding: 4px 8px; border: 0; border-radius: var(--r-ctl); background: transparent; color: var(--ink); cursor: pointer; text-align: start; font: inherit; }
  .o:hover, .me:hover { background: var(--surface-2); }
  .o .n { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: var(--fs-sub); }
  .o small { color: var(--muted); font-size: 12px; }
  .off :global(.t-avatar) { opacity: .55; }
  .me { margin-top: 6px; display: flex; gap: 10px; align-items: center; border: 0; background: transparent; padding: 8px; border-radius: var(--r-ctl); cursor: pointer; text-align: start; color: var(--ink); font: inherit; }
  .me span { display: grid; min-width: 0; } .me b { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; } .me small { color: var(--muted); font-size: var(--fs-sub); }
</style>
