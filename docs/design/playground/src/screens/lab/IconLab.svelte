<script lang="ts">
  /* Icon lab (icons.html, or ?lab=icons if main.ts gets the hook). NOT a product screen and not to port as is.
     Option 4 study: Tinline's own line icons from the mark's maths. Compare today's set with ours on real surfaces.
     Uses only app tokens; raised surfaces use --shadow-lift (the 1px rim in dark); icons never glow. */
  import { onMount } from 'svelte';
  import Mark from '../../lib/brand/Mark.svelte';
  import { geo } from '../../lib/geom/geo.svelte';
  import { OWN, CUR, KEPT, RETIRE, body, type Set } from './tinline-icons';

  let set = $state<Set>('own');
  let dark = $state(false);
  let zoom = $state(1);
  $effect(() => { document.documentElement.dataset.theme = dark ? 'dark' : 'light'; });
  onMount(() => { document.title = 'Icon lab'; });

  const sw = $derived(set === 'cur' ? 1.8 : 1.75);
  const signature = ['call', 'callend', 'phone', 'mic', 'micoff', 'vol', 'video', 'camoff', 'link', 'chat', 'send', 'reply', 'users', 'bell'];
  const signup = ['back', 'sun', 'moon', 'enter', 'next', 'copy', 'scan', 'qr', 'userplus', 'camoff'];
  const inventory = [...signature, 'voloff', 'forward', 'retry', 'user', 'mute', 'direct', 'relayed', ...signup.filter((n) => !signature.includes(n)), ...KEPT];
  const kind = (n: string) => (OWN[n] ? (OWN[n].kind === 'ours' ? 'ours already' : OWN[n].kind === 'signup' ? 'sign-up' : 'redrawn') : 'kept');
</script>

{#snippet I(n: string, size = 24, s: Set = set)}
  <svg class="i" width={size} height={size} viewBox="0 0 24 24" stroke-width={s === 'cur' ? 1.8 : sw} aria-hidden="true">{@html body(n, s)}</svg>
{/snippet}

{#snippet grid()}
  {#each Array.from({ length: 25 }, (_, i) => i) as i (i)}
    <path d="M{i} 0V24M0 {i}H24" class="gl" stroke-width={i % 12 === 0 ? 0.2 : i % 4 === 0 ? 0.11 : 0.05} />
  {/each}
{/snippet}

<div class="lab" class:gold={set === 'gold'}>
  <header>
    <div><b>Icon lab</b><small>Option 4: our own line icons, built from the mark. Corners use smoothing {geo.smooth.toFixed(2)}; stroke {sw}.</small></div>
    <div class="ctl" role="group" aria-label="Icon set">
      <button type="button" class="t-chip press" aria-pressed={set === 'cur'} onclick={() => (set = 'cur')}>Today</button>
      <button type="button" class="t-chip press" aria-pressed={set === 'own'} onclick={() => (set = 'own')}>Ours</button>
      <button type="button" class="t-chip press" aria-pressed={set === 'gold'} onclick={() => (set = 'gold')}>Ours, gold string</button>
      <button type="button" class="t-chip press" aria-pressed={dark} onclick={() => (dark = !dark)}>{dark ? 'Dark' : 'Light'}</button>
      {#each [1, 2] as z (z)}<button type="button" class="t-chip press" aria-pressed={zoom === z} onclick={() => (zoom = z)}>{z}×</button>{/each}
    </div>
  </header>

  <div class="zoom" style="zoom:{zoom}">
    <h3>Sign-up</h3>
    <div class="row2">
      <section class="surf pad su">
        <div class="bar">
          <span class="ib" aria-label="Back">{@render I('back')}</span>
          <span class="grow"></span>
          <span class="seg" role="group" aria-label="Theme"><span class="ib sm on">{@render I('sun', 20)}</span><span class="ib sm">{@render I('moon', 20)}</span></span>
        </div>
        <div class="mark"><Mark size={64} /></div>
        <p class="title">Here are your recovery words</p>
        <div class="btns">
          <span class="btn ghost">{@render I('copy', 20)}Copy my words</span>
          <span class="btn ghost">{@render I('scan', 20)}Scan their code</span>
          <span class="btn ghost">{@render I('qr', 20)}Show my code</span>
          <span class="btn ghost">{@render I('camoff', 20)}Not now, no camera</span>
        </div>
        <div class="foot">
          <span class="btn pri">{@render I('userplus', 20)}Add my first contact<kbd>{#if set === 'cur'}↵{:else}{@render I('enter', 13)}{/if}</kbd></span>
          <span class="round" aria-label="Continue">{@render I('next', 24)}</span>
        </div>
      </section>
      <section class="glyphs">
        {#each signup as n (n)}
          <div class="g"><span>{@render I(n, 24)}</span><span>{@render I(n, 20)}</span><span>{@render I(n, 16)}</span><code>{n}</code></div>
        {/each}
      </section>
    </div>

    <h3>Signature glyphs</h3>
    <div class="cards">
      {#each signature as n (n)}
        <div class="card">
          <svg class="big" viewBox="-1 -1 26 26" aria-hidden="true">{@render grid()}<g class="draw" stroke-width="1.75">{@html body(n, set === 'cur' ? 'cur' : 'own')}</g></svg>
          <div class="sizes">{@render I(n, 24)}{@render I(n, 20)}{@render I(n, 16)}<span class="was">today {@render I(n, 20, 'cur')}</span></div>
          <code>{n}</code>
          <small>{OWN[n]?.why}</small>
        </div>
      {/each}
    </div>

    <h3>In the app</h3>
    <div class="row2">
      <div class="col">
        <section class="surf">
          <div class="bar">
            <span class="ib">{@render I('back')}</span><span class="av">R</span>
            <span class="who"><b>Riya</b><span>{@render I('direct', 14)} online</span></span>
            <span class="ib" aria-label="Call Riya">{@render I('call')}</span><span class="ib" aria-label="Video call Riya">{@render I('video')}</span><span class="ib m">{@render I('info')}</span>
          </div>
        </section>
        <section class="surf"><div class="comp"><span class="ib m">{@render I('clip')}</span><span class="field">Message Riya</span><span class="fab" aria-label="Send">{@render I('send', 22)}</span></div></section>
        <section class="surf raised"><div class="acts">{#each ['reply', 'forward', 'copy', 'edit', 'trash'] as n (n)}<span class="ib m">{@render I(n, 22)}</span>{/each}</div></section>
        <section class="surf"><div class="chips">
          <span class="chip">{@render I('direct', 16)}Direct</span><span class="chip">{@render I('relayed', 16)}Through a relay</span><span class="chip">{@render I('micoff', 16)}Riya muted her mic</span>
        </div></section>
      </div>
      <div class="col">
        <section class="surf"><div class="call">
          <span class="cb on"><i>{@render I('mic')}</i>Mute my mic</span>
          <span class="cb"><i>{@render I('vol')}</i>Use my speaker</span>
          <span class="cb"><i>{@render I('camoff')}</i>Turn on my camera</span>
          <span class="cb end"><i>{@render I('callend')}</i>Hang up</span>
        </div></section>
        <section class="surf list">
          <div class="li"><span class="ic m">{@render I('phone', 20)}</span><span class="t">Riya<small>Today, 09:12 · 14 min</small></span><span class="ib sm" aria-label="Call Riya">{@render I('call', 20)}</span></div>
          <div class="li"><span class="ic m">{@render I('phone', 20)}</span><span class="t">Arjun<small>Yesterday · you missed this one</small></span><span class="ib sm" aria-label="Call Arjun">{@render I('call', 20)}</span></div>
        </section>
        <section class="surf list">
          <div class="li"><span class="ic">{@render I('link', 22)}</span><span class="t">My linked devices<small>Pixel 8, ThinkPad</small></span><span class="ic m">{@render I('chevr', 20)}</span></div>
          <div class="li"><span class="ic">{@render I('users', 22)}</span><span class="t">People I talk to</span><span class="ic m">{@render I('chevr', 20)}</span></div>
          <div class="li"><span class="ic">{@render I('bell', 22)}</span><span class="t">Notifications<small>A soft knock for calls</small></span><span class="ic m">{@render I('chevr', 20)}</span></div>
          <div class="li"><span class="ic">{@render I('key', 22)}</span><span class="t">My recovery words</span><span class="ic m">{@render I('chevr', 20)}</span></div>
        </section>
      </div>
    </div>

    <h3>Every icon</h3>
    <div class="surf scroll"><table>
      <thead><tr><th>Name</th><th>Today</th><th>24</th><th>20</th><th>16</th><th></th><th>Note</th></tr></thead>
      <tbody>
        {#each inventory as n (n)}
          <tr><td><code>{n}</code></td><td>{@render I(n, 24, 'cur')}</td><td>{@render I(n, 24)}</td><td>{@render I(n, 20)}</td><td>{@render I(n, 16)}</td>
            <td><span class="tag" class:new={kind(n) !== 'kept'}>{kind(n)}</span></td><td class="note">{OWN[n]?.why ?? 'Unchanged, one stroke weight.'}</td></tr>
        {/each}
        {#each Object.entries(RETIRE) as [n, why] (n)}
          <tr class="ret"><td><code>{n}</code></td><td>{@html `<svg class="i" width="24" height="24" viewBox="0 0 24 24" stroke-width="1.8" aria-hidden="true">${CUR[n]}</svg>`}</td><td colspan="3"></td><td><span class="tag">retire</span></td><td class="note">{why}</td></tr>
        {/each}
      </tbody>
    </table></div>
    <p class="fine">Open question: "Hang up" is shown without red to keep the mood mellow. Calls may still want the conventional red. The lab doesn't decide that.</p>
  </div>
</div>

<style>
  .lab { min-height: 100%; background: var(--bg); color: var(--ink); padding: 16px; }
  header { display: flex; flex-wrap: wrap; gap: 12px; align-items: center; justify-content: space-between; margin-bottom: 8px; }
  header b { display: block; font-size: 17px; }
  header small { color: var(--muted); }
  .ctl { display: flex; gap: 6px; flex-wrap: wrap; }
  h3 { margin: 28px 0 10px; font-size: 13px; font-weight: 600; letter-spacing: .06em; text-transform: uppercase; color: var(--muted); }
  code { font: 12px ui-monospace, monospace; }
  .row2 { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 380px), 1fr)); gap: 16px; align-items: start; }
  .col { display: grid; gap: 12px; }
  .surf { background: var(--surface); border: 1px solid var(--hairline); border-radius: 16px; overflow: hidden; }
  .surf.raised { background: var(--surface-raised); box-shadow: var(--shadow-lift); border-color: transparent; }
  .pad { padding: 12px; }
  :global(svg.i) { fill: none; stroke: currentColor; stroke-linecap: round; stroke-linejoin: round; flex: none; display: block; }
  :global(svg.i .f) { fill: currentColor; stroke: none; }
  .gold :global(svg.i .str) { stroke: var(--gold); }
  .big { display: block; inline-size: 100%; block-size: auto; aspect-ratio: 1; }
  .big .gl { stroke: var(--accent-soft); fill: none; }
  .big .draw { fill: none; stroke: var(--ink); stroke-linecap: round; stroke-linejoin: round; }
  .big .draw :global(.f) { fill: var(--ink); stroke: none; }
  .gold .big .draw :global(.str) { stroke: var(--gold); }

  .cards { display: grid; grid-template-columns: repeat(auto-fill, minmax(180px, 1fr)); gap: 12px; }
  .card { background: var(--surface); border: 1px solid var(--hairline); border-radius: 16px; padding: 12px; display: grid; gap: 8px; align-content: start; }
  .card small { color: var(--muted); line-height: 1.4; }
  .sizes { display: flex; align-items: center; gap: 12px; }
  .was { margin-inline-start: auto; display: flex; align-items: center; gap: 6px; color: var(--muted); font-size: 11px; }
  .glyphs { display: grid; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 8px; }
  .g { display: flex; align-items: center; gap: 10px; padding: 10px 12px; background: var(--surface); border: 1px solid var(--hairline); border-radius: 12px; }
  .g code { margin-inline-start: auto; color: var(--muted); }

  .bar { display: flex; align-items: center; gap: 4px; padding: 8px 8px 8px 4px; min-block-size: 56px; }
  .grow { flex: 1; }
  .ib { inline-size: 40px; block-size: 40px; display: grid; place-items: center; border-radius: 999px; color: var(--ink); flex: none; }
  .ib.sm { inline-size: 34px; block-size: 34px; }
  .ib.m, .ic.m { color: var(--muted); }
  .seg { display: flex; padding: 2px; border-radius: 999px; background: var(--surface-2); }
  .seg .on { background: var(--surface-raised); box-shadow: var(--shadow-lift); }
  .su .mark { display: grid; place-items: center; padding: 12px 0 4px; }
  .title { text-align: center; font-size: 19px; font-weight: 600; margin: 4px 0 14px; text-wrap: balance; }
  .btns { display: grid; gap: 8px; }
  .btn { display: flex; align-items: center; justify-content: center; gap: 8px; block-size: 44px; border-radius: 14px; font-size: 15px; font-weight: 500; }
  .btn.ghost { background: var(--surface-2); color: var(--ink); }
  .btn.pri { background: var(--accent); color: var(--on-accent); flex: 1; }
  .btn kbd { display: grid; place-items: center; min-inline-size: 22px; block-size: 20px; padding: 0 4px; border-radius: 6px; background: color-mix(in srgb, currentColor 16%, transparent); font: 12px system-ui; }
  .foot { display: flex; gap: 10px; align-items: center; margin-top: 16px; }
  .round { inline-size: 52px; block-size: 52px; border-radius: 999px; background: var(--accent); color: var(--on-accent); display: grid; place-items: center; flex: none; }

  .av { inline-size: 36px; block-size: 36px; border-radius: 999px; background: var(--accent-soft); color: var(--accent-ink); display: grid; place-items: center; font-weight: 600; flex: none; margin: 0 6px 0 2px; }
  .who { display: grid; line-height: 1.2; min-inline-size: 0; flex: 1; }
  .who b { font-weight: 600; }
  .who span { font-size: 12.5px; color: var(--muted); display: flex; align-items: center; gap: 4px; }
  .comp { display: flex; align-items: center; gap: 6px; padding: 8px; }
  .field { flex: 1; min-inline-size: 0; block-size: 40px; border-radius: 20px; background: var(--bg); border: 1px solid var(--hairline); color: var(--muted); display: flex; align-items: center; padding: 0 14px; }
  .fab { inline-size: 40px; block-size: 40px; border-radius: 999px; background: var(--accent); color: var(--on-accent); display: grid; place-items: center; flex: none; }
  .acts { display: flex; justify-content: space-around; padding: 6px; }
  .chips { display: flex; gap: 8px; padding: 10px; flex-wrap: wrap; }
  .chip { display: flex; align-items: center; gap: 6px; padding: 5px 10px 5px 8px; border-radius: 999px; background: var(--surface-2); font-size: 12.5px; }
  .call { display: flex; justify-content: center; gap: 14px; padding: 18px 8px; flex-wrap: wrap; }
  .cb { display: grid; justify-items: center; gap: 6px; font-size: 11.5px; color: var(--muted); inline-size: 76px; text-align: center; }
  .cb i { inline-size: 56px; block-size: 56px; border-radius: 999px; display: grid; place-items: center; background: var(--surface-2); color: var(--ink); }
  .cb.on i { background: var(--accent-soft); color: var(--accent-ink); }
  .cb.end i { background: var(--ink); color: var(--surface); }
  .li { display: flex; align-items: center; gap: 14px; padding: 10px 14px; border-top: 1px solid var(--hairline); }
  .li:first-child { border-top: 0; }
  .li .t { flex: 1; min-inline-size: 0; }
  .li .t small { display: block; color: var(--muted); font-size: 12.5px; }
  .ic { color: var(--accent-ink); display: grid; }

  .scroll { overflow-x: auto; }
  table { border-collapse: collapse; inline-size: 100%; min-inline-size: 640px; }
  th, td { text-align: start; padding: 7px 12px; border-top: 1px solid var(--hairline); vertical-align: middle; }
  thead th { border-top: 0; font-size: 11px; letter-spacing: .06em; text-transform: uppercase; color: var(--muted); font-weight: 600; }
  .note { color: var(--muted); font-size: 13px; }
  .tag { font: 11px ui-monospace, monospace; padding: 2px 6px; border-radius: 6px; border: 1px solid var(--line); color: var(--muted); white-space: nowrap; }
  .tag.new { color: var(--accent-ink); border-color: currentColor; }
  .ret td { opacity: .7; }
  .fine { color: var(--muted); font-size: 13px; margin-top: 12px; }
</style>
