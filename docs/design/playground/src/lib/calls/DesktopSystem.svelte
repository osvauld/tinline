<script lang="ts">
  /* Desktop: the window is minimised ("Tinline in background"). The simulated desktop and its taskbar
     take the whole area; an incoming call is an OS toast, a running call is a small always-on-top mini
     window. Clicking either restores the window to the in-app call screen. */
  import { fade } from 'svelte/transition';
  import Avatar from '../ui/Avatar.svelte';
  import Icon from '../ui/Icon.svelte';
  import Mark from '../brand/Mark.svelte';
  import AppTile from './AppTile.svelte';
  import { call, fmtDuration } from './call.svelte';
  import { gcall } from './gcall.svelte';
  import { system } from './system.svelte';
  import { displayName, person, chat } from '../model/store.svelte';
  import { clock, hhmm } from '../model/clock.svelte';
  import { toastIn, toastOut } from './motion';
  import { dur } from '../fx/motion';
  import { vid } from './video/video.svelte';
  import { motion } from '../fx/reduced.svelte';
  import { socialText } from './socialText';

  const name = $derived(call.peer ? displayName(call.peer) : '');
  const hue = $derived((call.peer && person(call.peer)?.hue) || 0);
  /* Call waiting: the same toast for a second caller, above the mini window, with the two actions the core allows. */
  const waiting = $derived(system.waitingOutside ? call.waitingNow : null);
  const toast = $derived((system.background && call.phase === 'incoming') || !!waiting);
  const tid = $derived(waiting ? waiting.peer : call.peer);
  const tname = $derived(tid ? displayName(tid) : '');
  const thue = $derived((tid && person(tid)?.hue) || 0);
  const mini = $derived(system.background && system.ongoing);
  const ms = $derived(motion.reduced ? 0 : dur('crossfade'));
  const text = $derived(system.ongoingActive ? fmtDuration(call.elapsedS) : call.statusText);
  /* Group calls: a quiet toast when one starts (no ring), and the same small always-on-top window while I am in one. */
  const gs = $derived(system.background && system.gStarted && system.gStarted.bannerOn ? system.gStarted : null);
  const gname = $derived.by(() => { const c = gs ? chat(gs.gid) : undefined; return c?.kind === 'group' ? c.name : ''; });
  const gmini = $derived(system.background && system.gOngoing);
  /* People (requests, acceptances, invitations): the newest as a toast while the window is away, one at a time. */
  const st = $derived(system.background && system.social.length ? system.social[system.social.length - 1] : null);
  const gtext = $derived(system.gOngoingActive ? fmtDuration(gcall.elapsedS) : gcall.statusText);
</script>

{#if system.background}
  <section class="desk" aria-label="Desktop" data-os="desktop" transition:fade={{ duration: ms }}>
    <div class="icons"><AppTile icon="file" label="Files" /><AppTile icon="edit" label="Notes" /><AppTile icon="search" label="Browser" /></div>
    <footer class="task">
      <button type="button" class="app press" aria-label="Restore Tinline" onclick={() => system.foreground()}><Mark size={22} /><span>Tinline</span></button>
      <time>{hhmm(clock.now)}</time>
    </footer>
  </section>
{/if}

{#if toast}
  <div class="toast" class:above={!!waiting} role="alert" aria-label={waiting ? `${tname} is calling during your call` : `Incoming call from ${tname}`} data-os="toast" in:toastIn out:toastOut>
    <button type="button" class="open" aria-label="Open call from {tname}" onclick={() => system.openCall()}>
      <Avatar name={tname} hue={thue} person={tid ?? undefined} size={44} />
      <span class="tx">
        <span class="hd"><span class="ico" aria-hidden="true"><Mark size={13} mono /></span>Tinline</span>
        <b dir="auto">{tname}</b>
        <small>{waiting ? 'Voice call · during your call' : vid.intent ? 'Incoming video call' : 'Incoming voice call'}</small>
      </span>
    </button>
    <div class="acts">
      {#if waiting}
        <button type="button" class="b dec press" onclick={() => call.declineWaiting()}>Decline</button>
        <button type="button" class="b ans press" onclick={() => call.answerWaiting()}>End &amp; answer</button>
      {:else}
        <button type="button" class="b dec press" onclick={() => call.decline()}>Decline</button>
        <button type="button" class="b ans press" onclick={() => call.answer()}>Answer</button>
      {/if}
    </div>
  </div>
{/if}

{#if gs}
  <div class="toast" role="status" aria-label="{displayName(gs.by)} started a call in {gname}" data-os="toast-group" in:toastIn out:toastOut>
    <button type="button" class="open" aria-label="Open {gname}" onclick={() => system.openGroup(gs.gid)}>
      <Avatar name={gname} group size={44} />
      <span class="tx">
        <span class="hd"><span class="ico" aria-hidden="true"><Mark size={13} mono /></span>Tinline</span>
        <b dir="auto">{gname}</b>
        <small>{displayName(gs.by)} started a call</small>
      </span>
    </button>
    <div class="acts">
      <button type="button" class="b quiet press" onclick={() => gcall.dismissAnnounce()}>Dismiss</button>
      <button type="button" class="b ans press" onclick={() => system.joinFromNotice(gs.gid)}>Join</button>
    </div>
  </div>
{/if}

{#if st && !toast && !gs}
  {@const t = socialText(st)}
  {#key st.id}
  <div class="toast" role="status" aria-label="{t.title}: {t.text}" data-os="toast-{st.kind}" in:toastIn out:toastOut>
    <button type="button" class="open" aria-label="Open: {t.title}" onclick={() => system.openSocial(st)}>
      {#if st.kind === 'invite'}<Avatar name={st.groupName} group size={44} />{:else}<Avatar name={st.name} hue={person(st.who)?.hue ?? 0} person={st.who} size={44} />{/if}
      <span class="tx">
        <span class="hd"><span class="ico" aria-hidden="true"><Mark size={13} mono /></span>Tinline</span>
        <b dir="auto">{t.title}</b>
        <small>{t.text}</small>
      </span>
    </button>
    <div class="acts">
      <button type="button" class="b quiet press" onclick={() => system.dismissSocial(st.id)}>Dismiss</button>
      <button type="button" class="b ans press" onclick={() => system.openSocial(st)}>{t.act}</button>
    </div>
  </div>
  {/key}
{/if}

{#if gmini}
  <div class="mini" role="group" aria-label="Group call in {gcall.group?.name ?? ''}" data-os="mini-group" in:toastIn out:toastOut>
    <button type="button" class="open" aria-label="Return to group call in {gcall.group?.name ?? ''}" onclick={() => system.openCall()}>
      <Avatar name={gcall.group?.name ?? ''} group size={36} />
      <span class="tx"><b dir="auto">{gcall.group?.name ?? ''}</b><small class:tab={system.gOngoingActive}>{gtext}</small></span>
    </button>
    <button type="button" class="r press" class:on={gcall.muted} aria-pressed={gcall.muted} aria-label={gcall.muted ? 'Unmute' : 'Mute'} onclick={() => gcall.toggleMute()}><Icon name={gcall.muted ? 'micoff' : 'mic'} size={18} /></button>
    <button type="button" class="r end press" aria-label="Leave call" onclick={() => gcall.leave()}><Icon name="phone" size={18} class="hang" /></button>
  </div>
{/if}

{#if mini}
  <div class="mini" role="group" aria-label="Call with {name}" data-os="mini" in:toastIn out:toastOut>
    <button type="button" class="open" aria-label="Return to call with {name}" onclick={() => system.openCall()}>
      <Avatar {name} {hue} person={call.peer ?? undefined} size={36} />
      <span class="tx"><b dir="auto">{name}</b><small class:tab={system.ongoingActive}>{text}</small></span>
    </button>
    <button type="button" class="r press" class:on={call.muted} aria-pressed={call.muted} aria-label={call.muted ? 'Unmute' : 'Mute'} onclick={() => call.toggleMute()}><Icon name={call.muted ? 'micoff' : 'mic'} size={18} /></button>
    <button type="button" class="r end press" aria-label="Hang up" onclick={() => call.hangup()}><Icon name="phone" size={18} class="hang" /></button>
  </div>
{/if}

<style>
  .desk { position: absolute; inset: 0; z-index: 60; background: var(--os-wall); color: var(--os-ink); pointer-events: auto; display: flex; flex-direction: column; }
  .icons { display: grid; gap: 18px; justify-content: start; padding: 24px; }
  .task { margin-top: auto; block-size: 48px; display: flex; align-items: center; justify-content: space-between; padding-inline: 12px 18px; background: var(--os-card); border-top: 1px solid var(--os-line); font-size: var(--fs-sub); font-weight: 500; }
  .task time { font-variant-numeric: tabular-nums; }
  .app { position: relative; display: flex; align-items: center; gap: 8px; block-size: 36px; padding-inline: 12px; border: 0; border-radius: 10px; background: color-mix(in srgb, var(--os-ink) 8%, transparent); color: inherit; font: inherit; font-weight: 600; cursor: pointer; }
  .app::after { content: ''; position: absolute; inset-block-end: 2px; inset-inline: 40% ; block-size: 3px; border-radius: 2px; background: var(--os-sub); opacity: .6; }
  .app:hover { background: color-mix(in srgb, var(--os-ink) 14%, transparent); }
  .app:focus-visible, .open:focus-visible, .b:focus-visible, .r:focus-visible { outline: 2px solid var(--os-ink); outline-offset: 2px; }

  .toast, .mini { position: absolute; z-index: 70; inset-block-end: 60px; inset-inline-end: 16px; background: var(--os-card); color: var(--os-ink); border: 1px solid var(--os-line); border-radius: 18px; box-shadow: var(--shadow-pop); pointer-events: auto; }
  .toast { inline-size: 360px; padding: 12px; display: grid; gap: 8px; }
  .toast.above { inset-block-end: 124px; }   /* clears the mini call window (60 + its height + 8) */
  .open { display: flex; align-items: center; gap: 12px; padding: 0; border: 0; background: transparent; color: inherit; font: inherit; text-align: start; cursor: pointer; min-inline-size: 0; border-radius: 12px; }
  .tx { display: grid; min-inline-size: 0; }
  .hd { display: flex; align-items: center; gap: 6px; font-size: var(--fs-meta); line-height: 16px; font-weight: 600; color: var(--os-sub); margin-block-end: 2px; }
  .ico { inline-size: 18px; block-size: 18px; border-radius: 50%; background: var(--accent); color: var(--on-accent); display: grid; place-items: center; }
  .tx b { font-size: var(--fs-body); line-height: 20px; font-weight: 650; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tx small { font-size: var(--fs-sub); line-height: 18px; color: var(--os-sub); }
  .tx small.tab { font-variant-numeric: tabular-nums; letter-spacing: .02em; }
  .acts { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; }
  .b { min-block-size: 40px; border: 0; border-radius: 20px; font-weight: 700; font-size: var(--fs-sub); cursor: pointer; }
  .dec { background: var(--danger-btn); color: var(--on-danger-btn); }
  .ans { background: var(--accent); color: var(--on-accent); }
  .b.quiet { background: color-mix(in srgb, var(--os-ink) 10%, transparent); color: var(--os-ink); }
  .b:hover { filter: brightness(1.08); }

  .mini { inline-size: 300px; padding: 8px 8px 8px 10px; display: flex; align-items: center; gap: 6px; }
  .mini .open { flex: 1; }
  .r { flex: none; inline-size: 40px; block-size: 40px; border-radius: 50%; border: 0; display: grid; place-items: center; background: color-mix(in srgb, var(--os-ink) 10%, transparent); color: var(--os-ink); cursor: pointer; padding: 0;
    transition: transform var(--dur-release) var(--ease-out), background-color var(--dur-switch) var(--ease-out), color var(--dur-switch) var(--ease-out); }
  .r:hover { background: color-mix(in srgb, var(--os-ink) 16%, transparent); }
  .r.on { background: var(--os-ink); color: var(--os-card); }
  .r.end { background: var(--danger-btn); color: var(--on-danger-btn); }
  .r.end:hover { background: var(--danger-btn); filter: brightness(1.08); }
  .r :global(.hang) { transform: rotate(135deg); }
</style>
