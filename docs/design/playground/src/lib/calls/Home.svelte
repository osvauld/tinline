<script lang="ts">
  /* Another app's / the home screen in front of Tinline (simulated OS, neutral). */
  import { fade } from 'svelte/transition';
  import AppTile from './AppTile.svelte';
  import StatusBar from './StatusBar.svelte';
  import Mark from '../brand/Mark.svelte';
  import { system } from './system.svelte';
  import { dur } from '../fx/motion';
  import { motion } from '../fx/reduced.svelte';
  import { hhmm, clock } from '../model/clock.svelte';
  const ms = $derived(motion.reduced ? 0 : dur('crossfade'));
</script>

<section class="home" aria-label="Home screen" data-os="home" transition:fade={{ duration: ms }}>
  <StatusBar />
  <div class="widget"><b>{hhmm(clock.now)}</b></div>
  <div class="grid">
    <AppTile icon="image" label="Photos" /><AppTile icon="clock" label="Clock" /><AppTile icon="file" label="Files" /><AppTile icon="edit" label="Notes" />
    <AppTile icon="search" label="Browser" /><AppTile icon="bell" label="Reminders" /><AppTile icon="gear" label="Settings" /><AppTile icon="info" label="Tips" />
  </div>
  <div class="dock">
    <AppTile icon="phone" label="Phone" /><AppTile icon="chat" label="Messages" />
    <button type="button" class="tl" aria-label="Open Tinline" onclick={() => system.foreground()}>
      <span class="sq"><Mark size={30} /></span><span class="lb">Tinline</span>
    </button>
    <AppTile icon="image" label="Camera" />
  </div>
</section>

<style>
  .home { position: absolute; inset: 0; z-index: 36; display: flex; flex-direction: column; background: var(--os-wall); color: var(--os-ink); pointer-events: auto; }
  .widget { padding: 28px 28px 12px; font-size: 44px; line-height: 48px; font-weight: 300; letter-spacing: -.02em; font-variant-numeric: tabular-nums; }
  .widget b { font-weight: inherit; }
  .grid { display: grid; grid-template-columns: repeat(4, 1fr); justify-items: center; row-gap: 22px; padding: 28px 12px 0; }
  .dock { margin: auto 12px 18px; padding: 14px 8px 10px; border-radius: 28px; background: var(--os-card); border: 1px solid var(--os-line); display: grid; grid-template-columns: repeat(4, 1fr); justify-items: center; }
  .tl { display: grid; justify-items: center; gap: 6px; inline-size: 64px; border: 0; background: transparent; padding: 0; cursor: pointer; color: var(--os-ink); }
  .tl .sq { inline-size: 52px; block-size: 52px; border-radius: 16px; display: grid; place-items: center; background: var(--os-tile); border: 1px solid var(--os-line); }
  .tl .lb { font-size: 11px; line-height: 14px; font-weight: 500; }
  .tl:focus-visible { outline: 2px solid var(--os-ink); outline-offset: 2px; border-radius: 16px; }
</style>
