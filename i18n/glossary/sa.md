# Sanskrit (sa), संस्कृतम्

Read `india.md` first. Confidence: **low**. Machine drafts "in Sanskrit" are often
Hindi with Sanskrit endings, wrong case endings, broken sandhi, or mixed
classical/modern vocabulary. A Sanskrit teacher or someone who writes modern Sanskrit
(e.g. the style of Sambhashana Sandesha) must check every string.

## Script and layout
- Devanagari, left to right. Sentence end `।`, paragraph end `॥` (not needed in UI).
- Sandhi: in UI strings, **avoid sandhi across words** where it hurts readability
  (modern Sanskrit UI writing usually keeps words separate). Never sandhi with a
  placeholder (`{name}`) since the name is unknown.

## Register
**त्वम्** (tvam). In Sanskrit tvam is not rude: it is used to address anyone,
including gods, and its verbs (2nd person) are **gender-neutral**. The honorific
भवान्/भवती takes 3rd-person verbs and **forces a gender choice**, which we can't make.
So: त्वम् with 2nd-person verbs, or impersonal passive constructions
("रक्ष्यताम्", "let it be saved").

## Buttons ("I/my")
Finite first-person verbs are gender-free: "I’ve saved them" → "अहं रक्षितवान्" is
**gendered** (participle), so use "मया रक्षितानि" (by me, saved — agrees with the
object) or "रक्षामि". "Create my account" → "मम खातं रचय" **verify** (gender of the modern loan खात-);
the imperative + मम keeps the user's voice. "Not now" → "इदानीं न".

## Plurals (CLDR)
I am not aware of a CLDR plural rule for Sanskrit; the OS will likely use `other` for
every count. Sanskrit has singular, dual and plural, so "{count} सन्देशाः" is wrong
for 1 and 2. Use a count-neutral label: "नवसन्देशाः : {count}" **verify**.

## Names and brand
"Tinline", "Osvauld", "DID" in Latin. They can't take case endings; use them in
apposition ("Tinline इति अनुप्रयोगे" style, **verify**) or as an indeclinable subject.

## Glossary (all **verify**)
Modern Sanskrit has coinages for some terms; for the rest use loans in Devanagari.

| English | Sanskrit | Note |
|---|---|---|
| root secret | मूलरहस्यम् | rahasya = secret; the calque is natural in Sanskrit. |
| recovery words | पुनःप्राप्तिशब्दाः | Long; "२४ शब्दाः" in context. |
| Your line is open | **translate meaning** | No phone-line tradition; e.g. "त्वं सम्पर्कयोग्यः असि" is **gendered** — needs a neutral phrasing from a speaker. Metaphor likely lost. |
| online | सक्रियः / ऑनलाइन | **verify** |
| contact | सम्पर्कः | |
| device | उपकरणम् | |
| link a device | उपकरणं योजय | |
| block / unblock | अवरुन्धि / अवरोधं निष्कासय | **verify** |
| group | समूहः | |
| call | आह्वानम् | **verify** (दूरवाणी-आह्वानम्) |
| video call | चलच्चित्र-आह्वानम् **verify** | |
| end call | आह्वानं समापय | |
| mute | मौनं कुरु | **verify** |
| message | सन्देशः | |
| delivered / read | प्राप्तः / पठितः | Masculine agreeing with सन्देशः. |
| missed call | अप्राप्तम् आह्वानम् **verify** | |
| endpoint, relay | loans | |
| vault | कोशः | |
| passphrase | गूढवाक्यम् **verify** | Not "कूटशब्दः" (password). |
| backup | प्रतिलिपिः | |
| notification | सूचना | |
| settings | विन्यासाः | |
| account | खातम् **verify** | |
| identity | परिचयः | "DID" unchanged. |

## Native speaker must check
Everything. Sanskrit UI is unusual; also ask whether the audience wants Sanskrit
coinages or loans, since modern Sanskrit writers differ.
