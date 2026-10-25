# Hindi (hi), हिन्दी

Read `india.md` first. Confidence: **medium**. Machine drafts are grammatical but
often too Sanskritised ("कृपया अपना कूटशब्द प्रविष्ट करें"), mix aap and tum, and use
gendered first-person forms on buttons. A native read is needed for tone.

## Script and layout
- Devanagari, left to right. System fonts on Android/iOS; desktop must have a
  Devanagari font with shaping (conjuncts, matras). Check desktop rendering.
- Full stop is `।` (danda) in formal prose; modern UI text commonly uses `।`. Use `।`
  consistently for sentence ends; `?` for questions. **verify** house style once.
- Nukta: use ज़, फ़ in loans (पासफ़्रेज़, नोटिफ़िकेशन) consistently, or never. Pick: use them.

## Register
**आप**, with plain everyday Hindustani words. Aap is the normal polite address in
apps and in conversation with someone you respect; tum would sound over-familiar from
an app, tu is rude. Warmth comes from simple words ("सहेज लें", "बाद में"), not from tum.
Avoid "कृपया" on every line.

## Buttons ("I/my")
Polite imperative + "मेरा/मेरे/मेरी" works: "Create my account" → "मेरा खाता बनाएँ",
"Copy my words" → "मेरे शब्द कॉपी करें", "Restore my account" → "मेरा खाता वापस लाएँ".
First-person confirmations only in gender-free form: "I’ve saved them" → "मैंने सहेज
लिए" (agrees with शब्द, not with the user). Never "मैं तैयार हूँ / हो गया". "I agree"
→ "मैं सहमत हूँ" (सहमत is gender-free). "Not now" → "अभी नहीं".

## Plurals (CLDR)
`one` (0 and 1), `other`. **0 is `one`**: keep `{count}` in `one`.
संदेश is the same in both ("{count} नया संदेश" / "{count} नए संदेश") — the adjective
changes. Check every adjective/verb agrees.

## Names and brand
"Tinline", "Osvauld", "DID" in Latin letters; postpositions as separate words:
"Tinline में", "Tinline को". Gender for agreement: treat Tinline as masculine
("Tinline चल रहा है") **verify**.

## Glossary
| English | Hindi | Note |
|---|---|---|
| root secret | रूट सीक्रेट | Loan recommended. "मूल रहस्य" reads as "the core mystery". **Product question.** Never पासवर्ड, never "रिकवरी फ़्रेज़". |
| recovery words | रिकवरी शब्द | Alternative "वापसी के शब्द" (plainer, **verify**). |
| Your line is open | आपकी लाइन खुली है | Everyday phone-line loan. Metaphor survives. |
| online | ऑनलाइन | |
| contact | संपर्क | Android Hindi term. |
| device | डिवाइस | Masculine. |
| link a device | डिवाइस लिंक करें | Linked devices: लिंक किए गए डिवाइस. Unlink: लिंक हटाएँ. |
| block / unblock | ब्लॉक करें / अनब्लॉक करें | |
| group | ग्रुप | "समूह" is formal; WhatsApp Hindi uses ग्रुप. |
| host | होस्ट | |
| admin / member | एडमिन / सदस्य | |
| call | कॉल (f.) | "{name} की कॉल". |
| video call | वीडियो कॉल | |
| end call | कॉल काटें | Everyday; "कॉल समाप्त करें" is stiffer. **verify**. |
| mute / unmute | म्यूट करें / अनम्यूट करें | |
| message | संदेश | Alternative मैसेज (colloquial). Pick one: संदेश. |
| delivered | पहुँच गया | Native and warm; "डिलीवर हुआ" also fine. |
| read | पढ़ लिया | |
| missed call | मिस्ड कॉल | Universal in India. |
| endpoint | एंडपॉइंट | Advanced text only. |
| relay | रिले | |
| vault | वॉल्ट | "तिजोरी" is a strongbox; fine in lead text. |
| passphrase | पासफ़्रेज़ | Not पासवर्ड. |
| backup | बैकअप | |
| notification | सूचना | Android Hindi uses सूचनाएँ. |
| settings | सेटिंग | Android Hindi term. |
| account | खाता | |
| identity | पहचान | "DID" unchanged. |
| encrypted end to end | एंड-टू-एंड एन्क्रिप्टेड | WhatsApp Hindi wording. |
| recording | रिकॉर्डिंग | |

## Watch for
- कृपया, प्रविष्ट करें, अमान्य, विफल, त्रुटि (stiff/alarming).
- Gendered first-person verbs on buttons; tum/aap mixing.
- Urdu-only vocabulary drifting in unevenly (fine in moderation; keep one style).
