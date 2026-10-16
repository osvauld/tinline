<script lang="ts">
  /* End-call lab (`?lab=endcall`). The apps' End glyph (`callend`, "cans apart, strings dropped") reads as a headset.
     User, 2026-10-16: draw it from the logo. Four candidates on the real call controls (64 px phone row, 56 px compact,
     the desktop row), light and dark, with today's two glyphs for reference. Press End to see what it does; it resets.
     "Open in a call" runs the app with `?endicon=` so the real call screen uses that glyph. */
  import EndGlyph, { type EndVariant } from '../../lib/calls/EndGlyph.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import VIcon from '../../lib/calls/video/VIcon.svelte';
  import { TIN, tinTier } from '../../lib/ui/tin-icons.gen';

  type Row = { v: EndVariant | 'headset'; title: string; line: string };
  const ROWS: Row[] = [
    { v: 'xcut', title: 'X, one line cut', line: 'The swirl X, one line cut where the other crosses it. Pressing End lets the cut line go slack: it sags and its halves fall back from the cut.' },
    { v: 'snip', title: 'The line, cut (sagging)', line: 'Only the logo\'s sagging line, cut at the bottom, the ends drooping. Pressing End lets them fall.' },
    { v: 'xsag', title: 'X, sagging', line: 'An X of two curved strokes that both sag, like two crossed strings. Pressing End lets them sag further.' },
    { v: 'xswirl', title: 'X, swirl', line: 'An X of two curved strokes bowing the same way round the centre. Pressing End pulls them nearly straight.' },
    { v: 'cutline', title: 'The line, cut (tight)', line: 'Only a tight line, cut in the middle, the ends dipping. Pressing End lets them fall.' },
    { v: 'curvy', title: 'Curvy scissors', line: 'Scissors drawn soft: leaf blades, oval handles. Pressing End snips them shut.' },
    { v: 'swoosh', title: 'Swoosh', line: 'A tapered curved stroke through the sagging line, which parts where it crosses. Pressing End follows through.' },
    { v: 'scissors', title: 'Scissors', line: 'Just scissors, open. Pressing End snips them shut.' },
    { v: 'knife', title: 'Knife', line: 'Just a kitchen knife. Pressing End brings it down in a short chop.' },
    { v: 'slack', title: 'A. The line lets go', line: 'The logo, its line tight while the call is up. Pressing End lets it go slack and swing, the same motion the home logo makes when a call ends.' },
    { v: 'cut', title: 'B. The line is cut', line: 'The logo with its line cut in the middle, each end dipping. Pressing End opens the cut and the ends drop. Reads as "line down" when still.' },
    { v: 'slash', title: 'C. The logo, off', line: 'The logo with the 45° slash that mic off and camera off use. Consistent, but reads closer to "mute calls" than to "hang up".' },
    { v: 'headset', title: 'Before', line: '`callend`: cans apart, strings dropped. Read as a headset; still used for "calls off". Reference.' },
    { v: 'phone', title: 'Today (hangup)', line: 'The phone can turned down: the apps\' End since 2026-10-16 (`hangup`). Reference.' },
  ];

  let theme = $state<'light' | 'dark'>(new URLSearchParams(location.search).get('theme') === 'dark' ? 'dark' : 'light');
  $effect(() => { document.documentElement.dataset.theme = theme; });
  /** Which row's End was pressed (plays its motion, then resets). */
  let pressed = $state<Record<string, boolean>>({});
  function press(v: string) {
    if (pressed[v]) return;
    pressed[v] = true;
    setTimeout(() => (pressed[v] = false), 1800);
  }
  const headset = (size: number) => { const g = TIN['callend'][tinTier(size)]; return g.body + g.string; };
  const open = (v: string) => `?form=android&theme=${theme}&endicon=${v}`;
</script>

{#snippet glyph(r: Row, size: number)}
  {#if r.v === 'headset'}
    <svg class="i" width={size} height={size} viewBox="0 0 24 24" aria-hidden="true">{@html headset(size)}</svg>
  {:else}
    <EndGlyph variant={r.v} {size} pressed={!!pressed[r.v]} />
  {/if}
{/snippet}

<main>
  <header>
    <h1>End call</h1>
    <p>Press any red button to see what it does. The line, cut, first; the other studies and today's glyphs below for comparison.</p>
    <div class="bar">
      <div class="seg">
        <button type="button" aria-pressed={theme === 'light'} onclick={() => (theme = 'light')}>Light</button>
        <button type="button" aria-pressed={theme === 'dark'} onclick={() => (theme = 'dark')}>Dark</button>
      </div>
    </div>
  </header>

  <!-- Close up: each End at three times its size, to judge the drawing. -->
  <section>
    <div class="big">
      {#each ROWS as r (r.v)}
        <div class="ctl"><button type="button" class="round end press xl" aria-label="End call ({r.title}, close up)" onclick={() => press(r.v)}>{@render glyph(r, 100)}</button><span>{r.title}</span></div>
      {/each}
    </div>
  </section>

  {#each ROWS as r (r.v)}
    <section class:ref={r.v === 'headset' || r.v === 'phone'}>
      <div class="head">
        <h2>{r.title}</h2>
        {#if r.v !== 'headset'}<a href={open(r.v)} target="_blank" rel="noopener">Open in a call ↗</a>{/if}
      </div>
      <p class="note">{r.line}</p>
      <div class="stages">
        <!-- Phone: Mute · Camera · End · Flip · Speaker -->
        <div class="stage phone">
          <div class="ctl"><span class="round tog"><Icon name="mic" size={26} /></span><span>Mute</span></div>
          <div class="ctl"><span class="round tog"><VIcon name="video" size={26} /></span><span>Camera</span></div>
          <div class="ctl"><button type="button" class="round end press" aria-label="End call ({r.title})" onclick={() => press(r.v)}>{@render glyph(r, 34)}</button><span>End</span></div>
          <div class="ctl idle"><span class="round tog"><VIcon name="flip" size={26} /></span><span>Flip</span></div>
          <div class="ctl"><span class="round tog"><Icon name="vol" size={26} /></span><span>Speaker</span></div>
        </div>
        <!-- On video: the same row over a picture -->
        <div class="stage video">
          <div class="ctl"><span class="round tog"><Icon name="mic" size={22} /></span></div>
          <div class="ctl"><button type="button" class="round end press sm" aria-label="End call ({r.title}, compact)" onclick={() => press(r.v)}>{@render glyph(r, 30)}</button></div>
          <div class="ctl"><span class="round tog"><VIcon name="video" size={22} /></span></div>
        </div>
        <!-- Small: incoming Decline in a notification-sized pill, and the glyph alone at 20 / 24 -->
        <div class="stage small">
          <span class="pill end">{@render glyph(r, 20)}<b>Decline</b></span>
          <span class="bare">{@render glyph(r, 24)}</span>
          <span class="bare">{@render glyph(r, 20)}</span>
        </div>
      </div>
    </section>
  {/each}
</main>

<style>
  main { min-block-size: 100dvh; background: var(--canvas, var(--bg)); color: var(--ink); padding: 24px 16px 48px; display: grid; gap: 28px; justify-items: center; }
  header { text-align: center; max-inline-size: 760px; display: grid; gap: 10px; justify-items: center; }
  h1 { font-size: 20px; margin: 0; }
  h2 { font-size: 16px; margin: 0; }
  header p { margin: 0; color: var(--muted); font-size: 14px; }
  .bar { display: flex; gap: 12px; flex-wrap: wrap; justify-content: center; }
  .seg { display: flex; gap: 4px; padding: 3px; border-radius: 12px; background: var(--surface-2); }
  .seg button { border: 0; padding: 7px 12px; min-block-size: 36px; border-radius: 9px; font: inherit; font-size: 13px; background: transparent; color: var(--ink); cursor: pointer; }
  .seg button[aria-pressed='true'] { background: var(--surface-raised, var(--bg)); box-shadow: var(--shadow-lift); }
  section { inline-size: min(960px, 100%); display: grid; gap: 8px; }
  section.ref { opacity: .8; }
  .head { display: flex; align-items: baseline; gap: 12px; flex-wrap: wrap; }
  .head a { font-size: 13px; color: var(--accent); }
  .note { margin: 0; color: var(--muted); font-size: 13px; max-inline-size: 720px; }
  .stages { display: flex; flex-wrap: wrap; gap: 12px; }
  .stage { display: flex; align-items: flex-start; justify-content: center; gap: 14px; padding: 18px 16px 14px; border-radius: 20px; background: var(--surface); border: 1px solid var(--line); }
  .stage.phone { flex: 1 1 380px; }
  .stage.video { align-items: center; background: linear-gradient(160deg, #3b4a44, #1c2321 70%); border-color: transparent; padding: 14px 18px; }
  .stage.small { align-items: center; gap: 18px; }
  .ctl { display: grid; justify-items: center; gap: 6px; font-size: 12px; color: var(--muted); }
  .ctl.idle .round { opacity: .45; }
  .round { inline-size: 64px; block-size: 64px; border-radius: 50%; border: 0; display: grid; place-items: center; padding: 0; background: var(--surface-2); color: var(--ink); }
  .big { display: flex; flex-wrap: wrap; justify-content: center; gap: 20px; }
  .round.xl { inline-size: 180px; block-size: 180px; }
  .round.sm { inline-size: 56px; block-size: 56px; }
  .video .round { inline-size: 56px; block-size: 56px; }
  .video .round.tog { background: rgba(255, 255, 255, .16); color: #fff; }
  .round.end { background: var(--danger-btn); color: var(--on-danger-btn); cursor: pointer; }
  .round.end:active { transform: scale(.93); }
  .round.end:focus-visible { outline: 2px solid var(--ink); outline-offset: 3px; }
  .round :global(.hang) { transform: rotate(135deg); }
  .pill { display: inline-flex; align-items: center; gap: 8px; block-size: 36px; padding-inline: 12px 16px; border-radius: 18px; background: var(--danger-btn); color: var(--on-danger-btn); font-size: 13px; }
  .bare { --eg-bg: var(--surface); display: grid; place-items: center; inline-size: 36px; block-size: 36px; color: var(--ink); }
  .i { fill: none; stroke: currentColor; stroke-width: 1.8; stroke-linecap: round; stroke-linejoin: round; flex: none; }
  .i :global(.f) { fill: currentColor; stroke: none; }
</style>
