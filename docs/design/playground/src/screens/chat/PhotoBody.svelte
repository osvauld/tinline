<script lang="ts">
  /* A photo inside its bubble (decided 2026-10-14, `?lab=photos`: Framed). The bubble keeps a 3 px rim of its own
     colour; the photo's corners follow the bubble's, 3 px in, so they tighten in a run like text bubbles do. Its box is
     the photo's true shape from the size the core sends (chat/5), so nothing moves when the photo arrives; until then
     the core's 4x3 colour preview fills it, soft. The time sits under the photo (MessageRow's meta). */
  import Icon from '../../lib/ui/Icon.svelte';
  import { photoSrc, previewOf, photoBox } from '../../lib/ui/photos';
  import { prefs } from '../../lib/model/prefs.svelte';
  import type { FileInfo } from '../../lib/model/types';
  import type { Radii } from '../../lib/geom/shape';

  let { f, radii, who, oncancel }: { f: FileInfo; radii: Radii; who: string; oncancel: () => void } = $props();

  const android = $derived(prefs.form === 'android');
  const box = $derived(photoBox(f.w, f.h, android ? 248 : 300, android ? 330 : 340));
  const src = $derived(photoSrc(f.name, f.w ?? 400, f.h ?? 300));
  let preview = $state('');
  $effect(() => { previewOf(src).then((p) => (preview = p)); });
  const inner = $derived(radii.map((r) => `${Math.max(3, r - 3)}px`).join(' '));
  const size = (b: number) => (b >= 1e6 ? (b / 1e6).toFixed(1) + ' MB' : Math.max(1, Math.round(b / 1e3)) + ' KB');
  const C = 2 * Math.PI * 21;
</script>

<span class="ph" class:dim={f.state === 'cancelled' || f.state === 'failed'} style:width="{box.w}px" style:height="{box.h}px" style:border-radius={inner}>
  {#if preview}<img class="pv" src={preview} alt="" aria-hidden="true" />{/if}
  {#if f.remote}
    <span class="get">
      <span class="what">Photo · {size(f.size)}</span>
      <button type="button" class="dl press" onclick={() => (f.remote = false)}><Icon name="chev" size={16} />Download</button>
    </span>
  {:else}
    <img class="pic" {src} alt={f.name} draggable="false" />
    {#if f.state === 'sending'}
      <span class="veil"></span>
      <button type="button" class="ring press" aria-label="Cancel sending {f.name}" onclick={oncancel}>
        <svg viewBox="0 0 48 48" aria-hidden="true"><circle class="trk" cx="24" cy="24" r="21" /><circle class="arc" cx="24" cy="24" r="21" style:stroke-dasharray={C} style:stroke-dashoffset={C * (1 - f.progress)} /></svg>
        <Icon name="x" size={18} />
      </button>
    {:else if f.state === 'waiting'}
      <span class="chip">Sends when {who} is online</span>
    {/if}
  {/if}
</span>

<style>
  .ph { position: relative; display: block; overflow: hidden; background: color-mix(in srgb, currentColor 8%, transparent); white-space: normal; }
  .ph.dim .pic { opacity: .55; }
  .pv { position: absolute; inset: -12%; inline-size: 124%; block-size: 124%; object-fit: fill; filter: blur(14px); image-rendering: auto; }
  .pic { position: absolute; inset: 0; inline-size: 100%; block-size: 100%; object-fit: cover; display: block; user-select: none; }
  .veil { position: absolute; inset: 0; background: rgb(0 0 0 / .18); }
  .ring { position: absolute; inset: 50% auto auto 50%; translate: -50% -50%; inline-size: 52px; block-size: 52px; border: 0; padding: 0; border-radius: 50%;
    background: rgb(0 0 0 / .35); color: #fff; display: grid; place-items: center; cursor: pointer; backdrop-filter: blur(6px); }
  .ring svg { position: absolute; inset: 2px; inline-size: 48px; block-size: 48px; rotate: -90deg; }
  .ring circle { fill: none; stroke-width: 3; }
  .ring .trk { stroke: rgb(255 255 255 / .28); }
  .ring .arc { stroke: #fff; stroke-linecap: round; transition: stroke-dashoffset var(--dur-enter) var(--ease-out); }
  .chip { position: absolute; inset: auto 8px 8px 8px; inline-size: max-content; max-inline-size: calc(100% - 16px); padding: 2px 8px; border-radius: 10px;
    background: rgb(0 0 0 / .42); color: #fff; font-size: var(--fs-meta); line-height: 15px; backdrop-filter: blur(6px); }
  .get { position: absolute; inset: 0; display: grid; place-content: center; justify-items: center; gap: 8px; }
  .what { padding: 2px 8px; border-radius: 10px; background: rgb(0 0 0 / .32); color: #fff; font-size: var(--fs-sub); line-height: 18px; }
  .dl { display: inline-flex; align-items: center; gap: 6px; padding: 7px 16px; border: 0; border-radius: 18px; background: var(--accent); color: var(--on-accent);
    font: inherit; font-size: var(--fs-sub); font-weight: 650; cursor: pointer; }
</style>
