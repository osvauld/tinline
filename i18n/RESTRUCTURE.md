# English sources to restructure

Sentences built from pieces can't be translated well. Each item below needs
whole-sentence English (one key per variant) and a matching code change on
every platform that uses it. Collected from the pilot translators (fr, de, fi);
the reviews and the ar/hi translators will add to it.

Rule for the fix: **never slot a translated word into a translated sentence.**
Slots may only hold user data (names, numbers, times, device names).

## 1. "You" / "Someone" in a name slot (fr de fi)

`{by} made {sub} an admin` with `{by}` = "You" gives "Du hat…", "vous a nommé…".

- Keys: `common_you`, `group_you`, `common_someone`, `group_someone`,
  `groupsc_admin_granted_other`, `groupsc_admin_revoked_other`,
  `group_sys_admin`, `group_sys_unadmin`, `group_sys_joined`, `group_sys_left`
  (+ `shell_` copies), `avail_you`.
- Fix: separate keys per actor: `group_sys_admin_by_you` "You made {sub} an admin",
  `group_sys_admin_by_other` "{by} made {sub} an admin",
  `group_sys_admin_by_someone` "Someone made {sub} an admin".

## 2. Fallback nouns slotted into sentences (fr de fi)

Case, contraction or article depends on the slot (fr "à le", de dative, fi illative).

- Keys: `groupsb_the_group`, `group_fallback_the_group`, `group_fallback_them`,
  `group_fallback_they`, `card_them`, `call_them`, `card_this_contact`,
  `group_this_member`, `link_fallback_new_device`, `link_the_other_device`,
  `devices_fallback_*`, `onboarding_other_device`, `onboarding_restored_name_fallback_*`,
  `groupsb_not_in_contacts`, `profile_a_status`, `group_a_message`.
- Fix: a named variant and an unnamed variant of each host sentence
  ("Leave {group}?" / "Leave this group?").

## 3. Possessive slot (fr de fi)

- `kit_tick_their` ("their" in place of `{first}'s`).
- Fix: "On {first}'s device" / "On their device" as two full strings.

## 4. `{what}` / `{kind}` outcome words (fr de fi)

- `calls_row_desc`, `calls_row_desc_duration`, `shell_calls_row_a11y`,
  `shell_calls_row_a11y_duration` ("Missed {kind} call"),
  `profile_the_cant_be_empty`, `profile_the_cant_contain_control`,
  `profile_that_is_too_long`, `profile_that_is_too_long_up` (field name as `{what}`).
- Fix: one string per outcome / per field.

## 5. Desktop: English hard-coded in Rust (fr de fi)

- `signup_view.rs:605` "By continuing, you agree to the " + Terms/Privacy spans
  (`signup_and_have_read_the`).
- `signup_view.rs:429` " and N more", "isn't/aren't" (`signup_word_in_the_recovery`).
- `gcall_view.rs:136` "carries" (`gcall_this_call_everyone_in`, `gcall_is_carrying`).
- `{noun}` = "computer"/"phone" in `signup_tinline_connects_your_s*`,
  `signup_your_account_is_its`, `signup_type_your_24_*`,
  `signup_your_current_account_stays`, `signup_your_recovery_key_is`.
  Desktop always passes "computer": drop the slot, write the desktop sentence.
- `profile` `{what}` values ("about") passed as English.
- `view.rs:1508` month names "Jan".."Dec"; restore tabs "Words" / "Key".
- Fix: whole strings in desktop.toml; use the platform date formatter for months.

## 6. Glued sentence pieces (fr de fi)

- Group-call "carries this call": `gcall_host_this_phone`, `gcall_the_host`,
  `gcall_about_is_carrying`, `gcall_about_carries`, `gcall_the_call_host`,
  `gcall_this_computer` (also assume a neuter host in German).
- `group_invited_you_to`, `call_waiting_ends_with`, `call_ends_your_call_with`,
  `chat_you_blocked` (`{name}{when}` glued), `link_approve_bold` + `link_approve_body`,
  Android `onboarding_terms_by_continuing` (link labels in a fixed case).
- Fix: whole sentences with bold/link markup inside one string.

## 7. Missing plurals (de)

- `gcall_people_in_the_call`, `group_invite_people`, `signup_words_are_still_missing`,
  `profile_not_seen_for_days`.

## 8. Weekday slot (fi)

- `profile_until_weekday`, `people_until_day` ("Busy until {day} {time}").
- Fix: a weekday sentence per day, or format the whole phrase with the
  platform's date formatter.

## 9. One string, two roles (fi)

- `common_call`: button ("Call" = verb) and fallback title (noun). Split.

Same problem (hi): `groupsb_open_link` (switch title, noun) shares "Open link"
with `profile_open_link_button` (verb). Split by giving each role its own English.

## 9b. Text that continues another element (hi)

Strings that start after a bold name or end before one, so the verb can't go last:
`link_match_body`, `call_is_calling_you` (desktop), `notif_accepted_text`,
iOS `group_member_*_callhost{device}`, comma-joined role parts
(`groupsb_role_*_lc`, `groupsb_desc_*`, `group_member_desc_*`), `history_detail_*`.
Fix: one string with `{name}` (and bold markup) inside.

## 9c. Found in review (fr)

- **Blocking on screen now (desktop):** `{noun}` → "cet computer"; `signup_word_in_the_recovery`
  English " and N more"/"isn't"; terms line half English; profile `{what}` = "nickname".
  Fix these first (section 5).
- "Sent": one entry is both the requests header and a message receipt. Make the
  header "Sent requests".
- "Call": verb on buttons, noun on notification/history titles. Use "Tinline call"
  for titles (same as §9).
- Count strings without plurals: "Invite {len} people", " · {value} flag",
  "{filled} words…", "{count} people in the call", "{flags} of {total}", "{count} unread",
  "{more} more". Make them plural entries.
- "Declined" for an invitation needs its own entry ("Invitation declined").
- `calls_row_desc*`: pass a full phrase ("Missed call") as `{what}`, never an adjective.

## 9d. Lists joined in code (ar)

Code joins names/roles with English ", " and " and ": `groupsb_desc_*`,
`group_member_desc_*`, `groups_msg_and`, `group_names_and`, desktop `group_and`.
Arabic needs «،», and other scripts have their own separators.
Fix: join with the platform list formatter (Android `ListFormatter`, iOS
`ListFormatter`/`formatted(.list)`, desktop: a translated separator key), never a literal.

## 9e. Found in review (de)

- Rename the group "Open link" switch to "Public link" so it no longer shares an
  entry with the browser button (also hi).
- `onboarding_step_phrase` header "Recovery phrase" → "Recovery words", one term per screen.
- `gcall_about_*`: one sentence per host kind ("{name}'s device carries this call."),
  since a bold device name at sentence start can't take an article.
- `group_a_message` glued into the desktop composer placeholder: full placeholder strings.
- Desktop `check_name`/`check_text` (`profile_ui.rs`): reuse the Android/iOS
  `profile_err_name_*` per-field strings.

## 9f. Found in review (fi)

- Desktop `group_flag`: English ordinal from `ordinal()` (`groups_view.rs:74`)
  → " · flag {n}" with a plain number.
- `groupsb_online`/`groupsb_offline` describe host devices but share English with
  the people labels → separate `groupsb_host_online`/`groupsb_host_offline`.
- Desktop `profile_text` "{value} {time}", `call_text` "{day} {value}": weekday/date
  glued to a time (same as §8) → platform date formatter or one string per relative day.

## 9g. Found in review (hi)

- "Connecting…", "Reconnecting…", "Couldn't connect" share one entry for a device
  (masc.) and a call (fem.) → separate call-status keys ("Connecting the call…") or a
  separate key whose note names the call as the subject.
- English hard-codes numbers that siblings pass as placeholders → "You were removed
  after {total} admin flags" (`shell_group_sys_removed*`), "({flags_needed} flags
  remove someone)", "up to {max} pixels on a side" (`profile_that_picture_is_pixels`).
- Desktop `groups_view.rs` `when()`/`day()` format with `%b` → English month names
  inside translated strings ("Flagged {value}"). Use the locale date formatter.

## 10. Not translatable

- `app_ev` "Ev" is a Debug format string: remove it from the catalog.

## Also outside the catalog

- Rust core `present` text (block dialogs, ago/time, end reasons, size, link
  failures), `group_call_label`, call-quality labels, cue palette names.
- Android `// i18n-deferred` strings kept English because JVM tests assert text.
- iOS Info.plist permission prompts (needs `InfoPlist.xcstrings`).
