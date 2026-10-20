# Sign-up port: brief for the implementing agent (2026-10-13)

You are porting the sign-up flow from the playground to the **Iced desktop app first, then Android**.
The playground is the spec. This page is the advice: what to read, what must not be lost, and where people
usually go wrong. The detailed "what and why" is in `SIGNUP-HANDOFF.md`; copy rules are in `VOICE.md`.

## 0. Before you start
- Work in your own worktree and branch, cut from `ui-parity/main`. Verify the directory, branch, HEAD and
  `git status --short` first (root `AGENTS.md`). Don't commit to dev, don't push, never `git add .`.
- Heavy builds go through `python3 scripts/buildlock.py --who <you> -- <cmd>`; check `--status` first.
  `scripts/build_android.py` takes the lock itself. Use emulator `emulator-5554` only, never the USB phone.
- Run **targeted** checks only. This is UI work: no full test suites.
- The playground runs at http://127.0.0.1:5191/ (the design session's; read-only). Jump to any step with
  `?form=desktop|android&theme=light|dark&step=<step>`. Measure computed values and bounding boxes;
  don't eyeball them.
- Unsure? Ask the design session. Can't match something in the toolkit? Write it in
  `SCREENS-PORT-STATUS.md`; never approximate silently.

## 1. Read, in this order
1. `docs/design/SIGNUP-HANDOFF.md`: every decision with its why and Iced/Compose hints.
2. `docs/design/VOICE.md`: the one caring voice. Every string you touch must pass it.
3. `docs/design/SCREENS-PORT-RULES.md`: the foundations (tokens, `Depth`, `shaped.rs`, `TinDialog`, `TinMark`).
4. Playground sources: `playground/src/screens/gate/*` (Gate, StepShell, Btn and one file per step),
   `lib/brand/tinmark.js` and `MARK-MATH.md` (the spring), `lib/fx/audio.ts` (the `line` cue),
   `screens/people/AddContact.svelte` (opens over the first screen), `lib/ui/EmptyLine.svelte` (the first
   screen), `lib/brand/springs.ts` (`LINE_SPRING`), `tokens.css` (dark rim).

## 2. Where the code lives
- **Desktop:** the gate screens are in `crates/desktop/src/view.rs` (recovery phrase around line 270) and
  `app.rs` (messages and state); the mark spring is in `mark.rs`; curved surfaces are in `shaped.rs`;
  tokens and `Depth` are in `ui.rs`; sound is in `audio.rs`.
- **Android:** `Onboarding.kt` and `Unlock.kt` (screens), `Mark.kt` (`TinMark` spring), `Theme.kt`
  (`Modifier.depth`), `Components.kt` (`TinDialog`).

## 3. The things that must survive the port (the user checks these first)
1. **Nothing moves.**
   - The desktop card is a fixed 600×640 and the body scrolls inside it.
   - The header (back slot 44, brand, progress, theme switch) is there from Welcome on, and the hidden
     "1 of 6" keeps its width.
   - **Every desktop footer button has min width 220 and the primary sits at the far right on every step,
     Welcome included.** The user rejected a stacked column and a centred pair; it sits where Continue sits.
   - Recovery words: "Copy my words" turns into "I've saved them" *in the same button*, at the same size.
2. **Keys and thumbs.**
   - Desktop: Enter continues on every step, shown with a ↵ keycap. Skip it when a text field has focus.
   - Android: Continue is a 56 round arrow. Choose options sit at the bottom.
   - Android: assume the keyboard is up on steps with a field (`imePadding()`). The field and the primary
     action must both stay visible.
3. **Voice.**
   - The app is called "Tinline"; Osvauld appears only where the company is meant. Buttons say I/my.
   - The word is "nickname".
   - Recovery copy is calm: no blur, no warning box, copying is allowed, "paper works great".
   - Replace the old desktop notice "Recovery phrase copied. Paste only into a trusted password manager."
     (`app.rs`); it is exactly the alarm tone the user asked to remove.
   - Terms have no checkbox: the line above the button is the consent, and the button is the agreement.
   - Tone pass (2026-10-13): Passphrase, Nickname and Permissions copy changed to say what you get rather than
     what could go wrong (handoff § 6). Take the strings from the playground, not from the old apps.
4. **The line story (the mark).**
   - Welcome: the mark is slack. Done: the mark pulls tight in **two tugs**, on the two notes of the
     `line` cue.
   - The line colour goes from faded gold to full gold on the first tug.
   - Port the playground's spring changes into `mark.rs` and `Mark.kt` as **parameters of the one
     existing model**: `tugs`, `recoil`, `recoilMin`, `settleHz`, and `lineColor` on sign-up marks.
     Don't write a second animation.
   - The existing desktop test `connect_pulls_taut_once_and_settles` asserts the old dead stop. Update it
     to the new parameters rather than deleting it.
   - With reduced motion, the mark settles taut, the colour still warms and the cue still plays.
5. **Dark mode.**
   - Every raised surface gets the **rim** (1px light top edge, 8% white) through the depth tokens.
   - **No underglow anywhere** (dropped 2026-10-13), sign-up included. Light mode is unchanged.
6. **Done → the first screen → first contact.**
   - "Add my first contact" lands on Chats (the empty hero, handoff § 6e) with Add contact open over it, on
     **Code**. Add contact has two ways only, Code and QR (§ 6b), and copies the **bare** code.
   - Adding someone pulls the hero's line tight (`LINE_SPRING`), and it turns to "Your line is open · Message {name}".
     The `line` cue comes from adding the contact, not from the hero: play it once.
   - Hints in Add contact are **muted, polite, never red** (§ 6b). The user read the old red one as angry.
   - Contact codes are the real core ticket (`OSVC2:…`, `Node::my_ticket`). Find them in pasted text with
     `find_card`, preview with `peek_card`, and refresh "My code" after a redeem.
   - Android: put the recovery words on the clipboard with `ClipDescription.EXTRA_IS_SENSITIVE`.

## 4. Sounds (only the sign-up part now)
- Done plays the `line` cue (two plucks with a soft low note) on the first tug.
- All sounds are **generated**, never sample files. The ideal is one synth in Rust (`crates/audio`, or
  `crates/desktop/src/audio.rs` for now) that both apps share; the recipe is in `SIGNUP-HANDOFF.md` § 6d.
- The other new cues (received, knock, sent) belong to the chat/people port. Don't wire them here.

## 5. Don't port
- The keyboard mock and the drawer.
- The `?step`, `?gate=chat`, `?align`, `?depth` and `?acswitch` switches, and the `?lab=*` pages.
- The fake QR/camera and the demo phrase.
- `ChatGate.svelte` (the conversational sign-up) is a proposal; port the stepped flow.

## 6. Still open: don't decide these, leave a clear seam
- Text alignment: keep it **left** (the default). The user may choose to centre only Welcome and Done.
- Groups on a phone still offers "New group · You'll be the owner" (contradicts "Groups are hosted on a
  computer"); the greyed call buttons in a new chat; the empty chat's shield. Port them as they are and list them.

## 7. How to show it's done
- Desktop: a test-hooks build (`P2P_DARK` / `P2P_SHOT`, throwaway `--data` dir; the `P2P_DEMO` tool was removed 2026-10-20, restorable from git history). Take a shot of
  every step in light and dark next to the playground at the same size, and list the differences.
- Android: the same on the emulator, with the keyboard up on the field steps.
- Report the exact commands you ran, what you measured, what you couldn't match and why, and any
  "Ideas for the user". Don't build those ideas.
