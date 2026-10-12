<script lang="ts">
  /* The notification shade (simulated OS): pulled down from the status bar or opened from the drawer.
     Tinline's ongoing-call and missed-call cards live here. Esc, the scrim or the handle close it. */
  import { onMount } from 'svelte';
  import Notices from './Notices.svelte';
  import StatusBar from './StatusBar.svelte';
  import { system } from './system.svelte';
  import { clock } from '../model/clock.svelte';
  import { shadeIn, shadeOut, scrimFade } from './motion';

  const date = $derived(new Date(clock.now).toLocaleDateString(undefined, { weekday: 'short', month: 'short', day: 'numeric' }));
  const empty = $derived(!system.ongoing && !system.notifications.length);
  let panel: HTMLElement;
  let y0 = 0;
  onMount(() => panel.focus({ preventScroll: true }));
  const key = (e: KeyboardEvent) => { if (e.key === 'Escape') { e.preventDefault(); system.closeShade(); } };
</script>

<div class="shade" data-os="shade">
  <button type="button" class="scrim" tabindex="-1" aria-label="Close notifications" onclick={() => system.closeShade()} transition:scrimFade></button>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="panel" role="dialog" aria-label="Notifications" tabindex="-1" bind:this={panel} onkeydown={key} in:shadeIn out:shadeOut>
    <StatusBar pull={false} />
    <p class="date">{date}</p>
    <div class="body">
      {#if empty}<p class="none">No notifications</p>{/if}
      <Notices />
      {#if system.notifications.length}<button type="button" class="clear" onclick={() => system.clearAll()}>Clear all</button>{/if}
    </div>
    <button type="button" class="grab" aria-label="Close notifications" onclick={() => system.closeShade()}
      onpointerdown={(e) => (y0 = e.clientY)} onpointerup={(e) => { if (e.clientY - y0 < -24) system.closeShade(); }}><i></i></button>
  </div>
</div>

<style>
  .shade { position: absolute; inset: 0; z-index: 60; pointer-events: auto; }
  .scrim { position: absolute; inset: 0; border: 0; padding: 0; background: rgb(0 0 0 / .38); cursor: default; }
  .panel { position: absolute; inset: 0 0 auto 0; max-block-size: 88%; display: flex; flex-direction: column; background: var(--os-shade); color: var(--os-ink); border-radius: 0 0 28px 28px; box-shadow: var(--shadow-sheet); outline: none; overflow: hidden; }
  .date { margin: 2px 0 0; padding-inline: 26px; font-size: var(--fs-title); line-height: 24px; font-weight: 500; }
  .body { overflow: auto; padding: 14px 12px 4px; display: grid; gap: 8px; align-content: start; }
  .none { margin: 18px 0 22px; text-align: center; font-size: var(--fs-sub); color: var(--os-sub); }
  .clear { justify-self: end; min-block-size: 36px; padding-inline: 14px; border: 0; border-radius: 18px; background: var(--os-card); color: var(--os-ink); font-size: var(--fs-sub); font-weight: 600; cursor: pointer; border: 1px solid var(--os-line); }
  .grab { flex: none; block-size: 28px; border: 0; background: transparent; display: grid; place-items: center; cursor: pointer; padding: 0; }
  .grab i { inline-size: 40px; block-size: 4px; border-radius: 2px; background: var(--os-sub); opacity: .55; }
  .grab:focus-visible, .clear:focus-visible { outline: 2px solid var(--os-ink); outline-offset: -2px; }
</style>
