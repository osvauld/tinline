<script lang="ts">
  /* Mounted once by each shell, after CallLayer: what the OS itself shows around a call (lock screen,
     other-app screen, heads-up, notification shade; on desktop the minimised-window desktop, toast and
     mini window). All simulated, all inside the device frame. Neutral on purpose: the --os-* palette is
     system grey, so only Tinline's own cards (small icon, accent) read as ours. */
  import Home from './Home.svelte';
  import LockScreen from './LockScreen.svelte';
  import HeadsUp from './HeadsUp.svelte';
  import Shade from './Shade.svelte';
  import DesktopSystem from './DesktopSystem.svelte';
  import { system } from './system.svelte';
  import { prefs } from '../model/prefs.svelte';

  const android = $derived(prefs.form === 'android');
</script>

<div class="os">
  {#if android}
    {#if system.background}<Home />{/if}
    {#if system.locked}<LockScreen />{/if}
    <HeadsUp />
    {#if system.shadeOpen}<Shade />{/if}
  {:else}
    <DesktopSystem />
  {/if}
</div>

<style>
  .os { position: absolute; inset: 0; pointer-events: none;
    --os-ink: light-dark(#1b1c1f, #eef0f3); --os-sub: light-dark(#53575d, #b0b5bc);
    --os-card: light-dark(#fbfbfc, #2a2c31); --os-line: light-dark(#d8dbe0, #3b3e44); --os-tile: light-dark(#ffffffcc, #ffffff14);
    --os-shade: light-dark(#e9ebee, #1d1f23);
    --os-wall: linear-gradient(168deg, light-dark(#dfe2e7, #24272c) 0%, light-dark(#c6cad1, #15171a) 100%); }
</style>
