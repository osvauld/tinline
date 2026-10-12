<script lang="ts">
  /* The device lock screen (simulated OS, neutral). Idle: clock, date and Tinline's notifications.
     During an incoming call: the call screen itself, full-screen (Android's full-screen intent), with the
     caller's name shown (Proposed). Answering shows the in-call screen over this one; hanging up returns here. */
  import { fade } from 'svelte/transition';
  import Icon from '../ui/Icon.svelte';
  import CallScreen from './CallScreen.svelte';
  import Notices from './Notices.svelte';
  import StatusBar from './StatusBar.svelte';
  import { call } from './call.svelte';
  import { system } from './system.svelte';
  import { clock, hhmm } from '../model/clock.svelte';
  import { dur } from '../fx/motion';
  import { motion } from '../fx/reduced.svelte';

  const ms = $derived(motion.reduced ? 0 : dur('crossfade'));
  const ringing = $derived(call.phase === 'incoming');
  const date = $derived(new Date(clock.now).toLocaleDateString(undefined, { weekday: 'long', month: 'long', day: 'numeric' }));
  let y0 = 0;
</script>

<section class="lock" aria-label="Lock screen" data-os="lock" transition:fade={{ duration: ms }}>
  <StatusBar />
  <div class="face">
    <time class="big">{hhmm(clock.now)}</time>
    <p class="date">{date}</p>
  </div>
  <div class="ns"><Notices /></div>
  <button type="button" class="unlock" aria-label="Unlock" onclick={() => system.setLocked(false)}
    onpointerdown={(e) => (y0 = e.clientY)} onpointerup={(e) => { if (y0 - e.clientY > 40) system.setLocked(false); }}>
    <Icon name="lock" size={18} /><span>Swipe up to unlock</span>
  </button>

  {#if ringing}<div class="ring" data-os="lock-call"><CallScreen lock /><StatusBar pull={false} /></div>{/if}
</section>

<style>
  .lock { position: absolute; inset: 0; z-index: 36; display: flex; flex-direction: column; background: var(--os-wall); color: var(--os-ink); pointer-events: auto; overflow: hidden; }
  .face { text-align: center; padding-block: 44px 20px; }
  .big { display: block; font-size: 80px; line-height: 84px; font-weight: 250; letter-spacing: -.03em; font-variant-numeric: tabular-nums; }
  .date { margin: 4px 0 0; font-size: var(--fs-body); font-weight: 500; color: var(--os-sub); }
  .ns { padding: 8px 12px; }
  .unlock { margin: auto auto 16px; display: grid; justify-items: center; gap: 4px; border: 0; background: transparent; color: var(--os-sub); font-size: var(--fs-sub); font-weight: 500; cursor: pointer; padding: 8px 20px; border-radius: 16px; }
  .unlock:focus-visible { outline: 2px solid var(--os-ink); outline-offset: 2px; }
  .ring { position: absolute; inset: 0; z-index: 2; display: flex; flex-direction: column; }
  .ring :global(.callscreen) { z-index: 1; }
  .ring :global([data-statusbar]) { position: relative; z-index: 3; }
</style>
