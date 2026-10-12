<script lang="ts">
  /* Platform frame for an incoming call while the app is open. Android: a heads-up banner across the
     top (tap it to open the full screen). Desktop: an always-on-top card over the app, top right.
     When the app is out of sight (locked / background) the OS surfaces in SystemLayer take over. */
  import Avatar from '../ui/Avatar.svelte';
  import { call } from './call.svelte';
  import { system } from './system.svelte';
  import { displayName, person } from '../model/store.svelte';
  import { prefs } from '../model/prefs.svelte';
  import { alertIn, alertOut } from './motion';

  const show = $derived(call.phase === 'incoming' && call.view === 'strip' && !system.away);
  const name = $derived(call.peer ? displayName(call.peer) : '');
  const hue = $derived((call.peer && person(call.peer)?.hue) || 0);
</script>

{#if show}
  <div class="alert {prefs.form}" role="alert" aria-label="Incoming call from {name}" in:alertIn out:alertOut data-incoming-alert>
    <button type="button" class="open" aria-label="Open call from {name}" onclick={() => call.restore()}>
      <Avatar {name} {hue} size={44} />
      <span class="tx"><b dir="auto">{name}</b><small>Incoming call</small></span>
    </button>
    <button type="button" class="b dec press" onclick={() => call.decline()}>Decline</button>
    <button type="button" class="b ans press" onclick={() => call.answer()}>Answer</button>
  </div>
{/if}

<style>
  .alert { position: absolute; z-index: 45; display: flex; align-items: center; gap: 8px; padding: 8px 10px 8px 8px; background: var(--surface); color: var(--ink);
    border: 1px solid var(--hairline); border-radius: 20px; box-shadow: var(--shadow-pop); }
  .alert.android { inset: 8px 8px auto 8px; }
  .alert.desktop { inset: 16px 16px auto auto; inline-size: 380px; }
  .open { flex: 1; min-inline-size: 0; display: flex; align-items: center; gap: 12px; padding: 0 4px; min-block-size: 48px; border: 0; background: transparent; color: inherit; cursor: pointer; text-align: start; font: inherit; border-radius: 14px; }
  .tx { display: grid; min-inline-size: 0; }
  .tx b { font-size: var(--fs-body); font-weight: 650; line-height: 20px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tx small { font-size: var(--fs-sub); line-height: 18px; }
  .b { flex: none; min-block-size: var(--btn); padding-inline: 16px; border: 0; border-radius: 22px; font-weight: 700; font-size: var(--fs-sub); cursor: pointer; }
  .dec { background: var(--danger-btn); color: var(--on-danger-btn); }
  .ans { background: var(--accent); color: var(--on-accent); }
  .open:focus-visible, .b:focus-visible { outline: 2px solid var(--ink); outline-offset: 2px; }
</style>
