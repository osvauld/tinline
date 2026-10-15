<script lang="ts">
  /* "Sam is calling the group · Join / Not now": a group call rings (up to 30 s, the incoming-call cue) when I am on no line.
     In the app this is the same banner turned into a ringing card; when the ring ends it settles into the quiet
     "Sam started a call" banner (it stays in the conversation and the Groups list). Out of the app the OS surfaces
     show it (heads-up, shade, desktop toast). Muted groups and blocked starters raise no ring and no banner. */
  import Avatar from '../ui/Avatar.svelte';
  import Icon from '../ui/Icon.svelte';
  import { gcall } from './gcall.svelte';
  import { call } from './call.svelte';
  import { system } from './system.svelte';
  import { displayName, firstName, person, chat as getChat, isGroup } from '../model/store.svelte';
  import { nav } from '../nav/router.svelte';
  import { prefs } from '../model/prefs.svelte';
  import { alertIn, alertOut, STRIP_H } from './motion';

  const a = $derived(gcall.announce);
  const grp = $derived.by(() => { const c = a ? getChat(a.gid) : undefined; return isGroup(c) ? c : null; });
  const show = $derived(!!a && a.bannerOn && !a.dismissed && !!gcall.liveFor(a.gid) && !system.away && !gcall.holdsLine && !grp?.muted && (nav.openChat !== a.gid || a.ringing));
  let pad = $state(0);
  $effect(() => { if (show) pad = document.querySelector('[data-callstrip]') && (prefs.form === 'desktop' || call.stripAt === 'top') ? STRIP_H : 0; });
  const by = $derived(a?.by ?? '');
</script>

{#if show && a && grp}
  <div class="alert {prefs.form}" class:ringing={a.ringing} style:--strip="{pad}px" role="status" aria-label="{displayName(by)} started a call in {grp.name}" in:alertIn out:alertOut data-gcall-announce>
    <button type="button" class="open" aria-label="Open {grp.name}" onclick={() => { gcall.dismissAnnounce(); nav.push({ name: 'conversation', chat: a.gid }); }}>
      <Avatar name={displayName(by)} hue={person(by)?.hue ?? 0} person={by} size={44} />
      <span class="tx"><b dir="auto">{grp.name}</b><small>{a.ringing ? `${firstName(by)} is calling the group` : `${displayName(by)} started a call`}</small></span>
    </button>
    {#if a.ringing}
      <button type="button" class="b quiet press" onclick={() => gcall.notNow()}>Not now</button>
    {:else}
      <button type="button" class="x press" aria-label="Dismiss" onclick={() => gcall.dismissAnnounce()}><Icon name="x" size={18} /></button>
    {/if}
    <button type="button" class="b ans press" onclick={() => gcall.join(a.gid)}>Join</button>
  </div>
{/if}

<style>
  .alert { position: absolute; z-index: 44; display: flex; align-items: center; gap: 4px; padding: 8px 10px 8px 8px; background: var(--surface-raised); color: var(--ink);
    border: 1px solid var(--hairline); border-radius: 20px; box-shadow: var(--shadow-pop); }
  .alert.android { inset: calc(8px + var(--strip, 0px)) 8px auto 8px; }
  .alert.desktop { inset: calc(16px + var(--strip, 0px)) 16px auto auto; inline-size: 380px; }
  .open { flex: 1; min-inline-size: 0; display: flex; align-items: center; gap: 12px; padding: 0 4px; min-block-size: 48px; border: 0; background: transparent; color: inherit; cursor: pointer; text-align: start; font: inherit; border-radius: 14px; }
  .tx { display: grid; min-inline-size: 0; }
  .tx b { font-size: var(--fs-body); font-weight: 650; line-height: 20px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tx small { font-size: var(--fs-sub); line-height: 18px; color: var(--muted); }
  .x { flex: none; inline-size: 36px; block-size: 36px; border-radius: 50%; border: 0; background: transparent; color: var(--muted); display: grid; place-items: center; cursor: pointer; }
  .x:hover { background: var(--surface-2); }
  .b { flex: none; min-block-size: var(--btn); padding-inline: 20px; border: 0; border-radius: 22px; font-weight: 700; font-size: var(--fs-sub); cursor: pointer; }
  .quiet { background: var(--surface-2); color: var(--ink); }
  .ringing { border-color: var(--accent); }
  .ans { background: var(--accent); color: var(--on-accent); }
  .open:focus-visible, .b:focus-visible, .x:focus-visible { outline: 2px solid var(--ink); outline-offset: 2px; }
</style>
