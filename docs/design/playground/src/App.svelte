<script lang="ts">
  /* Harness: form-factor + theme switches and the Simulation drawer live OUTSIDE the app frame. */
  import AndroidShell from './screens/shell/AndroidShell.svelte';
  import DesktopShell from './screens/shell/DesktopShell.svelte';
  import Drawer from './lib/sim/Drawer.svelte';
  import IconButton from './lib/ui/IconButton.svelte';
  import { prefs } from './lib/model/prefs.svelte';
  import { syncMotionAttr } from './lib/fx/reduced.svelte';
  import { db } from './lib/model/store.svelte';
  import { untrack } from 'svelte';

  let drawer = $state(false);
  /* `?bare=1`: just the device, no harness bar (labs that frame several phones). */
  const bare = new URLSearchParams(location.search).has('bare');

  $effect(() => {
    const t = prefs.theme;
    if (t === 'system') delete document.documentElement.dataset.theme;
    else document.documentElement.dataset.theme = t;
    prefs.save();
  });
  $effect(() => { prefs.form; prefs.save(); });
  $effect(() => syncMotionAttr());
  /* "This device" follows the form being shown: the phone on Android, the computer on desktop. */
  $effect(() => {
    const kind = prefs.form === 'desktop' ? 'desktop' : 'phone';
    untrack(() => {
      const t = db.devices.find((d) => d.kind === kind);
      if (!t || t.self) return;
      for (const d of db.devices) d.self = d === t;
    });
  });

  const cycleTheme = () => (prefs.theme = prefs.theme === 'system' ? 'dark' : prefs.theme === 'dark' ? 'light' : 'system');
</script>

<div class="harness" class:bare>
  {#if !bare}<header class="hbar">
    <b>Tinline playground</b>
    <div class="seg" role="group" aria-label="Form factor">
      <button type="button" class="t-chip" aria-pressed={prefs.form === 'android'} onclick={() => (prefs.form = 'android')}>Android</button>
      <button type="button" class="t-chip" aria-pressed={prefs.form === 'desktop'} onclick={() => (prefs.form = 'desktop')}>Desktop</button>
    </div>
    <span class="end">
      <IconButton icon={prefs.theme === 'dark' ? 'moon' : 'sun'} label="Theme: {prefs.theme}" onclick={cycleTheme} />
      <IconButton icon="sliders" label="Simulation" pressed={drawer} onclick={() => (drawer = !drawer)} />
    </span>
  </header>{/if}

  <!-- The drawer takes its own column: the app frame sits beside it, never under it (an incoming-call banner or
       Answer button must not be hidden behind the controls that triggered it). -->
  <div class="stage" class:withdrawer={drawer}>
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
  .bare .stage { padding: 0; }
  .bare .android { height: 100dvh; border-radius: 28px; }
  .hbar { display: flex; align-items: center; gap: 16px; padding: 6px 16px; font-size: var(--fs-sub); color: var(--muted); }
  .hbar b { color: var(--ink); }
  .seg { display: flex; gap: 6px; }
  .end { margin-inline-start: auto; display: flex; }
  .stage { flex: 1; min-height: 0; display: flex; align-items: center; justify-content: center; padding: 0 16px 16px; }
  .stage.withdrawer { padding-inline-end: calc(min(340px, 92vw) + 16px); }
  .device { overflow: hidden; border: 1px solid var(--line); background: var(--bg); box-shadow: var(--shadow-pop); }
  .android { width: 390px; height: min(844px, calc(100dvh - 72px)); border-radius: 32px; flex: none; }
  .desktop { width: min(1280px, 100%); height: min(800px, calc(100dvh - 72px)); border-radius: 12px; }
</style>
