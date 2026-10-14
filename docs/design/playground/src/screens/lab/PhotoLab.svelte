<script lang="ts">
  /* Photo lab (`?lab=photos`): how a sent picture sits in the chat. The playground had only a file row for images and the
     apps grew their own thumbnails without a design; this compares three bubble treatments on the same conversation,
     phone width, with the states that matter (caption, sending, waiting for them, not downloaded yet).
     Core fact (crates/core chat/api.rs Attachment): no thumbnail and no width/height travel with a photo, so before it
     downloads its shape is unknown. "Box" shows the two honest answers: the true shape (the bubble changes size when the
     photo arrives) or a fixed box the photo is cropped into (nothing moves; the full photo opens on tap).
     Chosen 2026-10-14: Framed, every corner round (no tail). Box (true shape or fixed crop) still open. */
  import Icon from '../../lib/ui/Icon.svelte';

  type Variant = 'edge' | 'frame' | 'bare';
  type Msg = { mine: boolean; img?: string; ar?: number; caption?: string; state?: 'done' | 'sending' | 'waiting' | 'remote'; size?: string; time: string; text?: string };

  const scene = (sky: [string, string], hills: [string, string, string], sun: string, w: number, h: number) =>
    'data:image/svg+xml,' + encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${w} ${h}" preserveAspectRatio="xMidYMid slice">
      <defs><linearGradient id="s" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="${sky[0]}"/><stop offset="1" stop-color="${sky[1]}"/></linearGradient></defs>
      <rect width="${w}" height="${h}" fill="url(#s)"/><circle cx="${w * .72}" cy="${h * .3}" r="${Math.min(w, h) * .09}" fill="${sun}"/>
      <path d="M0 ${h * .7} Q${w * .25} ${h * .5} ${w * .5} ${h * .68} T${w} ${h * .62} V${h} H0Z" fill="${hills[0]}"/>
      <path d="M0 ${h * .82} Q${w * .35} ${h * .64} ${w * .7} ${h * .8} T${w} ${h * .78} V${h} H0Z" fill="${hills[1]}"/>
      <path d="M0 ${h * .93} Q${w * .5} ${h * .82} ${w} ${h * .92} V${h} H0Z" fill="${hills[2]}"/></svg>`);
  const BALCONY = scene(['#9cc3e8', '#dfeaf2'], ['#5b7fa8', '#3c5e86', '#2a4566'], '#fff3c4', 400, 300);
  const KITCHEN = scene(['#f4c6c2', '#fbe7dc'], ['#c9776f', '#9a4f4b', '#6e3534'], '#f5c451', 300, 400);
  const SKETCH = scene(['#e9e3d3', '#f4f0e6'], ['#b9b08f', '#8f8768', '#6a644c'], '#e0a84a', 360, 360);
  const TALL = scene(['#b9dccb', '#e6f2ea'], ['#5f9c84', '#3f7a63', '#2b5948'], '#fff1b8', 270, 480);

  let theme = $state<'light' | 'dark'>('light');
  let fixedBox = $state(false);
  let progress = $state(42);
  $effect(() => { document.documentElement.dataset.theme = theme; });

  const msgs = $derived<Msg[]>([
    { mine: false, text: 'Did the movers come?', time: '16:58' },
    { mine: false, img: BALCONY, ar: 4 / 3, caption: 'The view from the balcony', time: '17:02' },
    { mine: true, img: KITCHEN, ar: 3 / 4, state: 'sending', time: '17:05' },
    { mine: true, img: SKETCH, ar: 1, time: '17:06' },
    { mine: true, img: TALL, ar: 9 / 16, state: 'waiting', caption: 'Hallway, before the paint', time: '17:07' },
    { mine: false, ar: 4 / 3, state: 'remote', size: '3.2 MB', time: '17:10' },
    { mine: false, text: 'Love the light in the kitchen', time: '17:11' },
  ]);

  const W = 248;                     // photo width on a phone (about 64 % of 390)
  const box = (ar = 1) => {
    if (fixedBox) return { w: W, h: Math.round(W * 3 / 4) };
    const h = Math.min(Math.round(W / ar), 330);
    return { w: ar < W / 330 ? Math.round(h * ar) : W, h };
  };
  const VARIANTS: { v: Variant; title: string; why: string }[] = [
    { v: 'edge', title: 'Edge to edge', why: 'The photo is the bubble: it fills it to the corners. The time sits on the photo in a small dark chip; a caption goes under it, inside the bubble.' },
    { v: 'frame', title: 'Framed ✓ chosen', why: 'Chosen 2026-10-14. The bubble keeps a 3 px rim of its colour around the photo (all corners round: 18 outside, 15 inside), and the time always sits under it, like a text message.' },
    { v: 'bare', title: 'Bare photo', why: 'No bubble: the photo stands alone with rounded corners and a hairline. A caption is a normal bubble joined underneath.' },
  ];
</script>

{#snippet meta(m: Msg, chip: boolean)}
  <span class="meta" class:chip>
    <time>{m.time}</time>
    {#if m.mine}
      {#if m.state === 'sending' || m.state === 'waiting'}<Icon name="clock" size={11} />{:else}<Icon name="check" size={12} />{/if}
    {/if}
  </span>
{/snippet}

{#snippet pic(m: Msg, v: Variant)}
  {@const b = box(m.ar)}
  <div class="pic {v}" class:mine={m.mine} class:cap={!!m.caption && v === 'edge'} style:width="{b.w}px" style:height="{b.h}px">
    {#if m.state === 'remote'}
      <div class="ph-empty">
        <Icon name="image" size={28} />
        <span>Photo · {m.size}</span>
        <button type="button" class="dl press"><Icon name="back" size={16} class="down" />Download</button>
      </div>
    {:else}
      <img src={m.img} alt="" draggable="false" />
      {#if m.state === 'sending'}
        <div class="veil"></div>
        <button type="button" class="ring press" aria-label="Cancel sending">
          <svg viewBox="0 0 48 48"><circle class="trk" cx="24" cy="24" r="21" /><circle class="arc" cx="24" cy="24" r="21" style:stroke-dashoffset={132 * (1 - progress / 100)} /></svg>
          <Icon name="x" size={18} />
        </button>
      {:else if m.state === 'waiting'}
        <span class="wait">Sends when Arjun is online</span>
      {/if}
    {/if}
    {#if v !== 'frame' && !(v === 'edge' && m.caption)}{@render meta(m, true)}{/if}
  </div>
{/snippet}

<div class="lab">
  <header>
    <b>Photos in a chat · phone</b>
    <span class="hint">Same conversation, three treatments. Sending, waiting for them and not downloaded yet are all in it.</span>
    <div class="ctl">
      <button type="button" onclick={() => (theme = theme === 'light' ? 'dark' : 'light')}>{theme === 'light' ? 'Dark' : 'Light'}</button>
      <span class="sep"></span>
      <span class="k">Box</span>
      <button type="button" aria-pressed={!fixedBox} onclick={() => (fixedBox = false)}>True shape</button>
      <button type="button" aria-pressed={fixedBox} onclick={() => (fixedBox = true)}>Fixed 4:3 crop</button>
      <span class="sep"></span>
      <label class="k">Sending <input type="range" min="0" max="100" bind:value={progress} /> {progress}%</label>
    </div>
    <p class="note">The core sends no preview and no size with a photo. With <b>True shape</b>, the not-downloaded box is a guess (4:3) and changes size when the photo arrives. With <b>Fixed 4:3 crop</b> nothing moves; the whole photo opens on tap. A third way needs the core to send the size (and maybe a tiny blurred preview): an open decision, not in this lab.</p>
  </header>
  <main>
    {#each VARIANTS as V (V.v)}
      <figure>
        <figcaption><b>{V.title}</b><span>{V.why}</span></figcaption>
        <div class="phone">
          <ol class="msgs">
            {#each msgs as m, i (i)}
              <li class="row" class:me={m.mine}>
                {#if m.text}
                  <div class="bub">{m.text}{@render meta(m, false)}</div>
                {:else if V.v === 'edge'}
                  <div class="bub photo edge">{@render pic(m, 'edge')}{#if m.caption}<div class="captxt">{m.caption}{@render meta(m, false)}</div>{/if}</div>
                {:else if V.v === 'frame'}
                  <div class="bub photo frame">{@render pic(m, 'frame')}<div class="captxt" class:only={!m.caption}>{m.caption ?? ''}{@render meta(m, false)}</div></div>
                {:else}
                  <div class="bare-col">{@render pic(m, 'bare')}{#if m.caption}<div class="bub joined">{m.caption}{@render meta(m, false)}</div>{/if}</div>
                {/if}
              </li>
            {/each}
          </ol>
          <div class="composer"><span class="clip"><Icon name="clip" size={22} /></span><span class="field">Message</span></div>
        </div>
      </figure>
    {/each}
  </main>
</div>

<style>
  .lab { min-block-size: 100vh; background: var(--bg); color: var(--ink); font: 14px/1.4 system-ui, sans-serif; }
  header { position: sticky; top: 0; z-index: 2; display: grid; gap: 6px; padding: 12px 16px; background: var(--bg); border-block-end: 1px solid var(--hairline); }
  .hint, .note { opacity: .8; margin: 0; font-size: 13px; max-inline-size: 1100px; }
  .ctl { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; }
  .ctl .k { font-size: 12px; opacity: .7; display: flex; gap: 6px; align-items: center; }
  .ctl button { border: 1px solid var(--line); background: transparent; color: inherit; border-radius: 10px; padding: 5px 10px; font: inherit; cursor: pointer; }
  .ctl button[aria-pressed='true'] { background: var(--accent-soft); border-color: var(--accent); }
  .sep { inline-size: 1px; block-size: 20px; background: var(--hairline); margin-inline: 4px; }
  main { display: grid; grid-template-columns: repeat(3, 390px); gap: 24px; padding: 16px; justify-content: center; }
  figure { margin: 0; display: grid; gap: 10px; align-content: start; }
  figcaption { display: grid; gap: 4px; min-block-size: 92px; align-content: start; font-size: 13px; }
  figcaption b { font-size: 15px; }
  figcaption span { opacity: .8; }

  .phone { inline-size: 390px; block-size: 844px; display: flex; flex-direction: column; border-radius: 28px; overflow: hidden; border: 1px solid var(--line); background: var(--canvas); box-shadow: var(--shadow-pop); }
  .msgs { list-style: none; margin: 0; padding: 12px 10px 12px; flex: 1; overflow: auto; display: flex; flex-direction: column; gap: 6px; }
  .row { display: flex; }
  .row.me { justify-content: flex-end; }
  .bub { position: relative; max-inline-size: 78%; padding: 7px 12px; border-radius: 18px; background: var(--bub-in); color: var(--bub-in-ink); box-shadow: inset 0 0 0 1px var(--bub-in-line); font-size: 15px; line-height: 21px; }
  .me .bub { background: var(--bub-out); color: var(--bub-out-ink); box-shadow: none; }
  .meta { float: right; display: inline-flex; gap: 3px; align-items: center; margin: 6px 0 -4px 10px; font-size: 11px; line-height: 14px; color: var(--bub-meta-in); font-variant-numeric: tabular-nums; }
  .me .meta { color: var(--bub-meta-out); }

  /* photo bubbles */
  .bub.photo { padding: 0; overflow: hidden; }
  .bub.frame { padding: 3px; }
  .pic { position: relative; overflow: hidden; display: block; background: color-mix(in srgb, var(--ink) 8%, var(--surface)); }
  .pic img { inline-size: 100%; block-size: 100%; object-fit: cover; display: block; user-select: none; }
  .pic.edge { border-radius: 0; }
  .pic.frame { border-radius: 15px; }
  .pic.bare { border-radius: 18px; box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--ink) 10%, transparent); }
  .bare-col { display: grid; gap: 2px; justify-items: end; max-inline-size: 78%; }
  .row:not(.me) .bare-col { justify-items: start; }
  .bare-col:has(.joined) .pic.bare { border-end-start-radius: 6px; border-end-end-radius: 6px; }
  .bub.joined { max-inline-size: 100%; }
  .bub.joined { border-start-start-radius: 6px; border-start-end-radius: 6px; }
  /* The caption never widens the bubble past the photo: it takes the photo's width and wraps. */
  .captxt { inline-size: 0; min-inline-size: 100%; box-sizing: border-box; padding: 6px 12px 8px; font-size: 15px; line-height: 21px; }
  .bub.photo { inline-size: min-content; }
  .row:not(.me) .bub.edge::after { content: ''; position: absolute; inset: 0; border-radius: inherit; box-shadow: inset 0 0 0 1px var(--bub-in-line); pointer-events: none; }
  .frame .captxt { padding: 5px 9px 5px; }
  .captxt.only { padding-block: 2px 4px; min-block-size: 0; }
  .captxt.only .meta { margin-block: 2px 0; }
  .captxt::after { content: ''; display: block; clear: both; }

  /* the time on the photo */
  .meta.chip { position: absolute; inset: auto 8px 8px auto; float: none; margin: 0; padding: 2px 7px; border-radius: 10px; background: rgb(0 0 0 / .42); color: #fff; backdrop-filter: blur(6px); }

  /* states */
  .veil { position: absolute; inset: 0; background: rgb(0 0 0 / .18); }
  .ring { position: absolute; inset: 50% auto auto 50%; translate: -50% -50%; inline-size: 52px; block-size: 52px; border: 0; padding: 0; border-radius: 50%; background: rgb(0 0 0 / .35); color: #fff; display: grid; place-items: center; cursor: pointer; backdrop-filter: blur(6px); }
  .ring svg { position: absolute; inset: 2px; inline-size: 48px; block-size: 48px; rotate: -90deg; }
  .ring circle { fill: none; stroke-width: 3; }
  .ring .trk { stroke: rgb(255 255 255 / .28); }
  .ring .arc { stroke: #fff; stroke-linecap: round; stroke-dasharray: 132; transition: stroke-dashoffset 200ms ease-out; }
  .wait { position: absolute; inset: auto 8px 8px 8px; inline-size: max-content; max-inline-size: calc(100% - 70px); padding: 2px 8px; border-radius: 10px; background: rgb(0 0 0 / .42); color: #fff; font-size: 11px; line-height: 14px; backdrop-filter: blur(6px); }
  .ph-empty { position: absolute; inset: 0; display: grid; place-content: center; justify-items: center; gap: 6px; color: var(--muted); font-size: 13px; }
  .dl { display: inline-flex; align-items: center; gap: 6px; margin-block-start: 4px; padding: 6px 14px; border: 0; border-radius: 16px; background: var(--accent); color: var(--on-accent); font: inherit; font-weight: 650; cursor: pointer; }
  .dl :global(.down) { rotate: -90deg; }

  .composer { display: flex; gap: 8px; align-items: center; padding: 8px 10px 14px; }
  .clip { color: var(--muted); display: grid; place-items: center; inline-size: 40px; }
  .field { flex: 1; padding: 10px 16px; border-radius: 22px; background: var(--surface); color: var(--muted); }
</style>
