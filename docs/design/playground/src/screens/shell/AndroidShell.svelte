<script lang="ts">
  /* Android: a stack. Home shows the tab list + bottom bar; other routes cover it full-screen. */
  import { nav } from '../../lib/nav/router.svelte';
  import { db } from '../../lib/model/store.svelte';
  import { TAB_LISTS, SCREENS, GATE } from '../registry';
  import { TABS, TAB_ICON } from './TabIcon';
  import Icon from '../../lib/ui/Icon.svelte';
  import IconButton from '../../lib/ui/IconButton.svelte';
  import Mark from '../../lib/brand/Mark.svelte';
  import TabPanel, { rememberScroll } from '../../lib/ui/TabPanel.svelte';
  import { statusLook } from '../../lib/brand/status.svelte';
  import CallStrip from '../../lib/calls/CallStrip.svelte';
  import CallLayer from '../../lib/calls/CallLayer.svelte';
  import SystemLayer from '../../lib/calls/SystemLayer.svelte';
  const look = $derived(statusLook());

  /** Screens that draw their own header (with Back) instead of the shell's bare one. */
  const OWN_BAR = new Set(['conversation', 'groupInfo', 'contact', 'profile', 'settings']);
  const top = $derived(nav.top);
  const unread = $derived(db.chats.reduce((n, c) => n + (c.muted ? 0 : c.unread), 0));
</script>

{#if db.account.gate !== 'ready'}
  <div class="gate" data-form-gate><GATE.component {...GATE.props(null)} /></div>
{:else}
<div class="phone">
  <CallStrip />
  <div class="vp">
  {#if top.name === 'home'}
    {@const L = TAB_LISTS[top.tab]}
    <header class="appbar">
      <span class="brand"><span class="st" title={look.label}><Mark size={28} live state={look.state} sway={look.sway} dim={look.dim} /><span class="vh" aria-live="polite">{look.label}</span></span><b>Tinline</b></span>
      <span class="acts">
        <IconButton icon="search" label="Search" onclick={() => nav.push({ name: 'search' })} />
        <IconButton icon="gear" label="Settings" onclick={() => nav.push({ name: 'settings' })} />
      </span>
    </header>
    <TabPanel key={top.tab}>
      <h1 class="title">{L.title}</h1>
      <main class="body" use:rememberScroll={top.tab}><L.component {...L.props(top)} /></main>
    </TabPanel>
    <nav class="bar" aria-label="Main">
      {#each TABS as t (t)}
        <button type="button" class="tab press" aria-current={nav.tab === t ? 'page' : undefined} onclick={() => nav.setTab(t)}>
          <span class="pill"><Icon name={TAB_ICON[t]} size={22} />{#if t === 'chats' && unread}<span class="dot">{unread}</span>{/if}</span>
          <span class="lbl">{TAB_LISTS[t].title}</span>
        </button>
      {/each}
    </nav>
  {:else}
    {@const S = SCREENS[top.name]}
    {#key top}
      <section class="screen" class:push={nav.dir === 'push'}>
        {#if !OWN_BAR.has(top.name)}<header class="appbar"><IconButton icon="back" label="Back" onclick={() => nav.back()} /></header>{/if}
        <main class="body"><S.component {...S.props(top)} /></main>
      </section>
    {/key}
  {/if}
  </div>
  <CallLayer />
  <SystemLayer />
</div>
{/if}

<style>
  .st { display: inline-flex; }
  .gate { height: 100%; overflow: auto; background: var(--bg); }
  .phone { position: relative; display: flex; flex-direction: column; height: 100%; background: var(--bg); overflow: hidden; }
  .vp { position: relative; flex: 1; min-height: 0; display: flex; flex-direction: column; }
  .appbar { display: flex; align-items: center; justify-content: space-between; min-height: 56px; padding: 4px 8px 0 12px; }
  .brand { display: inline-flex; align-items: center; gap: 8px; font-size: 19px; }
  .acts { display: flex; }
  .title { font-size: 28px; line-height: 34px; margin: 4px 16px 8px; }
  .body { flex: 1; min-height: 0; overflow: auto; }
  .screen { position: absolute; inset: 0; display: flex; flex-direction: column; background: var(--bg); }
  .screen.push { animation: push var(--dur-overlayOpen) var(--ease-out); }
  @keyframes push { from { translate: 24px 0; opacity: .6; } }
  .bar { display: grid; grid-template-columns: repeat(4, 1fr); border-top: 1px solid var(--hairline); background: var(--surface); padding: 6px 0 10px; }
  .tab { display: grid; justify-items: center; gap: 2px; border: 0; background: transparent; cursor: pointer; font-size: 12px; font-weight: 600; color: var(--muted); min-height: 56px; }
  .pill { position: relative; display: grid; place-items: center; width: 56px; height: 30px; }
  /* Active indicator grows out from the icon's centre (Material 3), never a hard swap. */
  .pill::before { content: ''; position: absolute; inset: 0; border-radius: 15px; background: var(--accent-soft);
    transform: scaleX(.3); opacity: 0; transition: transform var(--dur-indicator) var(--ease-out), opacity var(--dur-indicator) var(--ease-out); }
  .pill > :global(svg) { position: relative; }
  .tab { transition: color var(--dur-indicator) var(--ease-out); }
  .tab[aria-current='page'] { color: var(--accent-ink); }
  .tab[aria-current='page'] .pill::before { transform: none; opacity: 1; }
  .dot { position: absolute; inset-block-start: -4px; inset-inline-end: 4px; min-width: 18px; height: 18px; border-radius: 9px; padding: 0 5px;
    background: var(--accent); color: var(--on-accent); font-size: 11px; line-height: 18px; text-align: center; }
</style>
