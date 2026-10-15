<script lang="ts">
  /* Platform frame for an incoming call while the app is open. Android: a heads-up banner across the
     top (tap it to open the full screen). Desktop: an always-on-top card over the app, top right.
     When the app is out of sight (locked / background) the OS surfaces in SystemLayer take over. */
  import Avatar from '../ui/Avatar.svelte';
  import { call } from './call.svelte';
  import { system } from './system.svelte';
  import { displayName, person } from '../model/store.svelte';
  import { prefs } from '../model/prefs.svelte';
  import { vid } from './video/video.svelte';
  import { alertIn, alertOut, STRIP_H } from './motion';

  /* A second call over my call (call waiting) uses the same banner: the call is minimised, so the strip stays and this
     sits over it, with the two actions the core allows. Tapping the banner opens the call screen, which carries the card. */
  const waiting = $derived(call.waitingNow && call.view === 'strip' && !system.away ? call.waitingNow : null);
  const show = $derived((call.phase === 'incoming' && call.view === 'strip' && !system.away) || !!waiting);
  let pad = $state(0);   // frozen while the banner leaves, so it does not jump
  $effect(() => { if (show) pad = waiting ? STRIP_H : 0; });
  const id = $derived(waiting ? waiting.peer : call.peer);
  const name = $derived(id ? displayName(id) : '');
  const hue = $derived((id && person(id)?.hue) || 0);
</script>

{#if show}
  <div class="alert {prefs.form}" class:waiting role="alert" aria-label={waiting ? `${name} is calling during your call` : `Incoming call from ${name}`} style:--strip="{pad}px" in:alertIn out:alertOut data-incoming-alert>
    <button type="button" class="open" aria-label="Open call from {name}" onclick={() => call.restore()}>
      <Avatar {name} {hue} person={id ?? undefined} size={44} />
      <span class="tx"><b dir="auto">{name}</b><small>{waiting ? 'Calling during your call' : (vid.intent ? 'Video call' : 'Incoming call') + (call.toName ? ` to ${call.toName}` : '')}</small></span>
    </button>
    {#if waiting}
      <button type="button" class="b dec press" onclick={() => call.declineWaiting()}>Decline</button>
      <button type="button" class="b ans press" onclick={() => call.answerWaiting()}>End &amp; answer</button>
    {:else}
      <button type="button" class="b dec press" onclick={() => call.decline()}>Decline</button>
      <button type="button" class="b ans press" onclick={() => call.answer()}>Answer</button>
    {/if}
  </div>
{/if}

<style>
  .alert { position: absolute; z-index: 45; display: flex; align-items: center; gap: 8px; padding: 8px 10px 8px 8px; background: var(--surface-raised); color: var(--ink);
    border: 1px solid var(--hairline); border-radius: 20px; box-shadow: var(--shadow-pop); }
  .alert.android { inset: calc(8px + var(--strip, 0px)) 8px auto 8px; }   /* a waiting call sits under the strip, not over its End button */
  .alert.waiting { flex-wrap: wrap; padding: 8px; }
  .alert.waiting .open { flex: 1 1 100%; }
  .alert.waiting .b { flex: 1 1 0; min-block-size: 44px; }
  .alert.desktop { inset: calc(16px + var(--strip, 0px)) 16px auto auto; inline-size: 380px; }
  .open { flex: 1; min-inline-size: 0; display: flex; align-items: center; gap: 12px; padding: 0 4px; min-block-size: 48px; border: 0; background: transparent; color: inherit; cursor: pointer; text-align: start; font: inherit; border-radius: 14px; }
  .tx { display: grid; min-inline-size: 0; }
  .tx b { font-size: var(--fs-body); font-weight: 650; line-height: 20px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tx small { font-size: var(--fs-sub); line-height: 18px; }
  .b { flex: none; min-block-size: var(--btn); padding-inline: 16px; border: 0; border-radius: 22px; font-weight: 700; font-size: var(--fs-sub); cursor: pointer; }
  .dec { background: var(--danger-btn); color: var(--on-danger-btn); }
  .ans { background: var(--accent); color: var(--on-accent); }
  .open:focus-visible, .b:focus-visible { outline: 2px solid var(--ink); outline-offset: 2px; }
</style>
