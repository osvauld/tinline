<script lang="ts">
  /* A generated camera picture (or the real webcam stream when the drawer enables it). It fills its box
     like a video (object-fit: cover). `frozen` stops the picture on its last frame (low bandwidth, reconnecting). */
  import { onMount } from 'svelte';
  import { drawFeed, subscribe, type FeedKind } from './feed';

  let { kind, hue = 0, frozen = false, stream = null, seed = 0, w = 270, h = 480 }: { kind: FeedKind; hue?: number; frozen?: boolean; stream?: MediaStream | null; seed?: number; w?: number; h?: number } = $props();
  let cv = $state<HTMLCanvasElement>();
  let vd = $state<HTMLVideoElement>();

  $effect(() => { if (vd) vd.srcObject = stream; });
  onMount(() => {
    const c = cv?.getContext('2d');
    if (!c) return;
    draw(performance.now());   // a first frame at once, so a freeze never shows an empty box
    return subscribe({ on: () => !frozen && !stream, draw });
    function draw(t: number) { drawFeed(c!, w, h, t, { kind, hue, seed }); }
  });
</script>

{#if stream}
  <!-- svelte-ignore a11y_media_has_caption -->
  <video bind:this={vd} autoplay muted playsinline class="f"></video>
{:else}
  <canvas bind:this={cv} width={w} height={h} class="f" aria-hidden="true"></canvas>
{/if}

<style>
  .f { display: block; inline-size: 100%; block-size: 100%; object-fit: cover; background: #15171c; }
</style>
