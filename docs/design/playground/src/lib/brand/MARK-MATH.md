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

## 4. Size tiers (rendered px → parameter overrides)

Geometry is the same at every size. Only the stroke and the details change, at fixed thresholds:
- ≥ 32 px: full (rim + mouth + back cap), string stroke 1.5.
- 24 px: string stroke 1.6, μ = 0.58.
- 16 px: string stroke 1.75, mouth kept (it's the "can" cue), no back-cap overlap detail.
- Launcher (108, safe zone 66): scale so the glyph bbox fits a 60-unit-wide box centred at 54,54.
- Monochrome (Android themed): body + rim = ink, mouth = transparent hole (real cutout via
  fill-rule evenodd), string = ink.

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
