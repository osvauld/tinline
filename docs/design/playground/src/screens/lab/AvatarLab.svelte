<script lang="ts">
  /* Avatar lab (`?lab=avatars`): identities have one nickname, so today's initials are one letter ("Maya" -> "M").
     1. Initials: "M" vs two letters "Ma" with the lowercase letter optically compensated (x-height is ~70% of a
        capital, so it is drawn a little larger and nudged so both read as one weight), and a lowercase nickname.
     2. Fingerprint avatars: drawn from the identity's key fingerprint, a 256-bit number read byte by byte (here SHA-256 of the DID; in the apps it would
        be computed once in Rust, e.g. BLAKE3 of the DID's public key, and handed to both apps). Same on every device,
        never changes (the DID is stable; device keys don't affect it). Lines only (user, 2026-10-14): Harmonograph,
        Strings (the tin-line strings), Rose. Each can carry the
        initial on top, so the name still reads. Not a security check on its own: the safety number stays the proof. */
  import { onMount } from 'svelte';

  const NAMES = ['Maya', 'Studio', 'Home', 'arjun', 'Tara', 'Sam', 'Ravi', 'Nina'];
  const didOf = (n: string) => 'did:key:z6Mk' + n.toLowerCase() + 'synthetic' + n.length;

  let theme = $state<'light' | 'dark'>('light');
  let overlay = $state(true);
  let two = $state(true);
  /** Section 3: nicknames are not unique (anyone can call themselves Maya). Same names, different keys. */
  const PEOPLE = [
    { id: 'Maya', name: 'Maya' }, { id: 'maya-2', name: 'Maya' }, { id: 'Sam', name: 'Sam' }, { id: 'sam-2', name: 'Sam' },
    { id: 'Studio', name: 'Studio' }, { id: 'Tara', name: 'Tara' },
  ];
  let listStyle = $state<'harmo' | 'strands' | 'rose' | 'sacred'>('sacred');
  let tag = $state(true);
  $effect(() => { document.documentElement.dataset.theme = theme; });

  /** 32 bytes per identity from SHA-256 of its DID. */
  let fp = $state<Record<string, number[]>>({});
  onMount(async () => {
    const out: Record<string, number[]> = {};
    for (const n of [...NAMES, ...PEOPLE.map((x) => x.id)]) {
      if (out[n]) continue;
      const h = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(didOf(n)));
      out[n] = [...new Uint8Array(h)];
    }
    fp = out;
  });

  /** Deterministic stream of 0..1 numbers from the fingerprint bytes. */
  function stream(b: number[]) {
    let i = 0;
    return () => { const v = (b[i % 32] * 256 + b[(i + 7) % 32]) / 65536; i++; return v; };
  }
  const hueOf = (b: number[]) => b[0] % 6;

  /* ---- initials ---- */
  function letters(n: string, twoLetters: boolean) {
    const t = n.trim();
    const cap = t.charAt(0).toUpperCase();
    return twoLetters && t.length > 1 && !/\s/.test(t) ? [cap, t.charAt(1).toLowerCase()] : [cap];
  }

  /* Every style reads the fingerprint (a 256-bit number, 32 bytes) byte by byte; each byte sets one parameter.
     Lines only: one stroke weight, round caps, like the Tinline line icons. */
  const hex = (b: number[]) => b.slice(0, 6).map((x) => x.toString(16).padStart(2, '0')).join('').replace(/(.{4})/g, '$1 ').trim();

  /* ---- Harmonograph: one continuous curve, x and y each a slowly fading sine.
     byte 2 -> x frequency (1..4), byte 3 -> y frequency (1..4, never equal to x), byte 4 -> phase, byte 5 -> fade,
     byte 6 -> rotation. ---- */
  function harmo(b: number[]) {
    const fx = 1 + (b[2] % 4);
    let fy = 1 + (b[3] % 4); if (fy === fx) fy = (fy % 4) + 1;
    const ph = (b[4] / 256) * Math.PI * 2, fade = .05 + (b[5] / 256) * .05, rot = (b[6] / 256) * 180;
    const pts: string[] = [];
    for (let i = 0; i <= 420; i++) {
      const t = i * .06, e = Math.exp(-fade * t);
      pts.push(`${(50 + 38 * Math.sin(fx * t + ph) * e).toFixed(2)},${(50 + 38 * Math.sin(fy * t) * e).toFixed(2)}`);
    }
    return { d: 'M' + pts.join('L'), rot, label: `x ${fx} · y ${fy} · phase ${Math.round((ph * 180) / Math.PI)}°` };
  }

  /* ---- Strings: four strings across the face, each bent by two bytes (like voices on a line); one is gold.
     bytes 8..15 -> bends, byte 16 -> tilt. ---- */
  function strands(b: number[]) {
    const tilt = -25 + (b[16] / 256) * 50;
    return Array.from({ length: 4 }, (_, k) => {
      const y = 29 + k * 14, c1 = (b[8 + k * 2] / 256 - .5) * 34, c2 = (b[9 + k * 2] / 256 - .5) * 34;
      return { d: `M6,${y} C36,${(y + c1).toFixed(1)} 64,${(y + c2).toFixed(1)} 94,${y}`, gold: k === b[17] % 4 };
    }).map((x) => ({ ...x, tilt }));
  }

  /* ---- Rose: r = cos(k θ) with k = n/d from bytes 18 and 19, rotated by byte 20. Closed, symmetric. ---- */
  function rose(b: number[]) {
    let n = 2 + (b[18] % 5); const d = 1 + (b[19] % 3), rot = (b[20] / 256) * 360;
    if (n % d === 0 && n / d === 1) n += 1;   // k = 1 is a plain circle: never draw that
    const k = n / d, turns = d % 2 === 1 && n % 2 === 1 ? d : 2 * d;
    const pts: string[] = [];
    for (let i = 0; i <= 900; i++) {
      const th = (i / 900) * Math.PI * turns, r = 40 * Math.cos(k * th);
      pts.push(`${(50 + r * Math.cos(th)).toFixed(2)},${(50 + r * Math.sin(th)).toFixed(2)}`);
    }
    return { d: 'M' + pts.join('L'), rot, label: `${n}/${d} petals` };
  }
  /* ---- Sacred geometry v2: every part reads its OWN byte of the fingerprint, so parts vary independently and the
     combinations multiply. 9 of the 32 bytes are used; the rest are spare for later layers.
       byte 0 hue (smooth, 360 values, inside a calm lightness/chroma)    byte 5 inner shape (circle / polygon / small star, 4 sizes)
       byte 1 points n = 5..16                                             byte 6 extra layer (none / Metatron lines / vesica petals / golden rings)
       byte 2 star step k ({n/k})                                          byte 7 which layer is gold
       byte 3 ring circle size        byte 4 ring distance                 byte 8 rotation
     ---- */
  const PHI = (1 + Math.sqrt(5)) / 2;
  const EXTRAS = ['none', 'Metatron lines', 'vesica petals', 'golden rings'] as const;
  const INNERS = ['circle', 'polygon', 'small star'] as const;
  function sacred(b: number[]) {
    const n = 5 + (b[1] % 12);
    const kmax = Math.floor((n - 1) / 2), k = 2 + (b[2] % (kmax - 1));
    const rot = (b[8] / 256) * (360 / n);
    const pt = (r: number, i: number, m: number, off = 0) => {
      const a = ((i / m) * 360 + off - 90) * (Math.PI / 180);
      return [50 + r * Math.cos(a), 50 + r * Math.sin(a)];
    };
    const poly = (r: number, m: number, step: number, off = 0) => {
      const g = gcd(m, step), out: string[] = [];
      for (let s0 = 0; s0 < g; s0++) {
        const pts: string[] = [];
        for (let i = 0; i <= m / g; i++) { const [x, y] = pt(r, (s0 + i * step) % m, m, off); pts.push(`${x.toFixed(2)},${y.toFixed(2)}`); }
        out.push('M' + pts.join('L'));
      }
      return out;
    };
    const R = 42;
    const star = poly(R, n, k);
    const cr = 8 + (b[3] % 8), cd = 12 + (b[4] % 10);
    const ring = Array.from({ length: n }, (_, i) => { const [x, y] = pt(cd, i, n, 180 / n); return { x, y, r: cr }; });
    const innerKind = b[5] % 3, ir = 6 + ((b[5] >> 2) % 4) * 2;
    const inner = innerKind === 0 ? null : innerKind === 1 ? poly(ir, n, 1, 180 / n) : poly(ir, n, Math.max(2, Math.floor((n - 1) / 2)), 180 / n);
    const extra = b[6] % 4;
    let extraPaths: string[] = [];
    if (extra === 1) {
      // Metatron: the polygon through the ring centres and their spokes to the middle
      extraPaths = [...poly(cd, n, 1, 180 / n), ...ring.map((c) => `M50,50L${c.x.toFixed(2)},${c.y.toFixed(2)}`)];
    } else if (extra === 2) {
      // vesica petals: each petal is the lens of two circles that pass through each other's centre
      const pr = cd * 0.9;
      extraPaths = Array.from({ length: n }, (_, i) => {
        const [x, y] = pt(pr, i, n);
        return `M50,50A${(pr * .62).toFixed(2)},${(pr * .62).toFixed(2)} 0 0 1 ${x.toFixed(2)},${y.toFixed(2)}A${(pr * .62).toFixed(2)},${(pr * .62).toFixed(2)} 0 0 1 50,50`;
      });
    } else if (extra === 3) {
      // golden rings: R/phi and R/phi^2
      extraPaths = [R / PHI, R / (PHI * PHI)].map((r) => `M${50 - r},50a${r},${r} 0 1 0 ${2 * r},0a${r},${r} 0 1 0 ${-2 * r},0`);
    }
    return { n, k, rot, star, ring, inner, ir, innerKind, extra, extraPaths, gold: b[7] % 4, label: `${n} points · {${n}/${k}} · ${EXTRAS[extra]}` };
  }
  /** Calm colours from byte 0: any hue, but a fixed soft lightness and chroma, so no avatar shouts. */
  const sacredColours = (b: number[], dark: boolean) => {
    const h = Math.round((b[0] / 256) * 360);
    return dark ? { bg: `oklch(0.34 0.05 ${h})`, fg: `oklch(0.88 0.07 ${h})` } : { bg: `oklch(0.94 0.035 ${h})`, fg: `oklch(0.42 0.09 ${h})` };
  };
  /** The byte map for the "How it's built" panel. */
  function sacredMap(b: number[]) {
    const q = sacred(b), x = (i: number) => '0x' + b[i].toString(16).padStart(2, '0');
    return [
      [0, x(0), `hue ${Math.round((b[0] / 256) * 360)}°`],
      [1, x(1), `${q.n} points`],
      [2, x(2), `star step ${q.k}: {${q.n}/${q.k}}`],
      [3, x(3), `ring circles r ${8 + (b[3] % 8)}`],
      [4, x(4), `ring distance ${12 + (b[4] % 10)}`],
      [5, x(5), `inner ${INNERS[q.innerKind]}`],
      [6, x(6), `extra: ${EXTRAS[q.extra]}`],
      [7, x(7), `gold: ${['star', 'ring', 'inner', 'extra'][q.gold]}`],
      [8, x(8), `turned ${q.rot.toFixed(1)}°`],
    ] as const;
  }
  function gcd(a: number, b: number): number { return b ? gcd(b, a % b) : a; }
  const SIZES = [72, 44, 28];
</script>

<main>
  <header>
    <h1>Avatars from nicknames and fingerprints</h1>
    <p>Identities have one nickname, so initials give one letter. And every identity has a fingerprint: its key. A fingerprint is a number. Each byte of it sets one part of the lines, so the same identity always draws the same avatar, on every device.</p>
    <div class="bar">
      <div class="seg" role="group" aria-label="Letters">
        <button type="button" aria-pressed={!two} onclick={() => (two = false)}>One letter</button>
        <button type="button" aria-pressed={two} onclick={() => (two = true)}>Two letters</button>
      </div>
      <div class="seg" role="group" aria-label="Initial on the fingerprint">
        <button type="button" aria-pressed={overlay} onclick={() => (overlay = true)}>With initial</button>
        <button type="button" aria-pressed={!overlay} onclick={() => (overlay = false)}>Pattern only</button>
      </div>
      <div class="seg" role="group" aria-label="Theme">
        <button type="button" aria-pressed={theme === 'light'} onclick={() => (theme = 'light')}>Light</button>
        <button type="button" aria-pressed={theme === 'dark'} onclick={() => (theme = 'dark')}>Dark</button>
      </div>
    </div>
  </header>

  {#snippet initial(n: string, size: number, hue: number, plain = false)}
    {@const L = letters(n, two)}
    <span class="ini" class:plain style:--s="{size}px" style:--bg="var(--av{hue}-bg)" style:--fg="var(--av{hue}-ink)" aria-hidden="true">
      <span class="cap">{L[0]}</span>{#if L[1]}<span class="low">{L[1]}</span>{/if}
    </span>
  {/snippet}

  <section>
    <h2>1 · Initials</h2>
    <p class="note">"Two letters" draws the lowercase letter about 12% larger and nudged up, so "Ma" reads as one even weight next to "M". A lowercase nickname ("arjun") still starts with a capital in the avatar; the name itself is shown as typed.</p>
    <div class="grid">
      {#each NAMES as n (n)}
        <figure class="cell">
          {@render initial(n, 56, fp[n] ? hueOf(fp[n]) : 0)}
          <figcaption>{n}</figcaption>
        </figure>
      {/each}
    </div>
  </section>

  {#if Object.keys(fp).length}
    {#each [{ k: 'harmo', t: 'a · Harmonograph', d: 'One continuous line. Two bytes pick how fast it swings across and down, one its phase, one how it settles inward. Like a pendulum drawing.' }, { k: 'strands', t: 'b · Strings', d: 'Four strings across the face, each bent by two bytes of the number, one of them gold. The closest to the tin-can line.' }, { k: 'rose', t: 'c · Rose', d: 'A rose curve: two bytes give the petal ratio, one the turn. Always symmetric, always closed.' }, { k: 'sacred', t: 'd · Sacred geometry', d: 'A small mandala in three layers sharing one symmetry: a star polygon, a ring of circles (Seed of Life), an inner shape. Bytes pick the points, the star step, the circles and which layer is gold.' }] as st (st.k)}
      <section>
        <h2>2{st.t}</h2>
        <p class="note">{st.d}</p>
        <div class="grid">
          {#each NAMES as n (n)}
            {@const b = fp[n]}
            {@const hue = hueOf(b)}
            <figure class="cell">
              <div class="sizes">
                {#each SIZES as sz (sz)}
                  {@const sc = sacredColours(b, theme === 'dark')}
                  <span class="fpav" style:--s="{sz}px" style:--bg={st.k === 'sacred' ? sc.bg : `var(--av${hue}-bg)`} style:--fg={st.k === 'sacred' ? sc.fg : `var(--av${hue}-ink)`} aria-hidden="true">
                    <svg viewBox="0 0 100 100" width={sz} height={sz}>
                      {#if st.k === 'harmo'}
                        {@const h = harmo(b)}
                        <path d={h.d} transform="rotate({h.rot} 50 50)" fill="none" stroke="var(--fg)" stroke-width={sz < 40 ? 3.2 : 2.2} stroke-linecap="round" stroke-linejoin="round" opacity={overlay ? .45 : .9} />
                      {:else if st.k === 'strands'}
                        <g fill="none" stroke-width={sz < 40 ? 5 : 3.4} stroke-linecap="round" opacity={overlay ? .5 : .95}>
                          {#each strands(b) as l, j (j)}<path d={l.d} transform="rotate({l.tilt} 50 50)" stroke={l.gold ? 'var(--gold)' : 'var(--fg)'} />{/each}
                        </g>
                      {:else if st.k === 'sacred'}
                        {@const q = sacred(b)}
                        {@const G = (i: number) => (q.gold === i ? 'var(--gold)' : 'var(--fg)')}
                        <g transform="rotate({q.rot} 50 50)" fill="none" stroke-width={sz < 40 ? 2.6 : 1.8} stroke-linecap="round" stroke-linejoin="round">
                          {#each q.star as d, j (j)}<path {d} stroke={G(0)} />{/each}
                          {#if sz >= 40}{#each q.ring as c, j (j)}<circle cx={c.x} cy={c.y} r={c.r} stroke={G(1)} opacity=".75" />{/each}{/if}
                          {#if q.inner}{#each q.inner as d, j (j)}<path {d} stroke={G(2)} />{/each}{:else}<circle cx="50" cy="50" r={q.ir} stroke={G(2)} />{/if}
                          {#each q.extraPaths as d, j (j)}<path {d} stroke={G(3)} opacity=".7" />{/each}
                        </g>
                      {:else}
                        {@const r = rose(b)}
                        <path d={r.d} transform="rotate({r.rot} 50 50)" fill="none" stroke="var(--fg)" stroke-width={sz < 40 ? 3.6 : 2.6} stroke-linecap="round" stroke-linejoin="round" opacity={overlay ? .45 : .9} />
                      {/if}
                    </svg>
                    {#if overlay}{@const L = letters(n, two)}<span class="over"><span class="cap">{L[0]}</span>{#if L[1]}<span class="low">{L[1]}</span>{/if}</span>{/if}
                  </span>
                {/each}
              </div>
              <figcaption>{n}<code>{hex(b)}…</code>{#if st.k === 'harmo'}<small>{harmo(b).label}</small>{:else if st.k === 'rose'}<small>{rose(b).label}</small>{:else if st.k === 'sacred'}<small>{sacred(b).label}</small>{/if}</figcaption>
            </figure>
          {/each}
        </div>
      </section>
    {/each}
  {/if}

  {#snippet art(b: number[], sz: number, style: string)}
    {@const hue = hueOf(b)}
    {@const sc = sacredColours(b, theme === 'dark')}
    <span class="fpav" style:--s="{sz}px" style:--bg={style === 'sacred' ? sc.bg : `var(--av${hue}-bg)`} style:--fg={style === 'sacred' ? sc.fg : `var(--av${hue}-ink)`} aria-hidden="true">
      <svg viewBox="0 0 100 100" width={sz} height={sz}>
        {#if style === 'harmo'}
          {@const h = harmo(b)}<path d={h.d} transform="rotate({h.rot} 50 50)" fill="none" stroke="var(--fg)" stroke-width={sz < 40 ? 3.2 : 2.2} stroke-linecap="round" stroke-linejoin="round" />
        {:else if style === 'strands'}
          <g fill="none" stroke-width={sz < 40 ? 5 : 3.4} stroke-linecap="round">{#each strands(b) as l, j (j)}<path d={l.d} transform="rotate({l.tilt} 50 50)" stroke={l.gold ? 'var(--gold)' : 'var(--fg)'} />{/each}</g>
        {:else if style === 'sacred'}
          {@const q = sacred(b)}
                        {@const G = (i: number) => (q.gold === i ? 'var(--gold)' : 'var(--fg)')}
                        <g transform="rotate({q.rot} 50 50)" fill="none" stroke-width={sz < 40 ? 2.6 : 1.8} stroke-linecap="round" stroke-linejoin="round">
                          {#each q.star as d, j (j)}<path {d} stroke={G(0)} />{/each}
                          {#if sz >= 40}{#each q.ring as c, j (j)}<circle cx={c.x} cy={c.y} r={c.r} stroke={G(1)} opacity=".75" />{/each}{/if}
                          {#if q.inner}{#each q.inner as d, j (j)}<path {d} stroke={G(2)} />{/each}{:else}<circle cx="50" cy="50" r={q.ir} stroke={G(2)} />{/if}
                          {#each q.extraPaths as d, j (j)}<path {d} stroke={G(3)} opacity=".7" />{/each}
                        </g>
        {:else}
          {@const r = rose(b)}<path d={r.d} transform="rotate({r.rot} 50 50)" fill="none" stroke="var(--fg)" stroke-width={sz < 40 ? 3.6 : 2.6} stroke-linecap="round" stroke-linejoin="round" />
        {/if}
      </svg>
    </span>
  {/snippet}


  {#if Object.keys(fp).length}
    <section>
      <h2>Sacred geometry: how the number builds it (picked)</h2>
      <p class="note">The fingerprint is 32 bytes. Each part of the drawing reads its own byte, so the parts change independently and multiply: about 42 stars × 80 rings × 12 inner shapes × 4 extra layers × 4 gold choices × ~24 calm hues ≈ 15 million combinations, around 16,000 that look different at a glance. Bytes 9–31 are spare for later. Two people called Maya:</p>
      <div class="maps">
        {#each ['Maya', 'maya-2'] as id (id)}
          <div class="map">
            {@render art(fp[id], 96, 'sacred')}
            <table>
              <thead><tr><th>byte</th><th>value</th><th>sets</th></tr></thead>
              <tbody>{#each sacredMap(fp[id]) as [i, v, what] (i)}<tr><td>{i}</td><td><code>{v}</code></td><td>{what}</td></tr>{/each}</tbody>
            </table>
          </div>
        {/each}
      </div>
    </section>
  {/if}
  {#if Object.keys(fp).length}
    <section>
      <h2>3 · Whole names, two Mayas</h2>
      <p class="note">The full nickname sits beside the avatar, like a username. Nicknames aren’t unique, so two people can both be Maya: the lines (and the optional short tag, from the same number) tell them apart. Nothing here proves who someone is; the safety number does that.</p>
      <div class="bar left">
        <div class="seg" role="group" aria-label="Style">
          <button type="button" aria-pressed={listStyle === 'harmo'} onclick={() => (listStyle = 'harmo')}>Harmonograph</button>
          <button type="button" aria-pressed={listStyle === 'strands'} onclick={() => (listStyle = 'strands')}>Strings</button>
          <button type="button" aria-pressed={listStyle === 'rose'} onclick={() => (listStyle = 'rose')}>Rose</button>
          <button type="button" aria-pressed={listStyle === 'sacred'} onclick={() => (listStyle = 'sacred')}>Sacred</button>
        </div>
        <div class="seg" role="group" aria-label="Short tag">
          <button type="button" aria-pressed={tag} onclick={() => (tag = true)}>Name · tag</button>
          <button type="button" aria-pressed={!tag} onclick={() => (tag = false)}>Name only</button>
        </div>
      </div>
      <div class="lists">
        <div class="list" aria-label="Chats list, 44 px">
          {#each PEOPLE as x (x.id)}
            <div class="li">{@render art(fp[x.id], 44, listStyle)}<span class="nm"><b>{x.name}</b>{#if tag}<code>{hex(fp[x.id]).slice(0, 4)}</code>{/if}<small>Last message preview</small></span></div>
          {/each}
        </div>
        <div class="list small" aria-label="Compact, 28 px">
          {#each PEOPLE as x (x.id)}
            <div class="li">{@render art(fp[x.id], 28, listStyle)}<span class="nm one"><b>{x.name}</b>{#if tag}<code>{hex(fp[x.id]).slice(0, 4)}</code>{/if}</span></div>
          {/each}
        </div>
      </div>
    </section>
  {/if}
</main>

<style>
  main { min-block-size: 100dvh; background: var(--canvas, var(--bg)); color: var(--ink); padding: 24px 16px 48px; display: grid; gap: 28px; justify-items: center; }
  header { text-align: center; max-inline-size: 760px; display: grid; gap: 10px; justify-items: center; }
  h1 { font-size: 20px; margin: 0; }
  h2 { font-size: 16px; margin: 0 0 4px; }
  header p { margin: 0; color: var(--muted); font-size: 14px; }
  .bar { display: flex; gap: 12px; flex-wrap: wrap; justify-content: center; }
  .seg { display: flex; gap: 4px; padding: 3px; border-radius: 12px; background: var(--surface-2); }
  .seg button { border: 0; padding: 7px 12px; min-block-size: 36px; border-radius: 9px; font: inherit; font-size: 13px; background: transparent; color: var(--ink); cursor: pointer; }
  .seg button[aria-pressed='true'] { background: var(--surface-raised, var(--bg)); box-shadow: var(--shadow-lift); }
  section { inline-size: min(960px, 100%); display: grid; gap: 10px; }
  .note { margin: 0; color: var(--muted); font-size: 13px; max-inline-size: 720px; }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(190px, 1fr)); gap: 12px; }
  .cell { margin: 0; padding: 14px; border-radius: 16px; background: var(--surface); display: grid; justify-items: center; gap: 8px; }
  .cell figcaption { font-size: 13px; color: var(--muted); display: grid; justify-items: center; gap: 2px; }
  .cell code { font-size: 11px; opacity: .8; }
  .cell small { font-size: 11px; }
  .sizes { display: flex; align-items: center; gap: 12px; }
  /* Initials: Figtree semibold; the lowercase letter is drawn ~12% larger and lifted a hair so its x-height sits
     visually level with the capital's weight (optical compensation, not a size jump). */
  .ini, .fpav { position: relative; display: inline-grid; place-items: center; inline-size: var(--s); block-size: var(--s); border-radius: 50%; background: var(--bg); color: var(--fg); overflow: hidden; flex: none; }
  .ini { font-size: calc(var(--s) * .38); font-weight: 600; letter-spacing: -.02em; line-height: 1; display: inline-flex; align-items: baseline; justify-content: center; padding-block-start: .62em; box-sizing: border-box; }
  .ini > span, .over > span { display: inline-block; }
  .low { font-size: 1.12em; transform: translateY(-.02em); margin-inline-start: .01em; }
  .fpav svg { position: absolute; inset: 0; }
  .bar.left { justify-content: flex-start; }
  .maps { display: grid; grid-template-columns: repeat(auto-fit, minmax(300px, 1fr)); gap: 16px; }
  .map { display: flex; gap: 16px; align-items: flex-start; padding: 16px; border-radius: 18px; background: var(--surface); }
  .map table { border-collapse: collapse; font-size: 12px; }
  .map th { text-align: start; color: var(--muted); font-weight: 500; padding: 0 10px 4px 0; }
  .map td { padding: 2px 10px 2px 0; vertical-align: top; }
  .lists { display: grid; grid-template-columns: repeat(auto-fit, minmax(280px, 1fr)); gap: 16px; }
  .list { background: var(--surface); border-radius: 18px; padding: 6px; display: grid; }
  .li { display: flex; align-items: center; gap: 12px; padding: 8px 10px; min-block-size: 44px; }
  .small .li { gap: 10px; padding: 6px 10px; }
  .nm { display: grid; grid-template-columns: auto 1fr; column-gap: 8px; align-items: baseline; flex: 1; min-inline-size: 0; }
  .nm b { font-weight: 600; }
  .nm code { font-size: 12px; color: var(--muted); }
  .nm small { grid-column: 1 / -1; color: var(--muted); font-size: 13px; }
  .over { position: relative; font-size: calc(var(--s) * .38); font-weight: 600; letter-spacing: -.02em; line-height: 1; color: var(--fg); }
</style>
