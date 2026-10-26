# Finnish (fi), Suomi

Confidence: **high**. Machine drafts are usually good in grammar but often pick
over-formal or passive phrasing ("Viestiä ei voitu lähettää") and wrong cases after
numerals. A native read-through is still required.

## Script and layout
- Latin script, left to right. No extra font. Uses ä, ö (and å in Swedish names).
- Dates `25.10.2026`, 24-hour time `14.30` or `14:30` (platform decides), decimal comma,
  space as thousands separator. All from the platform; do not write them into strings.

## Register
**Sinä-form (informal "you")**, written lowercase. Finnish consumer apps and messengers
(WhatsApp, Signal) speak in sinä-form; te-form (formal) sounds like a bank or an
authority, which is the opposite of the caring friend voice. Prefer active, personal
sentences over the Finnish impersonal passive ("Voit linkittää laitteen", not
"Laite voidaan linkittää").

## Buttons ("I/my")
Works naturally. Use the imperative plus a possessive suffix: "Create my account" →
"Luo tilini", "Copy my words" → "Kopioi sanani". Past first person for confirmations:
"I've saved them" → "Tallensin ne". "Not now" → "Ei nyt". Do not use the
infinitive ("Tilin luominen") on buttons.

## Plurals (CLDR)
`one` (exactly 1, no decimals), `other`.
- After a numeral above one the noun is **partitive singular**: `one = "{count} uusi viesti"`,
  `other = "{count} uutta viestiä"`. Drafts often wrongly use the plural ("5 uudet viestit").
- `one` may drop `{count}` ("Uusi viesti") only where the English one-form reads naturally without it.

## Names and brand
- "Tinline" in Latin letters. Case endings attach directly, as with "Skype" or "Office"
  (written-form base, no colon): `Tinlinen`, `Tinlinessa`, `Tinlineen`. **verify** the
  vowel harmony (back vowels, -ssa) with a native speaker. Prefer phrasings that need
  no ending ("Tinline pitää…" as subject) when they read naturally.
- `{name}` placeholders: the name gets a case ending at runtime, which we can't add.
  Rephrase so the name stays in the nominative: "Soitetaan: {name}" or
  "{name} soittaa" rather than "Soitetaan {name}lle".

## Glossary
| English | Finnish | Note |
|---|---|---|
| root secret | juurisalaisuus | Like juurivarmenne/juurihakemisto. Never "salasana" or "palautuslause". |
| recovery words | palautussanat | "24 palautussanaa". |
| Your line is open | Linjasi on auki | "linja" is the phone-line word ("linja on varattu"). Metaphor survives. |
| online | paikalla | WhatsApp's Finnish word; "verkossa" is the alternative. **verify** preference. |
| contact | yhteystieto | Plural yhteystiedot (the contacts list). For a person: "henkilö" if smoother. |
| device | laite | |
| link a device | linkitä laite | Linked devices: linkitetyt laitteet. Unlink: poista linkitys. |
| block / unblock | estä / poista esto | Blocked: estetty. |
| group | ryhmä | |
| host | ylläpitäjä? **verify** | Avoid "palvelin". "isäntä" is literal host but sounds odd; native to decide. |
| admin / member | ylläpitäjä / jäsen | If "ylläpitäjä" is taken by admin, host = "isäntälaite" **verify**. |
| call | puhelu | Voice call: äänipuhelu. |
| video call | videopuhelu | |
| end call | lopeta puhelu | Button may be "Lopeta". |
| mute / unmute | mykistä / poista mykistys | |
| message | viesti | |
| delivered | toimitettu | |
| read | luettu | |
| missed call | vastaamaton puhelu | |
| endpoint | päätepiste | Technical text only. |
| relay | välityspalvelin, verb "välittää" | "Relayed" chip: välitetty. |
| vault | holvi | |
| passphrase | salalause | Established (e.g. GnuPG). |
| backup | varmuuskopio | |
| notification | ilmoitus | |
| settings | asetukset | |
| account | tili | |
| identity | identiteetti | "DID" unchanged. |
| encrypted end to end | päästä päähän salattu | |
| recording | tallennus | "{name} tallentaa puhelua". |

## Watch for
- Passive and nominalised sentences (MT tell); prefer "Voit…", "Tallenna…".
- Long compounds on buttons; check length.
- "Hups" or exclamation marks: never.
