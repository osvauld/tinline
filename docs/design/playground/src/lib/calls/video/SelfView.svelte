<script lang="ts">
  /* My own picture: a small mirrored tile. Drag it anywhere; on release it glides to the nearest corner.
     The arrow keys move it between corners too. Mirrored for the front camera only (as a mirror shows you). */
  import { tick } from 'svelte';
  import { fade, scale } from 'svelte/transition';
  import Feed from './Feed.svelte';
  import { vid, type Corner } from './video.svelte';
  import { person } from '../../model/store.svelte';
  import { prefs } from '../../model/prefs.svelte';
  import { dur } from '../../fx/motion';
  import { motion } from '../../fx/reduced.svelte';
  import { easeOut } from '../motion';

  const me = $derived(person('me')?.hue ?? 200);
  const android = $derived(prefs.form === 'android');
  const fadeMs = $derived(dur('crossfade'));

  let el = $state<HTMLElement>();
  let drag = $state<{ id: number; x0: number; y0: number; dx: number; dy: number; moved: boolean } | null>(null);

  const glide = (before: DOMRect) => {
    if (!el || motion.reduced) return;
    const a = el.getBoundingClientRect();
    const dx = before.left - a.left, dy = before.top - a.top;
    if (!dx && !dy) return;
    el.animate([{ translate: `${dx}px ${dy}px` }, { translate: '0 0' }], { duration: dur('overlayOpen') + 40, easing: 'cubic-bezier(.2,.8,.2,1)' });
  };
  function moveTo(c: Corner) {
    if (!el) { vid.corner = c; return; }
    const before = el.getBoundingClientRect();
    el.getAnimations().forEach((a) => a.cancel());
    vid.corner = c;
    void tick().then(() => glide(before));
  }

  function down(e: PointerEvent) {
    if (e.button > 0) return;
    el!.setPointerCapture(e.pointerId);
    el!.getAnimations().forEach((a) => a.cancel());
    drag = { id: e.pointerId, x0: e.clientX, y0: e.clientY, dx: 0, dy: 0, moved: false };
    vid.hold();
  }
  function move(e: PointerEvent) {
    if (!drag || e.pointerId !== drag.id) return;
    const dx = e.clientX - drag.x0, dy = e.clientY - drag.y0;
    drag = { ...drag, dx, dy, moved: drag.moved || Math.hypot(dx, dy) > 4 };
  }
  function up(e: PointerEvent) {
    if (!drag || e.pointerId !== drag.id) return;
    const d = drag; drag = null;
    vid.poke();
    const host = el?.parentElement;
    if (!d.moved || !host || !el) return;
    // Nearest corner to where the tile's centre was dropped.
    const h = host.getBoundingClientRect(), r = el.getBoundingClientRect();
    const cx = r.left + r.width / 2 - h.left, cy = r.top + r.height / 2 - h.top;
    const next: Corner = (cy < h.height * 0.5 ? 't' : 'b') + (cx < h.width * 0.5 ? 'l' : 'r') as Corner;
    // r includes the drag offset: glide from where it was dropped to the corner.
    vid.corner = next;
    void tick().then(() => glide(r));
  }
  function key(e: KeyboardEvent) {
    const k = e.key;
    if (!k.startsWith('Arrow')) return;
    e.preventDefault();
    const c = vid.corner;
    const v = k === 'ArrowUp' ? 't' : k === 'ArrowDown' ? 'b' : c[0], hz = k === 'ArrowLeft' ? 'l' : k === 'ArrowRight' ? 'r' : c[1];
    moveTo((v + hz) as Corner);
  }
  const pop = (n: Element) => motion.reduced ? fade(n, { duration: dur('crossfade') }) : scale(n, { start: 0.88, duration: dur('enter'), easing: easeOut, opacity: 0 });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="pip {vid.corner}" class:android class:dragging={!!drag} bind:this={el} role="button" tabindex="0" data-selfview data-corner={vid.corner}
  aria-label="Your camera. Drag to move it, or use the arrow keys"
  style:translate={drag ? `${drag.dx}px ${drag.dy}px` : undefined}
  onpointerdown={down} onpointermove={move} onpointerup={up} onpointercancel={up} onkeydown={key} transition:pop>
  <div class="mir" class:flipx={vid.facing === 'front'}>
    {#key vid.facing}
      <span class="lay" in:fade={{ duration: fadeMs }} out:fade={{ duration: fadeMs }}>
        <Feed kind={vid.facing === 'front' ? 'self' : 'back'} hue={me} stream={vid.stream} seed={2} w={android ? 270 : 320} h={android ? 360 : 240} />
      </span>
    {/key}
  </div>
</div>

<style>
  .pip { position: absolute; z-index: 6; --m: 14px; --t: 98px; --b: 168px; inline-size: 176px; aspect-ratio: 4 / 3; border-radius: 16px; overflow: hidden; touch-action: none; cursor: grab; user-select: none;
    background: #15171c; box-shadow: 0 0 0 1.5px rgb(255 255 255 / .38), 0 6px 22px rgb(0 0 0 / .32); }
  .pip.android { inline-size: 96px; aspect-ratio: 3 / 4; --b: 150px; }
  .pip.dragging { cursor: grabbing; box-shadow: 0 0 0 1.5px rgb(255 255 255 / .6), 0 10px 30px rgb(0 0 0 / .4); }
  .tl { inset-block-start: var(--t); left: var(--m); }
  .tr { inset-block-start: var(--t); right: var(--m); }
  .bl { inset-block-end: var(--b); left: var(--m); }
  .br { inset-block-end: var(--b); right: var(--m); }
  .mir { position: absolute; inset: 0; display: grid; }
  .mir.flipx { transform: scaleX(-1); }
  .lay { grid-area: 1 / 1; display: block; position: relative; }
  .pip:focus-visible { outline: 2px solid #fff; outline-offset: 3px; }
</style>
