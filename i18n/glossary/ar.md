# Arabic (ar), العربية

Confidence: **medium**. Machine drafts in Modern Standard Arabic are readable but
often: address the user in the masculine only, use stiff calques ("قم بـ…" "please
do…"), get number–noun agreement wrong for 3–10 / 11–99, and break bidi around Latin
names and numbers. A native speaker must check every plural and every string with a
placeholder.

## Script and layout
- Arabic script, **right to left**. The app mirrors layouts (`rtl = true`).
- Fonts: Android and iOS ship Arabic fonts (Noto Naskh/Sans Arabic, SF Arabic). The
  desktop app must find or bundle an Arabic font with shaping; verify on desktop.
- Punctuation: Arabic comma `،`, semicolon `؛`, question mark `؟`. Full stop `.`.
  Ellipsis `…`. No exclamation marks (voice rule).
- Digits: whether `{count}` shows Western (123) or Arabic-Indic (١٢٣) digits is decided
  by the platform locale. Do not type digits. (Product question: accept platform default.)
- Dates/months differ by region (Levant "تشرين الأول" vs Egypt/Gulf "أكتوبر"); always
  from the platform formatter, never in strings.
- Bidi: Latin words (Tinline, DID, names in Latin) and numbers inside Arabic text are
  handled by the Unicode bidi algorithm. The apps should isolate `{name}`-style
  placeholders (FSI…PDI / BidiFormatter). Translators do not add invisible marks;
  the validator checks rendering with a Latin name, a number and an Arabic name.

## Register and gender
**Modern Standard Arabic (فصحى), simple and modern**, the register of Android/iOS
Arabic. No dialect.

Arabic marks the gender of "you" in verbs and adjectives. We don't know the user's
gender, so write **gender-neutral by construction**, not masculine by default:
1. **Possessive "your" is fine**: unvocalised `حسابك` (your account), `كلماتك` reads for
   both genders. Never add the vowel marks (حسابكَ / حسابكِ).
2. **Avoid 2nd-person verbs and adjectives** (احفظ/احفظي، مستعد/مستعدة). Use the
   verbal noun (مصدر) or a passive/impersonal sentence instead:
   "You can link another device" → "يمكن ربط جهاز آخر بحسابك" or
   "يمكنك ربط جهاز آخر" (يمكنك is unvocalised and neutral).
   `يمكنك` + masdar is the workhorse: "يمكنك تغيير ذلك لاحقًا".
3. Contacts (`{name}`) are also of unknown gender: avoid 3rd-person verbs agreeing with
   `{name}` ("{name} يتصل"/"تتصل"). Use nouns: "مكالمة من {name}", "رسالة من {name}".
4. Only where no neutral phrasing exists, use the masculine as generic and flag it
   (`note: masculine generic`) for the validator.

## Buttons ("I/my")
Arabic UI buttons use the **verbal noun (masdar)**, which is genderless; keep "my" with
the possessive suffix ـي: "Create my account" → "إنشاء حسابي", "Copy my words" → "نسخ
كلماتي", "Restore my account" → "استعادة حسابي". First-person past verbs are
genderless, so confirmations can stay first person: "I’ve saved them" → "حفظتُها"
(write without diacritics: "حفظتها"), "I agree" → "أوافق". "Not now" → "ليس الآن".
Avoid imperatives on buttons (أنشئ) because unvocalised they read as "you (m.) create".

## Plurals (CLDR) — all six forms
| Form | Numbers | Noun rule (message = رسالة) | Example |
|---|---|---|---|
| zero | 0 | often a sentence: "لا رسائل جديدة" | لا توجد رسائل جديدة |
| one | 1 | singular, no numeral | رسالة جديدة |
| two | 2 | dual, no numeral | رسالتان جديدتان |
| few | 3–10 (and 103–110…) | numeral + plural genitive | {count} رسائل جديدة |
| many | 11–99 (and 111–199…) | numeral + singular accusative | {count} رسالة جديدة |
| other | 100, 101, 102, 1000… | numeral + singular genitive | {count} رسالة جديدة |

- All six forms are required for every plural key. `zero`, `one`, `two` may drop `{count}`.
- Numeral gender polarity (ثلاث رسائل vs ثلاثة أجهزة) only matters if numbers are spelled
  out; we show digits, so keep `{count}` and do not spell numbers.

## Names and brand
- "Tinline", "Osvauld", "DID" stay in Latin letters inside Arabic text. No Arabic
  transliteration (تينلاين). No article: "Tinline يحتفظ…" → prefer "يحتفظ Tinline…"
  word order (verb first) **verify** natural reading.
- Never attach Arabic prefixes to a Latin word (بTinline): use a separate word
  ("في Tinline", "عبر Tinline").

## Glossary
| English | Arabic | Note |
|---|---|---|
| root secret | السر الجذري | Like "الشهادة الجذرية". Never "كلمة المرور", never "عبارة الاسترداد". |
| recovery words | كلمات الاسترداد | "كلمات الاسترداد الـ24" **verify** number placement. |
| Your line is open | خطك مفتوح | "خط" is the phone line ("الخط مشغول"). Unvocalised = neutral. Metaphor survives. |
| online | متصل | Standard messenger word, masculine generic; alternative noun "على الخط" echoes the line. **Product question.** |
| contact | جهة اتصال | Plural جهات الاتصال. |
| device | جهاز | Plural أجهزة. |
| link a device | ربط جهاز | Linked devices: الأجهزة المرتبطة. Unlink: إلغاء الربط. Not "إلغاء/سحب الصلاحية". |
| block / unblock | حظر / إلغاء الحظر | |
| group | مجموعة | |
| host | مضيف | "هذا الحاسوب يستضيف المجموعة". |
| admin / member | مشرف / عضو | |
| call | مكالمة | Voice call: مكالمة صوتية. |
| video call | مكالمة فيديو | |
| end call | إنهاء المكالمة | Short button: "إنهاء". |
| mute / unmute | كتم الصوت / إلغاء الكتم | |
| message | رسالة | |
| delivered | تم التسليم | |
| read | تمت القراءة | Alternative "مقروءة". |
| missed call | مكالمة فائتة | |
| endpoint | عنوان الشبكة **verify** | "نقطة النهاية" is the textbook term but opaque. Advanced text only. |
| relay | خادم ترحيل | "Relayed": عبر خادم ترحيل. Never imply it can listen. |
| vault | خزنة | |
| passphrase | عبارة المرور | Distinct from كلمة المرور (password). |
| backup | نسخة احتياطية | |
| notification | إشعار | Plural إشعارات. |
| settings | الإعدادات | |
| account | حساب | |
| identity | هوية | "DID" unchanged. |
| encrypted end to end | مشفّر تمامًا بين الطرفين | WhatsApp-style phrasing; **verify**. |
| recording | تسجيل | |

## Watch for
- "قم بـ" + masdar ("قم بإنشاء") — MT calque; just use the masdar.
- "يرجى"/"الرجاء" on every line (too formal), "فشل" / "خطأ" / "غير صالح" on their own.
- Masculine-only 2nd person verbs and adjectives.
- Latin punctuation (`,` `?`) in Arabic sentences.
- Broken order when a sentence starts or ends with a Latin word or `{count}`.
