# Audio motion alignment with design/refinement — read-only audit

User asked to check current design refinement and its philosophy. Inspected sibling
`.claude/worktrees/design-refinement` read-only; its uncommitted group-call work is
intentional and was neither edited nor discarded. This audit is owned by the audio
simulation coordinating session in `bench/group-audio-capacity`.

## Source of truth

- `docs/design/PHILOSOPHY.md` (all 12 principles).
- `docs/design/playground/PLAN.md` (binding principles and §7 user decisions).
- `docs/design/playground/README.md`.
- `src/lib/calls/{CallScreen,GroupCallScreen}.svelte`, relative to playground.
- `src/lib/calls/field/{model.ts,CloudField.svelte,gl.ts}` and `calls/cues.ts`.
- Group field-state/roster/speaking simulation in `calls/gcall.svelte.ts`.

My earlier search of production call screens missed the sibling Svelte design
playground. The user was correct: the cloud exists and its use is already decided.

## Current direction, not a newly proposed default

One coherent Svelte/TS app playground with one Simulation drawer, shared data,
sound bus and motion tokens; the native apps follow that design. Avoid divergent
standalone design screens. The native Rust audio viewer is a DSP/motion validation
tool, not a replacement for the call UI or a new product direction.

Call screens use a full-screen cloud colour field, no avatar ring. The existing
live texture choices are **Fog** (soft haze, default) and **Clouds** (puffier lit
forms), with a drawer toggle and selection after review. Particles/contour plots
from our audio lab remain experiments only, not approved replacements.

Both 1:1 and WIP group call screens use the same CloudField component. 1:1 anchors
connection attraction at the avatar; groups at the header, with roster and host
line. Identity, timer and controls sit over the field. Their reserved rows,
stacked cross-fades, truncation and tabular numbers protect layout. Group WIP
currently uses soft speaking glow + 4% avatar scale, not rings; revisit any lift
in context rather than silently removing/changing that agent's work.

The model has five Gaussian colour masses on continuous Lissajous paths, eased
state parameters, domain warp, bounded coverage and OKLab teal/gold mixing
(dark mode retuned to teal/amber). Incoming/outgoing/connecting/active/waiting/
reconnecting/end/failure are separate states. Known synthesized cue notes drive
band energy and colour; sound, logo and background share cue timestamps/patterns.
Quiet mode suppresses sound without disabling the state visuals.

Native implementation remains Iced/Compose. Port documented parameters and
mathematical construction, not browser rendering infrastructure or platform-
specific approximations chosen independently. Audit existing native facilities
before selecting the field drawing adapter; no renderer migration is approved.

## Concrete integration gaps found by code inspection

1. Active voice uses `makeVoice(seed)` fake bursts, not captured/planned Opus
   playout descriptors. The FFT/microphone experiment supplies a tested input
   path to replace that simulation source; it is not already integrated.
2. Group roster speaking simulation and CloudField voice simulation are separate.
   GroupCallScreen passes state/muted, not audible per-source levels/spectra.
   A shared per-source observation stream must drive both attribution and field,
   with bounded aggregation and only eligible/audible sources.
3. The field receives local mute as a global fake-voice gain. Real group/1:1
   integration must suppress local mic contribution, NOT remote playout merely
   because the local user is muted.
4. Group screen supplies three calm patches (head, host, controls), but WebGL
   renderer uniform/loop supports two and uses `slice(0,2)`; the controls' patch
   is omitted there. CSS fallback handles all. Contrast review must catch this.
5. Reduced motion differs: playground uses a fixed spatial gradient with per-note
   colour crossfades; our lab freezes existing geometry and changes opacity.
   Reconcile behavior explicitly with the current policy before native integration.
6. CloudField's header comment says CSS/no WebGL, but actual gl.ts uses WebGL1
   with CSS fallback. Use implementation truth, not that stale comment. Browser
   rendering/700k pixel cap does not establish Android/native battery suitability.

Spectral centroid is not pitch: do not feed it into the synth-note pitch map and
claim a detected musical note. Retain exact cue-note scheduling; use measured
band energy/centroid/flux as bounded speech texture/swell/colour inputs with their
own smoothing. Muted/stale/end/roster/recovery gates remain explicit.

## Proposed next work (not performed here)

Keep existing Fog/Clouds baseline and tune its response using the validated audio
simulation and richer features. Compare in the actual unified 1:1/group screen
layouts, including silence, muted local + audible remote, overlap, speaker changes,
reconnect, host loss, roster changes, reduced motion, Quiet and both themes.

Coordinate with the design-refinement lead before shared-file edits; group WIP
is uncommitted and not ours to integrate. Any contour texture/particle alternative
must be explicitly Proposed in the drawer. Do not stack effects by default.

Preserve calmness, centre-line balance, stable text/controls, quiet depth, 44px
controls, contrast/readability, RTL and screen-reader labels. Then native adapters
use the same field/math/input contract. The microphone viewer is not the final
call-screen composition and is not a production call observation API.
