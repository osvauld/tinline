<script lang="ts">
  /* Settings > Names on this phone (2026-10-15). Every name here stays online, so calls and messages to any of them come
     through; one is open. Tap a name to open it (instant: it is already unlocked). The ⋯ menu takes a name offline on
     this device or removes it from this device. The limit is Proposed (phone 3, desktop 5) and said only when reached. */
  import Avatar from '../../lib/ui/Avatar.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import IconButton from '../../lib/ui/IconButton.svelte';
  import Layer from '../chat/Layer.svelte';
  import { db, unreadOf } from '../../lib/model/store.svelte';
  import { gate } from '../../lib/model/actions/gate.svelte';
  import { switchTo, setOnline, removeName, roomForAnother, nameLimit } from '../../lib/model/actions/names.svelte';

  let menu = $state<number | null>(null);
  let confirmRemove = $state(false);
  const m = $derived(menu === null ? null : db.others[menu]);
  const close = () => { menu = null; confirmRemove = false; };
  const sub = (i: number) => {
    const o = db.others[i];
    if (!o.online) return `Offline on this ${gate.noun}`;
    const n = unreadOf(o);
    return n ? `Online · ${n} new` : 'Online';
  };
  let full = $state('');   // said once, beside the row that hit the limit
  const open = (i: number) => { full = ''; if (!switchTo(i)) full = db.others[i].name; };
</script>

<section class="pf-sec" aria-labelledby="nm-h" data-names>
  <h2 id="nm-h">Names on this {gate.noun}</h2>
  <div class="pf-card">
    <div class="pf-act acc" aria-current="true">
      <Avatar name={db.account.name} hue={db.account.hue} person="me" size={36} />
      <span class="t"><b>{db.account.name}</b><small class="on">Open now</small></span>
      <Icon name="check" size={18} />
    </div>
    {#each db.others as o, i (o.id)}
      <div class="pf-act acc orow" class:off={!o.online}>
        <button type="button" class="pick press" onclick={() => open(i)} aria-label={`Open ${o.name}`}>
          <Avatar name={o.name} hue={o.hue} person={o.id} size={36} />
          <span class="t"><b>{o.name}</b><small class:new={o.online && unreadOf(o) > 0}>{sub(i)}</small>
            {#if full === o.name}<small class="lim" role="status">This {gate.noun} keeps {nameLimit()} names online. Take one offline first.</small>{/if}</span>
        </button>
        <IconButton icon="more" label={`More for ${o.name}`} onclick={() => { menu = i; confirmRemove = false; }} />
      </div>
    {/each}
    {#if roomForAnother()}
      <button type="button" class="pf-act press" onclick={() => gate.beginAdd()}><Icon name="plus" size={20} /><span class="t"><b>Add another name</b></span><Icon name="chevr" size={18} class="chev" /></button>
    {:else}
      <div class="pf-act dim"><Icon name="plus" size={20} /><span class="t"><b>Add another name</b><small>This {gate.noun} keeps {nameLimit()} names online. Take one offline to add another.</small></span></div>
    {/if}
  </div>
  <p class="accnote">{#if db.others.length}Each name is its own address. Calls to any online name come through, whichever one is open.{:else}A second name gets its own address and its own contacts.{/if}
    Link your {gate.isAndroid ? 'computer' : 'phone'} to use the same names there.</p>
</section>

{#if m && menu !== null}
  <Layer variant="dialog" label={m.name} onclose={close}>
  <div class="sheet">
    <div class="hd"><Avatar name={m.name} hue={m.hue} person={m.id} size={44} /><b>{m.name}</b></div>
    {#if !confirmRemove}
      {#if m.online}
        <button type="button" class="row press" onclick={() => { setOnline(menu!, false); close(); }}><Icon name="moon" size={20} /><span><b>Take offline on this {gate.noun}</b><small>Its calls and messages wait until it’s back online</small></span></button>
      {:else}
        <button type="button" class="row press" disabled={!roomForAnother()} onclick={() => { if (setOnline(menu!, true)) close(); }}><Icon name="globe" size={20} /><span><b>Bring online</b><small>{roomForAnother() ? 'Its calls and messages come through again' : `This ${gate.noun} keeps ${nameLimit()} names online`}</small></span></button>
      {/if}
      <button type="button" class="row press" onclick={() => (confirmRemove = true)}><Icon name="trash" size={20} /><span><b>Remove from this {gate.noun}</b></span></button>
    {:else}
      <p class="cf">{m.name} stays on your other devices. To bring it back here you’ll need its 24 words or key.</p>
      <div class="btns">
        <button type="button" class="b press" onclick={() => (confirmRemove = false)}>Keep it</button>
        <button type="button" class="b dng press" onclick={() => { removeName(menu!); close(); }}>Remove</button>
      </div>
    {/if}
  </div>
  </Layer>
{/if}

<style>
  .acc { min-block-size: 60px; }
  .acc .on { color: var(--accent-ink); font-weight: 600; }
  .orow { padding: 0 4px 0 0; gap: 0; }
  .pick { flex: 1; min-inline-size: 0; display: flex; align-items: center; gap: 14px; min-block-size: 60px; padding: 8px 8px 8px 16px; border: 0; background: none; color: inherit; font: inherit; text-align: start; cursor: pointer; }
  .pick:hover { background: var(--surface-2); }
  .off :global(.t-avatar) { opacity: .55; }
  .new { color: var(--accent-ink); font-weight: 600; }
  .lim { color: var(--warn-ink); }
  .dim { color: var(--muted); }
  .dim b { font-weight: 500; }
  .accnote { margin: 8px 16px 0; color: var(--muted); font-size: var(--fs-sub); line-height: 18px; }
  .sheet { display: grid; gap: 2px; padding: 12px 10px 18px; }
  .hd { display: flex; align-items: center; gap: 12px; padding: 4px 12px 10px; }
  .row { display: flex; align-items: center; gap: 14px; padding: 10px 12px; min-block-size: 56px; border: 0; border-radius: 12px; background: transparent; color: var(--ink); font: inherit; text-align: start; cursor: pointer; }
  .sheet .row:hover:not(:disabled) { background: var(--surface-2); }
  .sheet .row:disabled { opacity: .55; cursor: default; }
  .sheet .row span { display: grid; flex: 1; } .sheet .row small { color: var(--muted); font-size: 13px; }
  .cf { margin: 4px 12px 12px; line-height: 21px; }
  .btns { display: flex; gap: 8px; justify-content: flex-end; padding: 0 8px; }
  .b { min-block-size: 44px; padding-inline: 18px; border: 0; border-radius: 22px; font-weight: 700; background: var(--surface-2); color: var(--ink); cursor: pointer; }
  .dng { background: var(--danger-btn); color: var(--on-danger-btn); }
</style>
