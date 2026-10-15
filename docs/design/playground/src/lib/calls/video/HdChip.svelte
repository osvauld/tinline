<script lang="ts">
  /* The HD chip (2026-10-15): sharper video for this call, one tap away instead of deep in Settings. Shown while MY
     camera is on (HD is about what I send). Outline = off, filled = on, filled but dim = on and paused by a weak
     connection (the ladder steps down by itself). Its short line shows in the picture pill under the header (VideoChip); nothing moves. */
  import { vid } from './video.svelte';
  import { prefs } from '../../model/prefs.svelte';
  const max = $derived(prefs.form === 'android' ? '540p' : '720p');
  const label = $derived(vid.hdPaused ? `Sharper video on, paused while the connection is weak` : vid.hd ? `Sharper video on, up to ${max}` : 'Sharper video off');
</script>

{#if vid.live && (vid.camOn || vid.starting)}
  <span class="wrap">
    <button type="button" class="hd press" class:on={vid.hd} class:paused={vid.hdPaused} aria-pressed={vid.hd} aria-label={label} title={label}
      onclick={() => vid.setHd(!vid.hd)} data-hd-chip>HD</button>
  </span>
{/if}

<style>
  .wrap { position: relative; display: inline-grid; place-items: center; inline-size: 44px; block-size: 40px; }
  .hd { min-inline-size: 36px; block-size: 24px; padding: 0 7px; border-radius: 7px; border: 1.5px solid currentColor; background: transparent;
    color: var(--ink); font: 700 12px/1 var(--font-ui, inherit); letter-spacing: .04em; cursor: pointer;
    transition: background var(--dur-switch) var(--ease-out), color var(--dur-switch) var(--ease-out), opacity var(--dur-switch) var(--ease-out); }
  .hd::before { content: ''; position: absolute; inset: 0; }   /* 48 px touch target, 24 px chip */
  .hd.on { background: var(--ink); color: var(--bg); border-color: var(--ink); }
  .hd.paused { opacity: .55; }
  /* Over their picture the header is white (VideoTitle). */
  :global(.onvideo) .hd { color: #fff; text-shadow: 0 1px 6px rgb(0 0 0 / .35); }
  :global(.onvideo) .hd.on { background: #fff; color: #111; border-color: #fff; text-shadow: none; }
  .hd:focus-visible { outline: 2px solid var(--ink); outline-offset: 3px; }
</style>
