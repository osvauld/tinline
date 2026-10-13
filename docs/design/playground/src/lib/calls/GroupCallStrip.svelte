<script lang="ts">
  /* Minimized group call: the same in-flow strip as the 1:1 one (name, timer, Leave). Tapping it restores the full screen. */
  import Icon from '../ui/Icon.svelte';
  import { gcall } from './gcall.svelte';
  import { fmtDuration } from './call.svelte';
  import { stripIn, stripOut, STRIP_H } from './motion';

  const show = $derived(gcall.phase !== 'idle' && gcall.view === 'strip' && !gcall.fading);
  const name = $derived(gcall.group?.name ?? '');
</script>

{#if show}
  <div class="strip" style:--h="{STRIP_H}px" in:stripIn out:stripOut data-callstrip data-gstrip>
    <button type="button" class="open press" aria-label="Return to group call in {name}" onclick={() => gcall.restore()}>
      <Icon name="users" size={18} /><b class="nm" dir="auto">{name}</b>
      <span class="st">{#if gcall.phase === 'active'}<time class="t">{fmtDuration(gcall.elapsedS)}</time>{:else}{gcall.statusText}{/if}</span>
    </button>
    <button type="button" class="end press" aria-label="Leave call" disabled={gcall.phase === 'ended' || gcall.phase === 'failed'} onclick={() => gcall.leave()}>Leave</button>
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
