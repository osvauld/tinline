# Manipuri / Meitei (mni), ꯃꯤꯇꯩꯂꯣꯟ

Read `india.md` first. Confidence: **low**. Machine translation into Meitei is weak,
and into Meetei Mayek script weaker still: drafts are often transliterated from
Bengali-script Manipuri or are not Meitei at all. A Meitei speaker who reads Meetei
Mayek must rewrite them.

## Script and layout
- **Meetei Mayek** (U+ABC0–U+ABFF), left to right, as chosen in `languages.toml`.
  Manipur made Meetei Mayek the official script and schools teach it, but many adults
  still read Manipuri more easily in **Bengali script** (`mni-Beng`). **Product
  question**: ship Meetei Mayek only, or also Bengali script.
- Font: **Noto Sans Meetei Mayek**, bundled by the app (most phones lack it). Check
  desktop and iOS also use the bundled font; missing glyphs show as boxes.
- Meetei Mayek has its own full stop ꯫ (U+ABEB) and its own digits ꯰–꯹. Use ꯫ for
  sentence ends **verify** whether UI text uses it or `.`. Digits come from the platform.

## Register
Polite 2nd person **ꯅꯍꯥꯛ** (nahak) rather than ꯅꯪ (nang, familiar) **verify**.
Meitei verbs do not mark gender. Politeness is also carried by verb suffixes
(request forms such as -ꯕꯤꯌꯨ "please do") **verify**.

## Buttons ("I/my")
"ꯑꯩꯒꯤ" (my) + verb **verify**: "Create my account" → "ꯑꯩꯒꯤ ꯑꯦꯀꯥꯎꯟꯠ ꯁꯦꯝꯃꯨ" **verify**.
If first-person framing sounds odd, use the short request form with "my".

## Plurals (CLDR)
I am not aware of a CLDR plural rule for Manipuri **verify**; the OS will likely use
`other` for every count. Write `other` so it reads for 1 too. Meitei usually does not
mark plural after a numeral.

## Names and brand
"Tinline", "Osvauld", "DID" stay in Latin letters even inside Meetei Mayek text.
Case suffixes as separate words after Latin names **verify**.

## Glossary (all **verify**)
Loanwords are written in Meetei Mayek. Terms below are tentative transliterations;
a speaker must replace any with native words people actually use.

| English | Meitei (Meetei Mayek) | Note |
|---|---|---|
| root secret | ꯔꯨꯠ ꯁꯤꯛꯔꯦꯠ | Loan. |
| recovery words | ꯔꯤꯀꯚꯔꯤ ꯋꯥꯍꯩ | ꯋꯥꯍꯩ = word. |
| Your line is open | **needs speaker** | Do not guess; ask whether "line" works. |
| online | ꯑꯣꯟꯂꯥꯏꯟ | |
| contact | ꯀꯟꯇꯦꯛ | |
| device | ꯗꯤꯚꯥꯏꯁ | |
| link a device | ꯗꯤꯚꯥꯏꯁ ꯂꯤꯡꯛ ꯇꯧ | |
| block | ꯕ꯭ꯂꯣꯛ ꯇꯧ | |
| group | ꯒ꯭ꯔꯨꯞ | |
| call / video call | ꯀꯣꯜ / ꯚꯤꯗꯤꯑꯣ ꯀꯣꯜ | |
| message | ꯄꯥꯎꯖꯦꯜ | ꯄꯥꯎ = news/word **verify** |
| missed call | ꯃꯤꯁꯗ ꯀꯣꯜ | |
| other technical terms | loans in Meetei Mayek | |
| identity | ꯃꯁꯛ | **verify**; "DID" unchanged. |

## Native speaker must check
Everything, especially: the script decision, whether text is Meitei, the honorific
level, and every term. Also check the bundled font renders all letters including
lonsum (final) letters ꯛ ꯜ ꯝ ꯟ ꯠ ꯡ ꯢ and the apun mayek (꯭).
