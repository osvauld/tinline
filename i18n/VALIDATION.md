# Validating a translation draft

The validator (`i18n-linguist`) applies this checklist to each translated file
(`i18n/<tag>/<platform>.toml`), key by key, against the English source, its `note`,
`i18n/glossary/README.md`, the language page `i18n/glossary/<tag>.md` (and `india.md`
for Indian languages) and `docs/design/VOICE.md`.

Run `python3 scripts/i18n_gen.py --check` first. It catches missing `other`, plural
vs text shape and placeholder mismatches; the checklist below is what a script can't see.

## Checklist per key

1. **Meaning preserved.** Same facts, same promise, nothing added or dropped. Watch
   for over-claiming: never stronger than English on privacy or security ("can't hear"
   stays "can't hear", not "nobody can ever"); deletes, blocks and receipts are
   cooperative, so no wording that says they are enforced on the other side.
2. **Placeholders intact.** Every `{name}` from English is present, spelled exactly,
   not translated, not inflected, not quoted unless English quotes it. A plural form
   may drop `{count}` only where the form means exactly one value (`zero`/`one`/`two`
   in languages where that category is only 0, 1 or 2) — never in a `one` that also
   covers 0 (fr, hi, bn, gu, kn, as, doi, pa).
3. **Plural forms complete.** Every CLDR category the language uses is present
   (Arabic: zero, one, two, few, many, other; Santali: one, two, other; most: one,
   other). Noun/adjective agreement is correct in each form. For languages the OS may
   have no rule for (mai, mni, sa, tcy, possibly kok), `other` reads acceptably for 1.
4. **Glossary terms used.** Each glossary concept uses the chosen term, the same way
   in every file (Android, iOS, desktop). "root secret" is never rendered as password,
   recovery phrase or key. Tinline, Osvauld and DID unchanged in Latin letters.
5. **Register and voice.** The register chosen on the language page throughout (no
   tu/vous, du/Sie, aap/tum mixing). Buttons in the user's voice as the page
   describes; other text speaks to the user. No gendered first-person forms where the
   page says to avoid them; contacts addressed neutrally. Calm: no "error", "failed",
   "invalid", "wrong" on their own; no "please" on every line; positive before negative.
6. **Length.** Buttons, chips, tabs and short labels: flag anything over ~1.5x the
   English character count, or over ~20 characters where English is under 12. The UI
   must not shift or wrap when text appears (`docs/design` no-layout-shift rule).
   For tall scripts (Nastaliq, Malayalam, Telugu, Kannada, Odia) also flag lines likely
   to clip in single-line chips.
7. **RTL and bidi** (ar, ur, ks, sd). Native punctuation (`،` `؟`, Urdu `۔`). Check
   each string whose start or end is a Latin word, a number or a placeholder: render it
   mentally (or on device) with an Arabic-script name, a Latin name and a number, and
   flag misordering. No manual RLM/LRM marks unless agreed; the app isolates placeholders.
8. **Script hygiene.** Right script and code points: Assamese ৰ ৱ (not Bengali র ব),
   Malayalam atomic chillus, Sindhi ڪ and ه, Kashmiri vowel letters, Meetei Mayek and
   Ol Chiki only in mni/sat, nukta and ZWNJ used consistently, no Latin punctuation
   where the language has its own.
9. **No machine-translation tells.** Literal calques ("قم بـ", "Veuillez"), bookish or
   government-glossary words, English word order, wrong neighbour language (Assamese→
   Bengali, Konkani→Marathi, Maithili/Dogri/Nepali/Sanskrit→Hindi, Sindhi/Kashmiri→Urdu,
   Tulu→Kannada, Bodo→Hindi/Assamese), untranslated English left in by accident.
10. **No alarming wording.** No exclamation marks, emoji, urgency words, threats,
    "lose everything", "can't be undone" on the happy path; red-flag vocabulary only
    where English has a real danger warning.
11. **Fragments and glued sentences** (see below).
12. **Numbers and dates** never written into the string; no hard-coded digits, months,
    or units the platform should format.

## Fragments and glued sentences

Some English keys (mostly from the iOS extraction) are **pieces of a sentence**,
slotted into another string at runtime. Examples from `i18n/en/ios.toml`:

- `group_fallback_them` = "them", inserted as `{name}` in "Invitation sent to {name}";
  `group_fallback_they`, `group_someone` = "Someone" ("Someone joined"),
  `group_you` = "You" used as `{by}`/`{name}` in "{by} removed {name} as admin".
- "Part of a member row sub line" keys and "spoken description part (lower case,
  joined with commas)" keys, concatenated in code.
- `{name}, {kind}` and `{noun}`/`{kind}` slots such as "Missed {kind} call".

English gets away with this because it has no case, little agreement and fixed word
order. Most of our languages don't:

- **Case.** The fragment's form depends on its role in the host sentence: German
  ihnen/sie, Finnish heille/he, Arabic suffix pronouns, Sanskrit eight cases,
  Dravidian fused case suffixes (ta, te, kn, ml, tcy), Hindi-belt oblique forms
  (उन्हें/उनको). One fixed word can't fit every slot.
- **Person agreement.** "You" in a `{name}` slot needs a 2nd-person verb in fr, de,
  fi, ar, sa, and the Indian languages ("Du hast … entfernt" vs "Anna hat … entfernt";
  Hindi "आपने हटाया" vs "{name} ने हटाया" works, but "आप शामिल हुए" vs "{name} शामिल
  हुआ/हुई" does not).
- **Gender.** "Someone joined" / "them": the verb or participle must agree with a
  gender we don't know: ar, hi, ur, pa, gu, mr, doi, kok, sd, ks, mai, ne, fr ("arrivé(e)"), sa.
- **Word order and joining.** Lists joined with English commas, lower-casing, and
  "Missed {kind} call" adjective-in-the-middle order don't exist in many languages;
  Arabic, Urdu, Sindhi, Kashmiri need `،`, and SOV languages put the verb last.

**Rule.** For every key whose note says it is inserted into another string, or whose
English text is a bare pronoun/noun/adjective ("them", "Someone", "You", "edited",
role words), and for every host string with such a slot:
- mark the key **needs restructure** (not "fix") when the language can't produce a
  grammatical result for every host sentence it goes into;
- suggest the English fix: one complete string per case (e.g. separate keys
  "{by} removed you as admin" — already exists — "Someone joined",
  "Invitation sent" without a name), or a plural/select form;
- still give the best translation for the most common slot so the app is usable.

**Worst affected**: Arabic (case, person and gender together), Sanskrit (case, gender,
number including dual), Finnish (15 cases), Tamil, Telugu, Kannada, Malayalam and
Tulu (fused case suffixes), Kashmiri and Sindhi (gender + case + RTL joining), then
Hindi, Urdu, Punjabi, Marathi, Gujarati, Dogri, Konkani, Maithili (gendered verbs and
oblique forms), then German and French (case/agreement, fixable with rephrasing).
Least affected: Bengali, Assamese, Odia, Bodo, Manipuri, Santali (no grammatical
gender; still case suffixes, so check each slot).

## Report format

One report per file, `i18n-linguist` writes it as the task report (not committed
unless asked). Header, then one line per key that is not plain ok; keys not listed are ok.

```
Language: de · File: i18n/de/android.toml · Keys: 412 · ok 371 · fix 33 · native 6 · restructure 2
Confidence after review: medium · Biggest issues: Sie in 9 strings; "Passwort" for passphrase

key                       verdict      rule  note / suggested text
chat_send                 ok
call_end                  fix          4     "Anruf beenden" -> "Auflegen" (glossary: end call)
onboard_copy_words        fix          6     34 chars vs 14 en; "Meine Wörter kopieren"
group_fallback_them       restructure  11    case depends on slot; split host strings
recovery_lead             native       5     tone unclear: "sicher aufbewahren" may sound stern
```

Verdicts:
- **ok** — leave as is.
- **fix** — the validator is confident; gives the replacement text.
- **native** — needs a native speaker: register/tone, idiom, regional choice or any
  term marked **verify** on the language page. Gives the concern and a candidate.
- **restructure** — the English source must change (fragment, glued sentence,
  ambiguous note). Gives the suggested English change.

For low-confidence languages (brx, doi, ks, kok, mai, mni, sa, sat, sd, tcy) the
validator does not approve any string as final: the best verdict is "ok pending native".
