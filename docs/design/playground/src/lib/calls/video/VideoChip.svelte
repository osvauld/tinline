<script lang="ts">
  /* One calm line about the picture: "Video paused, audio continues" (low bandwidth) or "<Name>'s camera is off".
     Both texts are stacked in one grid cell so the pill is always as wide as the longer one and never resizes
     or moves when the text swaps; it only fades. Sits in a fixed slot under the header. */
  import Icon from '../../ui/Icon.svelte';
  import VIcon from './VIcon.svelte';
  import { vid } from './video.svelte';
  import { call } from '../call.svelte';
  import { firstName } from '../../model/store.svelte';

  const fn = $derived(call.peer ? firstName(call.peer) : '');
  const paused = $derived(vid.live && vid.peerCam && vid.lowBw);
  const off = $derived(vid.live && vid.videoCall && !vid.peerCam);
  const showing = $derived(paused || off);
  const kind = $derived(paused ? 'paused' : 'off');
</script>

<div class="slot" class:on={showing} role="status" aria-live="polite" data-chip={showing ? kind : ''}>
  <span class="pill">
    <span class="ic">
      <span class="a" class:v={kind === 'paused'}><Icon name="clock" size={15} /></span>
      <span class="a" class:v={kind === 'off'}><VIcon name="videooff" size={15} /></span>
    </span>
    <span class="tx">
      <span class="a" class:v={kind === 'paused'}>Video paused, audio continues</span>
      <span class="a" class:v={kind === 'off'}>{fn}’s camera is off</span>
    </span>
  </span>
</div>

<style>
  .slot { position: absolute; inset-inline: 0; inset-block-start: 60px; display: flex; justify-content: center; pointer-events: none; opacity: 0; transition: opacity var(--dur-crossfade) var(--ease-out); z-index: 3; }
  .slot.on { opacity: 1; }
  .pill { display: inline-flex; align-items: center; gap: 7px; block-size: 28px; padding-inline: 11px 13px; border-radius: 14px; font-size: var(--fs-sub); line-height: 18px; font-weight: 550;
    background: color-mix(in srgb, var(--surface) 78%, transparent); color: var(--ink); backdrop-filter: blur(10px); border: 1px solid var(--hairline); max-inline-size: calc(100% - 40px); }
  .ic, .tx { display: grid; }
  .ic > .a, .tx > .a { grid-area: 1 / 1; opacity: 0; transition: opacity var(--dur-crossfade) var(--ease-out); display: block; white-space: nowrap; }
  .ic > .a { display: inline-flex; align-items: center; }
  .a.v { opacity: 1; }
  .tx > .a { overflow: hidden; text-overflow: ellipsis; }
  /* On top of their video the pill is dark glass, in either theme. */
  :global(.callscreen.onvideo) .pill { background: rgb(18 20 26 / .55); color: #fff; border-color: rgb(255 255 255 / .16); }
</style>
