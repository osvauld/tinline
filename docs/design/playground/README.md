# Tinline design playground

One app to try how Tinline looks, moves and sounds, on Android and desktop, light and dark.
It is a design tool, not the product: the core (Rust) is simulated in one in-memory store, and
the Simulation drawer plays the other side (people, network, hosts, calls). Behaviour follows the
real core and the decisions in `PLAN.md` §7; anything not decided is tagged **Proposed** in the
drawer, never in app copy.

## Run

```sh
npm install
npm run dev          # http://127.0.0.1:5178/
npm run check        # svelte-check + tsc
```

Use Chromium (the Android WebView engine). Click the page once so the browser allows sound.

- Header: **Android / Desktop**, theme (system / light / dark), and the sliders icon for the
  **Simulation** drawer. The drawer takes its own column, so it never hides the app.
- Deep links: `?form=android|desktop&theme=light|dark` plus a hash route, e.g. `#/chat/hike`,
  `#/chat/hike/info`, `#/search/lunch`, `#/me`, `#/settings`, `#/contact/arjun`.

## What's in it

| Area | Where | Drawer section |
|---|---|---|
| Sign-up and unlock | `src/screens/gate` | Sign-up and unlock |
| Chats, conversation, find in chat | `src/screens/home`, `src/screens/chat` | Chat |
| Groups: roles, invites, hosts, flags, chat requests | `src/screens/groups` | Groups |
| 1:1 voice calls, call waiting, lock screen / notifications / desktop toast | `src/lib/calls` | Calls |
| Call background (Fog / Clouds), moved by the call sounds | `src/lib/calls/field` | Calls → Call background |
| Generated call sounds (palettes) | `src/lib/fx/audio.ts` | Environment → Call sounds |
| Profiles and pictures | `src/screens/profile`, `src/screens/settings` | Profiles |
| Search | `src/screens/search` | Search |
| Logo (parametric mark D) and live status | `src/lib/brand` (icon exports: `../brand/`) | — |

## How it's built

- **One store** (`src/lib/model/store.svelte.ts`): screens read it directly; area actions live in
  `src/lib/model/actions/*`. Times come from the sim clock (`clock.svelte.ts`).
- **One motion and sound layer**: durations and easing tokens in `src/lib/fx/motion.ts`,
  reduced motion in `reduced.svelte.ts`, sound via the fx bus (`emitFx` / `onFx`) into the
  WebAudio synth. The call background and the logo read the same note schedule as the sounds.
- **Rules the UI keeps**: nothing on screen moves when text appears or changes (reserved rows,
  in-place cross-fades, tabular numbers); respect reduced motion; 44 px targets; logical CSS.

## Tests

Playwright flows against the running dev server (`BASE=http://127.0.0.1:5178/` by default):

```sh
node tests/chat.flow.js      # also: gate, groups, call, waiting, system, field, profile, search, layout
```

`tests/layout.flow.js` measures layout shift (Chromium Layout Instability API + anchor geometry)
across every area. Run the area's own flow while working; the full set once before merging.
