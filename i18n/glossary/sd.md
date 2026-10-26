# Sindhi (sd), سنڌي

Read `india.md` first. Confidence: **low**. Machine drafts are often Urdu with Sindhi
letters, and miss Sindhi's implosive letters and verb agreement. A Sindhi speaker
must check every string.

## Script and layout
- **Perso-Arabic** (Sindhi Arabic alphabet, Naskh style), **right to left**, as in
  `languages.toml`. Why this script: it is the script of most Sindhi readers (Pakistan
  and older Indian Sindhis), CLDR's default for `sd`, and the one with an established
  written standard. India officially recognises both Arabic and Devanagari Sindhi;
  younger Indian Sindhis often read Devanagari (`sd-Deva`) or neither. **Product
  question**: add `sd-Deva` later?
- Sindhi letters beyond Urdu: ٻ ڄ ڃ ڇ ڊ ڌ ڍ ڏ ڙ ڦ ڪ ڳ ڱ ڻ ۾ etc. Sindhi uses ڪ
  (not ک) for k, and ک for kh. Fonts: Noto Naskh Arabic covers them **verify** on
  Android, iOS and desktop. Sindhi readers prefer Naskh over Nastaliq.
- Punctuation `،` `؟` and `.` as full stop **verify** (Sindhi commonly uses `.`
  rather than Urdu's `۔`).
- Bidi: Latin words (Tinline, DID) and `{count}` in RTL text; app isolates placeholders.

## Register
Polite **توهان** (tavhān), not تون. Sindhi verbs mark gender.

## Buttons ("I/my")
"منهنجو/منهنجي" (my, agrees with the noun) + polite imperative: "Create my account" →
"منهنجو اڪائونٽ ٺاهيو" **verify**. First person verbs are gendered: use object-agreeing
transitive past ("مون محفوظ ڪيا" **verify**).

## Plurals (CLDR)
`one` (exactly 1), `other`.

## Names and brand
"Tinline", "Osvauld", "DID" in Latin; postpositions separate ("Tinline ۾").

## Glossary (all **verify**)
| English | Sindhi | Note |
|---|---|---|
| root secret | بنيادي راز | راز = secret. Alternative loan "روٽ سيڪريٽ". |
| recovery words | بحاليءَ جا لفظ | |
| Your line is open | توهان جي لائين کليل آهي | **verify** |
| online | آن لائين | |
| contact | رابطو | |
| device | ڊوائيس | |
| link a device | ڊوائيس لنڪ ڪريو | |
| block / unblock | بلاڪ ڪريو / اَن بلاڪ ڪريو | |
| group | گروپ | |
| call / video call | ڪال / وڊيو ڪال | |
| end call | ڪال ختم ڪريو | |
| mute | ميوٽ | |
| message | پيغام / نياپو | نياپو is native **verify** |
| delivered / read | پهچي ويو / پڙهيو ويو | |
| missed call | مسڊ ڪال | |
| endpoint, relay, vault, passphrase, backup | loans in Sindhi spelling | |
| notification | اطلاع | |
| settings | سيٽنگون | |
| account | اڪائونٽ | |
| identity | سڃاڻپ | "DID" unchanged. |

## Native speaker must check
Urdu drift (ک vs ڪ, ہ vs ه — Sindhi uses ه), gender agreement, font coverage, every term.
