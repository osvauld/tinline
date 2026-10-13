<script lang="ts">
  /* The phone's status bar (simulated OS). Pull it down, or tap it, to open the shade. While a call runs
     and the app is not in front it carries a small call chip with the timer (accent, like Android's green chip). */
  import Icon from '../ui/Icon.svelte';
  import { call, fmtDuration } from './call.svelte';
  import { system } from './system.svelte';
  import { hhmm, clock } from '../model/clock.svelte';

  let { pull = true }: { pull?: boolean } = $props();
  let y0 = 0, down = false;

  const down_ = (e: PointerEvent) => { if (!pull || (e.target as HTMLElement).closest('.chip')) return; down = true; y0 = e.clientY; (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId); };
  const move = (e: PointerEvent) => { if (down && e.clientY - y0 > 14) { down = false; system.openShade(); } };
  const up = () => { down = false; };
</script>

<!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
<div class="bar" class:pull data-statusbar onpointerdown={down_} onpointermove={move} onpointerup={up} onpointercancel={up} onclick={() => pull && system.openShade()}>
  {#if pull}<button type="button" class="hit" aria-label="Open notifications"></button>{/if}
  <time class="clk">{hhmm(clock.now)}</time>
  {#if system.ongoing}
    <button type="button" class="chip" aria-label="Return to call" onclick={(e) => { e.stopPropagation(); system.openCall(); }}>
      <Icon name="phone" size={12} />{#if system.ongoingActive}<span class="tm">{fmtDuration(call.elapsedS)}</span>{/if}
    </button>
  {/if}
  <span class="gl" aria-hidden="true">
    <svg width="14" height="12" viewBox="0 0 14 12"><path d="M7 11.2 0.6 4.2a9.2 9.2 0 0 1 12.8 0z" fill="currentColor"/></svg>
    <svg width="12" height="12" viewBox="0 0 12 12"><path d="M11 1v10H1z" fill="currentColor"/></svg>
    <svg width="22" height="12" viewBox="0 0 22 12"><rect x=".6" y="1.6" width="18" height="8.8" rx="2.4" fill="none" stroke="currentColor" stroke-width="1.2" opacity=".7"/><rect x="2.2" y="3.2" width="12" height="5.6" rx="1.2" fill="currentColor"/><rect x="19.6" y="4.2" width="1.6" height="3.6" rx=".8" fill="currentColor" opacity=".7"/></svg>
  </span>
</div>

<style>
  .bar { position: relative; flex: none; block-size: 30px; display: flex; align-items: center; gap: 8px; padding-inline: 26px 24px; color: var(--os-ink); touch-action: none; user-select: none; }
  .hit { position: absolute; inset: 0; border: 0; background: transparent; padding: 0; cursor: pointer; }
  .hit:focus-visible { outline: 2px solid var(--os-ink); outline-offset: -3px; border-radius: 12px; }
  .clk { position: relative; font-size: 13px; font-weight: 600; font-variant-numeric: tabular-nums; letter-spacing: .01em; pointer-events: none; }
  .chip { position: relative; display: inline-flex; align-items: center; gap: 4px; block-size: 20px; padding-inline: 7px 8px; border: 0; border-radius: 10px; background: var(--accent); color: var(--on-accent); font-size: 12px; font-weight: 650; cursor: pointer; }
  .tm { font-variant-numeric: tabular-nums; letter-spacing: .01em; min-inline-size: 4ch; text-align: start; }
  .chip:focus-visible { outline: 2px solid var(--os-ink); outline-offset: 2px; }
  .gl { position: relative; margin-inline-start: auto; display: flex; align-items: center; gap: 5px; pointer-events: none; }
</style>
