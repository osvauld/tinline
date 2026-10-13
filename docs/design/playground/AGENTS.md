# Rules for agents working on the playground

Read PLAN.md first (principles are binding). The app lives in this folder.

- **Run it:** the dev server is normally already running at http://127.0.0.1:5178/ (`npm run dev`).
  `?form=android|desktop` and `?theme=light|dark` force the harness, and the hash is the route
  (`#/chat/arjun`). Check before starting another server; never kill one you didn't start.
- **Type check:** `npm run check` must report 0 errors / 0 warnings before you finish.
- **Look at it.** Screenshot with Playwright:
  `const L=require('/home/abe/p2p_chat/.claude/worktrees/design-refinement/docs/design/refined/qa/lib.js')`
  (`L.launch()`, `L.page(b,{w,h,dark,mobile})`). Save shots under your own folder in
  `/tmp/claude-1000/-home-abe-p2p-chat/6f1f15a0-05df-4e79-8378-c444aaf8d38e/scratchpad/pg/<area>/`.
  **Use a NEW filename for every re-shoot.** Reading an overwritten PNG can show you the old image.
  Check android (600×900 viewport) and desktop (1440×900), light and dark. Zero console errors.
- **Ownership.** Edit only:
  - your `src/screens/<area>/`;
  - your `src/lib/model/actions/<area>.ts`;
  - your `src/lib/sim/<Area>Sim.svelte`;
  - NEW files in `src/lib/ui/` (don't edit existing ones; ask instead);
  - your OWN line(s) in `src/screens/registry.ts` and `src/lib/sim/sections.ts`.

  Append-only additions to `src/lib/ui/icons.ts` are fine. Everything else (store.svelte.ts, types.ts,
  seed.ts, router, fx, brand, App, shells, app.css, tokens.css) belongs to the lead. If you need a change
  there, put it in your final report as a request with the exact diff you want. Small type additions to
  `types.ts` that only ADD optional fields are allowed; list them in your report.
- **Data:** read and mutate `db` from `lib/model/store.svelte.ts`. Never copy data into component state,
  except transient UI state (an open menu, a text field). Use `clock.now` for times and the
  `listStamp/hhmm/dayLabel` helpers.
- **Motion:** use `var(--dur-*)` / `var(--ease-*)` (from `lib/fx/motion.ts`) and Svelte transitions with
  durations from `dur('…')`. Never hard-code ms in CSS or setTimeout. Reduced motion is global
  (`:root[data-motion=reduce]`); in JS check `motion.reduced` (`lib/fx/reduced.svelte.ts`).
  Sound only via `emitFx` (`lib/fx/bus.ts`), never WebAudio directly.
- **Copy:** must not promise more than the app does (no anonymity, unlink ≠ revocation, rotation not
  built). Anything undecided gets a "Proposed" note in the Simulation drawer, never in app UI.
- **Logical CSS** (inline/block, start/end) so RTL works. 44 px minimum touch targets.
- **Don't** git add, commit, stash, reset or checkout. The lead commits.
- **Final report:** what you built, what you checked (commands + results), screenshot paths, open
  questions, and any requests for lead-owned files.
