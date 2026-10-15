# Main-tab motion study — quiet crossfade chosen

Open the running playground at:

`http://127.0.0.1:5178/src/screens/tab-motion/index.html`

This is a standalone Vite lab entry so the shared app, shell, router, motion
library and other agents' screens do not need edits. The chosen transition is
implemented separately in Android's `Shell.kt::TabPanel`; this page is a browser study.
If no server is running, start the normal playground `npm run dev`.

## Compare

- **A — current fade-through:** existing shared transitions, 90 ms out + 210 ms in,
  incoming 98% → 100% scale.
- **B — quiet crossfade:** shared 120 ms crossfade token, no translation or scale.
- **C — short lateral:** 16 px in tab order (mirrored in RTL), existing 210 ms in /
  90 ms out tokens. Not a full-page slide.

The controls offer linked or independent switching, a cancellable walkthrough,
quarter-speed playback, system/reduced/full motion, light/dark/system theme, RTL,
and a temporary preference vote. Same-tab taps do nothing. Each phone remembers
scroll position separately for each tab. Exiting panels become inert and hidden
from accessibility immediately; the active tab supports arrows, Home and End.

The app bar, bottom search dock and navigation are outside the transition.
Contacts moves search to the bottom in this study. To avoid conflating dock
reflow with panel motion, Calls also reserves that dock height, without an action
button. **Calls search placement is a separate proposal**, not an accepted change.
All list/search/action previews are read-only. Only tabs and lab controls are
interactive. Chats, Groups and Calls render the existing components. Contacts
uses the shared people store and PersonRow without its existing top filter; it
is a simplified layout preview, not a replacement contacts implementation.

## Decision

The user chose **B — quiet crossfade**. Android's `Shell.kt::TabPanel` uses the
existing 120 ms crossfade token with cubic-out easing for both simultaneous
fades, zero duration under reduced motion, and no delay, scale, translation or
size spring. Header/dock/tab-bar composition and per-tab scroll state are kept
outside/independent of that animation. The browser lab retains A and C for
comparison; its vote buttons remain temporary notes, not product settings.

This accepts the motion only. Contacts bottom search was requested separately;
Calls dock placement remains an open layout proposal. Neither layout is
silently changed by selecting B. Native Android smoothness is not established
by the browser tests.

## Checks

All heavyweight commands must go through `scripts/buildlock.py` from the root.

```sh
python3 scripts/buildlock.py --who tab-motion-types -- bash -lc \
  'cd docs/design/playground && npm run check'
python3 scripts/buildlock.py --who tab-motion-local -- bash -lc \
  'cd docs/design/playground && ./node_modules/.bin/svelte-check --workspace src/screens/tab-motion --no-tsconfig --fail-on-warnings && ./node_modules/.bin/tsc --noEmit -p tsconfig.app.json && ./node_modules/.bin/tsc -p tsconfig.node.json'
python3 scripts/buildlock.py --who tab-motion-visual --run -- \
  node docs/design/playground/src/screens/tab-motion/verify.cjs
```

The browser check samples opacity/translation per frame; checks stationary
chrome, rapid taps/reversals, same-tab no-op, scroll restoration, independent
switching, keyboard/RTL navigation, motion controls and walkthrough cancellation;
and captures 600×900 and 1440×900 light/dark screenshots plus a slow-transition
and Contacts view. It reports browser errors, failed requests and overflow.
Screenshots and JSON reports go to the documented `/tmp/.../scratchpad/pg/tab-motion`
folder with a unique timestamp; nothing generated is added to Git.
`TAB_LAB_URL` and `TAB_LAB_SHOTS` can override the URL/output folder.

Initial checks: the owned Svelte folder has 0 errors / 0 warnings; the browser
checks pass. Full-workspace `npm run check` is blocked by existing unrelated
CallsList type errors (4) and CallsList/Gate/ContactsList warnings (6). Those
shared files were not edited. These browser checks do not prove native Android
performance or keyboard/IME behavior.

After selecting B, the browser checks and scoped Svelte/TypeScript checks passed
again (`/tmp/quiet-crossfade-browser.log`, `/tmp/quiet-crossfade-check.log`).
`git diff --check` passed. The queued Android check used:

```sh
cd android
JAVA_HOME=/usr/lib/jvm/java-21-openjdk ./gradlew --console=plain \
  :app:compileDebugKotlin -x cargoNdkBuild -x uniffiBindgen
```

This failed against the existing stale generated UniFFI bindings (missing core
APIs and mismatched Attachment fields); no diagnostic pointed at `Shell.kt`.
Native regeneration was deliberately excluded from this targeted Kotlin check.
No Android unit/device tests ran and no phone installation was performed.

## Optional lead-owned integration

To expose the usual `?lab=tabs` URL later, the owner of `src/main.ts` can add this
one entry to its `labs` map (not applied by this study):

```diff
+  tabs: () => import('./screens/tab-motion/TabMotionLab.svelte'),
```

The standalone lab URL works without that shared-file edit.
