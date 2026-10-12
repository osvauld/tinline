<script lang="ts">
  /* Chats tab: 1:1 and group conversations together, most recent first (real timestamps). */
  import Avatar from '../../lib/ui/Avatar.svelte';
  import Badge from '../../lib/ui/Badge.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import { db, chatsByRecent, chatTitle, lastMessage, person, firstName } from '../../lib/model/store.svelte';
  import { listStamp } from '../../lib/model/clock.svelte';
  import { nav } from '../../lib/nav/router.svelte';
  import { prefs } from '../../lib/model/prefs.svelte';
  import type { Chat } from '../../lib/model/types';

  let filter = $state<'all' | 'unread'>('all');

  const visible = $derived(
    chatsByRecent(db.chats.filter((c) => (c.kind === 'direct' || c.status === 'active')))
      .filter((c) => filter === 'all' || c.unread > 0)
  );

  function preview(c: Chat) {
    const m = lastMessage(c);
    if (!m) return '';
    if (m.call) return m.call.direction === 'in' && (m.call.outcome === 'missed' || m.call.outcome === 'cancelled') ? 'Missed call' : (m.call.kind === 'video' ? 'Video call' : 'Voice call');
    const who = c.kind === 'group' && !m.system ? (m.from === 'me' ? 'You: ' : firstName(m.from) + ': ') : '';
    return who + (m.file ? m.file.name : m.text);
  }
  const open = (c: Chat) => (prefs.form === 'desktop' ? nav.open({ name: 'conversation', chat: c.id }) : nav.push({ name: 'conversation', chat: c.id }));
</script>

<div class="chips" role="group" aria-label="Filter chats">
  {#each [['all', 'All'], ['unread', 'Unread']] as [k, l] (k)}
    <button type="button" class="t-chip press" aria-pressed={filter === k} onclick={() => (filter = k as typeof filter)}>{l}</button>
  {/each}
</div>

<ul class="list" role="list">
  {#each visible as c (c.id)}
    {@const m = lastMessage(c)}
    {@const p = c.kind === 'direct' ? person(c.person) : undefined}
    <li>
      <button type="button" class="row press" class:sel={nav.openChat === c.id} onclick={() => open(c)}>
        <Avatar name={chatTitle(c)} hue={p?.hue ?? 0} group={c.kind === 'group'} person={p?.id} />
        <span class="mid">
          <span class="top"><span class="name">{chatTitle(c)}</span>
            <span class="when" class:hot={c.unread > 0 && !c.muted}>{#if c.muted}<Icon name="mute" size={14} label="Muted" />{/if}{m ? listStamp(m.at) : ''}</span></span>
          <span class="bot">
            <span class="pv">
              {#if c.draft}<b class="draft">Draft:</b> {c.draft}
              {:else if m?.delivery === 'waiting'}<span class="wait"><Icon name="clock" size={13} /> Waiting</span> · {preview(c)}
              {:else}{preview(c)}{/if}
            </span>
            <Badge count={c.unread} muted={c.muted} />
          </span>
        </span>
      </button>
    </li>
  {/each}
</ul>

<style>
  .chips { display: flex; gap: 8px; padding: 4px 16px 8px; }
  .list { list-style: none; margin: 0; padding: 0 8px; }
  .row { display: flex; gap: 12px; align-items: center; inline-size: 100%; min-height: 72px; padding: 8px; border: 0; border-radius: var(--r-ctl);
    background: transparent; text-align: start; cursor: pointer; }
  .row:hover { background: var(--surface-2); }
  .row.sel { background: var(--accent-soft); }
  .mid { flex: 1; min-width: 0; display: grid; grid-template-columns: minmax(0, 1fr); gap: 2px; }
  .top, .bot { display: flex; align-items: center; gap: 8px; justify-content: space-between; }
  .name { flex: 1; min-width: 0; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .when { font-size: var(--fs-meta); color: var(--muted); display: inline-flex; gap: 4px; align-items: center; flex: none; }
  .when.hot { color: var(--accent-ink); font-weight: 700; }
  .pv { flex: 1; min-width: 0; font-size: var(--fs-sub); color: var(--muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .draft { color: var(--accent-ink); font-weight: 600; }
  .wait { color: var(--gold-ink); font-weight: 600; display: inline-flex; gap: 3px; align-items: center; vertical-align: -2px; }
</style>
