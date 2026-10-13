<script lang="ts">
  /* Calls tab: every call record across chats, newest first. Row = open the chat; button = call back. */
  import Avatar from '../../lib/ui/Avatar.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import { db, chatTitle, person } from '../../lib/model/store.svelte';
  import { listStamp } from '../../lib/model/clock.svelte';
  import { nav } from '../../lib/nav/router.svelte';
  import { prefs } from '../../lib/model/prefs.svelte';
  import { call, fmtShort } from '../../lib/calls/call.svelte';
  import type { CallRecord, DirectChat, Message } from '../../lib/model/types';

  type Entry = { key: string; c: DirectChat; m: Message; r: CallRecord };
  const entries = $derived(
    db.chats.flatMap((c) => (c.kind === 'direct' ? c.messages.filter((m) => m.call).map((m) => ({ key: c.id + ':' + m.id, c, m, r: m.call! })) : []))
      .sort((a, b) => b.m.at - a.m.at || seq(b.m) - seq(a.m)) as Entry[]
  );

  /** Records made within the same clock second keep their creation order (ids count up). */
  const seq = (m: Message) => Number(m.id.replace(/\D/g, '')) || 0;
  const missed = (r: CallRecord) => r.direction === 'in' && (r.outcome === 'missed' || r.outcome === 'cancelled');
  function what(r: CallRecord) {
    if (r.outcome === 'answered') return r.direction === 'out' ? 'Outgoing' : 'Incoming';
    if (r.direction === 'in') return missed(r) ? 'Missed' : 'Declined';
    return ({ missed: 'No answer', declined: 'Declined', cancelled: 'Cancelled', failed: 'Couldn\'t connect' } as Record<string, string>)[r.outcome] ?? 'Call';
  }
  const open = (c: DirectChat) => (prefs.form === 'desktop' ? nav.open({ name: 'conversation', chat: c.id }) : nav.push({ name: 'conversation', chat: c.id }));
  const callable = (c: DirectChat) => call.canCall(c.person);
  const why = (c: DirectChat) => {
    const r = call.refusal(c.person);
    return r === 'blocked' ? 'Blocked' : r === 'messages-only' ? 'Messages only: calling needs their permission' : 'Call';
  };
</script>

{#if !entries.length}
  <div class="empty"><div class="art"><Icon name="phone" /></div><h3>No calls yet</h3><p>Calls you make and receive show up here.</p></div>
{:else}
  <ul class="list" role="list">
    {#each entries as e (e.key)}
      {@const bad = missed(e.r)}
      <li class="item" class:sel={nav.openChat === e.c.id}>
        <button type="button" class="pf press" aria-label="{chatTitle(e.c)}’s profile" onclick={() => nav.push({ name: 'contact', person: e.c.person })}>
          <Avatar name={chatTitle(e.c)} hue={person(e.c.person)?.hue ?? 0} person={e.c.person} />
        </button>
        <button type="button" class="row press" onclick={() => open(e.c)}
          aria-label="{chatTitle(e.c)}, {what(e.r)} call{e.r.outcome === 'answered' ? ', ' + fmtShort(e.r.durationS) : ''}, {listStamp(e.m.at)}. Open chat">
          <span class="mid">
            <span class="name">{chatTitle(e.c)}</span>
            <span class="sub" class:bad><Icon name={e.r.direction === 'out' ? 'call-out' : 'call-in'} size={15} /><span>{what(e.r)}{#if e.r.outcome === 'answered'}{' · '}<span class="dur">{fmtShort(e.r.durationS)}</span>{/if}</span></span>
          </span>
          <span class="when">{listStamp(e.m.at)}</span>
        </button>
        <button type="button" class="back press" class:dim={!callable(e.c)} aria-disabled={!callable(e.c)} title={why(e.c)}
          aria-label="Call {chatTitle(e.c)} back" onclick={() => { if (callable(e.c)) call.place(e.c.person); }}><Icon name="phone" size={22} /></button>
      </li>
    {/each}
  </ul>
{/if}

<style>
  .list { list-style: none; margin: 0; padding: 0 8px; }
  .item { display: flex; align-items: center; gap: 2px; border-radius: var(--r-ctl); }
  .item.sel { background: var(--accent-soft); }
  .item:hover { background: var(--surface-2); }
  .item.sel:hover { background: var(--accent-soft); }
  /* the avatar opens their profile; the rest of the row opens the chat */
  .pf { flex: none; display: grid; place-items: center; padding-block: 8px; padding-inline: 8px 0; border: 0; border-radius: var(--r-ctl); background: transparent; cursor: pointer; }
  .row { flex: 1; min-inline-size: 0; display: flex; gap: 12px; align-items: center; min-block-size: 68px; padding: 8px; padding-inline-start: 12px; border: 0; border-radius: var(--r-ctl); background: transparent; text-align: start; cursor: pointer; color: inherit; font: inherit; }
  .mid { flex: 1; min-inline-size: 0; display: grid; gap: 2px; }
  .name { font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sub { display: flex; align-items: center; gap: 5px; font-size: var(--fs-sub); color: var(--muted); }
  .sub.bad { color: var(--danger); }
  .dur, .when { font-variant-numeric: tabular-nums; }
  .when { font-size: var(--fs-meta); color: var(--muted); flex: none; }
  .back { flex: none; inline-size: var(--btn); block-size: var(--btn); border-radius: 50%; border: 0; background: transparent; color: var(--accent-ink); display: grid; place-items: center; cursor: pointer; }
  .back:hover { background: var(--surface-2); }
  .back.dim { color: var(--muted); opacity: .45; cursor: default; }
  .back.dim:hover { background: transparent; }
  .empty { margin: 48px auto 0; padding: 0 24px; text-align: center; max-inline-size: 320px; color: var(--muted); font-size: var(--fs-sub); line-height: 19px; }
  .empty h3 { color: var(--ink); margin: 14px 0 4px; font-size: var(--fs-body); font-weight: 600; }
  .empty p { margin: 0; }
  .art { inline-size: 52px; block-size: 52px; border-radius: 50%; background: var(--surface-2); color: var(--muted); display: grid; place-items: center; margin: 0 auto; }
</style>
