<script lang="ts">
  /* Over their picture the app mark steps aside and the header carries who and how long (or the call's state).
     The timer and the status are stacked in one cell and crossfade, so nothing moves when the text changes. */
  import { call, fmtDuration } from '../call.svelte';
  import { displayName } from '../../model/store.svelte';
  import { vid } from './video.svelte';

  const name = $derived(call.peer ? displayName(call.peer) : '');
  const st = $derived(call.statusText);
  const timed = $derived((call.phase === 'active' || call.phase === 'reconnecting' || call.phase === 'ended') && call.elapsedS > 0 && !(call.phase === 'active' && st));
</script>

<div class="vt" class:on={vid.stageOn} aria-hidden={!vid.stageOn}>
  <b class="n" dir="auto">{name}</b>
  <span class="l">
    <span class="a" class:v={!!st}>{st || ' '}</span>
    <span class="a t" class:v={!st && timed}>{fmtDuration(call.elapsedS)}</span>
  </span>
</div>

<style>
  .vt { position: absolute; inset-inline: 56px; inset-block: 0; display: grid; align-content: center; justify-items: center; color: #fff; text-align: center; pointer-events: none;
    opacity: 0; transition: opacity var(--dur-overlayOpen) var(--ease-out); text-shadow: 0 1px 8px rgb(0 0 0 / .35); }
  .vt.on { opacity: 1; }
  .n { font-size: var(--fs-body); line-height: 22px; font-weight: 650; max-inline-size: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .l { display: grid; font-size: var(--fs-sub); line-height: 18px; }
  .a { grid-area: 1 / 1; opacity: 0; transition: opacity var(--dur-crossfade) var(--ease-out); }
  .a.v { opacity: 1; }
  .t { font-variant-numeric: tabular-nums; letter-spacing: .02em; }
</style>
