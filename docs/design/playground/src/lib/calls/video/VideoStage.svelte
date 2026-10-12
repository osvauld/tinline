<script lang="ts">
  /* Their picture, full bleed, under the call controls. Only while their camera is on: otherwise the call
     keeps the voice look (cloud field + avatar). Frozen and softened while video is paused or reconnecting. */
  import { fade } from 'svelte/transition';
  import Feed from './Feed.svelte';
  import { vid } from './video.svelte';
  import { call } from '../call.svelte';
  import { prefs } from '../../model/prefs.svelte';
  import { person } from '../../model/store.svelte';
  import { dur } from '../../fx/motion';
  import { motion } from '../../fx/reduced.svelte';

  const wide = $derived(prefs.form === 'desktop');   // a landscape picture on a wide window, so the subject is not cropped to a face
  const hue = $derived((call.peer && person(call.peer)?.hue) || 0);
  const frozen = $derived(vid.lowBw || call.phase === 'reconnecting');
  let touch = false;   // a mouse click only shows the controls (moving the mouse already woke them); a tap toggles
  const ms = $derived(motion.reduced ? dur('crossfade') : dur('overlayOpen'));
</script>

{#if vid.stageOn}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="stage" class:frozen onpointerdown={(e) => (touch = e.pointerType !== 'mouse')} onclick={() => vid.tapStage(touch)} data-stage in:fade={{ duration: ms }} out:fade={{ duration: ms }}>
    <div class="pic"><Feed kind="peer" {hue} {frozen} seed={hue} w={wide ? 426 : 240} h={wide ? 240 : 426} /></div>
    <div class="shade top"></div><div class="shade bot"></div>
  </div>
{/if}

<style>
  .stage { position: absolute; inset: 0; background: #15171c; }
  .pic { position: absolute; inset: 0; transition: filter var(--dur-switch) var(--ease-out), opacity var(--dur-switch) var(--ease-out); }
  .frozen .pic { filter: blur(10px) saturate(.8) brightness(.7); scale: 1.04; }
  /* Soft darkening at both edges so white controls and the title always read, whatever the picture. */
  .shade { position: absolute; inset-inline: 0; pointer-events: none; }
  .top { inset-block-start: 0; block-size: 150px; background: linear-gradient(rgb(0 0 0 / .5), rgb(0 0 0 / 0)); }
  .bot { inset-block-end: 0; block-size: 230px; background: linear-gradient(rgb(0 0 0 / 0), rgb(0 0 0 / .55)); }
</style>
