<script module lang="ts">
  /* Reply context survives leaving and re-opening a chat (edit does not: it restores the draft). */
  const replyByChat = new Map<string, string>();
</script>

<script lang="ts">
  import { onDestroy, onMount, tick, untrack } from 'svelte';
  import { fade } from 'svelte/transition';
  import Avatar from '../../lib/ui/Avatar.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import Layer from './Layer.svelte';
  import MessageRow from './MessageRow.svelte';
  import FindBar from '../search/FindBar.svelte';
  import { find } from '../search/find.svelte';
  import { SEARCH, findInChat, jump } from '../../lib/model/actions/search';
  import Composer, { type Ctx } from './Composer.svelte';
  import { db, chat as getChat, person, firstName, markRead, chatTitle } from '../../lib/model/store.svelte';
  import { clock, dayKey, dayLabel, hhmm } from '../../lib/model/clock.svelte';
  import { prefs } from '../../lib/model/prefs.svelte';
  import { motion } from '../../lib/fx/reduced.svelte';
  import { dur } from '../../lib/fx/motion';
  import { nav } from '../../lib/nav/router.svelte';
  import { call } from '../../lib/calls/call.svelte';
  import { gcall } from '../../lib/calls/gcall.svelte';
  import OngoingCall from '../groups/OngoingCall.svelte';
  import {
    SAMPLE_FILES, SIM, attachFile, cancelFile, canDelete, canEdit, canPost, deleteMessage, firstUnreadId, retryMessage, waitingReason,
    type Undo,
  } from '../../lib/model/actions/chat';
  import type { Message } from '../../lib/model/types';

  let { chatId }: { chatId: string } = $props();

  const c = $derived(getChat(chatId));
  const peer = $derived(c?.kind === 'direct' ? person(c.person) : undefined);
  const popMode = $derived(prefs.form === 'desktop');
  const fname = $derived(c?.kind === 'direct' ? firstName(c.person) : (c?.name ?? ''));

  /* ---- opening: unread divider + mark read ---- */
  const unreadAtOpen = untrack(() => c?.unread ?? 0);
  const dividerId = untrack(() => (c ? firstUnreadId(c, unreadAtOpen) : null));
  const initialIds = new Set(untrack(() => c?.messages.map((m) => m.id) ?? []));
  $effect(() => { if (c && c.unread > 0) markRead(c); }); // anything arriving while open is read

  /* ---- transient UI state ---- */
  let root = $state<HTMLElement>();
  let listEl = $state<HTMLElement>();
  let stripH = $state(0);
  let stripPad = $state(0);
  // Strip space at the list top: when it changes, move scrollTop by the same amount so nothing visible moves.
  $effect(() => {
    const h = strip || gbanner ? stripH : 0;
    untrack(() => {
      const d = h - stripPad;
      if (!d) return;
      const el = listEl;
      stripPad = h;
      if (!el) return;
      const top = el.scrollTop;
      el.style.paddingBlockStart = `${6 + h}px`;     // same task as the scroll fix below: no frame shows a moved list
      if (el.scrollHeight > el.clientHeight) el.scrollTop = top + d;
    });
  });
  let composer = $state<ReturnType<typeof Composer>>();
  let ctx = $state<Ctx>(untrack(() => { const id = replyByChat.get(chatId); return id ? { type: 'reply', id } : null; }));
  let stash = '';
  let menu = $state<{ m: Message; mode: 'pop' | 'sheet'; opener: HTMLElement } | null>(null);
  let info = $state<Message | null>(null);
  let del = $state<Message | null>(null);
  let attachOpen = $state(false);
  let heldId = $state<string | null>(null);
  let flashId = $state<string | null>(null);
  /* ---- search: opened on a hit (jump + brief highlight) and find in this chat (Ctrl+F, desktop) ---- */
  const jumpId = untrack(() => jump.take(chatId));
  let pin = !!jumpId;                      // while opening on a hit, nothing may scroll the list to its end
  let foundId = $state<string | null>(null);
  let foundTimer = 0;
  const fm = $derived(find.open ? findInChat(c, find.q) : []);
  const fmap = $derived(new Map(fm.map((x) => [x.id, x.ranges])));
  const curId = $derived(fm.length ? fm[Math.max(0, Math.min(find.cur, fm.length - 1))].id : null);
  function closeFind() { find.close(); composer?.focus(); }
  function onkey(e: KeyboardEvent) {
    if (prefs.form === 'desktop' && (e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === 'f' && c) { e.preventDefault(); find.show(); }
  }
  $effect(() => { const id = curId; untrack(() => { if (id && find.open) tick().then(() => scrollToEl(listEl?.querySelector(`[data-mid="${id}"]`) ?? null, 'center', smooth())); }); });
  onDestroy(() => { clearTimeout(foundTimer); find.close(); });
  let undo = $state<{ h: Undo; what: string } | null>(null);
  let undoTimer = 0;
  let announce = $state('');
  let far = $state(false);
  let below = $state(0);
  let returnTo: HTMLElement | null = null;
  const layerOpen = $derived(!!menu || !!info || !!del || attachOpen);
  const target = $derived(ctx && c ? c.messages.find((m) => m.id === ctx!.id) : undefined);
  $effect(() => { if (ctx && !target) { if (ctx.type === 'edit' && c) { c.draft = stash; stash = ''; } ctx = null; } });

  /* ---- list model: day separators, unread divider, system lines, grouping ---- */
  type Item =
    | { k: 'day'; id: string; label: string }
    | { k: 'unread'; id: string; n: number }
    | { k: 'sys'; id: string; text: string }
    | { k: 'msg'; id: string; m: Message; pos: 'solo' | 'first' | 'mid' | 'last'; sender: boolean };
  const GROUP_GAP = 5 * 60_000;
  const items = $derived.by<Item[]>(() => {
    if (!c) return [];
    const ms = c.messages;
    const plain = (m?: Message) => !!m && !m.system && !m.call;
    const join = (a?: Message, b?: Message) => plain(a) && plain(b) && a!.from === b!.from && dayKey(a!.at) === dayKey(b!.at) && Math.abs(b!.at - a!.at) <= GROUP_GAP;
    const out: Item[] = [];
    let day = '';
    ms.forEach((m, i) => {
      const dk = dayKey(m.at);
      if (dk !== day) { day = dk; out.push({ k: 'day', id: 'day-' + dk, label: dayLabel(m.at) }); }
      if (m.id === dividerId) out.push({ k: 'unread', id: 'unread', n: unreadAtOpen });
      if (m.system) { out.push({ k: 'sys', id: m.id, text: m.text }); return; }
      const a = join(ms[i - 1], m), b = join(m, ms[i + 1]);
      const pos = a && b ? 'mid' : a ? 'last' : b ? 'first' : 'solo';
      out.push({ k: 'msg', id: m.id, m, pos, sender: c.kind === 'group' && m.from !== 'me' && !m.call && !a });
    });
    return out;
  });

  /* ---- header ---- */
  const status = $derived.by(() => {
    if (!c) return '';
    if (c.kind === 'group') return c.status === 'active' ? `${c.members.length} members` : c.status === 'removed' ? 'You were removed' : 'You left';
    if (!db.env.networkOnline) return 'Waiting for connection';
    if (!peer) return '';
    return (peer.online ? 'Online' : 'Offline') + (peer.callsAllowed ? '' : ' · messages only');
  });
  const callable = $derived(c?.kind === 'group' ? c.status === 'active' : !!peer?.callsAllowed);
  /* Group calls (step 6): the header button starts a call, joins the one running, or returns to mine. */
  const gLive = $derived(c?.kind === 'group' ? gcall.liveFor(c.id) : undefined);
  /* The banner is for a call I could join; once I am in it the strip above carries the call. */
  const gbanner = $derived(c?.kind === 'group' && c.status === 'active' && !!gLive && !gcall.inCall(c.id));
  const gLabel = $derived(c?.kind === 'group' ? (gcall.inCall(c.id) ? 'Return to group call' : gLive ? 'Join group call' : 'Start group call') : 'Call');
  const noCallHost = $derived(c?.kind === 'group' && !c.members.some((m) => m.callHost && !m.callHostPaused));
  const callTitle = $derived(c?.kind === 'group' ? (noCallHost && !gLive ? 'No call host yet: an admin can appoint one in Group info' : gLabel) : callable ? 'Call' : 'Messages only: calling needs their permission');
  const strip = $derived.by<{ text: string; icon: string; warn?: boolean } | null>(() => {
    if (!c) return null;
    if (c.kind === 'group' && c.status !== 'active') return null;
    if (!db.env.networkOnline) return { text: 'You’re offline. Messages send when you’re back online.', icon: 'off' };
    if (c.kind === 'group') return db.env.hostsOnline ? null : { text: 'Waiting for a host. No group host is online; your posts are saved and send when one is.', icon: 'clock', warn: true };
    return peer && !peer.online ? { text: `${fname} is offline. Messages send when you’re both online.`, icon: 'off' } : null;
  });

  /** Header button and call-record pills. The machine refuses blocked / message-only people; group calls are step 6. */
  function startCall() {
    if (c?.kind === 'group') { gcall.start(c.id); return; }
    if (c?.kind !== 'direct') return;
    if (gcall.holdsLine) { gcall.inCallToast(); return; }   // InCall: one line, group and 1:1 calls exclude each other
    call.place(c.person);
  }
  function openDetails() {
    if (!c) return;
    nav.push(c.kind === 'group' ? { name: 'groupInfo', chat: c.id } : { name: 'contact', person: c.person });
  }

  /* ---- scrolling ---- */
  const smooth = () => (motion.reduced ? 'auto' : 'smooth');
  const nearBottom = () => !listEl || listEl.scrollHeight - listEl.scrollTop - listEl.clientHeight < 80;
  function scrollEnd(behavior: ScrollBehavior = 'auto') {
    if (pin) return;
    listEl?.scrollTo({ top: listEl.scrollHeight, behavior });
    below = 0;
  }
  function scrollToEl(el: Element | null, where: 'start' | 'center', behavior: ScrollBehavior = 'auto') {
    if (!el || !listEl) return;
    const l = listEl.getBoundingClientRect(), r = el.getBoundingClientRect();
    const off = where === 'start' ? r.top - l.top - 12 : r.top - l.top - (l.height - r.height) / 2;
    listEl.scrollTo({ top: listEl.scrollTop + off, behavior });
  }
  function syncFar() {
    if (!listEl) return;
    far = listEl.scrollHeight - listEl.scrollTop - listEl.clientHeight > 160;
    if (!far) below = 0;
  }
  onMount(async () => {
    await tick();
    const hit = jumpId ? listEl?.querySelector(`[data-mid="${jumpId}"]`) ?? null : null;
    if (hit) {
      scrollToEl(hit, 'center');
      foundId = jumpId;
      foundTimer = window.setTimeout(() => (foundId = null), SEARCH.foundMs);
      requestAnimationFrame(() => requestAnimationFrame(() => { pin = false; syncFar(); }));
      syncFar();
      return;
    }
    pin = false;
    if (dividerId) scrollToEl(listEl?.querySelector('.unread') ?? null, 'start');
    else scrollEnd();
    syncFar();
    if (!ctx) return;
    composer?.focus();
  });

  let lastLen = untrack(() => c?.messages.length ?? 0);
  $effect(() => {
    const n = c?.messages.length ?? 0;
    untrack(() => {
      if (n > lastLen) {
        const last = c!.messages[n - 1];
        if (last.from === 'me' || nearBottom()) tick().then(() => scrollEnd(last.from === 'me' ? 'auto' : smooth()));
        else below += n - lastLen;
      }
      lastLen = n;
    });
  });
  $effect(() => { ctx; untrack(() => { if (!far) tick().then(() => scrollEnd()); }); });

  async function jumpTo(id: string) {
    const el = listEl?.querySelector(`[data-mid="${id}"]`) ?? null;
    scrollToEl(el, 'center', smooth());
    flashId = null; await tick(); flashId = id;
  }

  /* ---- composer glue ---- */
  function cancelCtx() {
    if (ctx?.type === 'edit' && c) { c.draft = stash; stash = ''; }
    ctx = null;
    composer?.focus();
  }
  function startReply(m: Message) {
    if (ctx?.type === 'edit' && c) { c.draft = stash; stash = ''; }
    ctx = { type: 'reply', id: m.id };
    composer?.focus();
  }
  function startEdit(m: Message) {
    if (!c) return;
    if (ctx?.type !== 'edit') stash = c.draft;
    ctx = { type: 'edit', id: m.id };
    c.draft = m.text;
    tick().then(() => composer?.focus(true));
  }
  function onsent() { ctx = null; scrollEnd(); }
  function onedited() {
    if (!c) return;
    c.draft = stash; stash = ''; ctx = null;
    say('Edit saved.');
    composer?.focus();
  }
  function say(t: string) { announce = ''; tick().then(() => (announce = t)); }

  /* ---- message menu ---- */
  function openMenu(m: Message, opener: HTMLElement, mode: 'pop' | 'sheet') {
    if (layerOpen) return;
    returnTo = opener;
    if (mode === 'sheet') heldId = m.id;
    menu = { m, mode, opener };
  }
  async function closeAll(restore = true) {
    menu = null; info = null; del = null; attachOpen = false; heldId = null;
    if (!restore) return;
    await tick();
    if (returnTo?.isConnected) returnTo.focus({ preventScroll: true });
    else composer?.focus();
  }
  async function pick(fn: () => void) { await closeAll(false); fn(); }
  async function copy(m: Message) {
    try { await navigator.clipboard.writeText(m.text); say('Copied.'); } catch { say('Copying isn’t available here. Select the text and copy it instead.'); }
  }

  /* long-press (touch / Android form), right-click (both) */
  const PRESS_MS = 500, PRESS_SLOP = 10;
  let press: { x: number; y: number; t: number } | null = null;
  const pressCancel = () => { if (press) { clearTimeout(press.t); press = null; } };
  const msgOf = (el: Element | null) => { const li = el?.closest<HTMLElement>('[data-mid]'); return li && c ? c.messages.find((m) => m.id === li.dataset.mid) : undefined; };
  const moreOf = (id: string) => listEl?.querySelector<HTMLElement>(`[data-mid="${id}"] .more`) ?? null;
  function onpointerdown(e: PointerEvent) {
    const t = e.target as Element;
    if (!t.closest('.bub') || t.closest('button,a')) return;
    if (e.pointerType === 'mouse' && (e.button !== 0 || popMode)) return;
    const m = msgOf(t);
    if (!m || m.deleted) return;
    pressCancel();
    press = { x: e.clientX, y: e.clientY, t: window.setTimeout(() => {
      press = null;
      const o = moreOf(m.id);
      if (o) openMenu(m, o, 'sheet');
    }, PRESS_MS) };
  }
  function onpointermove(e: PointerEvent) { if (press && Math.hypot(e.clientX - press.x, e.clientY - press.y) > PRESS_SLOP) pressCancel(); }
  function oncontextmenu(e: MouseEvent) {
    const m = msgOf(e.target as Element);
    if (!m || m.deleted || m.call) return;
    e.preventDefault(); pressCancel();
    const o = moreOf(m.id);
    if (o) openMenu(m, o, popMode ? 'pop' : 'sheet');
  }

  /* ---- delete with confirm + undo ---- */
  function commitUndo() { if (undo) { undo.h.commit(); undo = null; } clearTimeout(undoTimer); }
  async function confirmDelete(m: Message, scope: 'me' | 'everyone') {
    if (!c) return;
    await closeAll(false);
    commitUndo();
    const h = deleteMessage(c, m, scope);
    if (!h) return;
    undo = { h, what: scope === 'me' ? 'Deleted from this device.' : 'Deleted for everyone.' };
    undoTimer = window.setTimeout(commitUndo, SIM.undoMs);
    say(undo.what);
    await tick();
    composer?.focus();
  }
  function doUndo() {
    if (!undo) return;
    undo.h.undo(); undo = null; clearTimeout(undoTimer);
    say('Message restored.');
  }

  /* ---- files ---- */
  function attach(f: { name: string; size: number; kind: 'image' | 'doc' | 'video' | 'other' }) {
    if (!c) return;
    closeAll(false);
    attachFile(c, f, ctx?.type === 'reply' ? ctx.id : undefined);
    ctx = null;
    composer?.focus();
    say(`${f.name} added.`);
  }
  function onRealFile(e: Event & { currentTarget: HTMLInputElement }) {
    const f = e.currentTarget.files?.[0];
    e.currentTarget.value = '';
    if (!f) return;
    attach({ name: f.name, size: f.size, kind: f.type.startsWith('image/') ? 'image' : f.type.startsWith('video/') ? 'video' : 'doc' });
  }
  async function cancel(m: Message) {
    cancelFile(m);
    say(`Cancelled ${m.file?.name}. Nothing was left on ${fname}’s phone.`);
    await tick();
    listEl?.querySelector<HTMLElement>(`[data-mid="${m.id}"] [data-act="retry"]`)?.focus();
  }
  function retry(m: Message) { if (c) { retryMessage(c, m); say(m.file ? 'Starting again from the beginning.' : 'Retrying…'); } }

  onDestroy(() => {
    pressCancel();
    commitUndo();
    if (ctx?.type === 'edit' && c) c.draft = stash;
    else if (ctx?.type === 'reply') replyByChat.set(chatId, ctx.id);
    else replyByChat.delete(chatId);
  });

  /* ---- message info rows ---- */
  const when = (at: number) => (dayKey(at) === dayKey(clock.now) ? '' : dayLabel(at) + ' ') + hhmm(at);
  function infoRows(m: Message): [string, string][] {
    if (!c) return [];
    if (m.from !== 'me') return [['From', firstName(m.from) === 'You' ? 'You' : (person(m.from)?.name ?? m.from)], ['Received', when(m.at)]];
    const rows: [string, string][] = [['Sent', when(m.at)]];
    if (m.edited) rows.push(['Edited', m.editedAt ? when(m.editedAt) : 'Yes']);
    rows.push([c.kind === 'group' ? 'Host' : `${fname}’s devices`,
      m.delivery === 'failed' ? 'Not delivered' : m.delivery === 'sending' ? 'Sending…' : m.delivery === 'waiting' ? waitingReason(c)
        : (c.kind === 'group' ? 'Delivered to a host' : 'Delivered') + (m.deliveredAt ? ' ' + hhmm(m.deliveredAt) : '')]);
    return rows;
  }
  const summary = (m: Message) => m.text || m.file?.name || '';
</script>

<svelte:window onkeydown={onkey} />

<div class="conv" bind:this={root} data-conv={chatId}>
  {#if !c}
    <div class="empty"><b>Conversation not found</b></div>
  {:else}
    <div class="col" inert={layerOpen}>
      <header class="bar">
        {#if prefs.form === 'android'}
          <button type="button" class="ib press" aria-label="Back to chats" onclick={() => nav.back()}><Icon name="back" /></button>
        {/if}
        <button type="button" class="head" aria-label="{chatTitle(c)}. {status}{c.muted ? '. Muted' : ''}. Chat details" onclick={openDetails}>
          <Avatar name={chatTitle(c)} hue={peer?.hue ?? 0} group={c.kind === 'group'} person={peer?.id} size={36} />
          <span class="tx"><span class="nm"><b>{chatTitle(c)}</b>{#if c.muted}<Icon name="mute" size={14} />{/if}</span><span class="pr">{status}</span></span>
        </button>
        {#if c.kind === 'direct'}<button type="button" class="ib press" class:dim={!callable} aria-label="Call" aria-disabled={!callable} title={callTitle}
          onclick={() => { if (callable) startCall(); }}><Icon name="phone" /></button>
        {:else if c.status === 'active'}<button type="button" class="ib press" class:dim={noCallHost && !gLive} class:live={!!gLive} aria-label={gLabel} aria-disabled={noCallHost && !gLive} title={callTitle} data-gcall-btn
          onclick={startCall}><Icon name="phone" /></button>{/if}
        <button type="button" class="ib press" aria-label="Find in this chat" title="Find in this chat (Ctrl+F)" onclick={() => find.show()}><Icon name="search" /></button>
        <button type="button" class="ib press" aria-label="Chat details" onclick={openDetails}><Icon name="info" /></button>
        {#if find.open}<FindBar count={fm.length} onclose={closeFind} />{/if}
      </header>

      <div class="msgwrap">
        <!-- Overlaid on the top of the list (not a row above it): the strip coming and going never moves a message.
             While it shows, the list gains the same space at its top (scroll position compensated), so the oldest
             message can still be scrolled fully into view instead of hiding under the strip. -->
        <div class="tops" bind:offsetHeight={stripH}>
          {#if strip}<div class="strip" class:warn={strip.warn} role="status" transition:fade={{ duration: motion.reduced ? 0 : dur('crossfade') }}><Icon name={strip.icon} size={16} />{strip.text}</div>{/if}
          {#if gbanner && c.kind === 'group'}<div class="gb" transition:fade={{ duration: motion.reduced ? 0 : dur('crossfade') }}><OngoingCall gid={c.id} /></div>{/if}
        </div>
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <ul class="msgs" bind:this={listEl} role="log" aria-label="Messages with {chatTitle(c)}" onscroll={() => { pressCancel(); syncFar(); }}
          {onpointerdown} {onpointermove} onpointerup={pressCancel} onpointercancel={pressCancel} {oncontextmenu}>
          {#if !c.messages.length}
            <li class="empty"><div class="art"><Icon name="shield" /></div><h3>Say hello to {fname}</h3><p>Messages go straight to their phone.</p></li>
          {/if}
          {#each items as it (it.id)}
            {#if it.k === 'day'}
              <li class="day">{it.label}</li>
            {:else if it.k === 'unread'}
              <li class="unread" aria-label="{it.n} new {it.n === 1 ? 'message' : 'messages'}"><span>{it.n} new {it.n === 1 ? 'message' : 'messages'}</span></li>
            {:else if it.k === 'sys'}
              <li class="sys">{it.text}</li>
            {:else}
              <MessageRow
                {c} m={it.m} pos={it.pos} last={it.m.id === c.messages[c.messages.length - 1]?.id} sender={it.sender} fresh={!initialIds.has(it.m.id)} held={heldId === it.m.id}
                menuOpen={menu?.m.id === it.m.id} flash={flashId === it.m.id} found={foundId === it.m.id} find={fmap.get(it.m.id)} findCur={curId === it.m.id} onflashend={() => (flashId = null)} {popMode}
                quoted={it.m.replyTo ? c.messages.find((x) => x.id === it.m.replyTo) : undefined}
                onmenu={openMenu} onretry={retry} oncancel={cancel} onjump={jumpTo} oncall={startCall}
              />
            {/if}
          {/each}
        </ul>
        {#if far}
          <button type="button" class="tobottom press" aria-label={below ? `Scroll to latest, ${below} new` : 'Scroll to latest'} onclick={() => scrollEnd(smooth())}>
            <Icon name="chev" />{#if below}<span class="t-badge">{below}</span>{/if}
          </button>
        {/if}
      </div>

      {#if c.kind === 'group' && c.status !== 'active'}
        <div class="readonly"><span>{c.status === 'removed' ? 'You were removed from this group' : 'You left this group'}</span>
          <button type="button" class="details press" onclick={openDetails}>{c.status === 'removed' ? 'See details' : 'Group info'}</button></div>
      {:else if canPost(c)}
        <Composer bind:this={composer} {c} {ctx} {target} oncancel={cancelCtx} {onsent} {onedited} onattach={() => { returnTo = document.activeElement as HTMLElement; attachOpen = true; }} />
      {/if}
    </div>

    {#if undo}
      <div class="toast" role="status"><span>{undo.what}</span><button type="button" class="undo" onclick={doUndo}>Undo</button></div>
    {/if}
    <div class="vh" role="status" aria-live="polite">{announce}</div>

    {#if menu}
      {@const m = menu.m}
      <Layer variant={menu.mode} menu label="Message options" onclose={() => closeAll()} anchor={menu.opener} container={root} alignEnd={m.from === 'me'}>
        <div class="menu">
          {#if !m.call}<button type="button" role="menuitem" class="mi" onclick={() => pick(() => startReply(m))}><Icon name="reply" size={20} />Reply</button>{/if}
          {#if m.text}<button type="button" role="menuitem" class="mi" onclick={() => pick(() => copy(m))}><Icon name="copy" size={20} />Copy</button>{/if}
          {#if canEdit(c, m)}<button type="button" role="menuitem" class="mi" onclick={() => pick(() => startEdit(m))}><Icon name="edit" size={20} />Edit</button>{/if}
          {#if canDelete(c, m)}<button type="button" role="menuitem" class="mi danger" onclick={() => pick(() => (del = m))}><Icon name="trash" size={20} />Delete</button>{/if}
          {#if !m.call}<button type="button" role="menuitem" class="mi" onclick={() => pick(() => (info = m))}><Icon name="info" size={20} />Message info</button>{/if}
        </div>
      </Layer>
    {/if}

    {#if info}
      {@const m = info}
      <Layer variant="dialog" label="Message info" onclose={() => closeAll()}>
        <div class="dlg">
          <h2>Message info</h2>
          <div class="mprev">{summary(m)}</div>
          <div class="rows">{#each infoRows(m) as [k, v] (k)}<div class="r"><span>{k}</span><span>{v}</span></div>{/each}</div>
          {#if m.from === 'me'}<p>{c.kind === 'group' ? 'A check means a host has your post. Nobody is told when it is read.' : `Tinline shows when a message reaches ${fname}’s devices, not when it’s read.`}</p>{/if}
          <div class="acts"><button type="button" class="btn primary" data-autofocus onclick={() => closeAll()}>Done</button></div>
        </div>
      </Layer>
    {/if}

    {#if del}
      {@const m = del}
      {@const both = m.delivery !== 'failed' && !(m.file && m.file.state !== 'done')}
      <Layer variant="dialog" label="Delete message" onclose={() => closeAll()}>
        <div class="dlg">
          <h2>Delete message?</h2>
          <p>{!both ? 'It was never delivered, so it’s only on this phone.' : peer?.online && db.env.networkOnline
            ? `Delete for everyone asks ${fname}’s phone to remove it too. Copies elsewhere can’t be recalled.`
            : `${fname} is offline. Delete for everyone reaches their phone when you’re both online.`}</p>
          <div class="acts" class:stack={prefs.form === 'android'}>
            <button type="button" class="btn quiet" data-autofocus onclick={() => closeAll()}>Cancel</button>
            <button type="button" class="btn {both ? 'quiet dangertx' : 'danger'}" onclick={() => confirmDelete(m, 'me')}>Delete for me</button>
            {#if both}<button type="button" class="btn danger" onclick={() => confirmDelete(m, 'everyone')}>Delete for everyone</button>{/if}
          </div>
        </div>
      </Layer>
    {/if}

    {#if attachOpen}
      <Layer variant="dialog" label="Attach a file" onclose={() => closeAll()}>
        <div class="dlg">
          <h2>Attach a file</h2>
          <p>Only the name and size of a chosen file are used in this playground.</p>
          <div class="list">
            {#each SAMPLE_FILES as s (s.name)}
              <button type="button" class="li" onclick={() => attach(s)}>
                <Icon name={s.kind === 'image' ? 'image' : 'file'} />
                <span class="grow"><b>{s.name}</b><small>{s.kind === 'image' ? 'Photo' : s.kind === 'video' ? 'Video' : 'Document'} · {s.size >= 1e6 ? (s.size / 1e6).toFixed(1) + ' MB' : Math.round(s.size / 1e3) + ' KB'}</small></span>
              </button>
            {/each}
            <label class="li pick"><Icon name="plus" /><span class="grow"><b>Choose from this device</b></span><input type="file" class="vh" onchange={onRealFile} /></label>
          </div>
          <div class="acts"><button type="button" class="btn quiet" onclick={() => closeAll()}>Cancel</button></div>
        </div>
      </Layer>
    {/if}
  {/if}
</div>

<style>
  /* Android supplies its own header below (avatar, status, call); hide the shell's bare back bar for this route. */

  .conv { position: relative; display: flex; flex-direction: column; block-size: 100%; min-block-size: 0; background: var(--canvas); overflow: hidden; }
  .col { display: flex; flex-direction: column; flex: 1; min-block-size: 0; }
  .ib { inline-size: var(--btn); block-size: var(--btn); display: grid; place-items: center; border: 0; border-radius: 50%; background: transparent; color: var(--ink); flex: none; padding: 0; cursor: pointer; }
  .ib:hover { background: var(--surface-2); }
  .ib.live { color: var(--accent-ink); background: var(--accent-soft); }
  .ib.dim { color: var(--muted); opacity: .45; cursor: default; }
  .ib.dim:hover { background: transparent; }

  .bar { position: relative; display: flex; align-items: center; gap: 2px; min-block-size: 60px; padding: 8px; background: var(--surface); border-block-end: 1px solid var(--hairline); flex: none; }
  .head { flex: 1; min-inline-size: 0; display: flex; align-items: center; gap: 10px; border: 0; background: none; padding: 4px 10px 4px 6px; border-radius: 12px; min-block-size: var(--btn); text-align: start; cursor: pointer; }
  .head:hover { background: var(--surface-2); }
  .tx { flex: 1; min-inline-size: 0; }
  .nm { display: flex; align-items: center; gap: 6px; font-size: var(--fs-title); line-height: 22px; }
  .nm b { font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .nm :global(svg) { color: var(--muted); }
  .pr { display: block; font-size: var(--fs-sub); line-height: 18px; color: var(--muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  :global([data-form='android']) .bar { min-block-size: 56px; padding: 4px 6px; }
  .tops { position: absolute; inset-block-start: 0; inset-inline: 0; z-index: 5; display: grid; }
  .strip { display: flex; align-items: center; gap: 8px; padding: 7px 16px; font-size: var(--fs-sub); line-height: 18px; color: var(--muted); background: var(--surface-2); border-block-end: 1px solid var(--hairline); }
  .strip.warn { background: var(--warn-bg); color: var(--warn-ink); }

  .msgwrap { flex: 1; min-block-size: 0; position: relative; display: flex; flex-direction: column; }
  .msgs { list-style: none; margin: 0; padding: 6px 16px 19px; overflow: auto; overflow-anchor: none; flex: 1; min-block-size: 0; display: flex; flex-direction: column; overscroll-behavior: contain; }
  :global([data-form='android']) .msgs { padding: 6px 10px 19px; }
  .msgs > :global(:first-child) { margin-block-start: auto; }
  .msgs > :global(*) { flex-shrink: 0; }
  .day { align-self: center; margin: 10px 0; padding: 3px 10px; border-radius: 999px; background: var(--surface); box-shadow: inset 0 0 0 1px var(--hairline); color: var(--muted); font-size: var(--fs-meta); font-weight: 600; line-height: 16px; }
  .msgs :global(.day + .m) { margin-block-start: 0; }
  .sys { align-self: center; text-align: center; font-size: 12px; line-height: 17px; color: var(--muted); margin: 6px 0; max-inline-size: 86%; }
  .msgs :global(:is(.sys, .unread) + .m) { margin-block-start: 4px; }
  .unread { display: flex; align-items: center; gap: 10px; margin: 12px 0 6px; color: var(--accent-ink); font-size: var(--fs-meta); font-weight: 700; }
  .unread::before, .unread::after { content: ''; flex: 1; block-size: 1px; background: var(--accent-soft); background: color-mix(in srgb, var(--accent-ink) 35%, transparent); }
  .empty { margin: auto; padding: 32px 24px; text-align: center; max-inline-size: 320px; color: var(--muted); font-size: var(--fs-sub); line-height: 19px; list-style: none; }
  .empty h3 { color: var(--ink); margin: 14px 0 4px; font-size: var(--fs-body); font-weight: 600; }
  .empty p { margin: 0; }
  .art { inline-size: 52px; block-size: 52px; border-radius: 50%; background: var(--surface-2); color: var(--muted); display: grid; place-items: center; margin: 0 auto; }
  .tobottom { position: absolute; inset-inline-end: 16px; inset-block-end: 12px; inline-size: 44px; block-size: 44px; border-radius: 50%; border: 0; padding: 0; background: var(--surface); color: var(--ink); box-shadow: var(--shadow-pop); display: grid; place-items: center; cursor: pointer; animation: rise var(--dur-bubbleIn) var(--ease-out); }
  .tobottom .t-badge { position: absolute; inset-block-start: -4px; inset-inline-end: -4px; box-shadow: 0 0 0 2px var(--canvas); }
  @keyframes rise { from { opacity: 0; transform: translateY(4px); } to { opacity: 1; transform: none; } }
  .readonly { margin: 0; padding: 6px 16px calc(6px + env(safe-area-inset-bottom)); display: flex; align-items: center; justify-content: center; flex-wrap: wrap; column-gap: 4px; background: var(--surface); border-block-start: 1px solid var(--hairline); color: var(--muted); font-size: var(--fs-sub); line-height: 19px; }
  .details { min-block-size: var(--btn); padding: 0 10px; border: 0; border-radius: 12px; background: none; color: var(--accent-ink); font-weight: 650; font-size: var(--fs-sub); text-decoration: underline; text-underline-offset: 3px; cursor: pointer; }
  .details:hover { background: var(--surface-2); }

  .toast { position: absolute; inset-inline: 12px; inset-block-end: 76px; margin-inline: auto; inline-size: fit-content; max-inline-size: calc(100% - 24px); display: flex; align-items: center; gap: 12px;
    background: var(--ink); color: var(--bg); padding: 4px 6px 4px 16px; border-radius: 12px; box-shadow: var(--shadow-pop); font-size: var(--fs-sub); line-height: 19px; z-index: 20; animation: rise var(--dur-enter) var(--ease-out); }
  .undo { min-block-size: var(--btn); padding: 0 14px; border: 0; border-radius: 10px; background: transparent; color: inherit; font-weight: 700; text-decoration: underline; text-underline-offset: 3px; cursor: pointer; }

  /* menu + dialogs (content of <Layer>) */
  .menu { display: flex; flex-direction: column; padding: 6px 0 10px; }
  .mi { display: flex; align-items: center; gap: 16px; min-block-size: 52px; padding: 0 24px; border: 0; background: none; text-align: start; inline-size: 100%; font-size: var(--fs-body); color: var(--ink); cursor: pointer; }
  .mi :global(svg) { color: var(--muted); }
  .mi:hover, .mi:focus-visible { background: var(--surface-2); }
  .mi:focus-visible { outline-offset: -2px; }
  .mi.danger, .mi.danger :global(svg) { color: var(--danger); }
  :global([data-form='desktop']) .menu { padding: 6px 0; }
  :global([data-form='desktop']) .mi { min-block-size: var(--btn); padding: 0 14px; gap: 12px; font-size: 14px; }
  .dlg { padding: 20px 20px 16px; }
  .dlg h2 { margin: 0 0 4px; font-size: var(--fs-title); font-weight: 650; line-height: 24px; }
  .dlg p { margin: 6px 0; color: var(--muted); font-size: var(--fs-sub); line-height: 19px; }
  .acts { display: flex; gap: 8px; justify-content: flex-end; flex-wrap: wrap; margin-block-start: 18px; }
  .acts.stack { flex-direction: column-reverse; }
  .acts.stack .btn { inline-size: 100%; }
  .btn { min-block-size: var(--btn); display: inline-flex; align-items: center; justify-content: center; gap: 8px; padding: 0 18px; border-radius: 22px; border: 1px solid var(--line); background: var(--surface); font-weight: 600; font-size: var(--fs-body); color: var(--ink); cursor: pointer; }
  .btn:hover { background: var(--surface-2); }
  .btn.primary { background: var(--accent); border-color: transparent; color: var(--on-accent); }
  .btn.primary:hover { filter: brightness(1.08); }
  .btn.danger { background: var(--danger-btn); border-color: transparent; color: var(--on-danger-btn); }
  .btn.danger:hover { filter: brightness(1.08); }
  .btn.quiet { background: transparent; border-color: transparent; }
  .btn.quiet:hover { background: var(--surface-2); }
  .btn.dangertx { color: var(--danger); }
  .rows { margin: 14px 0 0; display: grid; }
  .r { display: flex; justify-content: space-between; align-items: baseline; gap: 16px; padding: 11px 0; border-block-start: 1px solid var(--hairline); font-size: 14px; line-height: 20px; }
  .r span:last-child { color: var(--muted); text-align: end; font-variant-numeric: tabular-nums; }
  .mprev { margin: 12px 0 0; padding: 8px 12px; border-radius: 14px; background: var(--surface-2); font-size: 14px; line-height: 20px; display: -webkit-box; -webkit-line-clamp: 3; line-clamp: 3; -webkit-box-orient: vertical; overflow: hidden; overflow-wrap: anywhere; }
  .list { display: flex; flex-direction: column; margin: 8px -8px 0; }
  .li { display: flex; align-items: center; gap: 12px; min-block-size: 52px; padding: 6px 10px; border: 0; border-radius: var(--r-ctl); background: none; text-align: start; inline-size: 100%; color: var(--ink); cursor: pointer; }
  .li:hover, .li:focus-within { background: var(--surface-2); }
  .li > :global(svg) { color: var(--muted); }
  .li .grow { flex: 1; min-inline-size: 0; display: grid; }
  .li .grow b { font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .li .grow small { font-size: var(--fs-sub); color: var(--muted); }
</style>
