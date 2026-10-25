# Tinline strings

One master list, three native outputs. Edit `i18n/`, then run
`python3 scripts/i18n_gen.py`; never edit the generated files by hand.

| Platform | Master file              | Generated file                                        |
|----------|--------------------------|-------------------------------------------------------|
| Android  | `i18n/<tag>/android.toml`| `android/app/src/main/res/values*/strings.xml`, `res/xml/locales_config.xml` |
| iOS      | `i18n/<tag>/ios.toml`    | `ios/Tinline/Resources/Localizable.xcstrings`         |
| Desktop  | `i18n/<tag>/desktop.toml`| `crates/desktop/src/i18n/strings.rs`                  |

Languages and their tags, scripts and RTL flags: `languages.toml`.

## Entries

```toml
[chat_send]                       # snake_case, screen prefix first
text = "Send"
note = "Button that sends the typed message."   # required, for translators

[chat_unread]                     # plural: CLDR forms, `other` required
one = "{count} new message"
other = "{count} new messages"
note = "Badge on a chat row."

[call_ringing]
text = "Calling {name}…"
note = "{name} is the contact's chosen name."
```

- Placeholders are `{name}`; `{count}` is the integer one. The generator turns
  them into `%1$s`/`%1$d` (Android), `%1$@`/`%1$lld` (iOS) and keeps `{name}`
  for desktop.
- Translations (`i18n/fi/android.toml`, …) repeat the key with `text` or plural
  forms only. Missing keys fall back to English.
- Copy follows `docs/design/VOICE.md`: buttons speak as the user ("I", "my"),
  other text speaks to them ("you", "your").
- Every non-English string is a draft until a native speaker checks it
  (iOS marks them `needs_review`).

`--check` validates keys and placeholders; `--report` prints coverage.
