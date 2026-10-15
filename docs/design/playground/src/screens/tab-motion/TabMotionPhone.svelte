<script lang="ts">
  import { tick } from 'svelte';
  import type { Tab } from '../../lib/nav/router.svelte';
  import { db, displayName } from '../../lib/model/store.svelte';
  import { TAB_LISTS } from '../registry';
  import { TABS, TAB_ICON } from '../shell/TabIcon';
  import Icon from '../../lib/ui/Icon.svelte';
  import Mark from '../../lib/brand/Mark.svelte';
  import PersonRow from '../people/PersonRow.svelte';
  import { panelIn, panelOut, type Treatment } from './transitions';

  let { treatment, tab, direction, onselect, rtl = false }:
    { treatment: Treatment; tab: Tab; direction: number; onselect: (tab: Tab) => void; rtl?: boolean } = $props();
  const positions = new Map<Tab, number>();
  const contacts = $derived([...db.people.filter((p) => p.contact && !p.blocked)]
    .sort((a, b) => displayName(a.id).localeCompare(displayName(b.id))));
  const unread = $derived(db.chats.reduce((sum, c) => sum + (c.muted ? 0 : c.unread), 0));
  const action = $derived(tab === 'contacts' ? 'Add contact' : tab === 'groups' ? 'New group' : tab === 'chats' ? 'New chat' : null);
  const params = $derived({ treatment, direction: direction * (rtl ? -1 : 1) });

  function scrollMemory(node: HTMLElement, key: Tab) {
    // Restore after children mount; every treatment owns a separate per-tab scroll memory.
    void tick().then(() => { if (node.isConnected) node.scrollTop = positions.get(key) ?? 0; });
    const save = () => positions.set(key, node.scrollTop);
    node.addEventListener('scroll', save, { passive: true });
    return { destroy() { node.removeEventListener('scroll', save); } };
  }
  function tabKey(event: KeyboardEvent, current: Tab) {
    const step = event.key === 'ArrowRight' ? (rtl ? -1 : 1) : event.key === 'ArrowLeft' ? (rtl ? 1 : -1) : 0;
    const index = event.key === 'Home' ? 0 : event.key === 'End' ? TABS.length - 1
      : step ? (TABS.indexOf(current) + step + TABS.length) % TABS.length : -1;
    if (index < 0) return;
    event.preventDefault();
    onselect(TABS[index]);
    document.getElementById(`${treatment}-${TABS[index]}-tab`)?.focus();
  }
  function deactivate(event: Event) {
    const node = event.currentTarget as HTMLElement;
    node.inert = true;
    node.setAttribute('aria-hidden', 'true');
  }
</script>

<div class="phone" dir={rtl ? 'rtl' : 'ltr'} data-phone={treatment}>
  <header class="appbar"><span class="brand"><Mark size={28} state="taut" /><b>Tinline</b></span><Icon name="gear" size={22} /></header>
  <div class="stack">
    {#each [tab] as current (current)}
      {@const L = TAB_LISTS[current]}
      <div class="panel" role="tabpanel" tabindex="0" id="{treatment}-{current}-panel" aria-labelledby="{treatment}-{current}-tab"
        data-panel={current} in:panelIn={params} out:panelOut={params} onoutrostart={deactivate}>
        <h2>{L.title}</h2>
        <div class="list" data-scroll={current} use:scrollMemory={current}>
          <!-- Read-only in this motion study: existing list actions must not open calls or mutate shared seed data. -->
          <div inert class="preview">
            {#if current === 'contacts'}
              <div class="contact-add"><span class="add-icon"><Icon name="userplus" size={22} /></span><span><b>Add contact</b><small>Share your code or paste theirs</small></span></div>
              {#each contacts as p (p.id)}
                <PersonRow {p} sub={p.alias ? p.name : ''} onclick={() => {}} />
              {/each}
              <p class="count">{contacts.length} contacts</p>
            {:else}
              <L.component {...L.props({ name: 'home', tab: current })} />
            {/if}
          </div>
        </div>
      </div>
    {/each}
  </div>
  <!-- Layout preview only. Same reserved dock height on every tab; Contacts no longer has a top filter. -->
  <div class="dock" data-dock>
    <div class="search"><Icon name="search" size={20} /><span>Search</span></div>
    <div class="add" class:absent={!action} aria-label={action ?? undefined}><Icon name="plus" size={24} /></div>
  </div>
  <div class="bar" role="tablist" aria-label="{treatment} main tabs">
    {#each TABS as item (item)}
      <button type="button" role="tab" id="{treatment}-{item}-tab" aria-selected={tab === item}
        aria-controls={tab === item ? `${treatment}-${item}-panel` : undefined} tabindex={tab === item ? 0 : -1}
        onclick={() => onselect(item)} onkeydown={(event) => tabKey(event, item)} data-tab={item}>
        <span class="pill"><Icon name={TAB_ICON[item]} size={22} />{#if item === 'chats' && unread}<span class="badge">{unread}</span>{/if}</span>
        <span>{TAB_LISTS[item].title}</span>
      </button>
    {/each}
  </div>
</div>

<style>
  .phone { block-size: 700px; max-block-size: calc(100dvh - 260px); min-block-size: 440px; display: flex; flex-direction: column; overflow: hidden; border: 1px solid var(--line); border-radius: 28px; background: var(--bg); box-shadow: var(--shadow-sheet); }
  .appbar { flex: none; min-block-size: 56px; display: flex; align-items: center; justify-content: space-between; padding-inline: 12px 16px; }
  .brand { display: flex; align-items: center; gap: 8px; font-size: 19px; }
  .stack { display: grid; flex: 1; min-block-size: 0; overflow: hidden; isolation: isolate; }
  .panel { grid-area: 1 / 1; display: flex; flex-direction: column; min-block-size: 0; min-inline-size: 0; transform-origin: 50% 0; }
  h2 { flex: none; font-size: 28px; line-height: 34px; margin: 4px 16px 8px; }
  .list { flex: 1; min-block-size: 0; overflow: auto; overscroll-behavior: contain; scrollbar-width: thin; }
  .preview { padding-block-end: 16px; }
  .contact-add { display: flex; align-items: center; gap: 12px; min-block-size: 72px; padding-inline: 16px; }
  .contact-add > span:last-child { display: grid; }
  .contact-add small, .count { color: var(--muted); font-size: var(--fs-sub); }
  .add-icon { display: grid; place-items: center; inline-size: 44px; block-size: 44px; border-radius: 50%; background: var(--accent-soft); color: var(--accent-ink); }
  .count { text-align: center; }
  .dock { display: flex; flex: none; align-items: center; gap: 10px; padding: 8px 12px 10px; }
  .search { display: flex; flex: 1; min-inline-size: 0; align-items: center; gap: 10px; block-size: 48px; border-radius: 24px; background: var(--surface-2); color: var(--muted); padding-inline: 16px; }
  .add { display: grid; flex: none; place-items: center; inline-size: 48px; block-size: 48px; border-radius: 50%; background: var(--accent-soft); color: var(--accent-ink); }
  .absent { visibility: hidden; }
  .bar { display: grid; flex: none; grid-template-columns: repeat(4, 1fr); border-block-start: 1px solid var(--hairline); background: var(--surface); padding-block: 6px 10px; }
  .bar button { display: grid; justify-items: center; align-content: center; gap: 2px; min-block-size: 56px; padding: 0; border: 0; background: none; color: var(--muted); font: inherit; font-size: 12px; font-weight: 600; cursor: pointer; transition: color var(--dur-indicator) var(--ease-out); }
  .pill { position: relative; display: grid; place-items: center; inline-size: 56px; block-size: 30px; }
  .pill::before { content: ''; position: absolute; inset: 0; border-radius: 15px; background: var(--accent-soft); transform: scaleX(.3); opacity: 0; transition: transform var(--dur-indicator) var(--ease-out), opacity var(--dur-indicator) var(--ease-out); }
  .pill :global(svg) { position: relative; }
  [aria-selected='true'] { color: var(--accent-ink) !important; }
  [aria-selected='true'] .pill::before { opacity: 1; transform: none; }
  .badge { position: absolute; inset-block-start: -4px; inset-inline-end: 2px; background: var(--accent); color: var(--on-accent); min-inline-size: 18px; block-size: 18px; border-radius: 9px; font-size: 11px; padding-inline: 4px; }
</style>
