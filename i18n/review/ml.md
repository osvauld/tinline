# Malayalam (ml) review

Language: ml · File: `i18n/tm/ml.toml` (feeds `i18n/ml/{android,ios,desktop}.toml`) · Entries: 2190 · ok 2137 · fixed 38 · native 15 · restructure 3 new (plus every item already in `i18n/RESTRUCTURE.md`, which applies to Malayalam too)
Reviewer: `i18n-review-ml` (2026-10-26), independent of the translator. Checked against `i18n/VALIDATION.md`, `i18n/glossary/README.md`, `india.md`, `ml.md` and `docs/design/VOICE.md`.
Confidence after review: **medium-high** · Biggest issues: "can become you / becomes you" calqued as നിങ്ങളായി മാറാം; call-history words that read as "reply" or "gone" (മറുപടിയില്ല, പോയത്); two verbs for "left" and two words for "emergency".

## Verdict

**Ship as a draft.** The Malayalam is plain, warm and consistent. It is everyday Kerala phone Malayalam, not a Sanskritised glossary: കോൾ, കോൺടാക്റ്റ്, ബ്ലോക്ക് ചെയ്യുക, ഗ്രൂപ്പ്, ഹോസ്റ്റ്, അഡ്മിൻ, ലിങ്ക് ചെയ്യുക and സേവ് ചെയ്യുക sit beside native സന്ദേശം, അംഗം, ക്ഷണം, അറിയിപ്പ്, ഉപകരണം and ക്രമീകരണം, which is how Android and WhatsApp mix them. Buttons use the normal -ഉക form and "എന്റെ …" for "my" (എന്റെ അക്കൗണ്ട് സൃഷ്ടിക്കുക, എന്റെ വാക്കുകൾ പകർത്തുക, ഞാൻ അവ സേവ് ചെയ്തു, ഞാൻ സമ്മതിക്കുന്നു, ഇപ്പോൾ വേണ്ട). There is no ദയവായി, പിശക്, പരാജയം, അസാധു, താങ്കൾ or നീ, and no exclamation mark. Errors say what happened and what to do (…കഴിഞ്ഞില്ല. വീണ്ടും ശ്രമിക്കുക.).

The mechanics are clean:
- All placeholders match English in every entry, including plural forms.
- Every `**bold**` pair and `[label](link)` is intact.
- No case suffix is fused onto a placeholder: `{name}` stays in the nominative, followed by എന്നയാൾ / എന്ന അക്കൗണ്ട് / എന്ന ഉപകരണം / ഗ്രൂപ്പ്.
- All chillus are atomic (U+0D7A–0D7F). No ZWJ anywhere. ZWNJ is used consistently in loans (പാസ്‌ഫ്രേസ്, ഓഫ്‌ലൈൻ, മിസ്‌ഡ്, വോയ്‌സ്), and no word appears both with and without it.
- Plurals have `one` and `other` with correct agreement (ആൾ / പേർ, അംഗം / അംഗങ്ങൾ, ഫ്ലാഗ് / ഫ്ലാഗുകൾ).

Malayalam verbs do not inflect for person or gender, so the "You / {name} / Someone" slots in group system lines produce grammatical sentences ("നിങ്ങൾ {sub} എന്നയാളെ അഡ്മിനാക്കി", "ഒരാൾ മറ്റൊരാളെ അഡ്മിനാക്കി"). Several choices are better than a literal draft would be:
- "പ്രിയപ്പെട്ടവരിലേക്ക് നേരിട്ടൊരു ലൈൻ" for "A direct line to your people".
- "എന്തൊക്കെയുണ്ട് വിശേഷം?" for "What are you up to?".
- "ഒന്നിനേക്കാൾ നല്ലത് രണ്ട് ഉപകരണങ്ങൾ".
- "സാങ്കേതിക വിവരങ്ങൾ" for "Stats for nerds".
- "നിങ്ങളുടെ പേരിൽ പ്രവർത്തിക്കാം" for "act as you".

After the 38 fixes, what remains is the native-speaker list (mostly Android system labels to copy from a device) and the restructure list.

Checks run after the edits:
- `python3 scripts/i18n_tm.py apply ml` (1618 / 1519 / 1263 entries) and `python3 scripts/i18n_gen.py --check` (passes, exit 0).
- The edit script asserts that placeholders are identical before and after every fix.
- Sweeps found no ZWJ, no `}` followed directly by a Malayalam letter, no `!`, and no ദയവായി/പിശക്/പരാജ/അസാധു/താങ്കൾ. ഒരിക്കലും ("never") appears only where English says "never".

## Translator's open choices, ruled (for `ml.md`)

| Question | Ruling |
|---|---|
| **message:** സന്ദേശം vs മെസേജ് | **സന്ദേശം everywhere, including വോയ്‌സ് സന്ദേശം.** It is the glossary term, WhatsApp and Google Malayalam use it, and the file already has no മെസേജ്. For the verb, the compound **സന്ദേശമയയ്ക്കുക** is the normal button form and is shorter than "സന്ദേശം അയയ്ക്കുക". Use it in buttons and menu items (changed in 3 entries). Running text may keep "സന്ദേശം അയയ്ക്കാം". If a native panel prefers മെസേജ്, change the whole file at once, never one string at a time. |
| **`{name} എന്നയാൾക്ക്/എന്നയാളെ`** | **Keep it as the default for people.** It is the pattern Google's own Malayalam UI uses for a name of unknown shape ("%s എന്നയാളെ വിളിക്കുക"), it is gender-free, and it keeps `{name}` bare. A hyphen suffix ("{name}-ന്") is not safe, because the case ending depends on the name's last sound (Anu-വിന്, John-ന്, Maya-യ്ക്ക്). Lighter safe patterns, in order of preference: (1) where the name is the subject, no helper word at all ("{name} വിളിക്കുന്നു", "{name} സ്വീകരിച്ചു"), as the file already does; (2) in headers, receipts and screen-reader labels, a label-colon form ("മറുപടി: {name}", "ക്ഷണിച്ചത്: {name}", "ഒപ്പിട്ടത് {name}"), as the file already does; (3) otherwise എന്നയാൾ. Use the right helper noun for other placeholders: എന്ന അക്കൗണ്ട് (account nicknames), എന്ന ഉപകരണം (device names), ഗ്രൂപ്പ് after `{group}` ("{group} ഗ്രൂപ്പിൽ"). The file does all of this correctly. |
| **Buttons:** the long -ഉക form | **Keep -ഉക.** It is the Android/Google Malayalam button form, and it is polite without being formal. The -ൂ form (ചേരൂ, പറയൂ) is right for invitations addressed to someone else, such as the share text "…ഗ്രൂപ്പിൽ ചേരൂ" and "ഒരു ഹലോ പറയൂ", and the file uses it only there. To shorten: use compounds (സന്ദേശമയയ്ക്കുക), and drop ഒരു/നിങ്ങളുടെ where the screen already makes it clear. Never truncate the verb. Long buttons are listed under "Needs a native speaker" for a layout check. |
| **Answer:** എടുക്കുക vs മറുപടി നൽകുക | **എടുക്കുക** ("pick up"). It is what people say for a phone call ("ഫോൺ എടുത്തില്ല"). This app also has "Reply" = മറുപടി നൽകുക, so using മറുപടി for Answer would give two actions the same word on screens that sit side by side. "No answer" therefore becomes **എടുത്തില്ല** (changed in 3 entries), and End & answer is "അവസാനിപ്പിച്ച് എടുക്കുക". Check once on a device: if Google Phone's Malayalam incoming-call button reads differently, align with it. |
| **Recovery words:** വീണ്ടെടുക്കൽ വാക്കുകൾ | **Keep.** വീണ്ടെടുക്കൽ is the word Google Malayalam uses for account recovery (അക്കൗണ്ട് വീണ്ടെടുക്കൽ), and "റിക്കവറി വാക്കുകൾ" mixes a loan with a native noun for no gain. English "recovery phrase" strings are correctly rendered with the same term (one term per screen). The recovery *key* is വീണ്ടെടുക്കൽ കീ. |
| **root secret** | **മൂല രഹസ്യം**, per the user's decision. It appears once (debug-log note) and reads naturally as "the root secret". |
| **"Your line is open":** ലൈൻ തുറന്നിരിക്കുന്നു | **Keep.** ലൈൻ as a phone line is everyday speech ("ലൈൻ കിട്ടുന്നില്ല"), and "നിങ്ങളുടെ ലൈൻ തുറന്നിരിക്കുന്നു" carries the metaphor without sounding like a technical state. Related forms: "Keep my line open" = എന്റെ ലൈൻ തുറന്നുവയ്ക്കുക, "Your line is ready" = നിങ്ങളുടെ ലൈൻ തയ്യാറാണ്. |
| **delivered:** എത്തി | **Keep എത്തി.** It is short, past-tense and neutral, it fits the receipt counters ("എത്തി {done}/{total}"), and it never claims the person saw the message. "ഡെലിവർ ചെയ്തു" is longer and adds nothing. "Not delivered" = എത്തിയില്ല; "On {first}'s device" = "{first} എന്നയാളുടെ ഉപകരണത്തിൽ എത്തി". |
| **"Tinline-ൽ":** the hyphen | **Keep hyphen + suffix for Latin-script names.** This is how Google Malayalam attaches case to Latin names (Android-ൽ, Google-ന്റെ), and the spelling of the brand stays intact. "Tinline" ends in /n/, so the endings are -ൽ, -ന്, -ന്റെ, -നെ, -ലേക്ക്; the file has these right. Other cases are also right: Android-ന്, HD-യിൽ, Wi-Fi-യിൽ, QR-ഓ, 2-ഓ. Where a sentence reads as well with Tinline as the subject, prefer that. |
| **Lists:** comma lists, no "and" | **Keep "{list}, {last}" and "{first}, {last}".** Malayalam closes a list with എന്നിവർ (people) or എന്നിവ (things), or with fused -ഉം. Both depend on the host sentence's case and on what is listed, and the slots receive names inside sentences like "ഫ്ലാഗ് ചെയ്തവർ: {names}". A plain comma list is grammatical in every host. The real fix is the platform list formatter (RESTRUCTURE §9d). |
| **Desktop date pattern:** "{month} {day}, {weekday}" | **Keep. It matches CLDR ml** (MMMEd "MMM d, EEE", yMMMEd "y, MMM d, EEE"; the full date is "y, MMMM d, EEEE"), so it agrees with what Android/iOS print. "{year}, {month} {day}, {weekday}" is also CLDR. The month abbreviations are the CLDR ml ones (ജനു, ഫെബ്രു, മാർ, ഏപ്രി, മേയ്, ജൂൺ, ജൂലൈ, ഓഗ, സെപ്റ്റം, ഒക്ടോ, നവം, ഡിസം). `date_day_month` keeps "{day} {month}" because the key says day-first; see restructure R3. |
| **Android battery labels** | **Copy from a device, don't translate** (native item 1). "നിയന്ത്രണമില്ലാത്തത്" / "നിയന്ത്രിതം" / "ഒപ്റ്റിമൈസ് ചെയ്തത്" are plausible, but the English notes say they must match the Settings labels exactly. Set a Pixel to Malayalam, open Settings › Apps › Tinline › App battery usage, and copy the three option names into `t_9ab6b9677e`, `t_986948ce6e` and `t_ce5337ffb2`, and the quoted word in `t_770eea319c`. |
| **Sound names** | **ക്ലാസിക് മണിനാദം** (Classic chime; മണിനാദം is the everyday chime/bell sound) and **അയഞ്ഞത് → മുറുകിയത്** (Slack → taut) work and keep the string metaphor. Their descriptions ("നാല് സ്വരങ്ങളുള്ള തെളിഞ്ഞ മണിനാദം.", "അയഞ്ഞ ചരട് പോലെ…") are good. "മൂന്ന് സ്വരമുട്ട്" (Triad knock) was a coinage; it is now **മൂന്ന് സ്വരത്തിൽ മുട്ട്** ("a knock in three notes"). All three names are listed for a native ear. |
| **emergency** (new) | **അടിയന്തര**, as on Android's Malayalam lock screen ("അടിയന്തര കോൾ"). The file mixed in എമർജൻസി (3 entries changed). |
| **left a group** (new) | **പുറത്തുകടന്നു** everywhere ("You left", "{name} left"), as in WhatsApp Malayalam. Two system lines used പുറത്തുപോയി (changed). |
| **"act as you / become you"** (new) | **നിങ്ങളുടെ പേരിൽ പ്രവർത്തിക്കാം** for both. "നിങ്ങളായി മാറാം" ("can turn into you") is a calque (5 entries changed). |
| **Last seen** (new) | **"അവസാനം കണ്ടത്: …"**, the WhatsApp shape. "{count} ദിവസം മുമ്പ് അവസാനം കണ്ടു" read as "I last saw … ago" (4 entries changed). |
| **pronoun density** | ml.md asks to avoid നിങ്ങൾ where possible. The draft uses it in about 200 strings, mostly where English has "you/your" for emphasis or contrast. Most uses are fine. A native pass can drop "നിങ്ങളുടെ" in short labels where the screen already makes ownership clear. It was not changed mechanically. |

## Changes made (38)

| TM id · keys | English | Before | After | Why |
|---|---|---|---|---|
| `t_55e7249a5f` · android onboarding_not_emergency, android settings_emergency, ios settings_not_emergency … | Not for emergency calls. | എമർജൻസി കോളുകൾക്കുള്ളതല്ല. | അടിയന്തര കോളുകൾക്കുള്ളതല്ല. | Android Malayalam says അടിയന്തര കോൾ (lock screen "Emergency call"); എമർജൻസി is the less familiar loan |
| `t_224dd02d53` · android onboarding_dialer_note, ios onboarding_emergency_rest | Use your phone’s dialer to reach emergency services. | എമർജൻസി സേവനങ്ങളെ വിളിക്കാൻ നിങ്ങളുടെ ഫോണിന്റെ ഡയലർ ഉപയോഗിക്കുക. | അടിയന്തര സേവനങ്ങളെ വിളിക്കാൻ നിങ്ങളുടെ ഫോണിന്റെ ഡയലർ ഉപയോഗിക്കുക. | same term as the platform: അടിയന്തര |
| `t_89cb7b48e0` · desktop signup_use_your_phone_to | Use your phone to reach emergency services. | എമർജൻസി സേവനങ്ങളെ വിളിക്കാൻ നിങ്ങളുടെ ഫോൺ ഉപയോഗിക്കുക. | അടിയന്തര സേവനങ്ങളെ വിളിക്കാൻ നിങ്ങളുടെ ഫോൺ ഉപയോഗിക്കുക. | same term as the platform: അടിയന്തര |
| `t_a7294226a3` · android groupsc_sys_left_other, ios group_sys_left_named, desktop group_sys_left | {name} left | {name} പുറത്തുപോയി | {name} പുറത്തുകടന്നു | one verb for "left" (പുറത്തുകടന്നു, as in "You left" and WhatsApp Malayalam), not പുറത്തുപോയി |
| `t_4cb9ecb704` · android groupsc_sys_left_someone, ios group_sys_left_someone, desktop group_sys_left_someone | Someone left | ഒരാൾ പുറത്തുപോയി | ഒരാൾ പുറത്തുകടന്നു | one verb for "left" |
| `t_b4ee944a9e` · desktop gate_anyone_who_sees_these | Anyone who sees these words can become you. Write them down and keep them somewhere safe. | ഈ വാക്കുകൾ കാണുന്ന ആർക്കും നിങ്ങളായി മാറാം. അവ എഴുതിവച്ച് സുരക്ഷിതമായ ഒരിടത്ത് സൂക്ഷിക്കുക. | ഈ വാക്കുകൾ കാണുന്ന ആർക്കും നിങ്ങളുടെ പേരിൽ പ്രവർത്തിക്കാം. അവ എഴുതിവച്ച് സുരക്ഷിതമായ ഒരിടത്ത് സൂക്ഷിക്കുക. | "become you" calqued as നിങ്ങളായി മാറാം (turn into you); use the file's own "act as you" phrase |
| `t_e3db10694d` · desktop link_a_linked_device_becomes | A linked device becomes you | ലിങ്ക് ചെയ്ത ഉപകരണം നിങ്ങളായി മാറുന്നു | ലിങ്ക് ചെയ്ത ഉപകരണത്തിന് നിങ്ങളുടെ പേരിൽ പ്രവർത്തിക്കാം | "becomes you" calque, as above |
| `t_96c47afa9a` · desktop link_show_this_to_the | Show this to the device you want to add. It becomes you, so only link your own. | നിങ്ങൾ ചേർക്കാൻ ആഗ്രഹിക്കുന്ന ഉപകരണത്തിന് ഇത് കാണിക്കുക. അത് നിങ്ങളായി മാറും, അതിനാൽ നിങ്ങളുടേത് മാത്രം ലിങ്ക് ചെയ്യുക. | നിങ്ങൾ ചേർക്കാൻ ആഗ്രഹിക്കുന്ന ഉപകരണത്തിന് ഇത് കാണിക്കുക. അതിന് നിങ്ങളുടെ പേരിൽ പ്രവർത്തിക്കാനാകും, അതിനാൽ നിങ്ങളുടേത് മാത്രം ലിങ്ക് ചെയ്യുക. | "becomes you" calque, as above |
| `t_96c9d2bee5` · desktop settings_anyone_who_sees_these | Anyone who sees these words can become you. Check nobody is looking, then show them. | ഈ വാക്കുകൾ കാണുന്ന ആർക്കും നിങ്ങളായി മാറാം. ആരും നോക്കുന്നില്ലെന്ന് ഉറപ്പാക്കിയ ശേഷം കാണിക്കുക. | ഈ വാക്കുകൾ കാണുന്ന ആർക്കും നിങ്ങളുടെ പേരിൽ പ്രവർത്തിക്കാം. ആരും നോക്കുന്നില്ലെന്ന് ഉറപ്പാക്കിയ ശേഷം കാണിക്കുക. | "become you" calque, as above |
| `t_b943e62fec` · desktop settings_anyone_who_sees_these_words | Anyone who sees these words can become you. Check nobody is looking. | ഈ വാക്കുകൾ കാണുന്ന ആർക്കും നിങ്ങളായി മാറാം. ആരും നോക്കുന്നില്ലെന്ന് ഉറപ്പാക്കുക. | ഈ വാക്കുകൾ കാണുന്ന ആർക്കും നിങ്ങളുടെ പേരിൽ പ്രവർത്തിക്കാം. ആരും നോക്കുന്നില്ലെന്ന് ഉറപ്പാക്കുക. | "become you" calque, as above |
| `t_c13b853fc2` · android link_no_match, ios link_no_match | They don’t match | ഒന്നല്ല | വ്യത്യസ്തമാണ് | button "They don't match": ഒന്നല്ല reads as "not one"; വ്യത്യസ്തമാണ് pairs clearly with ഒന്നുതന്നെ |
| `t_2d4587e96b` · desktop app_the_two_passphrases_differ | The two passphrases differ. | രണ്ട് പാസ്‌ഫ്രേസുകളും ഒന്നല്ല. | രണ്ട് പാസ്‌ഫ്രേസുകളും വ്യത്യസ്തമാണ്. | ഒന്നല്ല is ambiguous ("not one"); "differ" = വ്യത്യസ്തമാണ് |
| `t_0a2e27ec58` · android conv_call_no_answer, android calls_what_no_answer, ios chat_call_no_answer … | No answer | മറുപടിയില്ല | എടുത്തില്ല | "No answer" for a call is "not picked up" (എടുത്തില്ല), matching Answer = എടുക്കുക; മറുപടിയില്ല means "no reply" |
| `t_e43d94eac3` · android history_detail_no_answer | no answer | മറുപടിയില്ല | എടുത്തില്ല | same as the pill "No answer" |
| `t_7286c55835` · ios shell_call_desc_no_answer | Call with no answer | മറുപടി ലഭിക്കാത്ത കോൾ | ആരും എടുക്കാത്ത കോൾ | same wording as calls_desc_no_answer (ആരും എടുക്കാത്ത കോൾ); മറുപടി is "reply" |
| `t_4d01fb421d` · android calls_desc_outgoing, ios shell_call_desc_outgoing | Outgoing call | പോയ കോൾ | വിളിച്ച കോൾ | പോയ കോൾ ("gone call") is not how phones say outgoing; വിളിച്ച കോൾ = a call you made |
| `t_9258e751e3` · android history_kind_outgoing, ios shell_call_what_outgoing, desktop common_outgoing | Outgoing | പോയത് | വിളിച്ച കോൾ | call-history title "Outgoing": പോയത് ("went") is vague |
| `t_29f6680e08` · android history_kind_incoming, ios shell_call_what_incoming, desktop common_incoming | Incoming | വന്നത് | വന്ന കോൾ | call-history title "Incoming": വന്നത് ("came") alone is vague; matches വിളിച്ച കോൾ |
| `t_8269587304` · android history_kind_missed, ios shell_call_what_missed, desktop common_missed | Missed | മിസ്‌ഡ് | മിസ്‌ഡ് കോൾ | "Missed" history title: the bare adjective മിസ്‌ഡ് is not used alone |
| `t_7097e1d5bd` · android gcall_ended_host_lost, ios call_group_ended_host_lost, desktop gcall_call_ended_host_lost | Call ended — host lost | കോൾ അവസാനിച്ചു — ഹോസ്റ്റ് പോയി | കോൾ അവസാനിച്ചു — ഹോസ്റ്റ് ലഭ്യമല്ലാതായി | ഹോസ്റ്റ് പോയി ("host went away") is slangy and vague; "the host became unavailable" |
| `t_58d3e5c07e` · android gcall_about_who, ios call_group_about_who, desktop gcall_a_call_host_is | A call host is a device a group admin chose and that agreed to it. It is separate from chat hosting. Admins can change call hosts in Group info. | ഗ്രൂപ്പ് അഡ്മിൻ തിരഞ്ഞെടുക്കുകയും അത് സമ്മതിക്കുകയും ചെയ്ത ഒരു ഉപകരണമാണ് കോൾ ഹോസ്റ്റ്. ഇത് ഗ്രൂപ്പ് ഹോസ്റ്റിംഗിൽ നിന്ന് വ്യത്യസ്തമാണ്. ഗ്രൂപ്പ് വിവരങ്ങളിൽ അഡ്മിൻമാർക്ക് കോൾ ഹോസ്റ്റുകളെ മാറ്റാം. | ഗ്രൂപ്പ് അഡ്മിൻ തിരഞ്ഞെടുക്കുകയും അത് സമ്മതിക്കുകയും ചെയ്ത ഒരു ഉപകരണമാണ് കോൾ ഹോസ്റ്റ്. ഇത് ചാറ്റ് ഹോസ്റ്റിംഗിൽ നിന്ന് വ്യത്യസ്തമാണ്. ഗ്രൂപ്പ് വിവരങ്ങളിൽ അഡ്മിൻമാർക്ക് കോൾ ഹോസ്റ്റുകളെ മാറ്റാം. | "chat hosting" is ചാറ്റ് ഹോസ്റ്റിംഗ് everywhere else (was ഗ്രൂപ്പ് ഹോസ്റ്റിംഗ്) |
| `t_7bd075ae97` · android devices_remove_others, ios devices_others_keep | Your other devices keep everything. | നിങ്ങളുടെ മറ്റ് ഉപകരണങ്ങളിൽ എല്ലാം സുരക്ഷിതമായിരിക്കും. | നിങ്ങളുടെ മറ്റ് ഉപകരണങ്ങളിൽ എല്ലാം അതേപടി ഉണ്ടാകും. | "keep everything" was rendered "everything will be safe" (സുരക്ഷിതമായിരിക്കും): stronger than English |
| `t_665692620b` · android profile_bytes_of_limit | {used} of {limit} bytes | {limit} ബൈറ്റിൽ {used} | {used} / {limit} ബൈറ്റ് | counter: same "x / y" shape as every other counter in the file |
| `t_32daee4811` · ios people_ui_bytes, desktop profile_of_bytes | {bytes} of {limit} bytes | {limit} ബൈറ്റിൽ {bytes} | {bytes} / {limit} ബൈറ്റ് | counter: same "x / y" shape as every other counter |
| `t_4385d29350` · desktop profile_last_seen_just_now | Last seen just now | ഇപ്പോൾ അവസാനം കണ്ടു | അവസാനം കണ്ടത്: അൽപം മുമ്പ് | "ഇപ്പോൾ അവസാനം കണ്ടു" is ungrammatical; WhatsApp-style "അവസാനം കണ്ടത്: …" |
| `t_9d6873f660` · android unlock_forgot, ios unlock_forgot, desktop signup_i_forgot_it | I forgot it | എനിക്ക് അത് ഓർമ്മയില്ല | ഞാൻ മറന്നുപോയി | text button "I forgot it": shorter and the natural spoken form |
| `t_4b7a9203ca` · android profile_err_name_empty, ios people_rule_name_empty, desktop profile_err_name_empty | The name can’t be empty. | പേര് ശൂന്യമാകരുത്. | പേര് ശൂന്യമായി വിടാൻ കഴിയില്ല. | ശൂന്യമാകരുത് is a prohibitive ("must not be empty"), sterner than the calm voice |
| `t_5a7ab72466` · android settings_video_in_calls_off, ios settings_video_in_calls_off | Calls are voice only | കോളുകൾ ശബ്ദം മാത്രം | വോയ്‌സ് കോളുകൾ മാത്രം | "കോളുകൾ ശബ്ദം മാത്രം" is ungrammatical; uses the file's വോയ്‌സ് കോൾ |
| `t_1a59cd4dd4` · android settings_sound_triad, ios settings_sound_triad | Triad knock | മൂന്ന് സ്വരമുട്ട് | മൂന്ന് സ്വരത്തിൽ മുട്ട് | sound name: "സ്വരമുട്ട്" is a coinage; "a knock in three notes" |
| `t_ffbb50f111` · android devices_working, ios devices_working, desktop group_working | Working… | ചെയ്തുകൊണ്ടിരിക്കുന്നു… | നടക്കുന്നു… | button label while unlinking: ചെയ്തുകൊണ്ടിരിക്കുന്നു is 22 letters and literal; matches link_working (നടക്കുന്നു) |
| `t_8ae2b3cd52` · desktop common_working | Working... | ചെയ്തുകൊണ്ടിരിക്കുന്നു... | നടക്കുന്നു... | same as Working… above |
| `t_43eb8c32e8` · android avail_one_hour | For 1 hour | 1 മണിക്കൂറിലേക്ക് | 1 മണിക്കൂർ നേരത്തേക്ക് | "മണിക്കൂറിലേക്ക്" reads "into the hour"; നേരത്തേക്ക് = for (a duration) |
| `t_d543a00377` · android groupsb_message_fn, android empty_hero_message, ios group_member_message … | Message {name} | {name} എന്നയാൾക്ക് സന്ദേശം അയയ്ക്കുക | {name} എന്നയാൾക്ക് സന്ദേശമയയ്ക്കുക | button: compound സന്ദേശമയയ്ക്കുക is the normal, shorter form (Google Malayalam) |
| `t_118c865bad` · desktop signup_message_2 | Message {value} | {value} എന്നയാൾക്ക് സന്ദേശം അയയ്ക്കുക | {value} എന്നയാൾക്ക് സന്ദേശമയയ്ക്കുക | same as Message {name} |
| `t_d2290010b5` · desktop group_message_privately | Message privately | സ്വകാര്യമായി സന്ദേശം അയയ്ക്കുക | സ്വകാര്യമായി സന്ദേശമയയ്ക്കുക | menu item: compound form, shorter |
| `t_ae83393da3` · desktop profile_last_seen_days | Last seen {count} days ago | {count} ദിവസം മുമ്പ് അവസാനം കണ്ടു | അവസാനം കണ്ടത്: {count} ദിവസം മുമ്പ് | "Last seen …" in the WhatsApp shape; "…മുമ്പ് അവസാനം കണ്ടു" reads "I last saw … ago" |
| `t_e11bdbbee3` · desktop profile_last_seen_hours | Last seen {count} hours ago | {count} മണിക്കൂർ മുമ്പ് അവസാനം കണ്ടു | അവസാനം കണ്ടത്: {count} മണിക്കൂർ മുമ്പ് | same |
| `t_8f77fe1d28` · desktop profile_last_seen_minutes | Last seen {count} minutes ago | {count} മിനിറ്റ് മുമ്പ് അവസാനം കണ്ടു | അവസാനം കണ്ടത്: {count} മിനിറ്റ് മുമ്പ് | same |

## Needs a native speaker (15)

| TM id · key | Concern | Candidate |
|---|---|---|
| `t_9ab6b9677e` notif_perm_unrestricted, `t_986948ce6e` notif_perm_restricted, `t_ce5337ffb2` settings_battery_optimised, `t_770eea319c` perms_battery_body | The notes say these must match Android's own battery-setting labels. | Copy the three option names from Settings › Apps › Tinline › App battery usage on a Malayalam Pixel (current: നിയന്ത്രണമില്ലാത്തത് / നിയന്ത്രിതം / ഒപ്റ്റിമൈസ് ചെയ്തത്). |
| `t_b27bf4c5ff` common_answer (+ `t_c2597b3d54`, `t_aca5a8736d`) | Answer = എടുക്കുക (ruled above). Confirm against Google Phone's Malayalam incoming-call screen. | Keep എടുക്കുക unless the system dialer shows another verb. |
| `t_c2f6535864` onboarding_perm_fullscreen, `t_e69907b099` perms_fullscreen_title | Permission names should echo Android's Malayalam "Full screen notifications" setting so people can find it. | Copy the system label; current "ലോക്ക് സ്ക്രീനിൽ റിംഗ് ചെയ്യുക" / "ഫുൾ സ്ക്രീൻ കോളുകൾ". |
| `t_0f88da7d0c`, `t_ae23a1918a`, `t_0803302453`, `t_c3fc51db22`, `t_6566a09c14` (host explanations) | "while members' phones sleep" is rendered ഫോണുകൾ ഉറങ്ങുമ്പോൾ. It is charming, but may read as a calque. | Keep, or use "ഫോണുകൾ സ്ലീപ്പിലായിരിക്കുമ്പോൾ". |
| `t_19e4003e15` unlock_keylost_restore (primary button) | "വീണ്ടെടുക്കൽ വാക്കുകൾ ഉപയോഗിച്ച് പുനഃസ്ഥാപിക്കുക" is the longest primary button. It may wrap to two lines. | "വാക്കുകൾ ഉപയോഗിച്ച് പുനഃസ്ഥാപിക്കുക" if it wraps. |
| `t_e81557b008`, `t_f778dc6325`, `t_11442471e3` (chips on the found-contact sheet) | The chips are single-line. ക്യാമറ ഉപയോഗിച്ച് വായിച്ചു / ചിത്രത്തിൽ നിന്ന് വായിച്ചു / നിങ്ങളുടെ കോഡ് കൈവശമുണ്ട് may clip with Malayalam's tall glyphs. | ക്യാമറയിലൂടെ / ചിത്രത്തിൽ നിന്ന് / കോഡ് കൈവശമുണ്ട്. |
| `t_3052a4ba12` groupsb_open_group (sheet label + confirm button) | "ഗ്രൂപ്പ് എല്ലാവർക്കുമായി തുറക്കുക" is correct and avoids the "view the group" reading, but it is long for a confirm button. | Layout check; "എല്ലാവർക്കുമായി തുറക്കുക" if needed. |
| `t_909bc609cf`, `t_9b9e6e2e8f`, `t_1a59cd4dd4` (sound names) | Sound-set names need a native ear for charm and for how short they are. | ക്ലാസിക് മണിനാദം · അയഞ്ഞത് → മുറുകിയത് · മൂന്ന് സ്വരത്തിൽ മുട്ട്. |
| `t_b74eaf63a2` notif_ch_service | "കോൾ ലിസണർ" is an untranslated technical loan in a system settings list. | "കോളുകൾക്കായി കാതോർക്കുന്നു" or "കോൾ സേവനം". |
| `t_ffbb50f111` devices_working, `t_8ae2b3cd52` common_working | Changed to "നടക്കുന്നു…" (matches link_working). Check that it reads as "working on it" on a busy button. | "ദയവായി കാത്തിരിക്കൂ" is out (no "please"). Alternative: "ചെയ്യുന്നു…". |
| ഐഡന്റിറ്റി (9 entries, e.g. `t_f038a0e9d0`, `t_6d009aea8f`) | ml.md marks identity **verify**. The loan is clear to tech users. തിരിച്ചറിയൽ means "identification". | Keep ഐഡന്റിറ്റി unless a panel prefers "അക്കൗണ്ട്" in the unlock-screen strings. |
| `t_c62fdbd2c0` profile_busy_open chip "No end time" | "സമയപരിധിയില്ല" reads "no time limit". This is close, but a little bureaucratic for a chip. | "അവസാനിക്കുന്ന സമയമില്ല" or "ഞാൻ മാറ്റുന്നതുവരെ". |
| `t_a65f16b027` profile_field_about (label "About") | "എന്നെക്കുറിച്ച്" (About me) is WhatsApp's label, so it is right for my own profile. On a contact's profile it would read "about me". | Check where desktop shows it for other people. If it is shown there, use "വിവരണം". |
| `t_5a41ace4dc` / `t_c762f4efdb` Unfold / Fold (screen reader) | വികസിപ്പിക്കുക / ചുരുക്കുക is correct but bookish for TalkBack. | Android's own TalkBack Malayalam uses "വികസിപ്പിക്കുക" / "ചുരുക്കുക", so probably keep. Confirm on device. |
| `t_5fd8e248d3` date_day_month | "{day} {month}" is kept as the key says. CLDR ml prefers "{month} {day}". | See R3. |

## Restructure (new for Malayalam, not already in `i18n/RESTRUCTURE.md`)

- **R1. Say what kind of thing a placeholder is.** Malayalam needs a different helper noun after a name depending on what it names: എന്നയാൾ (person), എന്ന അക്കൗണ്ട് (account nickname), എന്ന ഉപകരണം (device), ഗ്രൂപ്പ് (group). Many desktop notes say only "{value} is a value filled in by the app". Several say "{name} is a person's name" where the value is a device or account: desktop `link_linked` "Linked {name}", `link_link` "Link “{name}”?", `link_was_removed_from_this*`, and `profile_will_stop_syncing_with` "{name} will stop syncing…" (a device). Suggested fix: rename desktop placeholders to `{person}`, `{device}`, `{account}`, `{group}` (or correct the notes), so every language can pick its form. The current Malayalam guesses from the English and is right in the strings checked, but it is guessing.
- **R2. `call_they` ("They" as a stand-in for a caller's name).** Malayalam host strings put a helper noun after `{name}` ("{name} എന്നയാളെ…"), so a pronoun in that slot gives "അവർ എന്നയാളെ…", which is broken. iOS already has `call_unreachable_body_blank` and `call_unreachable_title_blank`. Suggested fix: Android uses the same unnamed variants ("They may be offline…", "Couldn’t reach them"), and `call_they` is removed. (RESTRUCTURE §2 lists `call_them` but not `call_they`.)
- **R3. `date_day_month` vs `date_month_day`.** These are two keys whose word order every translator can set anyway, and the note does not say which one the desktop picks for which locale. Suggested fix: one key, "date without a year", whose pattern each language writes (Malayalam "{month} {day}"). Better still, use the platform date formatter for the locale (RESTRUCTURE §5, §9g).

Everything else that bites Malayalam is already listed in RESTRUCTURE.md and is handled here with best-effort strings:
- §1 "You/Someone" slots. These are grammatical in Malayalam because verbs do not agree.
- §4 `{what}` outcome words.
- §5 desktop hard-coded English.
- §9b lowercase role fragments joined by commas. These read acceptably as a spoken list.
- §9d list joining.
- §9h hard-coded numbers (7 ദിവസം, 2 GB, 24).
