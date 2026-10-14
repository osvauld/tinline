# Tinline mark D — parametric model (source of truth for drawing AND animation)

Coordinates: 24×24 viewBox, symmetry axis x = 12, centre line y = 12. y grows downward.
Everything is a function of a small parameter set; static icons are just this model evaluated at rest.
The right can is always the mirror of the left: x ↦ 24 − x. Never draw it separately.

## 1. Can (left; mouth faces the axis)

A cylinder of radius b seen at view angle θ from its axis, so circles foreshorten to ellipses with
horizontal semi-axis a = b·sin θ.

| param | meaning | rest value (24 grid) |
|---|---|---|
| c | x of mouth centre | 7.5 |
| b | can radius (vertical semi-axis of rim) | 5.5 |
| θ | view angle | 30.6° → a = b sin θ = 2.8 |
| μ | mouth-hole ratio (inner ellipse = rim scaled by μ, concentric) | 0.62 |
| L | projected body length behind the rim | 3.0 |

Shapes (left can):
- Body: rect x ∈ [c − L, c], y ∈ [12 − b, 12 + b], union the back cap = left half of ellipse
  E(c − L, 12; a, b). (Same a, b as the rim, a true cylinder: the board's D used rx 3 vs 2.8; fix it.)
- Rim: ellipse E(c, 12; a, b), fill = rim colour.
- Mouth: ellipse E(c, 12; μa, μb), fill = mouth colour (the can's inside base).
- Knot: string anchor K = (c, 12), the centre of the mouth.

Extents: leftmost x = c − L − a; height 2b. Glyph bounding box is used for launcher scaling.

## 2. String

Anchors K₁ = (c, 12), K₂ = (24 − c, 12). Span w = 24 − 2c.
Drawn ALWAYS as one quadratic Bézier with identical commands in every state:

    M c 12  Q (12 + 2s·sin φ) (12 + 2s·cos φ)  (24 − c) 12

The midpoint of a quadratic Bézier is (K₁ + 2C + K₂)/4, so with control point C at offset 2s the
string's midpoint sags by exactly s. A quadratic Bézier IS a parabola, which matches the small-sag
catenary well (error < 1% of sag for s/w < 0.25).

- s: sag at the midpoint (s = 0 → taut, a straight line still written as Q).
- φ: swing angle of the slack string (0 = hanging straight down).

### Length conservation (what makes it physical)

The string has fixed length ℓ. Parabolic arc length (small-sag form):

    ℓ ≈ w + 8s² / (3w)        ⇒      s(w) = √( 3w(ℓ − w) / 8 )   for w < ℓ,   s = 0 for w ≥ ℓ

Choose ℓ so the rest pose has the designed sag: with w₀ = 9, s₀ = 3.5 → ℓ = 9 + 8·3.5²/27 ≈ 12.63.
(Use the exact parabola length with a 3-step Newton solve if s/w > 0.3.)

So the string is never animated directly between two drawings. It is DERIVED from the can
separation. Pull the cans apart and the sag falls; at w = ℓ it is taut.

## 3. Motion (all as functions of time; one state = {c, φ, v})

Let Δ be the outward can displacement: c(t) = c₀ − Δ(t) (right can mirrors automatically).
Taut separation: Δ_T = (ℓ − w₀)/2 ≈ 1.82 units. (If that's too much travel for the icon, shorten ℓ
toward w₀: the travel is always (ℓ − w₀)/2; tune ℓ and s₀ together.)

1. **Ringing / connecting (slack sway):** Δ = 0, s = s₀,
   φ(t) = φ₀ sin(2πt / T), φ₀ = 12°, T = 1.6 s (pendulum-like; can be phase-locked to the ring cue).
2. **Connect (pull taut):** Δ springs 0 → Δ_T, critically damped (ζ = 1, ω ≈ 2π·3 Hz), so ~200 ms.
   **Recoil (2026-10-13):** the pull now arrives with speed (target 1.6·Δ_T, `hitOver`), and when the string
   goes tight the cans bounce back (velocity reflected, restitution `recoil` 0.5), then a soft 2 Hz pull
   (`settleHz`, undamped) carries them out again. Each bounce re-plucks the string a little; the `hit` event
   (and its cue) fires once. Bounces stop below `recoilMin` 1.2 units/s. Measured at 96px: about 1.7px, then
   0.9px, then 0.4px, settled about 770ms after connect. `recoil: 0` restores the old dead stop.
   **Windup (optional, `windup` units over `windupMs`):** before the pull the cans gather inward (Δ goes
   briefly negative, so the string sags a little deeper), then the pull starts from there. Default 0 (off).
   **Tugs (`tugs` n, event `tug`):** with n > 0 the string tugs exactly n times, each a full twang, then rests.
   Done uses `{tugs 2, recoil 0.95, recoilMin 0, settleHz 1/(2·gap)}`, where gap is the `line` cue's
   second-note offset (0.12s): the line gives ≈1.6px at 96 between the tugs, which land ~130ms apart.
   (An earlier long four-bounce version with windup read as artificial; windup stays available, default off.)
   s follows from length conservation, and φ eases to 0 over the same spring.
   The string can't stretch, so Δ is clamped at Δ_T. The moment Δ reaches Δ_T is the event `hit`
   (sync the `connected` cue's second note to it).
3. **Twang (at hit, and for "pluck" on message sent while taut):** a taut string vibrates both ways,
   approximately in its fundamental mode (half-sine ≈ the same parabola), so s may go negative here:
   s(t) = A e^{−t/τ} sin(2π f t), A = 0.9, f = 7 Hz, τ = 110 ms; done when |A e^{−t/τ}| < 0.03 (~370 ms).
4. **End (release):** Δ springs Δ_T → 0, overdamped (ζ = 1.6), ~260 ms; s grows back via conservation,
   with a small swing φ settling from ±6°. Event `settled` when |Δ| < 0.01.
5. **Reduced motion:** jump to the end state of each step; no sway, no twang. State is still
   shown (slack vs taut).
6. **Mid-flight interruption:** every step retargets from the current {Δ, Δ̇, φ}, never restarts.

## 4. Size tiers (final, 2026-10-12; source: tiers.js, regenerated by docs/design/brand/generate.mjs)

Optical sizes, not a uniform scale. Tiers below 32 are designed IN PIXELS (1 model unit = 1 px of the tier's own
box, so every flat edge lies on a whole pixel and rims/mouth on half pixels). Tiers 32..64 scale the base design and
snap b to whole px and the rest to 0.5 px; 96+ are exact. All come from markPaths; only parameters differ.

| box px | c | a | b | L | mu | mouth | string | rest sag | taut |
|---|---|---|---|---|---|---|---|---|---|
| 12 | 3.5 | 1.5 | 4 | 1.5 | .50 | flat 2-colour, string starts ko .75 | 1.5 | 2.5 | sag 0.5, cans fixed |
| 14 | 4.5 | 1.5 | 5 | 2 | .50 | flat, ko .75 | 1.5 | 3 | sag 0.5 |
| 16 | 5 | 2 | 5 | 2 | .50 | flat, ko 1.25 | 1.75 | 3 | sag 0.5 |
| 20 | 6 | 2.5 | 6 | 2.5 | .50 | flat, ko 1.25 | 1.75 | 3.5 | sag 0.75 |
| 24 | 7.5 | 3 | 6 | 3 | .60 | mint | 1.75 | 3.5 | full model |
| 28 | 8.5 | 3.5 | 7 | 3.5 | .60 | mint | 1.75 | 4 | full model |
| 32 | 10 | 3.5 | 7 | 4 | .62 | mint | 2 | 4.7 | full model |
| 40, 48, 64 | base x B/24 (snapped) | | | | .62 | mint | 2.5 / 3 / 4 | 5.8 / 7 / 9.3 | full model |
| 96..1024 | exact | | | | .62 | mint | 6 .. 64 | s0 x B/24 | full model |

All tiers share one silhouette: a rounded rim plus a rounded back (back cap kept at every size), mouth ~1/3 of the can
width from the axis side. Revised after lead review: the earlier straight-sided "slot" cans at <= 20 px were dropped.
Flat mouth (<= 20): cream hole (teal ground in tiles, deep teal on dark); string anchor offset ko starts it on the rim beside
the hole so no gold fills the mouth.

Decisions:
- Small tiers have bigger cans relative to the box (b 4/12 .. 7/28 vs 5.5/24 at base) but the same oval silhouette.
- The mouth is kept at every size (it says "can"); at 12-20 px it is a plain 1.5-3 px two-colour hole.
- Taut at <= 20 px is SAG ONLY (cans never move): the string flattens to 0.5-0.75 px of sag. The 16-px candidate
  from section 6 (b 6, theta 36) was tried and rejected: two ovals; b 5 with a straight slug reads better.
  Mark.svelte plays that state by easing the sag (tinmark attach: params.sagOnly).
- Mark.svelte calls tierParams(size) (nearest designed tier), so the top-bar 28 px mark uses the 28 tier,
  16-20 px uses sag-only taut. Animation at >= 24 px uses the full spring model on the tier geometry.
- Launcher (108, safe zone 66): the exact 1024 geometry scaled so the glyph is 54 units wide, centred.
- Monochrome: body + rim = ink, mouth = evenodd hole (omitted for solid white silhouettes).
- Notification icon: tier 20 centred in a 24 dp canvas (mouth cutout, alpha only).
- App tile (teal ground, cream cans): the mouth colour is the ground teal so it reads as a hole.

## 5. Implementation contract

One function, used for static icons, the launcher export and animation alike:

    markPaths({c, b, a, mu, L, s, phi, strokeW}) -> { canLeft, rim, mouth, string }   // SVG d strings
    TinMark.attach(svg) -> { connect(), end(), pluck(), ring(on), set(state), on('hit'|'settled') }

The animation writes only the `d` attribute (string) and a transform on the two can groups
(translate by ∓Δ). It uses requestAnimationFrame plus setAttribute, so it works in every engine and WebView.

## 6. Corrections after implementation (2026-10-12, verified by mark/verify.js)

1. **Use the exact parabola length**, not the small-sag form: at rest s/w ≈ 0.39, where the
   approximation is 6% off (string would stretch). Exact arc length of the Bézier/parabola,
   inverted by Newton/bisection for s(w). Gives ℓ = 11.87, can travel Δ_T = 1.43 per side, and the
   taut glyph stays inside the 24 box (left edge +0.27).
2. **Connect spring**: a critically damped spring never *reaches* its target, so `hit` would never
   fire. Spring toward 1.1·Δ_T and clamp at Δ_T. `hit` lands at ≈217 ms at 60 Hz.
3. s(w) has an infinite slope at taut, so the last frames before `hit` drop fast. Expected; it reads
   as the snap of the line going tight.
4. Twang amplitude A = 0.9 rebounds above the pre-hit sag; consider A ≈ 0.5.
5. `settled` waits for the swing φ as well as Δ.
6. Visual: 24 px and up reads as cans. At 16 px it's two ovals and sag vs taut barely differ;
   candidate 16-px tier b = 6, θ = 36°, sag only. Taut reads as a barbell at all sizes (open design issue).
Reference implementation: mark/mark.js (`lengthModel:'exact'`).
7. **Final tiering (see section 4).** Taut is still a barbell at 24+ (can-pull-in plus straight string); kept for in-app
   status because the contrast with slack is the point. Open: whether taut is needed at 16 (sag-only barely differs).
