<script lang="ts">
  /* Phone: the open name's face in the app bar (only when this phone has more than one name). A dot on it = something new
     under another name. Tap it for this sheet: every name, its news, one tap to open it. Settings has the rest. */
  import Avatar from '../../lib/ui/Avatar.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import Layer from '../chat/Layer.svelte';
  import { db, unreadOf } from '../../lib/model/store.svelte';
  import { switchTo } from '../../lib/model/actions/names.svelte';
  import { nav } from '../../lib/nav/router.svelte';

  let open = $state(false);
  const news = $derived(db.others.some((o) => unreadOf(o) > 0));
  const sub = (i: number) => { const o = db.others[i]; if (!o.online) return 'Offline on this phone'; const n = unreadOf(o); return n ? `${n} new` : 'Online'; };
</script>

{#if db.others.length}
  <button type="button" class="face press" aria-label={`${db.account.name}, switch name${news ? ', new under another name' : ''}`} aria-haspopup="dialog" aria-expanded={open} onclick={() => (open = true)} data-name-switch>
    <Avatar name={db.account.name} hue={db.account.hue} person="me" size={30} />
    {#if news}<span class="dot" aria-hidden="true"></span>{/if}
  </button>
{/if}

{#if open}
  <Layer variant="sheet" label="Switch name" onclose={() => (open = false)}>
  <div class="sheet">
    <p class="sh">Switch name</p>
    <button type="button" class="row cur press" onclick={() => (open = false)}><Avatar name={db.account.name} hue={db.account.hue} person="me" size={44} /><span><b>{db.account.name}</b><small>Open now</small></span><Icon name="check" size={18} /></button>
    {#each db.others as o, i (o.id)}
      <button type="button" class="row press" class:off={!o.online} onclick={() => { if (switchTo(i)) open = false; }}>
        <Avatar name={o.name} hue={o.hue} person={o.id} size={44} /><span><b>{o.name}</b><small class:new={unreadOf(o) > 0}>{sub(i)}</small></span>
      </button>
    {/each}
    <button type="button" class="row press" onclick={() => { open = false; nav.push({ name: 'settings' }); }}><Icon name="gear" size={20} /><span><b class="plain">Manage names</b></span></button>
  </div>
  </Layer>
{/if}

<style>
  .face { position: relative; display: grid; place-items: center; inline-size: 44px; block-size: 44px; border: 0; border-radius: 22px; background: transparent; cursor: pointer; }
  .dot { position: absolute; inset-block-start: 7px; inset-inline-end: 6px; inline-size: 10px; block-size: 10px; border-radius: 50%; background: var(--accent); box-shadow: 0 0 0 2px var(--bg); }
  .sheet { display: grid; gap: 2px; padding: 12px 10px 18px; }
  .sh { margin: 4px 12px 6px; font-size: 13px; color: var(--muted); }
  .row { display: flex; align-items: center; gap: 12px; padding: 10px 12px; min-block-size: 56px; border: 0; border-radius: 12px; background: transparent; color: var(--ink); font: inherit; text-align: start; cursor: pointer; }
  .row.cur { background: var(--surface); }
  .row span { display: grid; flex: 1; }
  .row small { color: var(--muted); font-size: 13px; }
  .row small.new { color: var(--accent-ink); font-weight: 600; }
  .off :global(.t-avatar) { opacity: .55; }
  .plain { font-weight: 400; }
</style>
