<script lang="ts">
  /* Voice lab (`?lab=voice`): the connected call's cloud field driven by a live microphone, Fog and Clouds side by side.
     Mic off: both run on the made-up talk bursts (model.ts makeVoice), as the app does today. Mic on: your voice stands
     in for theirs, through the same descriptors and response as the native audio-motion viewer
     (bench/group-audio-capacity tools/audio-motion-sim/cloud.rs): level + low/mid/high balance, 45 ms attack, 320 ms
     release, each mass swelling with its share of the spectrum. Balance is not pitch. In the apps the input would be
     the call's decoded audio. */
  import CloudField from '../../lib/calls/field/CloudField.svelte';
  import { voiceIn } from '../../lib/calls/field/voicein.svelte';

  let theme = $state<'light' | 'dark'>('dark');
  let callState = $state<'active' | 'waiting'>('active');
  $effect(() => { document.documentElement.dataset.theme = theme; });
  $effect(() => { voiceIn.sensitivity; voiceIn.save(); });

  const anchors = $state<(HTMLElement | undefined)[]>([]);
  const panels = [{ tex: 'fog' as const, label: 'Fog' }, { tex: 'clouds' as const, label: 'Clouds' }];
  const pct = (db: number) => Math.min(100, Math.max(0, (db + 90) / 90 * 100));
  const BANDS = ['Low <500 Hz', 'Mid 500–2500', 'High ≥2500'];
</script>

<main>
  <header>
    <h1>Voice lab</h1>
    <p>The call background, moving with a live voice. Speak, hum, whistle: low and high sounds move it differently.</p>
  </header>

  <section class="controls" aria-label="Mic and tuning">
    <button type="button" class="mic" aria-pressed={voiceIn.on} onclick={() => voiceIn.toggle()}>{voiceIn.on ? 'Stop my mic' : 'Use my mic'}</button>
    {#if voiceIn.error}<p class="err" role="alert">Mic: {voiceIn.error}</p>{/if}

    <div class="meter" aria-label="Mic level">
      <div class="bar"><i class="db" style:inline-size="{pct(voiceIn.db)}%"></i><b class="mark" style:inset-inline-start="{pct(-44.3)}%" title="Audible above"></b></div>
      <div class="bar drive"><i style:inline-size="{voiceIn.level * 100}%"></i></div>
      <small>{voiceIn.on ? `${voiceIn.db.toFixed(0)} dBFS · ${voiceIn.audible ? `drive ${(voiceIn.level * 100).toFixed(0)}%` : 'below the gate'}` : 'Mic off: the simulated voice is moving the field'}</small>
      <div class="bands">
        {#each BANDS as b, i (b)}<div><div class="bar"><i style:inline-size="{voiceIn.bands[i] * 100}%"></i></div><small>{b}</small></div>{/each}
      </div>
      <small>Balance {voiceIn.balance.toFixed(2)} (0 low · 1 high)</small>
    </div>

    <label><span>Sensitivity <output>{voiceIn.sensitivity.toFixed(1)}×</output></span><input type="range" min="0.5" max="3" step="0.1" bind:value={voiceIn.sensitivity} /></label>

    <div class="seg" role="group" aria-label="Call state">
      <button type="button" aria-pressed={callState === 'active'} onclick={() => (callState = 'active')}>Connected</button>
      <button type="button" aria-pressed={callState === 'waiting'} onclick={() => (callState = 'waiting')}>On hold</button>
    </div>
    <div class="seg" role="group" aria-label="Theme">
      <button type="button" aria-pressed={theme === 'light'} onclick={() => (theme = 'light')}>Light</button>
      <button type="button" aria-pressed={theme === 'dark'} onclick={() => (theme = 'dark')}>Dark</button>
    </div>
    <p class="note">Same mapping as the native viewer: drive = (60 − level) / 50 × sensitivity, audible above about −44 dBFS. Balance = high + ½ mid.</p>
  </section>

  <div class="phones">
    {#each panels as p, i (p.tex)}
      <figure>
        <div class="phone">
          {#key theme}<CloudField state={callState} texture={p.tex} anchorEl={anchors[i]} />{/key}
          <div class="who"><span class="av" bind:this={anchors[i]}>T</span><b>Tara</b><small>{callState === 'active' ? '02:14' : 'On hold'}</small></div>
        </div>
        <figcaption>{p.label}</figcaption>
      </figure>
    {/each}
  </div>
</main>

<style>
  main { min-block-size: 100dvh; background: var(--bg); color: var(--ink); padding: 24px 16px 48px; display: grid; gap: 20px; justify-items: center; }
  header { text-align: center; max-inline-size: 640px; }
  h1 { font-size: 20px; margin: 0 0 4px; }
  header p { margin: 0; color: var(--muted, var(--ink)); font-size: 14px; }
  @media (min-width: 1240px) {
    main { grid-template-columns: 360px auto; align-items: start; justify-content: center; column-gap: 32px; }
    header { grid-column: 1 / -1; }
    .controls { position: sticky; inset-block-start: 24px; }
  }
  .controls { display: grid; gap: 12px; inline-size: min(560px, 100%); padding: 16px; border-radius: 16px; background: var(--surface, transparent); border: 1px solid var(--line, rgba(127,127,127,.25)); }
  .mic { justify-self: start; padding: 10px 18px; border-radius: 999px; border: 0; font: inherit; font-weight: 600; background: var(--accent); color: var(--on-accent, #fff); cursor: pointer; }
  .mic[aria-pressed='true'] { background: transparent; color: var(--ink); box-shadow: inset 0 0 0 1.5px var(--ink); }
  .err { margin: 0; color: var(--danger, #c0392b); font-size: 13px; }
  .meter { display: grid; gap: 6px; }
  .bar { position: relative; block-size: 10px; border-radius: 5px; background: rgba(127,127,127,.18); overflow: hidden; }
  .bar i { position: absolute; inset-block: 0; inset-inline-start: 0; background: var(--accent); }
  .bar .db { opacity: .5; }
  .bar .mark { position: absolute; inset-block: 0; inline-size: 2px; background: var(--ink); }
  .drive i { background: var(--gold, #d4a23a); }
  .bands { display: grid; grid-template-columns: repeat(3, 1fr); gap: 8px; }
  .bands .bar { block-size: 6px; }
  small { font-size: 12px; opacity: .75; font-variant-numeric: tabular-nums; }
  .note { margin: 0; font-size: 12px; opacity: .7; }
  label { display: grid; gap: 4px; font-size: 13px; }
  label span { display: flex; justify-content: space-between; }
  output { font-variant-numeric: tabular-nums; opacity: .8; }
  .seg { display: flex; gap: 6px; }
  .seg button { flex: 1; padding: 8px; border-radius: 10px; font: inherit; font-size: 13px; border: 1px solid rgba(127,127,127,.3); background: transparent; color: inherit; cursor: pointer; }
  .seg button[aria-pressed='true'] { background: var(--accent); color: var(--on-accent, #fff); border-color: transparent; }
  .phones { display: flex; flex-wrap: wrap; gap: 24px; justify-content: center; max-inline-size: 100%; }
  figure { margin: 0; display: grid; gap: 8px; justify-items: center; }
  figcaption { font-size: 13px; font-weight: 600; }
  .phone { position: relative; inline-size: min(360px, calc(100vw - 32px)); aspect-ratio: 9 / 19; border-radius: 28px; overflow: hidden; box-shadow: 0 0 0 1px rgba(127,127,127,.3); }
  .who { position: absolute; inset-inline: 0; inset-block-start: 22%; display: grid; justify-items: center; gap: 6px; }
  .av { display: grid; place-items: center; inline-size: 88px; block-size: 88px; border-radius: 50%; background: var(--surface-2, rgba(127,127,127,.25)); font-size: 34px; font-weight: 600; }
  .who b { font-size: 22px; }
</style>
