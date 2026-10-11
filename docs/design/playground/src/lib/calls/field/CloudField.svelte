<script lang="ts">
  /* The call screen's background: five soft colour masses that drift, swell and warm with the call's
     own cue (notes read from the current sound palette, timing from the fx-bus anchors in ../cues.ts).
     One canvas, one rAF loop that sleeps when nothing moves; reduced motion crossfades per note
     instead of drifting. No WebGL: CSS gradients. The mass colours are hue-separated so the field
     never reads as grey/olive (see model.ts RAW). */
  import { onMount } from 'svelte';
  import { create } from './model';
  import { createRenderer, type Renderer } from './gl';
  import { cueFor } from '../cues';
  import { motion } from '../../fx/reduced.svelte';

  let { state, anchorEl, scrimEls = [], muted = false }: { state: string; anchorEl?: HTMLElement; scrimEls?: HTMLElement[]; muted?: boolean } = $props();

  let host: HTMLDivElement;
  let canvas: HTMLCanvasElement;
  let renderer: Renderer | null = null;
  let raf = 0, timer = 0, alive = false;

  const mq = typeof matchMedia === 'function' ? matchMedia('(prefers-color-scheme: dark)') : null;
  const themeName = () => {
    const t = document.documentElement.dataset.theme;
    return t === 'dark' || (t !== 'light' && !!mq?.matches) ? 'dark' : 'light';
  };
  let theme = themeName();
  const fx = create({ theme, reduced: motion.reduced });
  let started = false;
  /** Strength of the calm patches behind text in dark mode (0 = none). Light needs none. */
  const SCRIM_DARK = 0.45, GAIN_DARK = 1.55;

  /** Avatar centre in host coordinates, from layout (offset*, so the open/close transforms do not move it). */
  function measure() {
    if (!host) return;
    const w = host.clientWidth, h = host.clientHeight;
    let ax = w / 2, ay = h * 0.3;
    if (anchorEl) {
      let x = anchorEl.offsetWidth / 2, y = anchorEl.offsetHeight / 2, n: HTMLElement | null = anchorEl;
      while (n && n !== host.parentElement) { x += n.offsetLeft; y += n.offsetTop; n = n.offsetParent as HTMLElement | null; }
      if (n) { ax = x; ay = y; }
    }
    fx.setView(w, h, ax, ay);
    measureScrim();
  }

  /** Layout box of an element in host coordinates (offset*, unaffected by the open/close transforms). */
  function boxOf(el: HTMLElement) {
    let x = 0, y = 0, n: HTMLElement | null = el;
    while (n && n !== host.parentElement) { x += n.offsetLeft; y += n.offsetTop; n = n.offsetParent as HTMLElement | null; }
    return { x: x + el.offsetWidth / 2, y: y + el.offsetHeight / 2, w: el.offsetWidth, h: el.offsetHeight };
  }
  let scrimAt: [number, number, number, number][] = [];
  function measureScrim() {
    const w = host.clientWidth, h = host.clientHeight, sh = Math.min(w, h);
    scrimAt = scrimEls.filter(Boolean).map((el) => {
      const b = boxOf(el);
      return [(b.x - w / 2) / sh, (b.y - h / 2) / sh, (b.w / 2 + 44) / .55 / sh, (b.h / 2 + 30) / .55 / sh] as [number, number, number, number];
    });
  }

  function render() {
    const t = performance.now();
    const th = themeName();
    if (th !== theme) { theme = th; fx.setTheme(th); }
    const f = fx.update(t);
    renderer?.draw({ ...f, scrim: { k: th === 'dark' ? SCRIM_DARK : 0, at: scrimAt }, gain: th === 'dark' ? GAIN_DARK : 1 }, fx.P);
    if (!alive) return;
    if (f.animating) raf = requestAnimationFrame(render);
    else if (f.wakeAt != null) timer = window.setTimeout(render, Math.max(0, f.wakeAt - t));
  }
  function render0() { const th = themeName(), f = fx.update(performance.now()); renderer?.draw({ ...f, scrim: { k: th === 'dark' ? SCRIM_DARK : 0, at: scrimAt }, gain: th === 'dark' ? GAIN_DARK : 1 }, fx.P); }
  function kick() {
    if (!alive || !started) return;
    cancelAnimationFrame(raf); clearTimeout(timer);
    raf = requestAnimationFrame(render);
    // draw synchronously too, so the first frame after a state change is never a stale one
    render0();
  }

  $effect(() => {
    const s = state;
    if (!alive || fx.state === s) return;
    started = true;
    const { cueStart, pat } = cueFor(s);
    fx.setState(s, performance.now(), { cueStart, pat });
    kick();
  });
  $effect(() => { fx.setReduced(motion.reduced); kick(); });
  $effect(() => { fx.setMute(muted); });

  onMount(() => {
    renderer = createRenderer(canvas, host);
    host.dataset.render = renderer.kind;
    alive = true;
    measure();
    const ro = new ResizeObserver(() => { renderer?.resize(); measure(); kick(); });
    ro.observe(host);
    const vis = () => { if (!document.hidden) kick(); };
    document.addEventListener('visibilitychange', vis);
    started = true;
    const { cueStart, pat } = cueFor(state);
    fx.setState(state, performance.now(), { cueStart, pat });
    kick();
    return () => { alive = false; cancelAnimationFrame(raf); clearTimeout(timer); ro.disconnect(); document.removeEventListener('visibilitychange', vis); };
  });
</script>

<div class="field" bind:this={host} aria-hidden="true"><canvas bind:this={canvas}></canvas></div>

<style>
  .field { position: absolute; inset: 0; overflow: hidden; background: var(--bg); }
  canvas { position: absolute; inset: 0; inline-size: 100%; block-size: 100%; display: block; }
</style>
