# Translator glossary

One guide per language (`<tag>.md`, tags from `../languages.toml`). Read this page first,
then your language's page, then `docs/design/VOICE.md`. The validator checks drafts
against `../VALIDATION.md`.

These guides were written by the `i18n-linguist` agent on 2026-10-25. Every term
marked **verify** is a best guess that a native speaker must confirm before release.
Nothing here is a native speaker's judgement.

## What Tinline is (so the words fit)

Tinline is a personal app for messages, voice calls and video calls between people
and their own devices, directly from device to device. There is no central server
that holds your account. It keeps running in the background so people can reach
you. It is a non-commercial demo by Osvauld, not a product you buy. It does not
promise anonymity. The tone is a calm friend helping you: warm, plain, unhurried,
never alarming, never blaming.

## Rules for every language

1. **Brand names stay as written.** "Tinline" and "Osvauld" are always in Latin
   letters, in every script, including RTL text. Do not transliterate, translate,
   inflect with a hyphen-less suffix that changes the spelling, or put them in quotes.
   Where grammar needs a case ending, attach it the way that language attaches
   endings to foreign Latin-letter names (each page says how).
2. **"DID" stays "DID"** (Latin capitals). Explain it with the identity word if needed.
3. **Placeholders** (`{name}`, `{count}`, `{peer}` …) are copied exactly, never
   translated, never wrapped in extra quotes unless the English has them. You may
   move them to where your grammar needs them.
4. **Buttons speak as the user, other text speaks to the user.** Each page says how
   this works in that language (some use a verbal noun or infinitive instead of "I").
5. **Short buttons.** Aim for at most ~1.5x the English length on buttons, chips and
   tabs. If your language needs more, use a shorter natural phrasing, and say so in
   the draft so the validator can check the layout.
6. **Numbers, dates, times and durations come from the platform**, not from the
   translation. Keep `{count}` as is; do not write digits or month names into strings.
7. **No exclamation marks, no emoji, no "Oops", no urgency words**, unless the
   English has a real danger warning.
8. **Same action, same words** everywhere. Use the glossary term every time.
9. If an English string glues words together at runtime (for example
   `Missed {kind} call`), do not force it: translate as best you can and add a note
   so the English string can be restructured.

## The terms, and what they mean in Tinline

| English | Meaning in Tinline | Notes for every language |
|---|---|---|
| Tinline | The app's name | Never translated. Latin letters. |
| Osvauld | The company that made it | Only in About, Terms, Privacy. Latin letters. |
| root secret | The single secret the whole account comes from. All keys are derived from it. | **Never "password", never "recovery phrase", never "key".** It is a secret, the origin of the account. |
| recovery words / 24 words | The readable form of the root secret: 24 words you save | Plain "words" + "recovery/restore". Not "password". The UI says "recovery words". |
| line ("Your line is open") | Brand metaphor: a tin-can telephone line. "Your line is open" = Tinline is running and people can reach you. | Use the word for a *telephone line* if your language has one people still use; each page decides. |
| online | Presence label under a contact's name in a chat | The everyday word phone users see in other messengers. |
| contact | A person you've added | As in the phone's contacts app. |
| device | A phone or computer of yours | Platform's own word (Android/iOS system term). |
| link a device / linked devices / unlink | Add another of your own devices to your account by copying the root secret to it | Not "pair" (Bluetooth sense) unless your language uses one word for both. Unlinking is not a security revocation; do not translate it as "revoke". |
| block / unblock | Stop someone's messages and calls reaching you | Same word as other messengers. Personal, quiet, not "ban". |
| group | A group chat | |
| host (group host, call host) | A member's computer or phone that keeps a group's copy, or carries a group call | "Host" in the sense of hosting a guest/event, not "server". |
| admin / member | Group roles | Everyday messenger words. |
| call / voice call | Audio call | |
| video call | | |
| end call | The red button that ends a call | Use the verb people use for hanging up. |
| mute / unmute | Turn your microphone off/on | Microphone mute, not "silence notifications" (that is "mute notifications" if it appears). |
| message | A chat message | |
| delivered | Receipt: the message reached any of their devices | Past state, short. |
| read | Receipt: they opened it | Past participle "read", not the verb "to read". |
| missed call | A call nobody picked up | The everyday phrase. |
| endpoint | The network address a device can be reached at; it changes periodically (rotation) | Technical; appears in settings/advanced text only. Loanword is fine. |
| relay / relayed | An encrypted server that passes traffic along when a direct connection isn't possible. It can't hear or read. | Loanword or "forwarding" word. Never imply it can listen. |
| vault | Encrypted storage of the account on a device | A safe/strongbox word. |
| passphrase | Optional words you type to open Tinline on this device | Distinct from root secret. A "phrase" word; avoid plain "password" if your language has a distinct passphrase term, but a native "password-phrase" compound is fine. |
| backup | A saved copy you can open later | Platform's word. |
| notification | System notification | Platform's word. |
| settings | The settings screen | **Platform's word**: some languages differ between Android and iOS (French Paramètres / Réglages). Follow the platform file you are translating. |
| account | Your Tinline account | |
| identity (DID) | Your account's public identity, shown as a DID | Keep "DID" in Latin. |
| recording (call recording notice) | Someone in the call is recording | Neutral, factual. |
| encrypted end to end | | The standard phrase other messengers use in your language. |

## Page template

Each `<tag>.md` has: script and layout, register, buttons, plurals, names and brand,
glossary table, watch-outs, and confidence with what a native speaker must check.

## Decided by the user (2026-10-25)

These override any "verify" or open question in the per-language pages.

- **root secret, Indian languages:** use an English loanword in the local script
  (e.g. रूट सीक्रेट) wherever a literal translation reads like "the core mystery"
  (hi, bn, gu and similar). Use the literal native term where it plainly means
  "a secret" (ta, te, kn, ml, ur, sd, sa).
- **Register:** French *vous*, German *du*. The other languages follow their own pages.
- **One script per language:** Manipuri in Meetei Mayek, Sindhi in Arabic script,
  Konkani in Devanagari, Santali in Ol Chiki. No second-script variants for now.
- **Digits:** each platform's default for the locale; never hard-code digit shapes.
- **Not decided yet, so use the per-language page's recommendation:** whether the
  "line" metaphor survives in ta, sa, mni and sat, and "online" in Arabic.
