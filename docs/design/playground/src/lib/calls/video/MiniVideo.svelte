<script lang="ts">
  /* Android, call minimised during video: a small floating tile (their picture, or mine when theirs is off) over
     the app, in addition to the strip. Tap it to return to the call. Desktop keeps only the strip. */
  import { fade, scale } from 'svelte/transition';
  import Feed from './Feed.svelte';
  import { vid } from './video.svelte';
  import { call } from '../call.svelte';
  import { person, displayName } from '../../model/store.svelte';
  import { prefs } from '../../model/prefs.svelte';
  import { dur } from '../../fx/motion';
  import { motion } from '../../fx/reduced.svelte';
  import { easeOut, STRIP_H } from '../motion';

  const live = $derived(call.phase === 'active' || call.phase === 'reconnecting');
  const theirs = $derived(vid.peerSeen);
  const show = $derived(prefs.form === 'android' && live && call.view === 'strip' && !call.fading && (theirs || vid.camOn));
  const hue = $derived((theirs ? person(call.peer ?? '')?.hue : person('me')?.hue) || 0);
  const frozen = $derived(theirs && (vid.lowBw || call.phase === 'reconnecting'));
  const name = $derived(call.peer ? displayName(call.peer) : '');
  const pop = (n: Element) => motion.reduced ? fade(n, { duration: dur('crossfade') }) : scale(n, { start: 0.9, duration: dur('enter'), easing: easeOut, opacity: 0 });
</script>

{#if show}
  <button type="button" class="mini press" style:--sb="{call.stripAt === 'bottom' ? STRIP_H : 0}px" class:frozen aria-label="Return to video call with {name}" onclick={() => call.restore()} data-minivideo transition:pop>
    <span class="mir" class:flipx={!theirs && vid.facing === 'front'}>
      {#key theirs}<span class="lay" in:fade={{ duration: dur('crossfade') }} out:fade={{ duration: dur('crossfade') }}>
        <Feed kind={theirs ? 'peer' : vid.facing === 'front' ? 'self' : 'back'} {hue} {frozen} stream={theirs ? null : vid.stream} seed={theirs ? hue : 2} w={180} h={240} />
      </span>{/key}
    </span>
  </button>
{/if}

<style>
  .mini { position: absolute; z-index: 38; inset-inline-end: 12px; inset-block-end: calc(92px + var(--sb, 0px)); inline-size: 92px; aspect-ratio: 3 / 4; padding: 0; border: 0; border-radius: 16px; overflow: hidden; cursor: pointer; background: #15171c;
    box-shadow: 0 0 0 1.5px rgb(255 255 255 / .38), var(--shadow-pop); }
  .mini.frozen { filter: saturate(.8) brightness(.8); }
  .mir { position: absolute; inset: 0; display: grid; }
  .mir.flipx { transform: scaleX(-1); }
  .lay { grid-area: 1 / 1; display: block; }
  .mini:focus-visible { outline: 2px solid var(--ink); outline-offset: 3px; }
</style>
