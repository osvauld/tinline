# Corner geometry: one construction for web, Compose and iced

Every curved outline in the app is a path of **lines and cubic Béziers** built from `(w, h, radii[tl,tr,br,bl], smoothing)`.
Source of truth: `shape.ts` (`roundedRectPath`, `insetPath`, `bubbleRadii`). Apps port the function, not CSS `border-radius`.
Smoothing `s` is 0..1: 0 = circular corner, 0.6 = iOS-like continuous curvature. Default token `--smooth: .6`.

## Construction (one corner, radius r, smoothing s, side budget B)

1. **Clamp.** `p = (1+s)·r` is how far the corner reaches along each side. If `p > B` (B = the corner's share of the side, below):
   first lower smoothing, `s = clamp(B/r − 1, 0, s)`; if still `r > B`, set `r = B`, `s = 0`. Also `r ≤ min(w,h)/2`.
2. **Side budgets.** For each side with corners i, j: if `p_i + p_j ≤ side` both are free; otherwise split in proportion to radii,
   `B_i = side · r_i / (r_i + r_j)`. A corner uses the smaller budget of its two sides. This is symmetric (same result mirrored).
3. **Segment lengths** (all px):
   - `p = (1+s)·r`
   - `arcMeasure = 90°·(1−s)` (the part that stays a true circle)
   - `arc = sin(arcMeasure/2)·r·√2` (the circular section's chord runs `(arc, arc)`)
   - `α = 45°·s` (tangent angle where the transition meets the arc)
   - `p3p4 = r·tan((90° − arcMeasure)/4)`; `c = p3p4·cos α`; `d = c·tan α`
   - `b = (p − arc − c − d) / 3`; `a = 2b`
4. **Corner path.** Local frame: origin at the corner's start point on the incoming side, u along the incoming side, v along the
   outgoing side (turning clockwise). Three cubics:
   - B1: `(0,0) → c1 (a,0), c2 (a+b,0), e1 (a+b+c, d)`
   - Arc (circular, one cubic): from `e1` to `e2 = e1 + (arc, arc)`; handle `h = 4/3·tan(arcMeasure/4)·r` (= 0.5523·r at s=0);
     `c1 = e1 + h·(cos α, sin α)`, `c2 = e2 − h·(sin α, cos α)`
   - B2: `e2 → c1 (e2.x + d, e2.y + c), c2 (e2.x + d, e2.y + b + c), end (p, p)`
   B1 is skipped when `a+b+c+d = 0` (s = 0), and the arc cubic is skipped when `arc = 0` (s = 1).
5. **Whole outline.** Start at `(x + tl.p, y)`, clockwise: line to `(x+w−tr.p, y)`, TR corner (in = +x, out = +y), line down, BR corner
   (in = +y, out = −x), line left, BL corner (in = −x, out = −y), line up, TL corner (in = −y, out = +x), close. A line is omitted when
   zero length; a corner with r = 0 emits nothing. Output rounded to 0.01. No SVG `A` commands.
6. **Concentric rings.** `insetPath(w,h,radii,s,i)` = same function on `(w−2i, h−2i)` with `r − i` (floored at 0), same `s`, translated by `(i,i)`.
   A ring `k` px outside a shape is `i = −k`. Radii grow by exactly k, so offset curves stay parallel to the eye.

## Parameters (tokens.css)

| Use | radius | notes |
|---|---|---|
| Bubble outer | 18 | `--r-bub` |
| Bubble stack-facing corner | 6 | `--r-bub-tight` |
| Quote block / reply chip | 12 | `--r-quote` |
| Composer pill | 22 | `--r-field` (wrapper; ring = +3) |
| Cards, list rows, inputs, buttons | 14 | `--r-ctl` |
| Sheets, dialogs | 24 | `--r-sheet` (not applied in the playground: outer shadows) |
| Bubble found ring | outline r + 2..4 | concentric |
| Smoothing | 0.6 | `--smooth`, 0..1 |

Pills (buttons 44 px high, radius 22 = half height) clamp to a circle end-cap automatically: budget = h/2 kills the smoothing.
Practically, smoothing only shows on corners with room: a one-line bubble (~37 px, r 18) is almost a pill, so its smoothing degrades to ~0.03.

## Grouping rule

Position in a run of same-sender messages: `solo | first | mid | last`. Only the corners on the side facing the stack's screen edge
change (my messages: right side; theirs: left side). Outer = 18, tight = 6.

| pos | top (edge side) | bottom (edge side) |
|---|---|---|
| solo | 18 | 18 |
| first | 18 | 6 |
| mid | 6 | 6 |
| last | 6 | 18 |

The two corners on the far side are always 18. `bubbleRadii(pos, side, outer, tight)` returns `[tl, tr, br, bl]`; for RTL swap `side`.
Emoji-only messages have no surface. Call records are `solo` pills.

## Sample outputs (check ports against these, exact strings from `roundedRectPath`)

`120×40, [18,18,6,18], s=0.6` (left side budget 20, so TL/BL degrade to s≈0.11):

    M20 0L91.2 0C101.28 0 106.32 0 110.17 1.96C113.56 3.69 116.31 6.44 118.04 9.83C120 13.68 120 18.72 120 28.8L120 30.4C120 33.76 120 35.44 119.35 36.72C118.77 37.85 117.85 38.77 116.72 39.35C115.44 40 113.76 40 110.4 40L20 40C18.14 40 17.21 40 16.43 39.93C7.73 39.17 0.83 32.27 0.07 23.57C0 22.79 0 21.86 0 20C0 18.14 0 17.21 0.07 16.43C0.83 7.73 7.73 0.83 16.43 0.07C17.21 0 18.14 0 20 0Z

`120×60, [18,18,6,18], s=0.6` (no clamping; full smoothing; TR p = 28.8, BR p = 9.6):

    M28.8 0L91.2 0C101.28 0 106.32 0 110.17 1.96C113.56 3.69 116.31 6.44 118.04 9.83C120 13.68 120 18.72 120 28.8L120 50.4C120 53.76 120 55.44 119.35 56.72C118.77 57.85 117.85 58.77 116.72 59.35C115.44 60 113.76 60 110.4 60L28.8 60C18.72 60 13.68 60 9.83 58.04C6.44 56.31 3.69 53.56 1.96 50.17C0 46.32 0 41.28 0 31.2L0 28.8C0 18.72 0 13.68 1.96 9.83C3.69 6.44 6.44 3.69 9.83 1.96C13.68 0 18.72 0 28.8 0Z

`120×60, [18,18,6,18], s=0` (circular, kappa 0.5523·18 = 9.94):

    M18 0L102 0C111.94 0 120 8.06 120 18L120 54C120 57.31 117.31 60 114 60L18 60C8.06 60 0 51.94 0 42L0 18C0 8.06 8.06 0 18 0Z

`insetPath(120,60,[18,18,6,18],0.6,-3)` (ring 3 px out, radii 21/21/9/21):

    M30 -3L89.4 -3C101.16 -3 107.04 -3 111.53 -0.71C115.49 1.3 118.7 4.51 120.71 8.47C123 12.96 123 18.84 123 30.6L123 48.6C123 53.64 123 56.16 122.02 58.09C121.16 59.78 119.78 61.16 118.09 62.02C116.16 63 113.64 63 108.6 63L30 63C18.8 63 13.21 63 8.89 60.92C4.6 58.86 1.14 55.4 -0.92 51.11C-3 46.79 -3 41.2 -3 30C-3 18.8 -3 13.21 -0.92 8.89C1.14 4.6 4.6 1.14 8.89 -0.92C13.21 -3 18.8 -3 30 -3Z

Tolerance for a port: each coordinate within 0.01 px (compare the numbers, not the rendering).

## Porting notes

**Jetpack Compose.** Write a `Shape` (`createOutline(size, layoutDirection, density)`) that builds an `androidx.compose.ui.graphics.Path`
with `moveTo/lineTo/cubicTo/close` from the same function, radii converted `dp → px`, and `layoutDirection == Rtl` swapping the
`side` argument. This is the exact construction. Do not use `RoundedCornerShape` for bubbles (circular only).
`androidx.graphics.shapes` `RoundedPolygon` with `CornerRounding(radius, smoothing)` is also a smoothed corner, but it is a different construction (its own flanking cubics and its own clamping rule; not verified against this document). Expect sub-pixel to ~1 px differences versus this document; use it only if matching exactly doesn't matter,
and verify against the sample strings above. Per-corner different radii work (`perVertexRounding`), but with 4 vertices only.
Stroke/ring: `Modifier.border(w, color, shape)` takes a `Shape`, so it follows the outline; a ring at +k is a second `Shape`
with the inset path (`insetPath(.., -k)`) drawn with `drawBehind`. Clip with `Modifier.clip(shape)`.

**iced.** Build `canvas::Path::new(|b| { b.move_to; b.line_to; b.bezier_curve_to(c1, c2, to); b.close() })` from the same function
(port to Rust; the output is only lines and cubics). Fill with `frame.fill(&path, color)` and stroke with `frame.stroke`
(width centered on the path, so inset the path by width/2 as the web overlay does). A bubble needs its size, so draw it in a
`canvas::Program` sized by the layout, or pre-render SVG through the existing resvg path (`<path d="…"/>`; the strings above are valid SVG).
`Border { radius }` stays circular, so do not use it for bubbles.

## What differs between this web rendering and a native one

- Web uses `clip-path: path()` for the surface (anti-aliased by the compositor) and an SVG path for hairlines and rings. Native draws
  the filled path directly, so the edge pixels can differ by AA only.
- A CSS `border-inline-start` accent (quote bar) is clipped by the shape; natively, draw the bar as its own clipped rect.
- Shadows are not applied to shaped elements on web (clip-path cuts them); sheets and popovers keep CSS radius for that reason.
  Natively, `Modifier.shadow(shape)` / an iced shadow of the same path works.
