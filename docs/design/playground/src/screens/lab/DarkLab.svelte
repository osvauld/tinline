<script lang="ts">
  /* Dark-mode depth lab (?lab=dark). NOT a product screen and not to port. Shows the decision (2026-10-13) as
     before/after pairs: EVERYDAY parts get the rim (a 1px light top edge, now inside the dark shadow tokens);
     the ONE-TIME sign-up gets the rim plus the underglow. "Before" columns use the old black-only shadows. */
  import { onMount } from 'svelte';
  import Mark from '../../lib/brand/Mark.svelte';

  let dark = $state(true);
  let zoom = $state(2);
  let canvas = $state<'canvas' | 'bg'>('bg');
  $effect(() => { document.documentElement.dataset.theme = dark ? 'dark' : 'light'; });
  onMount(() => { document.title = 'Dark depth lab'; });
</script>

<div class="lab" data-on={canvas}>
  <header>
    <div><b>Dark-mode depth</b><small>Everyday screens: rim. First-time screens: rim + underglow. Left of each pair is today, right is the change.</small></div>
    <div class="ctl" role="group" aria-label="Zoom">
      {#each [1, 2, 3] as z (z)}<button type="button" class="t-chip press" aria-pressed={zoom === z} onclick={() => (zoom = z)}>{z}×</button>{/each}
      <button type="button" class="t-chip press" aria-pressed={dark} onclick={() => (dark = !dark)}>{dark ? 'Dark' : 'Light'}</button>
      <button type="button" class="t-chip press" onclick={() => (canvas = canvas === 'canvas' ? 'bg' : 'canvas')}>On: {canvas === 'canvas' ? 'canvas' : 'app background'}</button>
    </div>
  </header>

  <div class="zoom" style="zoom:{zoom}">
    <h3>Everyday</h3>
    <div class="pair">
      {#each ['before', 'rim'] as k (k)}
        <section class="col {k}">
          <h2>{k === 'before' ? 'Today' : 'Rim'}</h2>
          <div class="pop"><div class="mi">Reply</div><div class="mi">Copy text</div><div class="mi">Delete for me</div></div>
          <div class="dlg"><b>Remove Nikhil?</b><small>They won't be able to call you.</small>
            <div class="acts"><span class="btn txt">Keep</span><span class="btn pri sm">Remove</span></div></div>
          <div class="sheet"><span class="grab"></span><b>Add contact</b><div class="btn pri">Paste their code</div></div>
          <div class="chat"><div class="bub in">Are you home?</div><div class="bub out">On my way</div><span class="fab">↓</span></div>
        </section>
      {/each}
    </div>

    <h3>First time (sign-up)</h3>
    <div class="pair">
      {#each ['before', 'glow'] as k (k)}
        <section class="col {k}">
          <h2>{k === 'before' ? 'Today' : 'Rim + underglow'}</h2>
          <div class="card">
            <div class="hero"><Mark size={56} state="taut" /></div>
            <b class="ttl">Your line is open, Maya</b>
            <div class="btn pri">Add my first contact</div>
          </div>
        </section>
      {/each}
    </div>
  </div>
</div>

<style>
  .lab { min-block-size: 100vh; padding: 20px 24px 40px; background: var(--canvas); color: var(--ink); }
  .lab[data-on='bg'] { background: var(--bg); }
  header { display: flex; flex-wrap: wrap; gap: 12px 24px; align-items: center; justify-content: space-between; margin-block-end: 16px; }
  header b { display: block; font-size: var(--fs-title); }
  header small { color: var(--muted); font-size: var(--fs-sub); }
  .ctl { display: flex; gap: 8px; flex-wrap: wrap; }
  h3 { margin: 8px 0 10px; font-size: var(--fs-sub); color: var(--muted); text-transform: uppercase; letter-spacing: .06em; }
  .pair { display: grid; grid-template-columns: repeat(2, 240px); gap: 24px; margin-block-end: 24px; }
  .col { display: grid; gap: 16px; align-content: start; }
  h2 { margin: 0; font-size: var(--fs-sub); }

  /* "before" = the old dark shadows (black only); the others use the live tokens (rim inside) */
  .before { --rim: 0 0 0 0 transparent; --underglow: 0 0 0 0 transparent; --hero-glow: transparent; }

  .pop, .dlg, .sheet, .card { background: var(--surface-raised); border: 1px solid var(--hairline); }
  .pop { inline-size: 170px; padding: 6px; border-radius: 16px; box-shadow: var(--rim), 0 2px 6px rgb(0 0 0 / .3), 0 10px 32px rgb(0 0 0 / .4); }
  .mi { padding: 8px 12px; border-radius: 10px; font-size: var(--fs-sub); } .mi:first-child { background: var(--surface-2); }
  .dlg { padding: 16px; border-radius: var(--r-sheet); display: grid; gap: 4px; box-shadow: var(--rim), 0 2px 6px rgb(0 0 0 / .24), 0 20px 64px rgb(0 0 0 / .4); }
  .dlg small { color: var(--muted); font-size: var(--fs-sub); }
  .acts { display: flex; justify-content: flex-end; gap: 8px; margin-block-start: 8px; }
  .sheet { padding: 8px 14px 14px; border-radius: var(--r-sheet) var(--r-sheet) 0 0; border-block-end: 0; display: grid; gap: 10px; box-shadow: var(--rim), 0 2px 6px rgb(0 0 0 / .24), 0 20px 64px rgb(0 0 0 / .4); }
  .grab { justify-self: center; inline-size: 36px; block-size: 4px; border-radius: 2px; background: var(--line); }
  .btn { display: grid; place-items: center; min-block-size: 36px; padding: 0 14px; border-radius: var(--r-ctl); font-weight: 700; font-size: var(--fs-sub); }
  .pri { background: var(--accent); color: var(--on-accent); }
  .txt { color: var(--accent-ink); }
  .chat { position: relative; display: grid; gap: 6px; padding: 12px 12px 52px; border-radius: var(--r-ctl); background: var(--bg); border: 1px solid var(--hairline); }
  .bub { max-inline-size: 80%; padding: 7px 12px; border-radius: var(--r-bub); font-size: var(--fs-sub); }
  .in { background: var(--bub-in); color: var(--bub-in-ink); border: 1px solid var(--bub-in-line); justify-self: start; }
  .out { background: var(--bub-out); color: var(--bub-out-ink); justify-self: end; }
  .fab { position: absolute; inset-inline-end: 10px; inset-block-end: 8px; inline-size: 36px; block-size: 36px; border-radius: 50%; display: grid; place-items: center; background: var(--surface-raised);
    box-shadow: var(--rim), 0 1px 2px rgb(0 0 0 / .34), 0 3px 10px rgb(0 0 0 / .28); }
  .card { padding: 16px; border-radius: var(--r-sheet); display: grid; gap: 10px; justify-items: center; text-align: center;
    box-shadow: var(--rim), 0 2px 6px rgb(0 0 0 / .24), 0 20px 64px rgb(0 0 0 / .4), var(--underglow); }
  .hero { padding: 10px 24px; }
  .hero :global(.mark) { filter: drop-shadow(0 6px 22px var(--hero-glow)); }
  .ttl { font-size: 15px; }
  .card .btn { inline-size: 100%; }
</style>
