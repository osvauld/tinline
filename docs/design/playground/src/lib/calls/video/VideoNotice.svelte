<script lang="ts">
  /* Camera trouble, said calmly: what happened, where to fix it, and that the call goes on. A card that
     floats over the controls' top edge (no layout change), dismissable, gone by itself after a while. */
  import { fade } from 'svelte/transition';
  import VIcon from './VIcon.svelte';
  import { vid } from './video.svelte';
  import { prefs } from '../../model/prefs.svelte';
  import { waitIn, waitOut } from '../motion';

  const android = $derived(prefs.form === 'android');
  const T = $derived.by(() => {
    switch (vid.notice) {
      case 'denied': return { h: 'Camera access is off', p: android ? 'Allow the camera for Tinline in Settings > Apps > Tinline > Permissions. Your call goes on.' : 'Allow camera access for Tinline in your system’s privacy settings. Your call goes on.' };
      case 'nocamera': return { h: 'No camera found', p: android ? 'This phone has no camera Tinline can use. Your call goes on.' : 'Plug in or turn on a camera to share video. You can still talk.' };
      case 'failed': return { h: 'Couldn’t start the camera', p: 'Another app may be using it. Your call goes on.' };
      default: return null;
    }
  });
</script>

{#if T}
  <div class="slot">
    <div class="card" role="alert" data-vnotice={vid.notice} in:waitIn out:waitOut>
      <span class="ic"><VIcon name="videooff" size={20} /></span>
      <span class="tx"><b>{T.h}</b><small>{T.p}</small></span>
      <button type="button" class="ok press" onclick={() => vid.dismissNotice()}>OK</button>
    </div>
  </div>
{/if}

<style>
  .slot { position: relative; block-size: 0; flex: none; z-index: 8; }
  .card { position: absolute; inset: auto 0 22px 0; margin-inline: auto; max-inline-size: 360px; display: grid; grid-template-columns: auto 1fr auto; gap: 12px; align-items: center; padding: 12px 12px 12px 14px;
    background: var(--surface-raised); color: var(--ink); border: 1px solid var(--hairline); border-radius: 20px; box-shadow: var(--shadow-pop); text-align: start; }
  .ic { display: grid; place-items: center; inline-size: 36px; block-size: 36px; border-radius: 50%; background: color-mix(in srgb, var(--ink) 8%, var(--surface)); }
  .tx { display: grid; gap: 2px; min-inline-size: 0; }
  .tx b { font-size: var(--fs-body); font-weight: 650; line-height: 20px; }
  .tx small { font-size: var(--fs-sub); line-height: 18px; opacity: .85; }
  .ok { min-block-size: 40px; min-inline-size: 52px; border: 0; border-radius: 20px; font-weight: 700; font-size: var(--fs-sub); cursor: pointer; background: color-mix(in srgb, var(--ink) 9%, var(--surface)); color: var(--ink); }
  .ok:hover { background: color-mix(in srgb, var(--ink) 15%, var(--surface)); }
  .ok:focus-visible { outline: 2px solid var(--ink); outline-offset: 2px; }
</style>
