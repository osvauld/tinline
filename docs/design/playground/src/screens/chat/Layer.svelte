<script lang="ts">
  /* A modal layer inside the app frame (never the browser top layer, so it stays within the phone).
     sheet = bottom sheet (touch); pop = anchored popover (desktop menu); dialog = centred card on
     desktop, bottom sheet on Android. Escape/scrim close it; Tab is trapped; menu = arrow-key roving. */
  import { onMount, tick, type Snippet } from 'svelte';
  import { layerIn, layerOut } from './transitions';
  import { prefs } from '../../lib/model/prefs.svelte';

  let { variant, label, onclose, anchor = null, container = null, alignEnd = false, menu = false, children }: {
    variant: 'sheet' | 'pop' | 'dialog'; label: string; onclose: () => void;
    anchor?: HTMLElement | null; container?: HTMLElement | null; alignEnd?: boolean; menu?: boolean; children: Snippet;
  } = $props();

  const sheetLike = $derived(variant === 'sheet' || (variant === 'dialog' && prefs.form === 'android'));
  const kind = $derived(variant === 'pop' ? 'pop' : sheetLike ? 'sheet' : 'dialog');
  let el = $state<HTMLElement>();
  let armed = false; // a close needs a press that STARTED on the scrim (the release of the long-press that opened us must not count)
  let pos = $state<{ x: number; y: number } | null>(null);

  const focusables = () => [...(el?.querySelectorAll<HTMLElement>('button:not([disabled]),[href],input,textarea,[tabindex]:not([tabindex="-1"])') ?? [])];

  onMount(async () => {
    await tick();
    if (variant === 'pop' && anchor && container && el) {
      const a = anchor.getBoundingClientRect(), cr = container.getBoundingClientRect();
      const w = el.offsetWidth, h = el.offsetHeight;
      let x = (alignEnd ? a.right - w : a.left) - cr.left;
      x = Math.max(8, Math.min(cr.width - w - 8, x));
      let y = a.bottom - cr.top + 4;
      if (y + h > cr.height - 8) y = Math.max(8, a.top - cr.top - h - 4);
      pos = { x, y };
    }
    const first = el?.querySelector<HTMLElement>('[data-autofocus]') ?? focusables()[0];
    first?.focus({ preventScroll: true });
  });

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); onclose(); return; }
    const items = focusables();
    if (e.key === 'Tab' && items.length) {
      const i = items.indexOf(document.activeElement as HTMLElement);
      if (e.shiftKey && i <= 0) { e.preventDefault(); items[items.length - 1].focus(); }
      else if (!e.shiftKey && i === items.length - 1) { e.preventDefault(); items[0].focus(); }
    }
    if (menu && ['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(e.key)) {
      e.preventDefault();
      const i = items.indexOf(document.activeElement as HTMLElement);
      const n = e.key === 'Home' ? 0 : e.key === 'End' ? items.length - 1
        : e.key === 'ArrowDown' ? (i + 1) % items.length : (i - 1 + items.length) % items.length;
      items[n]?.focus();
    }
  }
</script>

<svelte:window onkeydown={(e) => { if (e.key === 'Escape' && !e.defaultPrevented) { e.preventDefault(); onclose(); } }} />

<div class="root" class:sheetlike={sheetLike} class:poproot={variant === 'pop'}>
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="scrim" class:clear={variant === 'pop'} onmousedown={(e) => e.preventDefault()} onpointerdown={() => (armed = true)} onclick={() => { if (armed) onclose(); armed = false; }} in:layerIn|global={{ kind: 'scrim' }} out:layerOut|global={{ kind: 'scrim' }}></div>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    bind:this={el} class="layer {variant}" class:sheetlike={sheetLike}
    role={menu ? 'menu' : 'dialog'} aria-modal={menu ? undefined : 'true'} aria-label={label} tabindex="-1"
    style:left={pos ? pos.x + 'px' : variant === 'pop' ? '-9999px' : undefined} style:top={pos ? pos.y + 'px' : undefined}
    {onkeydown} in:layerIn|global={{ kind }} out:layerOut|global={{ kind }}
  >
    {#if sheetLike}<div class="notch" aria-hidden="true"></div>{/if}
    {@render children()}
  </div>
</div>

<style>
  .root { position: absolute; inset: 0; z-index: 30; display: flex; align-items: center; justify-content: center; }
  .root.sheetlike { align-items: flex-end; }
  .scrim { position: absolute; inset: 0; background: rgb(10 16 14 / .5); }
  .scrim.clear { background: transparent; }
  .layer { position: relative; background: var(--surface-raised); color: var(--ink); box-shadow: var(--shadow-sheet); overflow: auto; max-height: calc(100% - 32px); }
  .layer:focus { outline: 0; }
  .layer.dialog { width: min(440px, calc(100% - 32px)); border-radius: var(--r-sheet); }
  .layer.sheetlike { width: 100%; max-width: 520px; border-radius: var(--r-sheet) var(--r-sheet) 0 0; padding-block-end: env(safe-area-inset-bottom); }
  .layer.pop { position: absolute; width: 216px; border-radius: var(--r-ctl); box-shadow: var(--shadow-pop); border: 1px solid var(--hairline); }
  .notch { inline-size: 28px; block-size: 4px; border-radius: 2px; background: var(--line); margin: 8px auto 2px; }
</style>
