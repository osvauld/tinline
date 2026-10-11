<script lang="ts">
  /* Desktop: sidebar (sections) | list | detail pane. Same routes as Android. */
  import { nav } from '../../lib/nav/router.svelte';
  import { db } from '../../lib/model/store.svelte';
  import { TAB_LISTS, SCREENS, GATE } from '../registry';
  import { TABS, TAB_ICON } from './TabIcon';
  import Icon from '../../lib/ui/Icon.svelte';
  import Avatar from '../../lib/ui/Avatar.svelte';
  import Mark from '../../lib/brand/Mark.svelte';
  import { statusLook } from '../../lib/brand/status.svelte';
  const look = $derived(statusLook());

  const L = $derived(TAB_LISTS[nav.tab]);
  const d = $derived(nav.detail);
  const unread = $derived(db.chats.reduce((n, c) => n + (c.muted ? 0 : c.unread), 0));
</script>

{#if db.account.gate !== 'ready'}
  <div class="gate" data-form-gate><GATE.component {...GATE.props(null)} /></div>
{:else}
<div class="win">
  <aside class="side">
    <div class="brand"><span class="st" title={look.label}><Mark size={28} live state={look.state} sway={look.sway} dim={look.dim} /><span class="vh" aria-live="polite">{look.label}</span></span><b>Tinline</b></div>
    <nav aria-label="Main">
      {#each TABS as t (t)}
        <button type="button" class="nav press" aria-current={nav.tab === t ? 'page' : undefined} onclick={() => nav.setTab(t)}>
          <Icon name={TAB_ICON[t]} size={20} /><span>{TAB_LISTS[t].title}</span>
          {#if t === 'chats' && unread}<span class="t-badge">{unread}</span>{/if}
        </button>
      {/each}
    </nav>
    <button type="button" class="me press" onclick={() => nav.open({ name: 'settings' })}>
      <Avatar name={db.account.name} hue={db.account.hue} size={36} />
      <span><b>{db.account.name.split(' ')[0]}</b><small>Your account</small></span>
    </button>
  </aside>
  <section class="listpane">
    <header><h1>{L.title}</h1></header>
    <div class="scroll"><L.component {...L.props(nav.stack[0])} /></div>
  </section>
  <section class="detail">
    {#if d}
      {@const S = SCREENS[d.name as keyof typeof SCREENS]}
      {#key d}<S.component {...S.props(d)} />{/key}
    {:else}
      <div class="empty"><Mark size={56} /><b>Choose a chat</b><span>Messages stay on your devices.</span></div>
    {/if}
  </section>
</div>
{/if}

<style>
  .st { display: inline-flex; }
  .gate { height: 100%; overflow: auto; background: var(--bg); }
  .win { display: grid; grid-template-columns: 232px 380px 1fr; height: 100%; background: var(--bg); }
  .side { display: flex; flex-direction: column; gap: 8px; padding: 16px 12px; border-inline-end: 1px solid var(--hairline); }
  .brand { display: flex; align-items: center; gap: 8px; font-size: 19px; padding: 4px 8px 16px; }
  nav { display: grid; gap: 2px; }
  .nav { display: flex; align-items: center; gap: 12px; min-height: 44px; padding: 0 12px; border: 0; border-radius: var(--r-ctl); background: transparent;
    cursor: pointer; font-weight: 600; color: var(--muted); text-align: start; }
  .nav span:not(.t-badge) { flex: 1; }
  .nav:hover { background: var(--surface-2); }
  .nav[aria-current='page'] { background: var(--accent-soft); color: var(--accent-ink); }
  .me { margin-top: auto; display: flex; gap: 10px; align-items: center; border: 0; background: transparent; padding: 8px; border-radius: var(--r-ctl); cursor: pointer; text-align: start; }
  .me span { display: grid; } .me small { color: var(--muted); font-size: var(--fs-sub); }
  .listpane { display: flex; flex-direction: column; min-width: 0; background: var(--surface); border-inline-end: 1px solid var(--hairline); }
  .listpane header { padding: 16px 20px 4px; } h1 { font-size: 24px; margin: 0; }
  .scroll { flex: 1; overflow: auto; }
  .detail { min-width: 0; position: relative; }
  .empty { height: 100%; display: grid; place-content: center; justify-items: center; gap: 6px; color: var(--muted); font-size: var(--fs-sub); }
  .empty b { color: var(--ink); font-size: var(--fs-body); }
</style>
