# Tinline playground — one app for interaction design

Replaces the separate `../refined/*.html` studies with ONE app: one shell, one data store, one
call/sound/motion layer, one Simulation drawer. The old studies stay untouched as reference until
each area is ported; the audit that motivated this is summarised in §6.

Stack: Svelte 5 (runes) + TypeScript + Vite, SPA, no SvelteKit. `npm run dev` (port 5178).
No runtime dependencies beyond svelte. No external fonts or network.

## 1. Principles (binding)

1. **One source of truth.** All people, chats, groups, calls, devices, permissions and connectivity
   live in `src/lib/model/store.svelte.ts`, seeded once from `seed.ts`. Screens never keep their own copies.
   Lists are keyed `{#each … (id)}` and update in place; never wipe and redraw.
2. **Playground leads the design, the core sets the rules.** We design what the app *should* be (as
   the bubbles were), but behaviour must not contradict the real core or decisions U1–U25
   (`git show dev:docs/HANDOFF.md`, rotation-open-decisions). Group posts are text + reply only (no
   edit/delete/attach); only the owner makes admins; the group call host can hear the call (U25); etc.
   Anything undecided is visibly tagged `Proposed` in the UI's Simulation notes, never as app copy.
3. **Motion and sound are one layer.** `src/lib/fx/`: timings in one table (also CSS vars), one
   `reducedMotion` signal (OS setting OR drawer toggle), one event bus so a sound cue and its visual
   are triggered together. No page-local `setTimeout(…, 220)` that must match CSS.
4. **Every animation must actually interpolate.** Tests sample per frame; a snap fails the test.
5. **Form factors share routes.** `android` = stack + bottom bar (390×844 frame); `desktop` =
   sidebar + list + detail pane. Same screens, different layout. Route names follow the Android
   `Route` enum where one exists.
6. **Brand mark is parametric** (candidate D, `src/lib/brand/markMath.ts`, model in MARK-MATH.md).
   Static icons and animation come from the same function.
7. **i18n-ready + RTL-safe** from day one (strings through `t()`; logical CSS properties).

## 2. Layout

```
src/
  main.ts, App.svelte            harness: form factor, theme, Simulation drawer, frame
  app.css                        tokens (from ../refined/tokens.css) + base
  lib/model/  types.ts seed.ts store.svelte.ts  clock.svelte.ts
  lib/nav/    router.svelte.ts   (route stack per form factor, hash deep links)
  lib/fx/     motion.ts  reduced.svelte.ts  bus.ts  audio.ts (synth; palette from sound-identity)
  lib/brand/  markMath.ts  Mark.svelte  (TinMark animation)
  lib/calls/  call.svelte.ts  (call state machine; drives fx bus)  CallOverlay/CallStrip
  lib/ui/     Icon Avatar Badge Chip Button IconButton Switch Dialog Sheet ListRow …
  lib/sim/    Drawer.svelte + per-area sim controls registered by screens
  screens/    home/ chat/ groups/ calls/ people/ devices/ settings/ search/ gate/
tests/        playwright: flows + frame-sampling motion checks
```

## 3. Build order (each step committed separately, screenshots both themes, both form factors)

1. **Foundation** (lead): scaffold, tokens, store + seed, router + two shells, ui kit, fx layer,
   Simulation drawer, Mark from markMath, test harness incl. frame sampler.
2. **Chats + conversation**: port the 3002236 bubble system unchanged in look (bubbles, grouping,
   inline meta, composer, on-demand menu, reply/edit/delete, files, offline/waiting). Fix audit items:
   date-aware sort, 44px targets, FAB overlap at 320, no splash.
2b. **Sign-up / gate** (parallel with 2; both form factors): welcome, create account (name,
   passphrase, recovery phrase shown + confirmed, device name), terms, OS permissions, restore from
   phrase, link to an existing device (QR + code, phrase transfer: see device-linking decision),
   unlock (passphrase, remembered key), key-lost / save-failed. Follow the real Android onboarding
   and desktop flows on dev; tag anything undecided.
3. **Calls 1:1**: overlay ↔ strip with real motion (spec in report-motion §5b), string connect/twang/
   release, ring/ringback/connected/ended cues synced via the bus, call records fixed (side, compact,
   no emoji in list). No send-pluck sound unless decided.
4. **Groups**: list folds, requests, invites (incl. short links), members/roles/hosts by core rules,
   flags per U15, removal, hosts offline/queued.
5. **Video 1:1** (shipped in the app, never designed): camera off by default, front cam, mirrored
   self-view PiP, flip, peer camera-off, permission denied, desktop no-camera.
6. **Group call**: call host model (U25 copy), roster (K loudest, animated), Leave, reconnecting,
   host lost, busy.
7. **Search** (core on branch `search`): field atop Chats/Groups, People & groups then Messages,
   snippets + highlights, jump-to-message, desktop Ctrl+K/Ctrl+F.
8. **People & privacy, devices, settings**: port the good copy (block flow,
   unlink), tag proposals, add OS-permission lifecycle, notification/sound settings, banners.

## 4. Coordination

Lead owns `lib/model`, `lib/nav`, `lib/fx`, `lib/brand`, `lib/calls`, `App.svelte`, `app.css`.
Area agents own only `screens/<area>/` and `lib/sim/<area>.ts`; they request store/fx changes
from the lead instead of editing shared files. Only the lead commits.

## 5. Verification per step

`npm run check` (svelte-check + tsc) clean; `npm run test` (playwright flows + motion frame
sampling); screenshots 390/1440 × light/dark read by a human-equivalent pass, not just asserted.

## 6. Why (audit 2026-10-12, summary)

Studies were separate pages with separate data (Home vs chat vs groups disagree), four copies of the
mark, eight of reduced-motion, three call UIs. The string animation never interpolated (CSS `d`
Q→L). Groups offered behaviour the core forbids. Video, group call UI, search and rotation status
were never designed. Same-author QA only checked markup. Kept: bubble system (unchanged since
3002236), composer/menu/file/dialog components, tokens, call-state sync idea, call-audio
generator, block flow and unlink copy.

## 7. Decisions log (user, 2026-10-12)

Recorded here so area work and the core teams see them. Drawer notes say "Decided" for these.

- **Sign-up**: no word-confirmation step; confirm-passphrase optional ("Check my copy").
- **Call screen**: no ring around the avatar; a cloud colour field that moves with the tune.
- **Groups**: owner can't leave (for now). A flagged member sees each flag (who, when, n of 3)
  as it happens, not only after removal. Chat requests are one per person at a time, like
  Instagram: no follow-ups until they answer (ask again 7 days after a decline: Proposed).
- **Calls, system surfaces**: the lock screen shows the caller's name; missed-call
  notifications fold into one per person ("2 missed calls · Sana Rao").
- **Profiles**: Busy does not stop calls; incoming calls still show while Busy.
  A nickname for a contact syncs to my other linked devices (never to the contact).
  Busy lengths 30 min / 1 h / 4 h / tomorrow morning / no end, default 1 h.

Core follow-ups these imply (not playground work):
- Availability: the core refuses calls while unavailable (`set_available` controls call admission);
  it must keep admitting calls while Busy.
- Nickname: needs a place in own-device sync (e.g. the private own-device profile cache), never in
  the shared profile document.
- Profile pictures: old-copy deletion request is automatic (profile-sync backend follow-up pending).
- Chat requests: one pending request per person; per-person cooldown after decline (Proposed).
- Flags: the flagged member must receive flags before removal.
