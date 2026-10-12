<script lang="ts">
  /* Mounted by CallLayer: the group call screen, the quiet "started a call" banner, and the small notes
     ("You're on a call", "This call is full") that explain a refusal. */
  import { fade } from 'svelte/transition';
  import GroupCallScreen from './GroupCallScreen.svelte';
  import GroupCallAnnounce from './GroupCallAnnounce.svelte';
  import { gcall } from './gcall.svelte';
  import { toastIn, toastOut } from './motion';

  const full = $derived(gcall.phase !== 'idle' && gcall.view === 'full' && !gcall.fading);
</script>

{#if full}<GroupCallScreen />{/if}
<GroupCallAnnounce />
{#if gcall.toast}
  {#key gcall.toast.id}
    <div class="toast" role="status" in:toastIn out:toastOut data-gcall-toast>{gcall.toast.text}</div>
  {/key}
{/if}

<style>
  .toast { position: absolute; z-index: 80; inset-inline: 16px; inset-block-end: calc(28px + env(safe-area-inset-bottom)); margin-inline: auto; inline-size: fit-content; max-inline-size: min(420px, calc(100% - 32px));
    padding: 12px 18px; border-radius: 22px; background: var(--ink); color: var(--bg); font-size: var(--fs-sub); line-height: 19px; font-weight: 500; text-align: center; box-shadow: var(--shadow-pop); pointer-events: none; }
</style>
