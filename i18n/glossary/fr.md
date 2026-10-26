# French (fr), Français

Confidence: **high**. Machine drafts are fluent but tend to be wordy, use "Veuillez…",
anglicisms ("supporter"), and mix tu/vous. Typography (spaces before ; : ? ! and
guillemets) is often wrong.

## Script and layout
- Latin script, left to right. No extra font.
- Dates `25/10/2026` (France) — platform formats; time 24-hour. Decimal comma.
- **Typography:** narrow no-break space (U+202F) or no-break space (U+00A0) before
  `? ! : ;` and inside « ». Typographic apostrophe ’ (as the English source uses).
  Ellipsis `…` as one character. A plain space before `?` can wrap the `?` onto its own
  line in a narrow button, so use the no-break space.
- Written for France/Belgium/Switzerland/Canada together: avoid regionalisms
  ("courriel" vs "e-mail" don't come up; "cellulaire"/"portable": prefer "téléphone").

## Register
**Vous.** French messengers (WhatsApp, Signal, Android, iOS) address the user with
vous, and in French vous is not cold: it is the polite default with a stranger,
including a friendly one. Tu would read as marketing-casual or youth-brand, which
clashes with "calm, never cute". Keep vous everywhere; warmth comes from short plain
sentences, not from tu. (Product question for the user: confirm vous.)

## Buttons ("I/my")
French buttons use the **infinitive**, and "mon/ma/mes" keeps the user's voice:
"Create my account" → "Créer mon compte", "Copy my words" → "Copier mes mots",
"Restore my account" → "Restaurer mon compte". Confirmations in first person past:
"I’ve saved them" → "Je les ai enregistrés", "I wrote them down" → "Je les ai notés".
"Not now" → "Plus tard" (more natural than "Pas maintenant"). "I agree" → "J’accepte".
Never "Cliquez ici" or imperative vous on buttons ("Créez votre compte").

## Plurals (CLDR)
`one` (0 and 1), `many` (round millions in compact form; practically never for our
counts), `other`.
- **0 is `one` in French**: "0 nouveau message". Do not hard-code "1" in the `one` form;
  keep `{count}`.
- Provide `many` equal to `other` with "de" where it applies, or omit it: `other` is the
  fallback. Do not leave `many` in a form that would read "1 000 000 messages" wrongly;
  the validator accepts `one` + `other`.

## Names and brand
- "Tinline" in Latin letters, masculine when an article is needed ("dans Tinline",
  avoid articles: "Tinline garde…"). Elision before Tinline is not needed (consonant).
- Elision before placeholders: write "de {name}", not "d’{name}", because the name may
  start with a consonant; accept the small awkwardness with vowel names. Prefer
  "Appel de {name}" over constructions that need agreement with the contact's gender.
- Avoid adjectives agreeing with `{name}`'s gender ("{name} est connecté(e)"): rephrase
  ("{name} est en ligne").

## Glossary
| English | French | Note |
|---|---|---|
| root secret | secret racine | Like "certificat racine". Never "mot de passe", never "phrase de récupération". |
| recovery words | mots de récupération | "vos 24 mots de récupération". |
| Your line is open | Votre ligne est ouverte | "ligne" is the phone line ("la ligne est occupée"). Metaphor survives. |
| online | en ligne | Shares "ligne" with the brand line, which is a happy echo. |
| contact | contact | |
| device | appareil | Android and iOS both say "appareil". |
| link a device | associer un appareil | Linked devices: appareils associés. Unlink: dissocier. Not "révoquer". |
| block / unblock | bloquer / débloquer | |
| group | groupe | |
| host | hôte | "Cet ordinateur héberge le groupe" for the verb. |
| admin / member | admin / membre | "administrateur" in full text if room. |
| call | appel | Voice call: appel vocal. |
| video call | appel vidéo | |
| end call | raccrocher | Button "Raccrocher". |
| mute / unmute | couper le micro / réactiver le micro | Short state chip: "Micro coupé". |
| message | message | |
| delivered | distribué | |
| read | lu | |
| missed call | appel manqué | |
| endpoint | adresse réseau | Plain wording; "point de terminaison" is the textbook term but opaque. Advanced text only. **verify**. |
| relay | relais | "Relayé · chiffré". |
| vault | coffre-fort | Short form "coffre". |
| passphrase | phrase secrète | Common in French security software. Keep distinct from "secret racine". |
| backup | sauvegarde | |
| notification | notification | |
| settings | Android: paramètres · iOS: réglages | Follow the platform file. Desktop: paramètres. |
| account | compte | |
| identity | identité | "DID" unchanged. |
| encrypted end to end | chiffré de bout en bout | Never "crypté" (anglicism). |
| recording | enregistrement | |

## Watch for
- "Veuillez", "Merci de", "Échec", "invalide", "erreur" on its own: tell what happened and what to do.
- "crypter/crypté" (use chiffrer), "supporter" for "support" (use "prendre en charge").
- Gender agreement with `{name}`.
- Missing no-break spaces before `? ! : ;`.
