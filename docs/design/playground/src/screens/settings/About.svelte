<script lang="ts">
  /* Settings > About (2026-10-15): what this is, which version, who makes it, and the open-source licences (required for
     the libraries we ship). No tracking, so nothing to opt out of; said once, plainly. */
  import Mark from '../../lib/brand/Mark.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  const LIBS = [
    { name: 'iroh', lic: 'MIT or Apache-2.0' },
    { name: 'rav1d', lic: 'BSD-2-Clause' },
    { name: 'libaom', lic: 'BSD-2-Clause' },
    { name: 'Opus', lic: 'BSD-3-Clause' },
    { name: 'redb', lic: 'MIT or Apache-2.0' },
    { name: 'BLAKE3', lic: 'CC0 or Apache-2.0' },
  ];
  let open = $state(false);
</script>

<div class="pf-scroll" data-about>
<div class="pf-col">
  <section class="pf-sec first hero">
    <Mark size={56} />
    <b class="nm">Tinline</b>
    <span class="v">Version 0.1.32</span>
    <span class="by">Made by Osvauld</span>
  </section>
  <section class="pf-sec">
    <div class="pf-card">
      <button type="button" class="pf-act press" aria-expanded={open} onclick={() => (open = !open)}>
        <Icon name="licence" size={20} /><span class="t"><b>Open-source licences</b><small>The libraries Tinline is built on</small></span><Icon name={open ? 'chevup' : 'chev'} size={18} class="chev" />
      </button>
      {#if open}
        <ul class="libs">{#each LIBS as l (l.name)}<li><b>{l.name}</b><span>{l.lic}</span></li>{/each}</ul>
      {/if}
      <a class="pf-act press" href="https://github.com/osvauld" target="_blank" rel="noreferrer"><Icon name="code" size={20} /><span class="t"><b>Source code</b><small>Tinline is open source</small></span><Icon name="chevr" size={18} class="chev" /></a>
    </div>
    <p class="pf-note">Tinline has no ads and no tracking. Your messages and calls go between your devices and the people you talk to.</p>
  </section>
</div>
</div>

<style>
  .first { margin-block-start: 24px; }
  .hero { display: grid; justify-items: center; gap: 4px; text-align: center; padding-block-end: 8px; }
  .nm { font-size: var(--fs-title); margin-block-start: 8px; }
  .v { color: var(--muted); font-size: var(--fs-sub); font-variant-numeric: tabular-nums; }
  .by { color: var(--muted); font-size: var(--fs-sub); }
  a.pf-act { text-decoration: none; }
  .libs { list-style: none; margin: 0; padding: 4px 16px 12px 50px; display: grid; gap: 8px; font-size: var(--fs-sub); }
  .libs li { display: flex; justify-content: space-between; gap: 12px; }
  .libs span { color: var(--muted); }
</style>
