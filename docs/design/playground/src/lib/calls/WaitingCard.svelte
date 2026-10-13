<script lang="ts">
  /* Someone else is calling during my call (core: Live::waiting). A calm card on the call screen, just above the
     controls: who, and the two things the core allows. There is no hold: End & answer ends this call. */
  import Avatar from '../ui/Avatar.svelte';
  import { call } from './call.svelte';
  import { displayName, firstName, person } from '../model/store.svelte';
  import { waitIn, waitOut } from './motion';

  const w = $derived(call.waitingNow);
  const who = $derived(w ? displayName(w.peer) : '');
  const hue = $derived((w && person(w.peer)?.hue) || 0);
  const current = $derived(call.peer ? firstName(call.peer) : '');
</script>

{#if w}
  <div class="slot">
    <div class="card" role="group" aria-label="{who} is calling during your call" in:waitIn out:waitOut data-waiting>
      <div class="who">
        <Avatar name={who} {hue} person={w.peer} size={40} />
        <span class="tx"><b dir="auto" role="status" aria-live="polite">{who} is calling</b><small>during your call</small></span>
      </div>
      <div class="acts">
        <button type="button" class="b dec press" onclick={() => call.declineWaiting()}>Decline</button>
        <button type="button" class="b ans press" onclick={() => call.answerWaiting()}>End &amp; answer</button>
      </div>
      <p class="hint"><span class="pre">Ends your call with</span><span class="nm" dir="auto">{current}</span></p>
    </div>
  </div>
{/if}

<style>
  /* Zero-height anchor sitting directly above the controls; the card grows upward from it, so the name/avatar never move. */
  .slot { position: relative; block-size: 0; flex: none; z-index: 2; }
  .card { position: absolute; inset: auto 0 22px 0; margin-inline: auto; max-inline-size: 360px; display: grid; gap: 12px; padding: 14px 14px 12px; text-align: start;
    background: var(--surface-raised); color: var(--ink); border: 1px solid var(--hairline); border-radius: 22px; box-shadow: var(--shadow-pop); }
  .who { display: flex; align-items: center; gap: 12px; min-inline-size: 0; }
  .tx { display: grid; min-inline-size: 0; }
  .tx b { font-size: var(--fs-body); font-weight: 650; line-height: 20px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tx small { font-size: var(--fs-sub); line-height: 18px; opacity: .8; }
  .acts { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; }
  .b { min-block-size: 44px; padding-inline: 12px; border: 0; border-radius: 22px; font-weight: 700; font-size: var(--fs-sub); cursor: pointer; white-space: nowrap;
    transition: transform var(--dur-release) var(--ease-out), background-color var(--dur-switch) var(--ease-out); }
  .b:not(:disabled):active { transform: scale(.97); }
  .dec { background: color-mix(in srgb, var(--ink) 9%, var(--surface)); color: var(--ink); }
  .dec:hover { background: color-mix(in srgb, var(--ink) 15%, var(--surface)); }
  .ans { background: var(--accent); color: var(--on-accent); }
  .ans:hover { filter: brightness(1.08); }
  .hint { margin: 0; display: flex; justify-content: center; gap: .3em; font-size: var(--fs-meta); line-height: 16px; opacity: .8; min-inline-size: 0; }
  .hint .pre { flex: none; white-space: nowrap; }
  .hint .nm { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-inline-size: 0; }
  :global(:root[data-motion='reduce']) .b:active { transform: none; }
  .b:focus-visible { outline: 2px solid var(--ink); outline-offset: 2px; }
</style>
