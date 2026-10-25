# Shared rules for the Indian languages

Applies to as, bn, brx, doi, gu, hi, kn, ks, kok, mai, ml, mni, mr, ne, or, pa, sa,
sat, sd, ta, te, ur, tcy. Each language page adds its own decisions.

## Words people actually use on their phones
Indian phone users read many English tech words in their own script every day
(कॉल, मिस्ड कॉल, ऑनलाइन, ब्लॉक, बैकअप, सेटिंग). A caring, plain app uses those words,
not dictionary coinages nobody says ("दूरभाष वार्ता" for a call). Rule:

1. If WhatsApp/Android in that language uses an English loan written in the native
   script, and people say it, use the loan.
2. If there is a native word everybody says (message → संदेश/செய்தி, account →
   खाता/கணக்கு), use the native word.
3. Never use a heavy Sanskrit/Persian coinage that only appears in government
   glossaries. It is the most common machine-translation tell in Indian languages.
4. Write loans in the native script, not in Latin letters (except Tinline, Osvauld, DID).

Tamil is the main exception: Tamil users and Tamil software prefer native terms
more strongly (அழைப்பு for call, not கால்). Its page says so.

## Product terms that need a decision (the same question in every language)
- **root secret.** A native calque (मूल रहस्य, মূল রহস্য) can read as "the main mystery"
  in Hindi, Bengali and others, which is wrong and a little ominous. In Dravidian
  languages ரகசியம்/రహస్యం/ರಹಸ್ಯ/രഹസ്യം is the plain word for "a secret", so the calque
  works there. Each page gives a recommendation; the fallback everywhere is the
  transliterated loan "रूट सीक्रेट" style, introduced once with its 24 words.
- **line ("Your line is open").** "लाइन" as a phone line is everyday Indian usage
  ("लाइन बिज़ी है", "line cut ho gayi"), so the metaphor survives in most languages as a
  loan. Where the page says it does not (Sanskrit, possibly Tamil formal), translate the
  meaning ("you're reachable") and flag it.
- **passphrase.** Most languages have no separate word; use the loan (पासफ़्रेज़) and
  never the word for password, so it stays distinct from the phone's password.

## Grammar traps shared by many Indian languages
- **First-person gender.** In Hindi, Urdu, Punjabi, Marathi, Gujarati, Dogri, Konkani,
  Maithili, Sindhi, Kashmiri, Nepali (partly), the speaker's gender shows in some verb
  forms ("मैं तैयार हो गया/गई", Marathi "मी करतो/करते"). Buttons speak as the user, whose
  gender we don't know. Use only forms that are gender-free:
  - transitive past with the agent marker, which agrees with the *object*:
    "मैंने सहेज लिए" (I saved them), Marathi "मी जतन केले";
  - polite imperative + "my" ("मेरा खाता बनाएँ");
  - or a noun ("सहेज लिया" without "I" if it still reads as the user's own answer).
  Never a first-person form that ends in a gendered ending (-ा/-ी, -तो/-ते).
- **Third-person gender for contacts.** `{name}` has unknown gender. Use the honorific
  plural, which is the respectful default and doesn't single out gender in most
  languages (Hindi "{name} कॉल कर रहे हैं" is masc. honorific plural — acceptable generic;
  Tamil/Telugu/Kannada honorific -ார்/-ారు/-ಆರೆ are gender-free). Better still, use
  nouns: "{name} की कॉल" / "{name} से कॉल".
- **Postpositions after placeholders**: write them as a separate word after the
  placeholder ("{name} को", "{name} से"). Do not fuse case suffixes onto `{name}` in
  languages that write them joined (Tamil, Malayalam, Telugu, Kannada); use a phrasing
  with the name in the nominative, or attach the suffix with a hyphen only if that is
  normal in the language **verify** per language.

## Numbers, dates, digits
- Indian grouping (1,00,000) and dates (25/10/2026) come from the platform.
- Native digits vs Latin digits: the OS decides per locale (CLDR defaults vary: e.g.
  Marathi and Nepali may default to Devanagari digits, Hindi to Latin). Translators
  never type digits. **Product question**: keep platform defaults.
- 12-hour time with localised AM/PM markers comes from the platform.

## Platform support
- Android ships fonts for the major Indian scripts; Ol Chiki (sat) and Meetei Mayek
  (mni) need the bundled Noto fonts named in `languages.toml`.
- Some languages are not system languages on every phone (brx, doi, kok, mai, mni, sa,
  sat, ks, sd, tcy). Users choose them inside Tinline (per-app language). If the OS has
  no plural rule for the language, it uses **`other` for every count, including 1**.
  For those languages write `other` so it reads acceptably for 1 too; a neutral
  label form ("नए संदेश: {count}") is safest.

## Neighbour-language drift (main tell)
Machine drafts of smaller languages often slide into a bigger neighbour:
Assamese→Bengali, Bodo→Hindi/Assamese, Dogri→Punjabi/Hindi, Maithili→Hindi,
Konkani→Marathi, Nepali→Hindi, Sindhi→Urdu, Kashmiri→Urdu, Tulu→Kannada,
Sanskrit→Hindi. The validator marks such strings "needs native speaker".
