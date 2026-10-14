# Voice

Tinline speaks like a calm friend who is helping you set something up: warm, plain, unhurried, casual.
Mellow like its sounds: nothing is said in an alarming way, even the serious things. Caring, never cute. It explains why, reassures when something is serious, and never blames.
This sits beside PHILOSOPHY.md §2 (honest, never over-claiming): caring never means promising more.

## One grammar per element

| Element | Grammar | Examples |
|---|---|---|
| Button | First person, the user's own words: verb + "my"/"I" + object. Sentence case, no full stop, 2–4 words. | Create my account · Restore my account · Copy my words · I’ve saved them |
| Secondary / skip | Same voice, gentle and without guilt. | Not now · Skip for now · I wrote them down |
| Title | Speaks to you: a short sentence or a question, one line where possible. | What should people call you? · Here are your recovery words |
| Lead | Why it matters to you, at most two sentences, one sentence per line where it fits. | You just need a nickname. Encrypted end to end. |
| Error | What happened, then what to do. Never "wrong", "invalid", "failed" on its own. | That isn’t word 4. Check your saved copy. |
| Hint (nothing went wrong) | Muted ink, not red, `role="status"` not `alert`. Say how things are, then the easy next step. Never order people away ("go do X first, then come back"). | Your clipboard is empty right now. When their code arrives, copy it and paste it here. |
| Warning | Stated once, calmly, with the reason. Bold only the fact. | **Not for emergency calls.** Use your phone to reach emergency services. |

## Rules
- "You/your" in text; "I/my" on buttons. Never "the user", never "we" for the app.
- The app is **Tinline** in all UI text ("Tinline doesn’t collect…"). Osvauld is the company that made it; it
  appears only where the company itself is meant (Terms, Privacy Policy, About, store listing).
- In a conversation (the PROPOSED chat sign-up, `?gate=chat`), Tinline may speak as "I" in its own bubbles
  ("What should I call this computer?"); your replies stay first person.
- Positive before negative: say what it is before what it isn’t, and at most one "no" per line.
- No exclamation marks, emoji, "Oops", or jokes. No urgency words (now!, immediately) except for real danger.
- Red is only for something that actually went wrong and needs fixing. An empty clipboard, an optional field or
  "not yet" is a hint, in muted ink.
- Same action, same words everywhere (one label per action across Android and desktop).
- Labels in a group are the same shape and roughly the same length, so no button looks heavier.

## Owning your account (say it, gently)
Tinline has no central server holding your account. That is the point, and it comes with care the user has to take.
Say it as a fact about what they own, with the way to stay safe right after it. Never as a threat, never in red,
never twice on one screen, no "nobody can", "lose everything" or "can’t be undone" on the happy path.

| What is true | How we say it | Where |
|---|---|---|
| The 24 words bring the account back | Title "Here are your recovery words". Lead: "If you ever switch devices, these 24 words bring your account along." / "Save a copy somewhere safe. Paper works great." | Recovery words step |
| Copying is fine, paper is nicer | Main button "Copy my words", which becomes "I’ve saved them" once copied. Quiet option "I wrote them down". No blur, no reveal, no warning box. | Recovery words step |
| Ownership | "Your account is yours. It lives on your devices, not on a server. Your recovery words are the way back in, so keep them safe." | Before you start |
| Two devices | "Two devices are better than one. Link another device when you can, so losing one never locks you out." | Done (tip) |
| Groups need a computer to host | Phone: "Groups are hosted on a computer. To start a group, add Tinline on a computer that can stay on." Computer: "This computer can host your groups. Keep Tinline open and your groups stay reachable." | Done (tip) |

- For the recovery words, say "save a copy", "somewhere safe", "paper works great". Not "write these down" as an order, and not "lose everything".
- Agreement is the next button ("I agree") with one line above it; no checkbox.
