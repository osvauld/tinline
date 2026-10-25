# German (de), Deutsch

Confidence: **high**. Machine drafts are fluent but drift into Sie, formal nouns
("Durchführung der Verknüpfung"), and long compounds that break buttons.

## Script and layout
- Latin script, left to right. No extra font. ä ö ü ß.
- Dates `25.10.2026`, 24-hour time, decimal comma. From the platform.
- German text runs ~30% longer than English. Long compounds do not wrap; check every
  button and chip.
- Quotes: „…“ in running text; the source's “…” around a name becomes „{peer}“.

## Register
**Du**, lowercase (du, dein). German messengers (WhatsApp, Signal) and most consumer
apps now use du; Sie sounds like a bank or an insurer, which fights the calm-friend
voice. Within the du register avoid slang; keep it warm and plain. (Product question:
confirm du.)

## Buttons ("I/my")
German buttons use the **infinitive** with the object first, and "mein" keeps the
user's voice: "Create my account" → "Mein Konto erstellen", "Copy my words" → "Meine
Wörter kopieren", "Restore my account" → "Mein Konto wiederherstellen". First-person
confirmations: "I’ve saved them" → "Ich habe sie gespeichert". If that is too long for
the button, prefer a shorter first-person form ("Hab ich gesichert", **verify** tone)
over a bare participle ("Gespeichert"), which loses the user's voice. "I agree" → "Ich stimme zu". "Not now" → "Später".
Never imperative du on buttons ("Erstelle dein Konto").

## Plurals (CLDR)
`one` (exactly 1), `other`. Straightforward: "{count} neue Nachricht" / "{count} neue
Nachrichten". Adjective endings change with articles; keep the pattern without article.

## Names and brand
- "Tinline" in Latin letters, neuter if an article is unavoidable ("das Tinline"), but
  prefer no article ("Tinline startet…"). Genitive: "von Tinline", never "Tinlines".
- Names in placeholders: avoid genitive -s ("{name}s Gerät" breaks for names ending in
  s/x/z). Use "das Gerät von {name}".

## Glossary
| English | German | Note |
|---|---|---|
| root secret | Stammgeheimnis | Like "Stammverzeichnis" (root directory). **verify** with a native speaker; alternative "Ursprungsgeheimnis". Never "Passwort", never "Wiederherstellungsphrase", never "Schlüssel". |
| recovery words | Wiederherstellungswörter | Long; in lead text "deine 24 Wörter" is fine once the term is introduced. |
| Your line is open | Deine Leitung ist offen | "Leitung" is the phone line ("die Leitung ist besetzt"). **verify** "offen" vs "frei" (frei = not busy, a slightly different meaning). |
| online | online | |
| contact | Kontakt | |
| device | Gerät | |
| link a device | Gerät verknüpfen | Linked devices: verknüpfte Geräte. Unlink: Verknüpfung lösen / "trennen". Not "widerrufen". |
| block / unblock | blockieren / nicht mehr blockieren | "Blockierung aufheben" is stiffer; pick one and keep it. |
| group | Gruppe | |
| host | Host | Loanword is common in German IT; verb "hosten". **verify** vs "Gastgeber". |
| admin / member | Admin / Mitglied | |
| call | Anruf | Voice call: Sprachanruf. |
| video call | Videoanruf | |
| end call | auflegen | Button "Auflegen". |
| mute / unmute | stummschalten / Stummschaltung aufheben | Chip: "Stumm". Button pair could be "Mikro aus" / "Mikro an". |
| message | Nachricht | |
| delivered | zugestellt | |
| read | gelesen | |
| missed call | verpasster Anruf | |
| endpoint | Endpunkt | Advanced text only. |
| relay | Relay | Loanword; "weitergeleitet" for "Relayed". |
| vault | Tresor | |
| passphrase | Passphrase (die) | Established loanword. |
| backup | Backup / Sicherung | Pick "Backup" consistently (platform uses both). |
| notification | Benachrichtigung | Plural "Mitteilungen" on iOS settings; follow the platform. |
| settings | Einstellungen | |
| account | Konto | |
| identity | Identität | "DID" unchanged. |
| encrypted end to end | Ende-zu-Ende-verschlüsselt | Hyphenated. |
| recording | Aufnahme | |

## Watch for
- Sie/Ihr creeping in; capitalised "Du" (we use lowercase).
- "Fehler", "ungültig", "fehlgeschlagen" on their own.
- Nominal style and passive ("Es wurde…").
- Compounds over ~18 characters on buttons.
