<script lang="ts">
  /* Picture flow: the chosen picture is already checked (images.ts inspectImage). Here: position it, see how it will
     look at the sizes it appears at, then Use photo. A failed save keeps this open with the choice intact. */
  import { onDestroy, untrack } from 'svelte';
  import Layer from '../chat/Layer.svelte';
  import Btn from '../groups/Btn.svelte';
  import Cropper from './Cropper.svelte';
  import { centred, coverScale, renderSquare, type Picked } from '../../lib/model/images';
  import { replacePicture } from '../../lib/model/actions/profile';
  import { PICTURE_HELP } from './copy';

  let { picked: pickedProp, replacing, onclose, ondone }: { picked: Extract<Picked, { ok: true }>; replacing: boolean; onclose: () => void; ondone: () => void } = $props();

  /* The parent re-keys this flow per picture and nulls its own copy when done, so keep this one. */
  const picked = untrack(() => pickedProp);
  const VIEW = 264;
  let crop = $state(centred(picked.w, picked.h, VIEW));
  let busy = $state(false);
  let error = $state('');
  const s = $derived(coverScale(picked.w, picked.h, VIEW, crop.zoom));
  const SIZES = [72, 48, 40];
  /** The same crop, drawn as a background so each preview needs no canvas work. */
  const bg = (px: number) => { const r = px / VIEW; return `background-image:url(${picked.url});background-size:${picked.w * s * r}px ${picked.h * s * r}px;background-position:${crop.ox * r}px ${crop.oy * r}px`; };

  async function use() {
    if (busy) return;
    busy = true; error = '';
    try {
      const blob = await renderSquare(picked, crop, VIEW);
      const r = await replacePicture(blob);
      if (r.ok) { ondone(); return; }
      error = r.error;
    } catch { error = 'Couldn’t prepare that picture. Try a different one.'; }
    busy = false;
  }
  onDestroy(() => { URL.revokeObjectURL(picked.url); picked.bitmap.close(); });
</script>

<Layer variant="dialog" label="Choose your picture" {onclose}>
  <div class="pfl" data-picture-flow>
    <h2>Position your picture</h2>
    <Cropper url={picked.url} w={picked.w} h={picked.h} view={VIEW} bind:crop />
    <div class="pv" aria-hidden="true">
      {#each SIZES as px (px)}<span class="c pf-ring" style:width="{px}px" style:height="{px}px" style={bg(px)}></span>{/each}
    </div>
    {#if replacing}<p class="help">{PICTURE_HELP}</p>{/if}
    {#if error}<p class="pf-err" role="alert">{error}</p>{/if}
    <div class="pf-acts">
      <Btn variant="quiet" onclick={onclose}>Cancel</Btn>
      <Btn variant="primary" disabled={busy} onclick={use} data-autofocus>Use photo</Btn>
    </div>
  </div>
</Layer>

<style>
  .pfl { padding: 20px 20px 16px; display: grid; justify-items: center; }
  h2 { margin: 0 0 14px; font-size: var(--fs-title); font-weight: 650; line-height: 24px; justify-self: start; }
  .pv { display: flex; align-items: flex-end; justify-content: center; gap: 14px; margin-block-start: 6px; }
  .c { border-radius: 50%; background-color: var(--surface-2); background-repeat: no-repeat; }
  .help { margin: 14px 0 0; font-size: var(--fs-sub); line-height: 19px; color: var(--muted); justify-self: stretch; }
  .pfl :global(.pf-acts) { justify-self: stretch; }
  .pfl :global(.pf-err) { justify-self: stretch; }
</style>
