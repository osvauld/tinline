# Urdu (ur), اردو

Read `india.md` first. Confidence: **medium**. Machine drafts are readable but often
lean on formal Arabic/Persian vocabulary, mix آپ/تم, use Latin punctuation, and break
bidi with Latin names and digits.

## Script and layout
- Perso-Arabic script in **Nastaliq** style, **right to left**.
- Font: Android ships Noto Nastaliq Urdu on recent versions; iOS has a Nastaliq font.
  Nastaliq is tall: check line height and clipping in buttons and chips. The desktop
  app must find or bundle a Nastaliq font (or Naskh as a fallback). Verify on all three.
- Punctuation: `،` comma, `؟` question mark, `۔` (U+06D4) full stop. Not the Latin `.`.
- Digits: platform decides (Urdu in India/Pakistan mostly shows Latin digits; some
  locales show ۱۲۳). Translators never type digits.
- Bidi: Latin words (Tinline, DID) and `{count}` inside RTL text; the app isolates
  placeholders. Validator checks rendering.

## Register
**آپ** (aap) with everyday Urdu. Same reasoning as Hindi: polite and warm; تم is
over-familiar from an app.

## Buttons ("I/my")
Polite imperative + "میرا/میرے/میری": "Create my account" → "میرا اکاؤنٹ بنائیں",
"Copy my words" → "میرے الفاظ کاپی کریں". Gender-free first person only: "I’ve saved
them" → "میں نے محفوظ کر لیے". "I agree" → "میں متفق ہوں". "Not now" → "ابھی نہیں".

## Plurals (CLDR)
`one` (exactly 1), `other`. Watch adjective/verb agreement ("{count} نیا پیغام" /
"{count} نئے پیغامات").

## Names and brand
"Tinline", "Osvauld", "DID" in Latin letters inside Urdu text; postpositions separate
("Tinline میں"). No transliteration (ٹِن لائن).

## Glossary
| English | Urdu | Note |
|---|---|---|
| root secret | بنیادی راز | "راز" is the plain word for a secret; reads well. Alternative loan "روٹ سیکرٹ". **verify**. Never پاس ورڈ. |
| recovery words | بحالی کے الفاظ | **verify** vs "ریکوری الفاظ". |
| Your line is open | آپ کی لائن کھلی ہے | Phone-line loan. Metaphor survives. |
| online | آن لائن | |
| contact | رابطہ | Plural رابطے. |
| device | ڈیوائس | |
| link a device | ڈیوائس لنک کریں | Linked devices: لنک شدہ ڈیوائسز. |
| block / unblock | بلاک کریں / ان بلاک کریں | |
| group | گروپ | |
| host | ہوسٹ | |
| admin / member | ایڈمن / رکن | |
| call | کال | |
| video call | ویڈیو کال | |
| end call | کال ختم کریں | |
| mute / unmute | میوٹ / ان میوٹ | |
| message | پیغام | |
| delivered | پہنچ گیا | |
| read | پڑھ لیا | |
| missed call | مسڈ کال | |
| endpoint | اینڈ پوائنٹ | Advanced only. |
| relay | ریلے | |
| vault | والٹ | Alternative تجوری. |
| passphrase | پاس فریز | Not پاس ورڈ. |
| backup | بیک اپ | |
| notification | اطلاع | Plural اطلاعات. |
| settings | ترتیبات | |
| account | اکاؤنٹ | |
| identity | شناخت | "DID" unchanged. |
| encrypted end to end | اینڈ ٹو اینڈ انکرپٹڈ | |
| recording | ریکارڈنگ | |

## Watch for
- `.` instead of `۔`; Latin `?` instead of `؟`.
- Heavy Arabic/Persian vocabulary (برائے مہربانی on every line, ناکام, غلط).
- Gendered first person on buttons.
