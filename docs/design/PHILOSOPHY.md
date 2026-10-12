# Tinline design philosophy

What every screen, sound and motion in Tinline should follow, on Android and desktop alike.
Written 2026-10-12 from the decisions made while building the design playground
(`playground/`, decisions log in `playground/PLAN.md` §7). The playground is where these are
tried first; the apps follow.

## 1. Calm over loud
Soft colour, quiet motion, gentle sound. No rings around avatars, no flashing badges. Status is
shown subtly: the logo's string sways while ringing and pulls taut when connected; the call
screen breathes with clouds that move with the tune.

## 2. Honest, never over-claiming
Copy says only what the core really does: hosts can read posts; unlinking is not a lock;
deleting a picture is a request, not an erase; "Saved", never "delivered to everyone"; no
anonymity promise. Anything not decided is tagged **Proposed** (in the playground's drawer),
never shown as product copy.

## 3. Nothing moves unless you moved it
Text appearing, changing or disappearing never shifts the layout: reserved space, in-place
cross-fades, tabular numbers, one-line truncation. Things you did yourself (opening a sheet,
sending a message) may move; text-driven changes may not. Verified with layout-shift
measurement, not just by eye.

## 4. The little things are the design
Spacing, alignment, optical balance, easing, cue timing, colour mixing, text weight, corner
curves. Every change is looked at closely in light and dark, at true size and zoomed.

## 5. One geometry, defined once
Curves are mathematics, not per-platform guesses. Shapes (bubble corners, cards, the logo) come
from one parameter set and one construction (Bézier curves with defined corner smoothing; the
logo's string keeps a fixed length), written down so the playground, Android (Compose) and desktop
(iced) draw the same thing. Bubbles: 18 outer, 6 tight on the stacking side, grouped
single / first / middle / last, mirrored for mine and theirs. See `playground/src/lib/geom/`.

## 6. Physical, maths-driven motion
Motion is driven by models, not arbitrary keyframes: springs that retarget mid-flight, a string
that conserves its length, a pendulum sway locked to the ring tone, clouds that move with the
notes (OKLab colour mixing). Reduced motion jumps to each state's end pose.

## 7. One source of truth, everything in sync
One store, one set of motion tokens, one sound bus. Sound, logo and call background read the
same note schedule, so you hear, see and feel the same moment (the string goes taut on the
connect cue's landing note).

## 8. Balance and symmetry
Focus screens (calls, welcome, empty states) are mirror-balanced on a centre line. Controls come
in odd numbers so the main action sits on the axis; pairs mirror each other (Decline / Answer,
mine / theirs, can / can). Lists and headers follow platform convention instead.

## 9. Depth is quiet
Things at rest are flat, separated by hairlines. Elevation is a short ladder: flat → lift
(touchable raised controls) → pop (menus, banners) → sheet (dialogs). Shadows are two-layer
(contact + ambient) and tinted with the palette, never plain black grey. In dark mode depth comes
mostly from lighter surfaces; the shadow only helps.

## 10. Visible and fair
When something happens to you (a block, a flag, a removal, a declined request), you are told,
with who and when.

## 11. Respect people
Reduced motion and Quiet are honoured; 44 px touch targets; contrast ≥ 4.5:1; screen-reader
labels; right-to-left works. No tracking, no ads, no phone number.

## 12. One app, shaped to each device
The same product everywhere, adapted to the hand and the desk: thumb-reach search at the bottom
on Android, Ctrl+K and side panes on desktop.
