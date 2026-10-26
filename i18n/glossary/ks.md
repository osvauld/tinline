# Kashmiri (ks), کٲشُر

Read `india.md` first. Confidence: **low**. Machine drafts for Kashmiri are weak:
often Urdu with Kashmiri spelling, missing Kashmiri vowel letters, and wrong verb
agreement. A Kashmiri speaker must rewrite them.

## Script and layout
- **Perso-Arabic** script (the official script in Jammu and Kashmir), **right to
  left**. Devanagari Kashmiri exists (`ks-Deva`) but is a minority use; not shipped.
- Kashmiri needs letters Urdu doesn't use: ٲ ٳ ۄ ێ and the diacritic ٕ (U+0655), among
  others. **Fonts must carry them**: check Noto Naskh Arabic / Noto Nastaliq Urdu
  render them on Android, iOS and desktop. Missing glyphs show as boxes or dotted circles.
- Punctuation: `،` `؟` `۔` as in Urdu.
- Bidi: Latin words (Tinline, DID) and `{count}` in RTL text; app isolates placeholders.

## Register
Polite **تۄہۍ** (tohi) **verify spelling**, not ژٕ (tsə, familiar). Kashmiri verbs
agree in gender and also carry pronominal suffixes for the person addressed; keep the
polite suffixes consistent **verify**.

## Buttons ("I/my")
"میون/میانہِ" (my, agreeing with the noun) + polite imperative **verify**. First person
verbs are gendered; use object-agreeing transitive past or a noun phrase.

## Plurals (CLDR)
`one` (exactly 1), `other`. Make `other` acceptable for 1 in case the OS lacks the rule.

## Names and brand
"Tinline", "Osvauld", "DID" in Latin inside RTL text; no transliteration.

## Glossary (all **verify**)
| English | Kashmiri | Note |
|---|---|---|
| root secret | روٗٹ سیٖکرٹ | Loan **verify spelling**; or "بُنیٲدی راز" (راز = secret). |
| recovery words | ریکوری لفظ | |
| Your line is open | تُہٕنٛز لائن چھےٚ کھۄلمٕژ | **verify** — low confidence wording. |
| online | آن لائن | |
| contact | رابطہٕ | |
| device | ڈیوائس | |
| link a device | ڈیوائس لِنک کٔرِو | **verify** imperative |
| block / unblock | بلاک / اَن بلاک | |
| group | گروپ | |
| call / video call | کال / ویڈیو کال | |
| end call | کال کٔرِو بند | **verify** |
| mute | میوٗٹ | |
| message | پیغام | |
| delivered / read | واتیوٚ / پرٕنہٕ آو | **verify** both |
| missed call | مِسڈ کال | |
| endpoint, relay, vault, passphrase, backup | اینڈ پوائنٹ، ریلے، والٹ، پاس فریز، بیک اپ | Loans in Urdu spelling; adjust to Kashmiri. |
| notification | اطلاع | |
| settings | سیٹِنگ | |
| account | اکاؤنٹ | |
| identity | پہچان / شِناخت | "DID" unchanged. |

## Native speaker must check
Everything: whether drafts are Kashmiri or Urdu, vowel letters and diacritics, font
rendering, polite forms, gender agreement.
