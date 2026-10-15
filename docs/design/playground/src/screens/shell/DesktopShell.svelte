<script lang="ts">
  /* Desktop: sidebar (sections) | list | detail pane. Same routes as Android. */
  import { nav } from '../../lib/nav/router.svelte';
  import { db } from '../../lib/model/store.svelte';
  import { TAB_LISTS, SCREENS, GATE } from '../registry';
  import { TABS, TAB_ICON } from './TabIcon';
  import Icon from '../../lib/ui/Icon.svelte';
  import Mark from '../../lib/brand/Mark.svelte';
  import SideNames from '../names/SideNames.svelte';
  import EmptyLine from '../../lib/ui/EmptyLine.svelte';
  import TabPanel, { rememberScroll } from '../../lib/ui/TabPanel.svelte';

  // One highlight that glides between sidebar items instead of jumping.
  let navEl = $state<HTMLElement>();
  let hl = $state({ y: 0, h: 0, ready: false });
  $effect(() => {
    nav.tab;
    const el = navEl?.querySelector<HTMLElement>('[aria-current=page]');
    if (el) hl = { y: el.offsetTop, h: el.offsetHeight, ready: true };
  });
  import { statusLook } from '../../lib/brand/status.svelte';
  import CallStrip from '../../lib/calls/CallStrip.svelte';
  import CallLayer from '../../lib/calls/CallLayer.svelte';
  import SystemLayer from '../../lib/calls/SystemLayer.svelte';
  import PeopleLayers from '../people/PeopleLayers.svelte';
  import DesktopField from '../search/DesktopField.svelte';
  import SearchResults from '../search/SearchResults.svelte';
  import { ui } from '../search/state.svelte';
  let activeOpt = $state('');
  const look = $derived(statusLook());

  const L = $derived(TAB_LISTS[nav.tab]);
  const d = $derived(nav.detail);
  const unread = $derived(db.chats.reduce((n, c) => n + (c.muted ? 0 : c.unread), 0));
</script>

<svelte:window onkeydown={(e) => ui.hotkey(e)} />

{#if db.account.gate !== 'ready'}
  <div class="gate" data-form-gate><GATE.component {...GATE.props(null)} /></div>
{:else}
<div class="win">
  <div class="callstrip"><CallStrip /></div>
  <aside class="side">
    <div class="brand"><span class="st" title={look.label}><Mark size={28} live state={look.state} sway={look.sway} dim={look.dim} /><span class="vh" aria-live="polite">{look.label}</span></span><b>Tinline</b></div>
    <nav aria-label="Main" bind:this={navEl}>
      <span class="hl" class:ready={hl.ready} style:transform="translateY({hl.y}px)" style:height="{hl.h}px" aria-hidden="true"></span>
      {#each TABS as t (t)}
        <button type="button" class="nav press" aria-current={nav.tab === t ? 'page' : undefined} onclick={() => { ui.clear(); nav.setTab(t); }}>
          <Icon name={TAB_ICON[t]} size={20} /><span>{TAB_LISTS[t].title}</span>
          {#if t === 'chats' && unread}<span class="t-badge">{unread}</span>{/if}
        </button>
      {/each}
    </nav>
    <SideNames />
  </aside>
  <section class="listpane">
    {#if ui.position === 'top'}<DesktopField active={activeOpt} />{/if}
    <!-- The field stays put; while it has focus or text the pane shows results instead of the list. -->
    {#if ui.active}
      <div class="results"><SearchResults onactive={(id) => (activeOpt = id)} /></div>
    {:else}
      <TabPanel key={nav.tab}>
        <header><h1>{L.title}</h1></header>
        <div class="scroll" use:rememberScroll={nav.tab}><L.component {...L.props(nav.stack[0])} /></div>
      </TabPanel>
    {/if}
    {#if ui.position === 'bottom'}<div class="fbottom"><DesktopField active={activeOpt} /></div>{/if}
  </section>
  <section class="detail">
    {#if d}
      {@const S = SCREENS[d.name as keyof typeof SCREENS]}
      {#key d}<S.component {...S.props(d)} />{/key}
    {:else}
      {#if !db.chats.length}<EmptyLine where="pane" />
      {:else}<div class="empty"><Mark size={56} /><b>Choose a chat</b><span>Messages stay on your devices.</span></div>{/if}
    {/if}
  </section>
  <CallLayer />
  <SystemLayer />
  <PeopleLayers />
</div>
{/if}

<style>
  .st { display: inline-flex; }
  .gate { height: 100%; overflow: auto; background: var(--bg); }
  .win { position: relative; display: grid; grid-template-columns: 232px 380px 1fr; grid-template-rows: auto minmax(0, 1fr); height: 100%; background: var(--bg); }
  .callstrip { grid-column: 2 / 4; grid-row: 1; min-width: 0; }
  .side { grid-row: 1 / 3; }
  .listpane, .detail { grid-row: 2; }
  .side { display: flex; flex-direction: column; gap: 8px; padding: 16px 12px; border-inline-end: 1px solid var(--hairline); }
  .brand { display: flex; align-items: center; gap: 8px; font-size: 19px; padding: 4px 8px 16px; }
  nav { display: grid; gap: 2px; position: relative; }
  .hl { position: absolute; inset-inline: 0; top: 0; border-radius: var(--r-ctl); background: var(--accent-soft); opacity: 0; }
  .hl.ready { opacity: 1; transition: transform var(--dur-indicator) var(--ease-out), height var(--dur-indicator) var(--ease-out); }
  .nav { position: relative; transition: color var(--dur-indicator) var(--ease-out); }
  .nav { display: flex; align-items: center; gap: 12px; min-height: 44px; padding: 0 12px; border: 0; border-radius: var(--r-ctl); background: transparent;
    cursor: pointer; font-weight: 600; color: var(--muted); text-align: start; }
  .nav span:not(.t-badge) { flex: 1; }
  .nav:hover { background: var(--surface-2); }
  .nav[aria-current='page'] { color: var(--accent-ink); }
  .nav[aria-current='page']:hover { background: transparent; }
  .listpane { display: flex; flex-direction: column; min-width: 0; background: var(--surface); border-inline-end: 1px solid var(--hairline); }
  .listpane header { padding: 16px 20px 4px; } h1 { font-size: 24px; margin: 0; }
  .scroll { flex: 1; overflow: auto; }
  .results { flex: 1; min-height: 0; padding-block-start: 6px; }
  .fbottom { border-block-start: 1px solid var(--hairline); }
  .detail { min-width: 0; position: relative; }
  .empty { height: 100%; display: grid; place-content: center; justify-items: center; gap: 6px; color: var(--muted); font-size: var(--fs-sub); }
  .empty b { color: var(--ink); font-size: var(--fs-body); }
</style>
