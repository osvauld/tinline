<script lang="ts">
  /* Harness: form-factor + theme switches and the Simulation drawer live OUTSIDE the app frame. */
  import AndroidShell from './screens/shell/AndroidShell.svelte';
  import DesktopShell from './screens/shell/DesktopShell.svelte';
  import Drawer from './lib/sim/Drawer.svelte';
  import IconButton from './lib/ui/IconButton.svelte';
  import { prefs } from './lib/model/prefs.svelte';
  import { syncMotionAttr } from './lib/fx/reduced.svelte';

  let drawer = $state(false);

  $effect(() => {
    const t = prefs.theme;
    if (t === 'system') delete document.documentElement.dataset.theme;
    else document.documentElement.dataset.theme = t;
    prefs.save();
  });
  $effect(() => { prefs.form; prefs.save(); });
  $effect(() => syncMotionAttr());

  const cycleTheme = () => (prefs.theme = prefs.theme === 'system' ? 'dark' : prefs.theme === 'dark' ? 'light' : 'system');
</script>

<div class="harness">
  <header class="hbar">
    <b>Tinline playground</b>
    <div class="seg" role="group" aria-label="Form factor">
      <button type="button" class="t-chip" aria-pressed={prefs.form === 'android'} onclick={() => (prefs.form = 'android')}>Android</button>
      <button type="button" class="t-chip" aria-pressed={prefs.form === 'desktop'} onclick={() => (prefs.form = 'desktop')}>Desktop</button>
    </div>
    <span class="end">
      <IconButton icon={prefs.theme === 'dark' ? 'moon' : 'sun'} label="Theme: {prefs.theme}" onclick={cycleTheme} />
      <IconButton icon="sliders" label="Simulation" pressed={drawer} onclick={() => (drawer = !drawer)} />
    </span>
  </header>

  <div class="stage">
    {#if prefs.form === 'android'}
      <div class="device android" data-form="android"><AndroidShell /></div>
    {:else}
      <div class="device desktop" data-form="desktop"><DesktopShell /></div>
    {/if}
  </div>
</div>

<Drawer bind:open={drawer} />

<style>
  .harness { display: flex; flex-direction: column; height: 100%; }
  .hbar { display: flex; align-items: center; gap: 16px; padding: 6px 16px; font-size: var(--fs-sub); color: var(--muted); }
  .hbar b { color: var(--ink); }
  .seg { display: flex; gap: 6px; }
  .end { margin-inline-start: auto; display: flex; }
  .stage { flex: 1; min-height: 0; display: flex; align-items: center; justify-content: center; padding: 0 16px 16px; }
  .device { overflow: hidden; border: 1px solid var(--line); background: var(--bg); box-shadow: var(--shadow-pop); }
  .android { width: 390px; height: min(844px, calc(100dvh - 72px)); border-radius: 32px; flex: none; }
  .desktop { width: min(1280px, 100%); height: min(800px, calc(100dvh - 72px)); border-radius: 12px; }
</style>
