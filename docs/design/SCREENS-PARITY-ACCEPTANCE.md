# Screen parity acceptance — user clarification, 2026-10-13

Applies to the next Android Compose and Iced desktop port passes. This supplements
SCREENS-HANDOFF.md and SCREENS-PORT-RULES.md; the integrated milestone remains partial.

## Explicit user requirement

Match the **current Svelte playground** exactly, screen by screen, including the
small details. Functional similarity or startup success is not visual acceptance.
Older HTML studies and the previous native UI are not design references.

- Measure computed styles and bounding boxes; do not eyeball or invent values.
- Match pane widths, inner padding, spacing, hairlines, row heights, icon sizes,
  typography, alignment, colours, corner paths, focus rings and depth.
- Distinguish actual gutters from internal padding and boundary strokes. The current
  DesktopShell grid has no inter-column gap; copy its measured separation rather
  than adding arbitrary spacing.
- Use the actual shape.ts cubic Bézier construction wherever Svelte uses it:
  smoothing 0.6, bubble radii 18/6, correct side budgets, grouping and RTL mirrors.
  Geometry samples must agree within 0.01 px. Match intentional circular/pill
  shapes and CSS-radius/shadow exceptions too; do not force smoothing everywhere.
- Cover every screen AND its menus, dialogs, sheets, editing, loading, empty,
  offline, permission, failure and call states. Include settings/profile/picture,
  privacy/devices, search, groups, gate flows, voice/video/waiting/minimized calls.
- Compare native and Svelte with the same synthetic data/state, app-interior size,
  scale and theme, in both light and dark, true size and zoomed. Do not copy the
  playground harness, simulation drawer or simulated OS chrome into the app.
- Verify reserved layout space when statuses/counts/errors change; motion,
  reduced motion, keyboard/focus and touch targets are part of the comparison.
- Core profile/picture APIs existing does NOT establish native UI integration.
  Wire actual APIs, use workers for blocking picture reads and marshal events to
  the UI thread. Do not replace real functionality with preview-only controls.
- Keep unsupported core semantics as explicit gaps. In particular, do not silently
  equate the playground's Busy status with call refusal or change security policy.

## Evidence and completion

Keep a screen/state checklist and a discrepancy list. Each item requires paired
screenshots and measurements, targeted behaviour checks, and an explicit verified
or blocked result. Do not call the complete app identical while any item remains.

Baseline audit evidence (desktop route inventory, NOT complete parity verification):
`/home/abe/build-cache/p2p_chat/device-smoke-15ad3cd-3i1JEO/parity-audit-isolated/`
contains 42 isolated route/theme screenshots and measured `screens.json`.
Call/gate/overlay variants and Android still need the full state matrix.

Follow shared-checkout file ownership and buildlock rules. Do not overwrite other
agents' work or clear/install user data as part of the comparison. Only the
coordinator integrates shared files; installations need user authorization.
