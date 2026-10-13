<script lang="ts">
  /* Settings > Sounds: Quiet, and the call sound set. The same choice as the Simulation drawer's (it used to live only there);
     either one changes what the next call sounds like. Previews play the ringtone once. */
  import { onDestroy } from 'svelte';
  import Switch from '../../lib/ui/Switch.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import { audio, PALETTES, type Palette } from '../../lib/fx/audio';

  let quiet = $state(audio.quiet);
  let palette = $state<Palette>(audio.palette);
  /* The drawer can change either from the side; mirror it. */
  const t = window.setInterval(() => { quiet = audio.quiet; palette = audio.palette; }, 300);
  onDestroy(() => clearInterval(t));

  const INFO: Record<Palette, string> = {
    classic: 'A bright four-note chime.',
    slack: 'Two close notes beat like a loose string, then one pulls tight.',
    triad: 'Three rising notes, like a knock.',
  };
  function pick(k: Palette) { palette = k; audio.setPalette(k); preview('ring'); }
  function preview(cue: string) { audio.arm(); if (!audio.quiet) audio.play(cue); }
  function setQuiet(v: boolean) { quiet = v; audio.setQuiet(v); }
</script>

<div class="pf-scroll" data-sounds-page>
<div class="pf-col">
  <section class="pf-sec first" aria-labelledby="qt-h">
    <h2 id="qt-h">Quiet</h2>
    <div class="pf-card">
      <label class="pf-act row-sw"><Icon name="mute" size={20} /><span class="t"><b>Quiet</b><small>No sounds from Tinline: ringing, call tones, joins</small></span>
        <Switch label="Quiet" checked={quiet} onchange={setQuiet} /></label>
    </div>
  </section>

  <section class="pf-sec" aria-labelledby="cs-h">
    <h2 id="cs-h">Call sounds</h2>
    <div class="pf-card" role="radiogroup" aria-label="Call sounds">
      {#each Object.entries(PALETTES) as [k, p] (k)}
        <div class="pf-act opt" data-palette={k}>
          <button type="button" role="radio" class="pick press" aria-checked={palette === k} onclick={() => pick(k as Palette)}>
            <span class="radio" aria-hidden="true"></span>
            <span class="t"><b>{p.label}</b><small>{INFO[k as Palette]}</small></span>
          </button>
        </div>
      {/each}
    </div>
    <div class="prev">
      <button type="button" class="t-chip press" data-preview="ring" onclick={() => preview('ring')}><Icon name="vol" size={16} />Ringtone</button>
      <button type="button" class="t-chip press" data-preview="connected" onclick={() => preview('connected')}><Icon name="vol" size={16} />Connected</button>
      <button type="button" class="t-chip press" data-preview="ended" onclick={() => preview('ended')}><Icon name="vol" size={16} />Call ended</button>
    </div>
    <p class="pf-note quiet" aria-live="polite">{quiet ? 'Quiet is on, so previews are silent.' : 'Tap a sound to hear it. The choice applies to the next call.'}</p>
  </section>
</div>
</div>

<style>
  .first { margin-block-start: 16px; }
  .row-sw { cursor: default; }
  .opt { padding: 0; }
  .pick { display: flex; align-items: center; gap: 14px; inline-size: 100%; min-block-size: 64px; padding: 8px 16px; border: 0; background: none; color: inherit; font: inherit; text-align: start; cursor: pointer; }
  .pick:hover { background: var(--surface-2); }
  .t { flex: 1; min-inline-size: 0; display: grid; gap: 1px; }
  .t b { font-weight: 600; line-height: 20px; } .t small { font-size: var(--fs-sub); line-height: 18px; color: var(--muted); }
  .radio { flex: none; inline-size: 22px; block-size: 22px; border-radius: 50%; box-shadow: inset 0 0 0 2px var(--line); display: grid; place-items: center; transition: box-shadow var(--dur-switch) var(--ease-out); }
  .radio::after { content: ''; inline-size: 12px; block-size: 12px; border-radius: 50%; background: var(--accent); transform: scale(0); transition: transform var(--dur-switch) var(--ease-out); }
  [aria-checked='true'] .radio { box-shadow: inset 0 0 0 2px var(--accent); }
  [aria-checked='true'] .radio::after { transform: none; }
  .prev { display: flex; flex-wrap: wrap; gap: 8px; margin: 12px 4px 0; }
  .quiet { min-block-size: 19px; }
</style>
