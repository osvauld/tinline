/* Single source of truth for every curved outline in the app.
   roundedRectPath builds a rounded rectangle from straight lines and cubic Béziers only (no arc commands, so it ports 1:1 to
   Compose Path.cubicTo and iced canvas::Path bezier_curve_to). Corners use the Figma "corner smoothing" construction:
   smoothing 0 is a circular arc (cubic, kappa 0.5523); 0.6 is the iOS-like continuous-curvature look. The maths is written up in GEOMETRY.md.
   Pure functions, no DOM. Radii are physical [topLeft, topRight, bottomRight, bottomLeft]; callers flip for RTL. */

export type Radii = [tl: number, tr: number, br: number, bl: number];

const RAD = Math.PI / 180;
const r2 = (n: number) => (Math.round(n * 100) / 100) + 0;   // round to 0.01; "+ 0" turns -0 into 0
const fmt = (n: number) => String(r2(n));

export type Corner = { r: number; s: number; p: number; a: number; b: number; c: number; d: number; arc: number; alpha: number; h: number };

/* Segment lengths of one corner for radius r, smoothing s and an extent budget p_max along each side (all lengths in px). */
export function cornerParams(radius: number, smoothing: number, budget = Infinity): Corner {
  let r = Math.max(0, radius);
  let s = Math.min(1, Math.max(0, smoothing));
  if (r === 0) return { r: 0, s: 0, p: 0, a: 0, b: 0, c: 0, d: 0, arc: 0, alpha: 0, h: 0 };
  if ((1 + s) * r > budget) {
    s = Math.min(s, Math.max(0, budget / r - 1));   // 1. degrade smoothing first
    if (r > budget) { r = budget; s = 0; }          // 2. then the radius itself
  }
  const p = (1 + s) * r;                             // extent of the corner along each side
  const arcMeasure = 90 * (1 - s);                   // degrees of true circle left in the middle
  const arc = Math.sin((arcMeasure / 2) * RAD) * r * Math.SQRT2;   // chord legs of that arc: (arc, arc)
  const alpha = (45 * s) * RAD;                      // tangent angle where the arc starts = angle where the transition ends
  const p3p4 = r * Math.tan(((90 - arcMeasure) / 4) * RAD);
  const c = p3p4 * Math.cos(alpha);
  const d = c * Math.tan(alpha);
  const b = (p - arc - c - d) / 3;
  const a = 2 * b;
  const h = (4 / 3) * Math.tan((arcMeasure / 4) * RAD) * r;   // cubic handle length for the circular section
  return { r, s, p, a, b, c, d, arc, alpha, h };
}

/* Extent budgets: per corner, how far along each side it may reach. Where two neighbours would overlap, the side is split in
   proportion to their radii, so the result is symmetric. A corner takes the tighter of its two sides. */
function budgets(w: number, h: number, [tl, tr, br, bl]: Radii, s: number): number[] {
  const ext = (r: number) => (1 + s) * r;
  const side = (len: number, ra: number, rb: number): [number, number] => {
    const need = ext(ra) + ext(rb);
    if (need <= len || need === 0) return [Infinity, Infinity];
    return [(len * ra) / (ra + rb), (len * rb) / (ra + rb)];
  };
  const top = side(w, tl, tr), right = side(h, tr, br), bottom = side(w, br, bl), left = side(h, bl, tl);
  return [Math.min(top[0], left[1]), Math.min(top[1], right[0]), Math.min(right[1], bottom[0]), Math.min(bottom[1], left[0])];
}

/* Emits one corner as cubics. `S` is the start point on the incoming side, din the unit direction along it, dout the unit
   direction of the outgoing side; local (u along din, v along dout). Returns the end point. */
function corner(out: string[], S: [number, number], din: [number, number], dout: [number, number], k: Corner): [number, number] {
  const pt = (u: number, v: number): string => `${fmt(S[0] + din[0] * u + dout[0] * v)} ${fmt(S[1] + din[1] * u + dout[1] * v)}`;
  const { a, b, c, d, arc, alpha, h, p } = k;
  const e1: [number, number] = [a + b + c, d];                 // end of first Bézier = start of circular section
  const e2: [number, number] = [e1[0] + arc, e1[1] + arc];      // end of circular section
  // first transition Bézier (absent when smoothing is 0)
  if (a + b + c + d > 1e-9) out.push(`C${pt(a, 0)} ${pt(a + b, 0)} ${pt(e1[0], e1[1])}`);
  // circular section as one cubic (skipped when it degenerates to a point, i.e. smoothing 1)
  if (arc > 1e-9) {
    const ca = Math.cos(alpha), sa = Math.sin(alpha);
    out.push(`C${pt(e1[0] + h * ca, e1[1] + h * sa)} ${pt(e2[0] - h * sa, e2[1] - h * ca)} ${pt(e2[0], e2[1])}`);
  }
  // second transition Bézier
  if (a + b + c + d > 1e-9) out.push(`C${pt(e2[0] + d, e2[1] + c)} ${pt(e2[0] + d, e2[1] + b + c)} ${pt(p, p)}`);
  return [S[0] + din[0] * p + dout[0] * p, S[1] + din[1] * p + dout[1] * p];
}

/* The outline of a w×h box, with its top-left at (x, y). Clockwise from the top edge. */
export function roundedRectPath(w: number, h: number, radii: Radii, smoothing = 0, x = 0, y = 0): string {
  w = Math.max(0, w); h = Math.max(0, h);
  const bud = budgets(w, h, radii, Math.min(1, Math.max(0, smoothing)));
  const [tl, tr, br, bl] = radii.map((r, i) => cornerParams(Math.min(r, w / 2, h / 2), smoothing, bud[i])) as [Corner, Corner, Corner, Corner];
  const out: string[] = [`M${fmt(x + tl.p)} ${fmt(y)}`];
  let cur = [x + tl.p, y];
  const line = (lx: number, ly: number) => { if (r2(lx) !== r2(cur[0]) || r2(ly) !== r2(cur[1])) out.push(`L${fmt(lx)} ${fmt(ly)}`); cur = [lx, ly]; };
  line(x + w - tr.p, y);
  if (tr.r) cur = corner(out, [x + w - tr.p, y], [1, 0], [0, 1], tr);
  line(x + w, y + h - br.p);
  if (br.r) cur = corner(out, [x + w, y + h - br.p], [0, 1], [-1, 0], br);
  line(x + bl.p, y + h);
  if (bl.r) cur = corner(out, [x + bl.p, y + h], [-1, 0], [0, -1], bl);
  line(x, y + tl.p);
  if (tl.r) cur = corner(out, [x, y + tl.p], [0, -1], [1, 0], tl);
  out.push('Z');   // closes with the last point equal to the start; sharp corners (r = 0, p = 0) need no segments
  return out.join('');
}

/* The same shape shrunk by `inset` px on every side (negative = grown), keeping the concentric ring property:
   radius - inset with the same smoothing. Coordinates stay in the ORIGINAL box's space (the path is translated by inset),
   so for a ring around a w×h box use inset -3 and an overlay that starts 3 px outside it. */
export function insetPath(w: number, h: number, radii: Radii, smoothing: number, inset: number): string {
  return roundedRectPath(w - 2 * inset, h - 2 * inset, radii.map((r) => Math.max(0, r - inset)) as Radii, smoothing, inset, inset);
}

/* Bubble corner radii. `side` is the side of the bubble that faces the screen edge it sits against ('right' for my messages,
   'left' for theirs); the stack-facing corners on that side go tight depending on position in the run. RTL: callers swap side. */
export type Pos = 'solo' | 'first' | 'mid' | 'last';
export function bubbleRadii(pos: Pos, side: 'left' | 'right', outer: number, tight: number): Radii {
  const top = pos === 'mid' || pos === 'last' ? tight : outer;
  const bot = pos === 'mid' || pos === 'first' ? tight : outer;
  return side === 'right' ? [outer, top, bot, outer] : [top, outer, outer, bot];
}
