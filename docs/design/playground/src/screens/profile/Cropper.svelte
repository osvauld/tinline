<script lang="ts">
  /* Crop / reposition: the picture sits under a round window. Drag to move; arrow keys move (Shift = bigger steps);
     + / - or the slider zoom; Home recentres. The window is the preview. Pure app-side preparation. */
  import { centred, clampCrop, coverScale, zoomAt, type Crop } from '../../lib/model/images';

  let { url, w, h, view, crop = $bindable() }: { url: string; w: number; h: number; view: number; crop: Crop } = $props();

  const s = $derived(coverScale(w, h, view, crop.zoom));
  let drag: { x: number; y: number; ox: number; oy: number } | null = null;
  let dragging = $state(false);
  const STEP = 10, BIG = 40, ZSTEP = 0.1;

  const move = (dx: number, dy: number) => (crop = clampCrop({ ...crop, ox: crop.ox + dx, oy: crop.oy + dy }, w, h, view));
  const zoomTo = (z: number) => (crop = zoomAt(crop, Math.min(4, Math.max(1, z)), w, h, view));

  function down(e: PointerEvent) {
    drag = { x: e.clientX, y: e.clientY, ox: crop.ox, oy: crop.oy };
    dragging = true;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }
  function mv(e: PointerEvent) {
    if (!drag) return;
    crop = clampCrop({ ...crop, ox: drag.ox + (e.clientX - drag.x), oy: drag.oy + (e.clientY - drag.y) }, w, h, view);
  }
  function up() { drag = null; dragging = false; }
  function key(e: KeyboardEvent) {
    const d = e.shiftKey ? BIG : STEP;
    const k = e.key;
    if (k === 'ArrowLeft') move(d, 0);
    else if (k === 'ArrowRight') move(-d, 0);
    else if (k === 'ArrowUp') move(0, d);
    else if (k === 'ArrowDown') move(0, -d);
    else if (k === '+' || k === '=') zoomTo(crop.zoom + ZSTEP);
    else if (k === '-' || k === '_') zoomTo(crop.zoom - ZSTEP);
    else if (k === 'Home') crop = centred(w, h, view, crop.zoom);
    else return;
    e.preventDefault();
  }
</script>

<div class="wrap" style:--v="{view}px">
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div class="stage" class:dragging role="group" tabindex="0" aria-label="Position your picture" aria-describedby="crop-help"
    onpointerdown={down} onpointermove={mv} onpointerup={up} onpointercancel={up} onkeydown={key} data-crop>
    <img src={url} alt="" draggable="false" style:width="{w * s}px" style:height="{h * s}px" style:translate="{crop.ox}px {crop.oy}px" />
    <span class="win" aria-hidden="true"></span>
  </div>
  <p id="crop-help" class="help">Drag to move the picture, or use the arrow keys. Plus and minus zoom.</p>
  <label class="zoom"><span class="vh">Zoom</span>
    <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="3"/></svg>
    <input type="range" min="1" max="4" step="0.01" value={crop.zoom} aria-label="Zoom" oninput={(e) => zoomTo(+e.currentTarget.value)} />
    <svg viewBox="0 0 24 24" width="22" height="22" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="3"/></svg>
  </label>
</div>

<style>
  .wrap { display: grid; justify-items: center; gap: 12px; }
  .stage { position: relative; inline-size: var(--v); block-size: var(--v); overflow: hidden; border-radius: var(--r-ctl); background: var(--surface-2); cursor: grab; touch-action: none; user-select: none; }
  .stage.dragging { cursor: grabbing; }
  .stage:focus-visible { outline: 2px solid var(--accent-ink); outline-offset: 3px; }
  img { position: absolute; inset-block-start: 0; inset-inline-start: 0; max-inline-size: none; pointer-events: none; }
  /* the round window: everything outside it is dimmed, and a thin light edge says where the circle ends */
  .win { position: absolute; inset: 0; border-radius: 50%; pointer-events: none; box-shadow: 0 0 0 999px rgb(10 16 14 / .5), inset 0 0 0 1.5px rgb(255 255 255 / .85); }
  .help { margin: 0; font-size: var(--fs-sub); line-height: 19px; color: var(--muted); text-align: center; }
  .zoom { display: flex; align-items: center; gap: 10px; inline-size: min(100%, 260px); color: var(--muted); }
  .zoom svg { flex: none; fill: none; stroke: currentColor; stroke-width: 1.8; }
  input[type='range'] { flex: 1; min-inline-size: 0; accent-color: var(--accent); block-size: var(--btn); margin: 0; cursor: pointer; }
</style>
