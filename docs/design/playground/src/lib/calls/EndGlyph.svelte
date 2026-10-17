<script lang="ts" module>
  /* The End button's glyph, drawn from the logo (lab: `?lab=endcall`). The apps' `callend` icon ("cans apart,
     strings dropped") reads as a headset, so these are the candidates (user, 2026-10-16: "the logo"):
       slack  the logo in white, its line tight while the call is up; pressing End lets the line go slack
       cut    the logo with its line cut in the middle; pressing End opens the cut
       slash  the logo with the shared 45° off-slash (mic off, camera off)
       scissors  just scissors, open; pressing End snips them shut (user: "just knife or scissors is fine")
       knife     just a kitchen knife; pressing End brings it down in a short chop
     Curvy, "cut the line" (user: "curvy... it need not be scissors or knife, cut is the idea"):
       curvy     scissors drawn soft: leaf blades, oval handles; pressing End snips them shut
       snip      only the logo's sagging line, cut at the bottom, the ends drooping; pressing End lets them fall
                 (user: "just remove the cans and just the lines cut")
       xsag      an X of two curved strokes that both sag, like two crossed strings (user: "lets also try x as curves");
                 pressing End lets them sag further
       xcut      the swirl X with one line cut where the other crosses it (user: "swirl but with one line cut"); pressing
                 End lets the cut line go slack: its sag grows and its halves fall back from the cut. One curve, one number.
       xswirl    an X of two curved strokes bowing the same way round the centre; pressing End pulls them nearly straight
       cutline   only a tight straight line, cut in the middle, the ends dipping; pressing End lets them fall
       swoosh    a tapered curved stroke through the sagging line, which parts where it crosses; End follows through
       phone  today's playground handset, tipped down (the apps' `hangup` since 2026-10-16)
     `?endicon=` picks one for the whole playground; without it the call screen uses `xcut` (chosen 2026-10-16; the
     apps' `hangup` / `hangup-go` from crates/tin-icons/src/cut.rs, same numbers). */
  export type EndVariant = 'xcut' | 'curvy' | 'snip' | 'cutline' | 'xsag' | 'xswirl' | 'swoosh' | 'slack' | 'cut' | 'scissors' | 'knife' | 'slash' | 'phone';
  export const END_VARIANTS: EndVariant[] = ['xcut', 'snip', 'xsag', 'xswirl', 'cutline', 'curvy', 'swoosh', 'slack', 'cut', 'scissors', 'knife', 'slash', 'phone'];
  const q = new URLSearchParams(location.search).get('endicon');
  export const endVariant: EndVariant = (END_VARIANTS as string[]).includes(q ?? '') ? (q as EndVariant) : 'xcut';
</script>

<script lang="ts">
  import Mark from '../brand/Mark.svelte';
  import Icon from '../ui/Icon.svelte';
  import { TinMark } from '../brand/tinmark.js';
  import { tierParams } from '../brand/tiers.js';

  let { variant = endVariant, size = 34, pressed = false }:
    { variant?: EndVariant; size?: number; /** End was pressed: play the line going down. */ pressed?: boolean } = $props();

  /* Cut and slash draw the logo's cans from its own geometry in the tight pose (cans apart, as in A while the call
     is up), so the line runs straight between them. Cut: the line broken in the middle, each end dipping; pressing End
     opens the gap and lets the ends fall. Slash: the whole line, with the off-slash over it. */
  const tp = $derived(tierParams(size));
  const DT = $derived(TinMark.resolve(tp).DT as number);
  const cans = $derived(TinMark.markPaths({ ...tp, s: 0, delta: DT }).mono as string);
  const mx = $derived((tp.c as number) - DT);
  const r = (n: number) => Math.round(n * 100) / 100;
  /** One end of the cut line: straight out of the can's mouth, then dipping `d` at the gap (half-width `g`). */
  function end(right: boolean, g: number, d: number) {
    const f = (x: number) => (right ? 24 - x : x), ex = 12 - g;
    return `M${r(f(mx))} 12Q${r(f(ex - 1.6))} 12 ${r(f(ex))} ${r(12 + d)}`;
  }
  /** One string as the mark draws it: a parabola from A to B that sags `sag` at its middle, along the unit normal n.
   *  P(u) = A + u(B - A) + 4·sag·u(1 - u)·n, u in [0, 1]; as a quadratic Bézier its control point is M + 2·sag·n
   *  (M = the chord's midpoint). `side` picks which normal: +1 = (-dy, dx)/w, -1 the other. */
  function parabola(ax: number, ay: number, bx: number, by: number, sag: number, side: 1 | -1) {
    const dx = bx - ax, dy = by - ay, w = Math.hypot(dx, dy), nx = (-dy / w) * side, ny = (dx / w) * side;
    return `M${ax} ${ay}Q${r((ax + bx) / 2 + 2 * sag * nx)} ${r((ay + by) / 2 + 2 * sag * ny)} ${bx} ${by}`;
  }
  /** The X: two strings corner to corner (chord 18.4). Sagging: both normals point down (mirror pair, like two crossed
   *  strings hanging). Swirl: both turn the same way round the centre (half-turn pair). Sag 0.13 of the chord at rest. */
  const X = 5.4, W = 24 - 2 * X;
  const xsag = (sag: number) => parabola(X, X, X + W, X + W, sag, 1) + parabola(X + W, X, X, X + W, sag, -1);
  const xswirl = (sag: number) => parabola(X, X, X + W, X + W, sag, -1) + parabola(X + W, X, X, X + W, sag, -1);
  /** The string's parabola as Bézier points: ends A, B and control C = M + 2·sag·n. */
  function strand(ax: number, ay: number, bx: number, by: number, sag: number, side: 1 | -1) {
    const dx = bx - ax, dy = by - ay, w = Math.hypot(dx, dy), nx = (-dy / w) * side, ny = (dx / w) * side;
    return { a: [ax, ay], c: [(ax + bx) / 2 + 2 * sag * nx, (ay + by) / 2 + 2 * sag * ny], b: [bx, by] };
  }
  type Strand = ReturnType<typeof strand>;
  const at = (q: Strand, u: number) => [0, 1].map((k) => (1 - u) ** 2 * q.a[k] + 2 * u * (1 - u) * q.c[k] + u * u * q.b[k]);
  /** The piece u0..u1 of a parabola is a parabola too: its ends are P(u0), P(u1); its control is the blossom
   *  (1-u0)(1-u1)A + ((1-u0)u1 + u0(1-u1))C + u0·u1·B. */
  function piece(q: Strand, u0: number, u1: number) {
    const p0 = at(q, u0), p1 = at(q, u1);
    const c = [0, 1].map((k) => (1 - u0) * (1 - u1) * q.a[k] + ((1 - u0) * u1 + u0 * (1 - u1)) * q.c[k] + u0 * u1 * q.b[k]);
    return `M${r(p0[0])} ${r(p0[1])}Q${r(c[0])} ${r(c[1])} ${r(p1[0])} ${r(p1[1])}`;
  }
  /** Where the two swirl strands cross, exactly. The cut strand is the whole one turned 90° about the centre, so with
   *  h = half the X, k = 4·sag/√2, a = 2u - 1 (whole) and b = 2v - 1 (cut), matching x+y and x-y gives
   *  1 - b² = c·a and 1 - a² = -c·b, c = 4h/k = √2·h/sag; so (a² - 1)² = c²(1 - c·a), solved by Newton from a = 0. */
  function crossing(sag: number) {
    const h = W / 2, c = (Math.SQRT2 * h) / sag;
    let a = 0;
    for (let i = 0; i < 8; i++) a -= ((a * a - 1) ** 2 - c * c + c ** 3 * a) / (4 * a * (a * a - 1) + c ** 3);
    return { u: (1 + a) / 2, v: (1 + (a * a - 1) / c) / 2 };
  }
  const tangent = (q: Strand, u: number) => [0, 1].map((k) => 2 * (1 - u) * (q.c[k] - q.a[k]) + 2 * u * (q.b[k] - q.c[k]));
  /** The swirl X, the TR-to-BL strand cut where the TL-to-BR strand crosses it. The cut is the whole strand's stroke plus
   *  `clear` on each side, measured across the cut strand, so it reads the same at any crossing angle θ:
   *  half-length along the cut strand = (stroke/2 + clear) / sin θ, turned into u by the strand's speed there.
   *  End: the cut strand goes slack (its sag grows) and the clearance opens, so the halves fall back from the cut. */
  function xcut(cutSag: number, clear: number) {
    const SAG = 2.4, SW = 1.75;  // the icon set's stroke (crates/tin-icons STROKE)
    const whole = strand(X, X, X + W, X + W, SAG, -1);
    const rest = strand(X + W, X, X, X + W, SAG, -1);
    const { u, v } = crossing(SAG);
    const tw = tangent(whole, u), tc = tangent(rest, v), speed = Math.hypot(tc[0], tc[1]);
    const sin = Math.abs(tw[0] * tc[1] - tw[1] * tc[0]) / (Math.hypot(tw[0], tw[1]) * speed);
    const du = (SW / 2 + clear) / sin / speed;
    const cut = strand(X + W, X, X, X + W, cutSag, -1);
    return { whole: piece(whole, 0, 1), a: piece(cut, 0, v - du), b: piece(cut, v + du, 1) };
  }
  const xc = $derived(xcut(pressed ? 4.2 : 2.4, pressed ? 3.8 : 2.6));
  const full = $derived(`M${r(mx)} 12H${r(24 - mx)}`);
  const sw = $derived(Math.max(1.6, (tp.strokeW as number) * 1.15));
</script>

{#if variant === 'phone'}
  <Icon name="phone" size={size - 2} class="hang" />
{:else}
  <span class="eg {variant}" class:pressed style:--s="{size}px">
    {#if variant === 'slack'}
      <Mark {size} live mono state={pressed ? 'slack' : 'taut'} />
    {:else if variant === 'scissors'}
      <!-- Two rigid halves about the pivot (12, 12): ring, handle, blade. Closed, the blades lie on one line; open, each half
           turns 13° (upper clockwise: ring up, blade down), so the blades cross. -->
      <svg width={size} height={size} viewBox="0 0 24 24" aria-hidden="true" class="tl">
        <g class="half up" class:shut={pressed}><circle cx="6.2" cy="7.4" r="2.7" /><path d="M8.32 9.08L12 12L21 12" /></g>
        <g class="half dn" class:shut={pressed}><circle cx="6.2" cy="16.6" r="2.7" /><path d="M8.32 14.92L12 12L21 12" /></g>
        <circle class="pin" cx="12" cy="12" r="0.95" />
      </svg>
    {:else if variant === 'curvy'}
      <!-- Soft scissors: each half is an oval handle, a curved shank to the pivot, and a leaf blade (two arcs meeting
           at the tip), turned about (12, 12) like the plain scissors. -->
      <svg width={size} height={size} viewBox="0 0 24 24" aria-hidden="true" class="tl">
        <g class="half up" class:shut={pressed}>
          <ellipse cx="6" cy="7.6" rx="2.9" ry="2.3" transform="rotate(28 6 7.6)" />
          <path d="M8.5 9.1Q10.6 10.1 12 12" />
          <path class="leaf" d="M11.6 12.1Q15.6 8.4 21.4 10.6Q17.2 13.6 11.6 12.1Z" />
        </g>
        <g class="half dn" class:shut={pressed}>
          <ellipse cx="6" cy="16.4" rx="2.9" ry="2.3" transform="rotate(-28 6 16.4)" />
          <path d="M8.5 14.9Q10.6 13.9 12 12" />
          <path class="leaf" d="M11.6 11.9Q15.6 15.6 21.4 13.4Q17.2 10.4 11.6 11.9Z" />
        </g>
        <circle class="pin" cx="12" cy="12" r="0.9" />
      </svg>
    {:else if variant === 'snip'}
      <!-- The logo's line alone: the sag M2.5 8 Q12 18 21.5 8 split at t = .42 (End: .36), each cut end dropped 1.2 (End: 3.2). -->
      <svg width={size} height={size} viewBox="0 0 24 24" aria-hidden="true" class="tl line">
        <path class="mv" style:d="path('{pressed ? 'M2.5 8Q5.92 11.6 9.34 15.8' : 'M2.5 8Q6.49 12.2 10.48 14.07'}')" />
        <path class="mv" style:d="path('{pressed ? 'M21.5 8Q18.08 11.6 14.66 15.8' : 'M21.5 8Q17.51 12.2 13.52 14.07'}')" />
      </svg>
    {:else if variant === 'xcut'}
      <svg width={size} height={size} viewBox="0 0 24 24" aria-hidden="true" class="tl line set">
        <path d={xc.whole} />
        <path class="mv give" style:d="path('{xc.a}')" />
        <path class="mv give" style:d="path('{xc.b}')" />
      </svg>
    {:else if variant === 'xsag'}
      <svg width={size} height={size} viewBox="0 0 24 24" aria-hidden="true" class="tl line">
        <path class="mv" style:d="path('{xsag(pressed ? 3.6 : 2.4)}')" />
      </svg>
    {:else if variant === 'xswirl'}
      <svg width={size} height={size} viewBox="0 0 24 24" aria-hidden="true" class="tl line">
        <path class="mv" style:d="path('{xswirl(pressed ? 0.6 : 2.4)}')" />
      </svg>
    {:else if variant === 'cutline'}
      <!-- A tight line alone, cut in the middle: each half runs straight, then dips at the cut. -->
      <svg width={size} height={size} viewBox="0 0 24 24" aria-hidden="true" class="tl line">
        <path class="mv" style:d="path('{pressed ? 'M2.5 11Q7.6 11 9.4 15.4' : 'M2.5 11Q8.6 11 10.6 12.9'}')" />
        <path class="mv" style:d="path('{pressed ? 'M21.5 11Q16.4 11 14.6 15.4' : 'M21.5 11Q15.4 11 13.4 12.9'}')" />
      </svg>
    {:else if variant === 'swoosh'}
      <!-- A tapered crescent stroke cuts the sagging line; a knockout in the button's colour parts the line there. -->
      <svg width={size} height={size} viewBox="0 0 24 24" aria-hidden="true" class="tl">
        <path class="mv" style:d="path('{pressed ? 'M2.6 8.6Q6.2 13.4 9.4 14.6' : 'M2.6 8.6Q6.6 13.6 10.6 14.8'}')" />
        <path class="mv" style:d="path('{pressed ? 'M21.4 8.6Q17.8 13.4 14.6 14.6' : 'M21.4 8.6Q17.4 13.6 13.4 14.8'}')" />
        <g class="sw" class:through={pressed}>
          <path class="crescent knock" d="M18.2 3.2Q10.2 9.4 7.2 20.8Q15.6 13.2 18.2 3.2Z" />
          <path class="crescent" d="M18.2 3.2Q10.2 9.4 7.2 20.8Q15.6 13.2 18.2 3.2Z" />
        </g>
      </svg>
    {:else if variant === 'knife'}
      <!-- A kitchen knife, handle low left, blade up right, edge underneath; End brings it down in a short chop. -->
      <svg width={size} height={size} viewBox="0 0 24 24" aria-hidden="true" class="tl">
        <g class="knife" class:chop={pressed}>
          <path class="handle" d="M4.2 19.8L7.6 16.4" />
          <path class="blade" d="M9 15.9L18.6 5.2Q20.9 3.2 20.3 6.3Q17.6 12.6 10.6 17.4Z" />
        </g>
      </svg>
    {:else}
      <svg width={size} height={size} viewBox={tp.viewBox} aria-hidden="true" style:--sw={sw}>
        <path class="cans" d={cans} /><path class="cans" d={cans} transform="matrix(-1 0 0 1 24 0)" />
        {#if variant === 'cut'}
          <path class="str" style:d="path('{end(false, pressed ? 2.6 : 1.6, pressed ? 4.4 : 2.1)}')" />
          <path class="str" style:d="path('{end(true, pressed ? 2.6 : 1.6, pressed ? 4.4 : 2.1)}')" />
        {:else}
          <path class="str" d={full} />
          <path class="slash gap" d="M4 3.5L20 20.5" /><path class="slash" d="M4 3.5L20 20.5" />
        {/if}
      </svg>
    {/if}
  </span>
{/if}

<style>
  .eg { position: relative; display: grid; place-items: center; inline-size: var(--s); block-size: var(--s); color: inherit; }
  .eg :global(.mark) { --tm-ink: currentColor; }
  .eg svg { overflow: visible; }
  .cans { fill: currentColor; fill-rule: evenodd; }
  .str, .slash { fill: none; stroke: currentColor; stroke-width: var(--sw); stroke-linecap: round; }
  .str { transition: d var(--dur-switch, 240ms) var(--ease-out); }
  .tl { overflow: visible; fill: none; stroke: currentColor; stroke-width: 1.85; stroke-linecap: round; stroke-linejoin: round; }
  .half { transform-origin: 12px 12px; transition: transform var(--dur-switch, 240ms) var(--ease-out); }
  .half.up { transform: rotate(13deg); }
  .half.dn { transform: rotate(-13deg); }
  .half.shut { transform: rotate(0deg); }
  .pin { fill: currentColor; stroke: none; }
  .tl.line { stroke-width: 2.1; }
  .tl.line.set { stroke-width: 1.75; }
  /* The cut strand lets go with a little overshoot, like the logo's line settling. */
  .mv.give { transition: d var(--dur-switch, 240ms) cubic-bezier(.34, 1.4, .64, 1); }
  .leaf { fill: currentColor; stroke-width: 1.1; }
  .mv { transition: d var(--dur-switch, 240ms) var(--ease-out); }
  .crescent { fill: currentColor; stroke: currentColor; stroke-width: 1.2; }
  .crescent.knock { stroke: var(--eg-bg, var(--danger-btn)); stroke-width: 4.2; fill: var(--eg-bg, var(--danger-btn)); }
  .sw { transition: transform var(--dur-switch, 240ms) var(--ease-out); }
  .sw.through { transform: translate(-1.2px, 1.6px); }
  .knife { transform-origin: 4px 20px; transition: transform var(--dur-switch, 240ms) var(--ease-out); }
  .knife.chop { transform: rotate(14deg); }
  .knife .handle { stroke-width: 3.3; }
  .knife .blade { fill: currentColor; stroke-width: 1; }
  .slash.gap { stroke: var(--danger-btn); stroke-width: calc(var(--sw) * 2.6); }
</style>
