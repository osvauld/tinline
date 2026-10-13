<script lang="ts">
  /* Android heads-up for an incoming call while the phone is unlocked and Tinline is in the background.
     Tapping the card opens the full in-app incoming screen; Answer connects and brings the in-call screen forward. It stays until the call is answered, declined
     or the machine's ring timeout ends it (then the missed call lands in the shade). */
  import Avatar from '../ui/Avatar.svelte';
  import Mark from '../brand/Mark.svelte';
  import { call } from './call.svelte';
  import { gcall } from './gcall.svelte';
  import { vid } from './video/video.svelte';
  import { system } from './system.svelte';
  import { displayName, person, chat } from '../model/store.svelte';
  import { headsUpIn, headsUpOut } from './motion';

  /* Call waiting: someone else calls during my call. Same card, with the two actions the core allows. */
  const waiting = $derived(system.waitingOutside && system.background && !system.locked ? call.waitingNow : null);
  const show = $derived((call.phase === 'incoming' && system.background && !system.locked) || !!waiting);
  const id = $derived(waiting ? waiting.peer : call.peer);
  const name = $derived(id ? displayName(id) : '');
  const hue = $derived((id && person(id)?.hue) || 0);
  /* A group call started (no ring): the same card, quiet, with Join. It settles into the shade after a few seconds. */
  const gs = $derived(system.gStarted && system.gStarted.bannerOn && system.background && !system.locked && !show ? system.gStarted : null);
  const gname = $derived.by(() => { const c = gs ? chat(gs.gid) : undefined; return c?.kind === 'group' ? c.name : ''; });
</script>

{#if gs}
  <div class="hu" role="status" aria-label="{displayName(gs.by)} started a call in {gname}" data-os="headsup-group" in:headsUpIn out:headsUpOut>
    <button type="button" class="open" aria-label="Open {gname}" onclick={() => system.openGroup(gs.gid)}>
      <Avatar name={gname} group size={44} />
      <span class="tx">
        <span class="hd"><span class="ico" aria-hidden="true"><Mark size={13} mono /></span>Tinline · now</span>
        <b dir="auto">{gname}</b>
        <small>{displayName(gs.by)} started a call</small>
      </span>
    </button>
    <div class="acts one">
      <button type="button" class="b dec quiet press" onclick={() => gcall.dismissAnnounce()}>Dismiss</button>
      <button type="button" class="b ans press" onclick={() => system.joinFromNotice(gs.gid)}>Join</button>
    </div>
  </div>
{/if}
{#if show}
  <div class="hu" role="alert" aria-label={waiting ? `${name} is calling during your call` : `Incoming call from ${name}`} data-os="headsup" in:headsUpIn out:headsUpOut>
    <button type="button" class="open" aria-label="Open call from {name}" onclick={() => system.openCall()}>
      <Avatar {name} {hue} person={id ?? undefined} size={44} />
      <span class="tx">
        <span class="hd"><span class="ico" aria-hidden="true"><Mark size={13} mono /></span>Tinline · now</span>
        <b dir="auto">{name}</b>
        <small>{waiting ? 'Voice call · during your call' : vid.intent ? 'Incoming video call' : 'Incoming voice call'}</small>
      </span>
    </button>
    <div class="acts">
      {#if waiting}
        <button type="button" class="b dec press" onclick={() => call.declineWaiting()}>Decline</button>
        <button type="button" class="b ans press" onclick={() => { system.background = false; call.answerWaiting(); }}>End &amp; answer</button>
      {:else}
        <button type="button" class="b dec press" onclick={() => call.decline()}>Decline</button>
        <button type="button" class="b ans press" onclick={() => { system.background = false; call.answer(); }}>Answer</button>
      {/if}
    </div>
  </div>
{/if}

<style>
  .hu { position: absolute; z-index: 50; inset: 36px 8px auto 8px; display: grid; gap: 4px; padding: 12px 12px 12px; background: var(--os-card); color: var(--os-ink);
    border: 1px solid var(--os-line); border-radius: 26px; box-shadow: var(--shadow-pop); pointer-events: auto; }
  .open { display: flex; align-items: center; gap: 12px; padding: 0; border: 0; background: transparent; color: inherit; font: inherit; text-align: start; cursor: pointer; min-inline-size: 0; border-radius: 14px; }
  .tx { display: grid; min-inline-size: 0; }
  .hd { display: flex; align-items: center; gap: 6px; font-size: var(--fs-meta); line-height: 16px; font-weight: 600; color: var(--os-sub); margin-block-end: 2px; }
  .ico { inline-size: 18px; block-size: 18px; border-radius: 50%; background: var(--accent); color: var(--on-accent); display: grid; place-items: center; }
  .tx b { font-size: var(--fs-title); line-height: 22px; font-weight: 650; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tx small { font-size: var(--fs-sub); line-height: 18px; color: var(--os-sub); }
  .acts { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; margin-block-start: 8px; }
  .b { min-block-size: 44px; border: 0; border-radius: 22px; font-weight: 700; font-size: var(--fs-sub); cursor: pointer; }
  .dec { background: var(--danger-btn); color: var(--on-danger-btn); }
  .dec.quiet { background: color-mix(in srgb, var(--os-ink) 10%, transparent); color: var(--os-ink); }
  .ans { background: var(--accent); color: var(--on-accent); }
  .b:hover { filter: brightness(1.08); }
  .open:focus-visible, .b:focus-visible { outline: 2px solid var(--os-ink); outline-offset: 2px; }
</style>
