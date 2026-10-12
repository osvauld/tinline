<script lang="ts">
  /* Groups tab: invitations to me, chat requests (incoming + sent), active groups newest first, and a
     folded "Left or removed" section. Rows match the Chats list; sections animate in place. */
  import { flip } from 'svelte/animate';
  import Avatar from '../../lib/ui/Avatar.svelte';
  import Badge from '../../lib/ui/Badge.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import Layer from '../chat/Layer.svelte';
  import Btn from './Btn.svelte';
  import Collapse from './Collapse.svelte';
  import { fold, foldOut, flipMs } from './fold';
  import { db, chatsByRecent, lastMessage, firstName, displayName, person, isGroup } from '../../lib/model/store.svelte';
  import { listStamp, clock } from '../../lib/model/clock.svelte';
  import { nav } from '../../lib/nav/router.svelte';
  import { prefs } from '../../lib/model/prefs.svelte';
  import { gx } from '../../lib/model/actions/groups.svelte';
  import { gcall } from '../../lib/calls/gcall.svelte';
  import {
    acceptInvitation, declineInvitation, acceptRequestIn, declineRequestIn, blockRequester, cancelRequest, createGroup, group, dayOnly, requestExpired,
  } from '../../lib/model/actions/groups';
  import type { Chat, Group } from '../../lib/model/types';

  const groups = $derived(db.chats.filter(isGroup));
  const active = $derived(chatsByRecent(groups.filter((g) => g.status === 'active')) as Group[]);
  const gone = $derived(chatsByRecent(groups.filter((g) => g.status !== 'active')) as Group[]);
  let goneOpen = $state(false);
  let dialog = $state<'new' | null>(null);
  let creating = $state('');
  let inertList = $derived(dialog !== null);

  function preview(g: Group) {
    const m = lastMessage(g);
    if (!m) return 'No posts yet';
    const who = !m.system ? (m.from === 'me' ? 'You: ' : firstName(m.from) + ': ') : '';
    return who + m.text;
  }
  const goTo = (c: Chat) => (prefs.form === 'desktop' ? nav.open({ name: 'conversation', chat: c.id }) : nav.push({ name: 'conversation', chat: c.id }));

  function accept(id: string) { acceptInvitation(id); }
  function acceptReq(id: string) { acceptRequestIn(id); }
  function viaName(groupId: string) { return group(groupId)?.name ?? 'a group'; }

  function reqLine(r: (typeof gx.requestsOut)[number]) {
    const n = firstName(r.to);
    if (requestExpired(r)) return 'No answer after 7 days';
    return r.state === 'pending' ? `Waiting for ${n} · via ${viaName(r.groupId)}` : r.state === 'declined' ? `${n} declined` : r.state === 'blocked' ? 'Blocked' : `${n} accepted`;
  }

  function openNew() { creating = ''; dialog = 'new'; }
  // The search dock's + button (Android) opens the same dialog as the "New group" row.
  $effect(() => { addEventListener('tinline-new-group', openNew); return () => removeEventListener('tinline-new-group', openNew); });
  function closeDialog() { dialog = null; }
  function create() {
    const id = createGroup(creating);
    if (!id) return;
    dialog = null;
    const c = db.chats.find((x) => x.id === id);
    if (c) goTo(c);
  }
  const removedText = (g: Group) => {
    const at = lastMessage(g)?.at;
    return (g.status === 'removed' ? 'Removed' : 'You left') + (at ? ' · ' + dayOnly(at) : '');
  };
</script>

<div class="wrap" inert={inertList}>
  <button type="button" class="row press new" onclick={openNew}>
    <span class="plus"><Icon name="plus" size={22} /></span>
    <span class="mid"><span class="name">New group</span><span class="pv">You’ll be the owner</span></span>
  </button>

  {#if gx.invitations.length}
    <section in:fold out:foldOut aria-label="Invitations to you">
      <h2 class="sec"><span>Invitations to you <span class="n">{gx.invitations.length}</span></span></h2>
      <ul class="cards" role="list">
        {#each gx.invitations as inv (inv.id)}
          <li class="card" in:fold out:foldOut>
            <div class="who">
              <Avatar name={displayName(inv.by)} hue={person(inv.by)?.hue ?? 0} person={inv.by} size={40} />
              <span class="tx"><span class="line"><b>{firstName(inv.by)}</b> invited you to <b>{inv.groupName}</b></span>
                <span class="sub">Hosted by {inv.hostName}’s device · valid until {dayOnly(inv.expiresAt)}</span></span>
            </div>
            <div class="acts">
              <Btn variant="quiet" onclick={() => declineInvitation(inv.id)}>Decline</Btn>
              <Btn variant="primary" onclick={() => accept(inv.id)}>Accept</Btn>
            </div>
          </li>
        {/each}
      </ul>
    </section>
  {/if}

  {#if gx.requestsIn.length || gx.requestsOut.length}
    <section in:fold out:foldOut aria-label="Chat requests">
      <h2 class="sec"><span>Chat requests{#if gx.requestsIn.length} <span class="n">{gx.requestsIn.length}</span>{/if}</span></h2>
      {#if gx.requestsIn.length}
        <ul class="cards" role="list" aria-label="Incoming chat requests">
          {#each gx.requestsIn as r (r.id)}
            <li class="card" in:fold out:foldOut>
              <div class="who">
                <Avatar name={displayName(r.from)} hue={person(r.from)?.hue ?? 0} person={r.from} size={40} />
                <span class="tx"><span class="line"><b>{displayName(r.from)}</b> wants to chat</span>
                  <span class="sub">Member of {viaName(r.groupId)}</span></span>
              </div>
              {#if r.note}<p class="note">{r.note}</p>{/if}
              <p class="hint">Accepting adds {firstName(r.from)} as a contact for messages only. You can allow calls later.</p>
              <div class="acts">
                <Btn variant="dangerquiet" onclick={() => blockRequester(r.id)}>Block</Btn>
                <Btn variant="quiet" onclick={() => declineRequestIn(r.id)}>Decline</Btn>
                <Btn variant="primary" onclick={() => acceptReq(r.id)}>Accept</Btn>
              </div>
            </li>
          {/each}
        </ul>
      {/if}
      {#if gx.requestsOut.length}
        <h3 class="sub2" in:fold out:foldOut>Sent</h3>
        <ul class="list" role="list" aria-label="Sent chat requests">
          {#each gx.requestsOut as r (r.id)}
            {@const p = person(r.to)}
            <li class="sent" in:fold out:foldOut>
              <div class="row static">
                <Avatar name={displayName(r.to)} hue={p?.hue ?? 0} person={r.to} />
                <span class="mid"><span class="top"><span class="name">{displayName(r.to)}</span><span class="when">{listStamp(r.at, clock.now)}</span></span>
                  <span class="pv" class:bad={r.state === 'blocked'}>{reqLine(r)}</span></span>
                <button type="button" class="x press" aria-label={r.state === 'pending' && !requestExpired(r) ? `Cancel request to ${displayName(r.to)}` : `Remove request to ${displayName(r.to)}`}
                  onclick={() => cancelRequest(r.id)}><Icon name="x" size={18} /></button>
              </div>
            </li>
          {/each}
        </ul>
      {/if}
    </section>
  {/if}

  <h2 class="sec"><span>Your groups <span class="n">{active.length}</span></span></h2>
  {#if !active.length}
    <p class="empty" transition:fold>No groups yet. Create one, or accept an invitation.</p>
  {/if}
  <ul class="list" role="list" aria-label="Groups">
    {#each active as g (g.id)}
      {@const m = lastMessage(g)}
      <li animate:flip={{ duration: flipMs() }} in:fold out:foldOut>
        <button type="button" class="row press" class:sel={nav.openChat === g.id} onclick={() => goTo(g)}>
          <Avatar name={g.name} group />
          <span class="mid">
            <span class="top"><span class="name">{g.name}</span>
              <span class="when" class:hot={g.unread > 0 && !g.muted}>{#if g.muted}<Icon name="mute" size={14} label="Muted" />{/if}{m ? listStamp(m.at) : ''}</span></span>
            <span class="bot">
              <span class="pv">
                {#if g.draft}<b class="draft">Draft:</b> {g.draft}
                {:else if gcall.liveFor(g.id)}<span class="callpv"><i class="eq" aria-hidden="true"><b></b><b></b><b></b></i>{gcall.inCall(g.id) ? 'You’re in the call' : `Call in progress · ${gcall.liveFor(g.id)?.parts.length} joined`}</span>
                {:else if m?.delivery === 'waiting'}<span class="wait"><Icon name="clock" size={13} /> Waiting for a host</span>
                {:else}{preview(g)}{/if}
              </span>
              {#if gcall.liveFor(g.id) && !gcall.inCall(g.id)}<span class="join" aria-hidden="true">Join</span>{/if}
              <Badge count={g.unread} muted={g.muted} />
            </span>
          </span>
        </button>
      </li>
    {/each}
  </ul>

  {#if gone.length}
    <div class="fold" in:fold out:foldOut>
      <button type="button" class="sec toggle press" aria-expanded={goneOpen} aria-controls="gone-list" onclick={() => (goneOpen = !goneOpen)}>
        <span>Left or removed <span class="n">{gone.length}</span></span><span class="chev" class:open={goneOpen}><Icon name="chev" size={18} /></span>
      </button>
      <Collapse open={goneOpen}>
        <ul class="list" id="gone-list" role="list" aria-label="Left or removed groups">
          {#each gone as g (g.id)}
            <li in:fold out:foldOut>
              <button type="button" class="row press dim" class:sel={nav.openChat === g.id} onclick={() => goTo(g)}>
                <Avatar name={g.name} group />
                <span class="mid"><span class="name">{g.name}</span><span class="pv">{removedText(g)}</span></span>
              </button>
            </li>
          {/each}
        </ul>
      </Collapse>
    </div>
  {/if}
</div>

{#if dialog === 'new'}
  <Layer variant="dialog" label="New group" onclose={closeDialog}>
    <form class="dlg" onsubmit={(e) => { e.preventDefault(); create(); }}>
      <h2>New group</h2>
      <label class="fld"><span>Group name</span>
        <input data-autofocus type="text" maxlength="60" placeholder="Name" bind:value={creating} autocomplete="off" /></label>
      <p>You’ll be the owner. This device is the group’s first host, so it stores the group and passes posts on.</p>
      <div class="dacts">
        <Btn variant="quiet" onclick={closeDialog}>Cancel</Btn>
        <Btn variant="primary" type="submit" disabled={!creating.trim()}>Create</Btn>
      </div>
    </form>
  </Layer>
{/if}

<style>
  .wrap { padding-block-end: 24px; }
  .list, .cards { list-style: none; margin: 0; padding: 0 8px; }
  .row { display: flex; gap: 12px; align-items: center; inline-size: 100%; min-block-size: 72px; padding: 8px; border: 0; border-radius: var(--r-ctl);
    background: transparent; text-align: start; cursor: pointer; color: inherit; font: inherit; }
  .wrap > .row { inline-size: calc(100% - 16px); margin-inline: 8px; min-block-size: 64px; }
  .row.static { cursor: default; }
  button.row:hover { background: var(--surface-2); }
  .row.sel { background: var(--accent-soft); }
  .row.dim .name, .row.dim :global(.t-avatar) { opacity: .72; }
  .plus { inline-size: 48px; block-size: 48px; border-radius: 50%; display: grid; place-items: center; background: var(--accent-soft); color: var(--accent-ink); flex: none; }
  .mid { flex: 1; min-inline-size: 0; display: grid; grid-template-columns: minmax(0, 1fr); gap: 2px; }
  .top, .bot { display: flex; align-items: center; gap: 8px; justify-content: space-between; }
  .name { flex: 1; min-inline-size: 0; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .when { font-size: var(--fs-meta); color: var(--muted); display: inline-flex; gap: 4px; align-items: center; flex: none; font-variant-numeric: tabular-nums; }
  .when.hot { color: var(--accent-ink); font-weight: 700; }
  .pv { flex: 1; min-inline-size: 0; font-size: var(--fs-sub); color: var(--muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .callpv { display: inline-flex; align-items: center; gap: 7px; color: var(--accent-ink); font-weight: 600; font-variant-numeric: tabular-nums; }
  /* three calm bars: the row's only motion, and only while a call is running */
  .eq { display: inline-flex; align-items: flex-end; gap: 2px; block-size: 12px; flex: none; }
  .eq b { inline-size: 2.5px; border-radius: 1px; background: currentColor; block-size: 4px; animation: eq 1.6s ease-in-out infinite; }
  .eq b:nth-child(2) { animation-delay: -.5s; }
  .eq b:nth-child(3) { animation-delay: -1s; }
  @keyframes eq { 50% { block-size: 12px; } }
  :global(:root[data-motion='reduce']) .eq b { animation: none; block-size: 8px; }
  .join { flex: none; padding: 2px 11px; border-radius: 12px; background: var(--accent); color: var(--on-accent); font-size: var(--fs-meta); font-weight: 700; line-height: 16px; }
  .pv.bad { color: var(--danger); }
  .draft { color: var(--accent-ink); font-weight: 600; }
  .wait { color: var(--gold-ink); font-weight: 600; display: inline-flex; gap: 3px; align-items: center; vertical-align: -2px; }

  .sec { display: flex; align-items: center; justify-content: space-between; margin: 0; padding: 18px 16px 6px; font-size: 12px; font-weight: 650; letter-spacing: .06em;
    text-transform: uppercase; color: var(--muted); line-height: 16px; }
  .sec .n { font-weight: 650; font-variant-numeric: tabular-nums; margin-inline-start: 6px; color: var(--muted); opacity: .8; }
  .sec .n::before { content: '·'; margin-inline-end: 6px; }
  .toggle { inline-size: 100%; border: 0; background: transparent; cursor: pointer; min-block-size: 44px; text-align: start; }
  .toggle:hover { color: var(--ink); }
  .chev { display: grid; transition: rotate var(--dur-enter) var(--ease-out); }
  .chev.open { rotate: 180deg; }
  .fold { margin-block-start: 6px; }
  .sub2 { margin: 14px 16px 2px; font-size: 12px; font-weight: 650; color: var(--muted); line-height: 16px; }

  .card { margin: 6px 0; padding: 14px; border-radius: var(--r-ctl); background: var(--surface); box-shadow: inset 0 0 0 1px var(--hairline); display: grid; gap: 10px; }
  :global([data-form='desktop']) .card { background: var(--bg); }
  .who { display: flex; gap: 12px; align-items: center; }
  .tx { display: grid; gap: 1px; min-inline-size: 0; }
  .line { font-size: var(--fs-body); line-height: 20px; overflow-wrap: anywhere; }
  .line b { font-weight: 650; }
  .sub { font-size: var(--fs-sub); line-height: 18px; color: var(--muted); }
  .note { margin: 0; padding: 8px 12px; border-radius: 12px; background: var(--surface-2); font-size: var(--fs-sub); line-height: 19px; border-inline-start: 3px solid var(--accent); overflow-wrap: anywhere; }
  .hint { margin: 0; font-size: var(--fs-sub); line-height: 18px; color: var(--muted); }
  .acts { display: flex; gap: 8px; justify-content: flex-end; flex-wrap: wrap; }
  .x { flex: none; inline-size: var(--btn); block-size: var(--btn); border-radius: 50%; border: 0; background: transparent; color: var(--muted); display: grid; place-items: center; cursor: pointer; }
  .x:hover { background: var(--surface-2); color: var(--ink); }
  .empty { margin: 8px 20px; color: var(--muted); font-size: var(--fs-sub); line-height: 19px; }

  .dlg { padding: 20px 20px 16px; }
  .dlg h2 { margin: 0 0 10px; font-size: var(--fs-title); font-weight: 650; line-height: 24px; }
  .dlg p { margin: 10px 0 0; color: var(--muted); font-size: var(--fs-sub); line-height: 19px; }
  .fld { display: grid; gap: 6px; font-size: var(--fs-sub); font-weight: 600; color: var(--muted); }
  .fld input { min-block-size: var(--btn); padding: 0 14px; border-radius: var(--r-ctl); border: 1px solid var(--line); background: var(--surface); font-size: var(--fs-body); color: var(--ink); font-weight: 400; }
  .fld input:focus { outline: 0; border-color: var(--accent); box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent-ink) 14%, transparent); }
  .dacts { display: flex; gap: 8px; justify-content: flex-end; margin-block-start: 18px; }
</style>
