<script lang="ts">
  /* Minimized call: a strip at the top of the app showing the name, the timer and End. In flow, so the
     app below moves down with it. Tapping the strip restores the full screen. */
  import Icon from '../ui/Icon.svelte';
  import GroupCallStrip from './GroupCallStrip.svelte';
  import VIcon from './video/VIcon.svelte';
  import { vid } from './video/video.svelte';
  import { call, fmtDuration } from './call.svelte';
  import { displayName } from '../model/store.svelte';
  import { stripIn, stripOut, STRIP_H } from './motion';

  const show = $derived(call.phase !== 'idle' && call.phase !== 'incoming' && call.view === 'strip' && !call.fading);
  const name = $derived(call.peer ? displayName(call.peer) : '');
  const timed = $derived(call.phase === 'active' || call.phase === 'reconnecting');
</script>

<GroupCallStrip />
{#if show}
  <div class="strip" style:--h="{STRIP_H}px" in:stripIn out:stripOut data-callstrip>
    <button type="button" class="open press" aria-label="Return to call with {name}" onclick={() => call.restore()}>
      {#if vid.videoCall}<VIcon name="video" size={18} />{:else}<Icon name="phone" size={18} />{/if}<b class="nm" dir="auto">{name}</b>
      <span class="st">{#if call.phase === 'active'}<time class="t">{fmtDuration(call.elapsedS)}</time>{:else}{call.statusText}{/if}</span>
    </button>
    <button type="button" class="end press" aria-label="End call" disabled={call.phase === 'ended'} onclick={() => call.hangup()}>End</button>
  </div>
{/if}

<style>
  .strip { block-size: var(--h); flex: none; display: flex; align-items: center; gap: 8px; padding-inline: 4px 8px; background: var(--accent); color: var(--on-accent); position: relative; z-index: 35; }
  .open { flex: 1; min-inline-size: 0; block-size: 100%; display: flex; align-items: center; gap: 10px; padding-inline: 12px; border: 0; background: transparent; color: inherit; cursor: pointer; text-align: start; font: inherit; }
  .nm { font-weight: 650; font-size: var(--fs-body); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-inline-size: 0; }
  .st { margin-inline-start: auto; font-size: var(--fs-sub); font-variant-numeric: tabular-nums; white-space: nowrap; flex: none; min-inline-size: 5ch; text-align: end; }
  .end { flex: none; min-inline-size: 56px; block-size: 32px; padding-inline: 14px; border-radius: 16px; border: 0; background: var(--danger-btn); color: var(--on-danger-btn); font-weight: 700; font-size: var(--fs-sub); cursor: pointer; position: relative; }
  .end::after { content: ''; position: absolute; inset: -6px -2px; }
  .end:not(:disabled):active { transform: scale(.93); }
  .open:focus-visible, .end:focus-visible { outline: 2px solid var(--on-accent); outline-offset: -3px; }
</style>
