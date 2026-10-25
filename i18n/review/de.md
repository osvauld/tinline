# German (de) translation review

Reviewer: `i18n-review-de` (native-level German review, independent of the translator), 2026-10-25.
File: `i18n/tm/de.toml` (2053 entries → `i18n/de/{android,ios,desktop}.toml`).
Checked against `i18n/VALIDATION.md`, `i18n/glossary/README.md` (user decision: lowercase *du*), `i18n/glossary/de.md`, `docs/design/VOICE.md`.
`python3 scripts/i18n_tm.py apply de` and `python3 scripts/i18n_gen.py --check` pass after the fixes.

## Verdict

**Ship as draft.** The translation is fluent, consistently *du* (no capitalised Du/Dein/Sie/Ihr anywhere), placeholders and plural shapes are intact, and the glossary terms are used consistently across Android, iOS and desktop. The fixes below were mostly small: German first-person/imperative verb forms used for status lines ("Verbinde neu…", "Sende…", "Suche…"), a handful of calques, two sentence-initial "Ihre" that read as the formal register, and a few over-long buttons. What keeps it a draft is the English side: about 40 entries are fragments or glued strings that German grammar can't fill correctly in every slot (list below). Those need English changes, not translation fixes.

## Counts

| verdict | entries |
|---|---|
| ok | ≈ 1,880 (everything not listed below) |
| fixed | 110 |
| needs native speaker | 13 concepts (≈ 30 entries, text left as is) |
| restructure (English must change) | 14 items (≈ 40 entries; 6 of them also got a best-effort fix) |

Confidence after review: **high** for meaning, register and grammar; **medium** for tone choices marked "native" below.

## Top issues found

1. **Status lines in first-person/imperative form.** German has no -ing; the draft used bare verb stems ("Verbinde neu…", "Sende 40 %", "Suche einen Anruf-Host…", "Erreiche das andere Gerät", "Versuche…", "Prüfe…", "Rufe an…"), which read as an order to the user or as "I connect". Changed to infinitive or passive status forms ("Neu verbinden…", "Anruf-Host wird gesucht…", "Wird geprüft…"). One was a real meaning error: `addcontact_why_slow` "Versuche es weiter" told the user to keep trying; the English means the app is still trying.
2. **Sentence-initial "Ihre" reads as Sie.** `groups_chat_left_footer` and `groupsc_left_group` ("Ihre Nachrichten bleiben hier") → "Die Nachrichten …".
3. **Calques and broken grammar.** "kann zu dir werden" (becomes you), "Deine wird Markierung 2 von 3", "Ohne Ende" (slang for "loads of") for *No end time*, "Ohne es hört man dich nicht", "Anrufe sind nur Sprache", "Während nicht erreichbar", "im Gleichschritt" (marching), "Erlaube die Kamera".
4. **Long buttons.** `settings_add_restore` 46 → 31 chars, desktop `link_restore_with_recovery_phrase` 53 → 38, `onboarding_done_add_contact` 32 → 25, `onboarding_phrase_copy_key` 25 → 18, `groupsb_remove_admin` 21 → 19, `groupsb_flag_removal` 24 → 23.
5. **"Open link" is one TM entry for two meanings** (group switch title *and* the "open in browser" button). "Offener Link" was wrong on the browser button; now "Link öffnen", which also works as a switch title. Needs an English split (restructure 13).

Also: Mute and Muted were both "Stumm", so the call button label never changed; Mute is now "Mikro aus", pairing with Unmute "Mikro an".

## Rulings on the translator's own doubts

| doubt | ruling |
|---|---|
| "trennen" for unlink (glossary: Verknüpfung lösen / trennen) | **Keep.** Short, natural, and the glossary allows it. Never "widerrufen" — correct. Native check: the status word "Getrennt" next to a device could be read as "disconnected"; if testers stumble, use "Nicht mehr verknüpft" for the status only. |
| "Freigeben" for the Unblock button | **Keep for the button** (a word German messenger UIs use for unblock; 9 chars vs "Nicht mehr blockieren" 21). The a11y label stays "{name} nicht mehr blockieren" and the heading "Blockierung aufheben". Native check whether the button should match those; "Freigeben" also means "share/release" in some UIs. |
| "Wiederherstellungswörter" for recovery phrase | **Correct, not a deviation.** The glossary says the UI term is "recovery words", and "Wiederherstellungsphrase" is forbidden only for the root secret. Where it got too long on buttons, "24 Wörter" is used ("Mit 24 Wörtern wiederherstellen"). |
| "Deine Leitung ist offen" | **Keep.** "frei" means "not busy", which is a different meaning; "offen" carries "you can be reached". Native check only for the feel. |
| Host vs Gastgeber | **Keep "Host".** "Gastgeber" sounds like a party host; "Host/hosten" is everyday German IT. Fallback stand-in for an unknown call host is now neuter "das Host-Gerät" so the following "es" agrees. |
| "Hab ich gesichert" | **Changed** to "Ich hab sie gesichert" / "Ich hab ihn gesichert" (21 chars). The casual tone fits the voice, but dropping "Ich" reads like chat shorthand, and the two buttons (key = ihn, words = sie) should differ like the English (it/them). Desktop "I have saved it" follows. |
| "Eigentümer" generic masculine | **Keep as the role label** (pill, "Eigentümer: {name}"); German apps use masculine role labels. Fixed the one place that went to "seine Besitzerin oder sein Besitzer" (long and a second word for the same role) with a neutral rephrase. Native/product check: if the owner should be gender-neutral, "Gruppe gehört: {name}" and "Die Gruppe gehört dir" (already used) are the neutral forms. |
| "Werte für Technikfans" | **Keep.** Plain, friendly, not slangy. Alternative if a native prefers the familiar YouTube phrasing: "Statistiken für Interessierte". |
| Sound names | **Keep** "Klassischer Gong", "Locker → straff", "Dreiklang-Klopfen"; fixed the description "dann zieht einer straff" (no object) → "dann spannt sich einer straff". "Schweben" is the right acoustic word for beating notes. Native check on "Dreiklang-Klopfen" (alt. "Dreiklang-Klopfer"). |
| Longest buttons | Fixed: settings_add_restore, link_restore_with_recovery_phrase, onboarding_done_add_contact, onboarding_phrase_copy_key, groupsb_remove_admin, groupsb_flag_removal. Kept: `unlock_forgot` "Ich hab sie vergessen" (21, text link on its own line; "Vergessen?" would lose the voice) and `onboarding_phrase_elsewhere_key` "Ich hab ihn woanders gesichert" (30 = 1.5× of 20, text button). |

## Needs a native speaker (text left as is)

| key(s) | current | concern / candidate |
|---|---|---|
| `settings_log_note` | Stammgeheimnis | Only use of the root-secret term; glossary marks it **verify** (alt. "Ursprungsgeheimnis"). |
| `onboarding_done_title`, `notif_idle_available_text`, `addcontact_why_added` … | Deine Leitung ist offen | "offen" vs "frei" feel (see rulings). |
| `blocking_unblock` | Freigeben | Match "nicht mehr blockieren" or keep the short messenger word (see rulings). |
| `devices_sub_unlinked`, `devices_unlinked_label` | Getrennt / GETRENNT | Could read as "disconnected"; alt. "Nicht mehr verknüpft". |
| `groupsb_pill_owner`, `groupsb_hero_owner`, `groups_list_new_sub` … | Eigentümer / Du wirst Eigentümer | Generic masculine; product decision on gender-neutral wording. |
| `unlock_forgot`, `onboarding_phrase_saved_*`, `onboarding_phrase_elsewhere_*` | Ich hab sie vergessen / Ich hab sie gesichert | Casual "hab" tone in buttons: fine for the voice? |
| `profile_saved_offline` | Gespeichert · synct bei Verbindung | "synct" is a common anglicism; alt. "Gespeichert · Abgleich folgt". |
| `settings_quiet`, `settings_sounds_note_quiet` | Lautlos (changed from "Ruhe") | Confirm "Lautlos" (phone's word) over "Ruhemodus". |
| `notif_mute`, `notif_unmute`, `gcall_muted` | Mikro aus / Mikro an / Stumm | Confirm the trio on the call screen. |
| `settings_sound_triad` | Dreiklang-Klopfen | Alt. "Dreiklang-Klopfer". |
| `call_stats_for_nerds` (desktop) | Werte für Technikfans | Alt. "Statistiken für Interessierte". |
| `settings_backup_private`, `link_approve_*`, `profile_*` | … kann als du auftreten | Grammatical but slightly unusual; alt. "kann sich als dich ausgeben" / "kann in deinem Namen handeln". |
| `settings_dl_tap_sub`, `profile_each_shows_a_soft_preview` | weiche Vorschau | alt. "unscharfe Vorschau". |

## Restructure (English must change)

Each item: what breaks in German, the best German now in the file, and a suggested English change.

1. **You/Someone slotted into {by}/{name}** — `common_you` (Du), `common_someone` (Jemand), used in `groupsc_admin_granted_other` "{by} hat {sub} zum Admin gemacht", `groupsc_admin_revoked_other`, iOS `group_sys_admin` / `group_sys_unadmin`. With {by}=Du German needs "Du hast", not "Du hat"; with {sub}=Du it needs "dich". *Confirmed.* English: add one string per person, e.g. "You made {name} an admin", "{by} made you an admin" (exists), "You removed {name} as admin"; never pass "You" into {by}/{sub}.
2. **Case-dependent stand-ins** — `groupsb_the_group` (die Gruppe), `groupsb_not_in_contacts` (dative "jemandem, der nicht in deinen Kontakten ist": right after "Eingeladen von", wrong after "Eigentümer:"), `onboarding_other_device` / `link_fallback_other_device` (dative "deinem anderen Gerät"), `devices_fallback_the_device` (dative "dem Gerät"), `card_them` / `group_fallback_them` (dative "der Person": right in "Mit der Person chatten?", wrong in "Einladung an {name} gesendet", which needs "die Person"), `groupsc_they` (Die Person). *Confirmed.* English: give each host sentence its own no-name variant ("Invitation sent", "Invited by someone outside your contacts", "Owner not in your contacts", "Syncing…").
3. **`avail_you`** (du) inside `avail_note` "„{name} nicht erreichbar“" gives "„du nicht erreichbar“". *Confirmed.* English: a separate no-name note: "While you're away, people who call see that you can't be reached."
4. **`kit_tick_their`** (ihrem) as a stand-in for {first} in "On {first}'s device" → "Auf dem Gerät von ihrem". *Confirmed.* English: separate no-name tick labels ("On their device", "Read on their device", "Sent, waiting for their device").
5. **`link_approve_bold` / `link_approve_warning` / iOS `link_approve_body`** — works in German now because the bold part is plural ("gehen deine Wiederherstellungswörter an {name}"), but it relies on the bold fragment's grammar. *Corrected: not broken today, fragile.* English: one string with inline markup, "Approving sends your **recovery phrase** to {name}…".
6. **Desktop English glued in code** — `signup_and_have_read_the` (" zu und hast gelesen: " splits the verb "stimmst … zu" across fragments), `signup_word_in_the_recovery` ("Word {value}{value2} {value3} …" with "isn't"/"and N more aren't" glued in), `gcall_this_call_everyone_in` (" {value} diesen Anruf" with {value} = "trägt"), `chat_you_blocked` ({when} glued), `group_invited_you_to` (" hat dich eingeladen: "). *Confirmed.* English: whole sentences with placeholders, like iOS `onboarding_terms_consent` and Android `onboarding_restore_bad_word(s)` already do.
7. **{noun} = "computer"** (`signup_tinline_connects_your_straight`, `signup_type_your_24_recovery`, `signup_type_your_24_words`, `signup_your_account_is_its`, `signup_your_current_account_stays`, `signup_your_recovery_key_is`). New finding: `gate_noun()` in `crates/desktop/src/signup_view.rs` returns the untranslated English word "computer", so German shows "deinen computer". The German now uses masculine forms (deinen/diesem) to match Computer. English: drop {noun} and write "computer" into each desktop string.
8. **Desktop profile {what}** (`profile_the_cant_be_empty`, `profile_the_cant_contain_control`, `profile_that_is_too_long_up`, `profile_that_is_too_long`, `profile_a_status`). New finding: `check_name`/`check_text` in `crates/desktop/src/profile_ui.rs` pass the untranslated words "name", "nickname", "about", so German reads "name ist zu lang". English: one string per field ("The name can't be empty." already exists on Android/iOS as `profile_err_name_empty`; reuse those).
9. **Missing plurals** — desktop `gcall_people_in_the_call` ({count} Personen im Anruf), `group_invite_people` ({len} Personen einladen), `signup_words_are_still_missing` ({filled} Wörter fehlen noch), `profile_not_seen_for_days` (Seit {value} Tagen nicht gesehen). With 1 they read "1 Personen", "1 Wörter". *Confirmed.* English: make them plural entries like the Android/iOS twins (`gcall_people`, `groupsb_invite_people`, `onboarding_restore_missing`).
10. **gcall_about_* host as sentence subject** — `gcall_about_is_carrying`, `gcall_about_carries`, desktop `gcall_is_carrying` follow a bold host name: "dieses Handy" works, "Gerät von Sam" (`gcall_host_device`, desktop `gcall_s_device`) lacks the article at sentence start, and the follow-up pronoun must match the host's gender. Fixed the unknown-host fallback to neuter "das Host-Gerät". *Confirmed.* English: full sentences per host kind ("This phone is carrying this call. …", "{name}'s device carries this call. …").
11. **`group_a_message`** (desktop composer, "eine Nachricht") — glued into a composer placeholder; article/case depends on the host. English: full placeholder strings.
12. **Owner/admin role words mid-sentence** — `groupsb_role_owner_lc` etc. are fine in German as spoken list parts (no inflection needed). *Corrected: not a restructure for German.*
13. **"Open link" homograph** (new) — one English phrase for the group switch title (`groupsb_open_link`, a noun: the open link) and the browser button (`profile_open_link_button`, iOS `people_profile_open_link`, a verb). English: rename the switch to "Open invite link" or "Public link" so the TM gets two entries.
14. **"Recovery phrase" header vs words** — `onboarding_step_phrase` header label "Recovery phrase" but the UI says "recovery words"; German already uses Wörter. Suggest the English header say "Recovery words" for one term per screen (VOICE: same action, same words).

## Every change

| # | key (first of entry) | before | after | why |
|---|---|---|---|---|
| 1 | `groups_chat_left_footer` | Du hast diese Gruppe verlassen. Ihre Nachrichten bleiben hier, nur zum Lesen. | Du hast diese Gruppe verlassen. Die Nachrichten bleiben hier, nur zum Lesen. | sentence-initial "Ihre" reads as the formal Sie register |
| 2 | `groups_msg_who_has_it` | Wer hat sie | Wer sie erhalten hat | "Wer hat sie" reads as an unfinished question |
| 3 | `groupsb_callhosts_note_admin` | Lege einen über das Menü eines Mitglieds fest oder hoste Anrufe auf diesem Handy. Ein Handy hostet nur Anrufe, die seine Besitzerin oder sein Besitzer startet. | Lege einen über das Menü eines Mitglieds fest oder hoste Anrufe auf diesem Handy. Ein Handy hostet nur Anrufe, die auf ihm gestartet werden. | "seine Besitzerin oder sein Besitzer" long and clashes with "Eigentümer" (group role); neutral rephrase |
| 4 | `groupsb_remove_admin` | Admin-Rolle entziehen | Als Admin entfernen | "entziehen" sounds punitive; matches "{device} als Host entfernen"; shorter (19) |
| 5 | `groupsb_open_link` | Offener Link | Link öffnen | entry is also the browser button (profile, iOS); "Offener Link" is wrong on a button; verb phrase also works as a switch title |
| 6 | `groupsb_turn_on_open` | Schalte „Offener Link“ ein, um ihn zu teilen | Schalte den offenen Link ein, um ihn zu teilen | no longer quotes the switch title, which is now "Link öffnen" |
| 7 | `groupsb_flag_removal` | Zur Entfernung markieren | Zum Entfernen markieren | verb form reads more naturally than the noun "Entfernung"; one char shorter |
| 8 | `groupsb_flag_title` | {name} zur Entfernung markieren? | {name} zum Entfernen markieren? | consistent with the menu item |
| 9 | `groupsb_new_group_host_note` | Dieses Handy hostet die Gruppe zuerst: Es hält die Gruppe und gibt Nachrichten weiter. Ein Computer, der an bleibt, ist ein besserer Host – leg ihn in der Gruppeninfo fest, sobald er beigetreten ist. | Dieses Handy hostet die Gruppe vorerst: Es hält die Gruppe und gibt Nachrichten weiter. Ein Computer, der an bleibt, ist ein besserer Host – leg ihn in der Gruppeninfo fest, sobald er beigetreten ist. | "zuerst" = first in a sequence; "at first" here means "for now" (vorerst) |
| 10 | `groupsb_bad_link` | Mit diesem Link ging es nicht: Er ist keine Gruppeneinladung, ist abgelaufen, oder sein Host ist offline. | Dieser Link hat nicht funktioniert: Er ist keine Gruppeneinladung, ist abgelaufen oder sein Host ist offline. | "Mit diesem Link ging es nicht" is clumsy; no comma before "oder" in a list |
| 11 | `unlock_wrong_passphrase` | Mit dieser Passphrase ging das Konto nicht auf. | Diese Passphrase hat das Konto nicht geöffnet. | "ging das Konto nicht auf" too colloquial for the unlock error |
| 12 | `accounts_not_on_call` | Du bist in keinem Anruf. Wärst du es, würde der Wechsel ihn beenden. | Du telefonierst gerade nicht. Sonst würde der Wechsel den Anruf beenden. | "Wärst du es" is stilted |
| 13 | `profile_remove_field_body` | Es verschwindet aus deinem Profil. Frühere Versionen können im synchronisierten Verlauf bleiben. | Die Angabe verschwindet aus deinem Profil. Frühere Versionen können im synchronisierten Verlauf bleiben. | "Es" has no neuter antecedent (die Angabe) |
| 14 | `profile_busy_open` | Ohne Ende | Bis auf Weiteres | "Ohne Ende" is slang for "loads of"; "Bis auf Weiteres" is the idiom for an open end |
| 15 | `profile_err_need_value` | Trag etwas ein, das gespeichert werden kann. | Trag etwas ein, um es zu speichern. | "das gespeichert werden kann" sounds like only some input can be saved |
| 16 | `profile_err_account_changed` | Das ausgewählte Konto hat gewechselt. Im neuen Konto wurde nichts gespeichert. | Das ausgewählte Konto hat sich geändert. Im neuen Konto wurde nichts gespeichert. | "hat gewechselt" needs a reflexive/other verb here |
| 17 | `profile_pick_wrong_type` | Tinline kann PNG-, JPEG- oder WebP-Bilder verwenden. Diese Datei ist von einer anderen Art. | Tinline kann PNG-, JPEG- oder WebP-Bilder verwenden. Diese Datei hat ein anderes Format. | "von einer anderen Art" is a calque |
| 18 | `onboarding_restore_key_typo` | In diesem Schlüssel steckt ein Tippfehler, oder er ist abgeschnitten. Füg ihn nochmal ein, von da, wo du ihn gespeichert hast. | In diesem Schlüssel steckt ein Tippfehler, oder er ist abgeschnitten. Füg ihn nochmal aus deiner gespeicherten Kopie ein. | "von da, wo du ihn gespeichert hast" is clumsy |
| 19 | `onboarding_tip_devices_body` | Verknüpfe ein weiteres Gerät, wenn du kannst, damit du nie ausgesperrt bist, wenn du eins verlierst. | Verknüpfe ein weiteres Gerät, sobald es geht. Dann sperrt dich ein verlorenes Gerät nie aus. | double "wenn" clause was heavy; same meaning |
| 20 | `onboarding_phrase_saved_key` | Hab ich gesichert | Ich hab ihn gesichert | rule on translator doubt: keep the casual tone but keep the subject "Ich" (dropping it reads like chat shorthand); "ihn" = key; 21 chars |
| 21 | `onboarding_phrase_saved_words` | Hab ich gesichert | Ich hab sie gesichert | as above; "sie" = words, so the two buttons now differ like the English (it/them) |
| 22 | `onboarding_linked_lead` | Deine Kontakte und Chats kommen von {device} und bleiben auf beiden im Gleichschritt. | Deine Kontakte und Chats kommen von {device} und bleiben auf beiden Geräten synchron. | "im Gleichschritt" has a marching connotation |
| 23 | `onboarding_savefail_trying` | Versuche… | Neuer Versuch… | "Versuche…" reads as an imperative or "I try" |
| 24 | `settings_video_in_calls_off` | Anrufe sind nur Sprache | Nur Sprachanrufe | "Anrufe sind nur Sprache" is ungrammatical |
| 25 | `settings_add_restore` | Mit Wiederherstellungswörtern wiederherstellen | Mit 24 Wörtern wiederherstellen | 46 -> 31 chars, no "Wiederherstellung… wiederherstellen" repetition; same wording as the key-lost restore button |
| 26 | `notif_alert_about` | Benachrichtige mich bei | Mich benachrichtigen bei | "Benachrichtige mich" is a du-imperative to the app; first-person infinitive like the buttons |
| 27 | `settings_quiet` | Ruhe | Lautlos | "Ruhe" reads as "Silence!"; "Lautlos" is the phone's own word for no sounds |
| 28 | `settings_sounds_note_quiet` | Ruhe ist an, also sind die Vorschauen stumm. | Lautlos ist an, also sind die Vorschauen stumm. | follows the switch rename |
| 29 | `settings_sound_slack_sub` | Zwei nahe Töne schweben wie eine lockere Saite, dann zieht einer straff. | Zwei nahe Töne schweben wie eine lockere Saite, dann spannt sich einer straff. | "zieht einer straff" lacks an object |
| 30 | `settings_backup_copied` | Kopiert. Bewahre es an einem privaten Ort auf. | Kopiert. Bewahre ihn an einem privaten Ort auf. | it = der Schlüssel (masculine) |
| 31 | `devices_syncing` | Gleiche mit {name} ab… | Abgleich mit {name}… | "Gleiche … ab" reads as an imperative; noun form also fits the dative fallback "dem Gerät" |
| 32 | `devices_remove_only` | Das ist das einzige Gerät mit deinem Konto. Ohne deine Wiederherstellungswörter kann es nichts zurückholen. | Das ist das einzige Gerät mit deinem Konto. Ohne deine Wiederherstellungswörter lässt es sich nicht zurückholen. | "kann es nichts zurückholen" was ambiguous ("it can't bring anything back") |
| 33 | `devices_unlink_warn` | Trennen kann es von hier aus nicht sperren. Wer es entsperren kann, kann dein Konto weiter nutzen. Ganz abschneiden kann nur ein neues Konto. | Trennen kann es von hier aus nicht sperren. Wer es entsperren kann, kann dein Konto weiter nutzen. Ganz aussperren kannst du es nur mit einem neuen Konto. | "Ganz abschneiden kann nur ein neues Konto" is unclear; meaning kept, no over-claim |
| 34 | `link_reaching` | Erreiche das andere Gerät | Verbindung zum anderen Gerät | "Erreiche …" reads as imperative or first person |
| 35 | `link_sending` | Sende dein Konto | Dein Konto wird gesendet | "Sende dein Konto" reads as an imperative |
| 36 | `link_scan_step2` | 2.  Es zeigt einen QR-Code und einen kurzen Code. Sie funktionieren 5 Minuten lang, einmal. | 2.  Es zeigt einen QR-Code und einen kurzen Code. Sie gelten 5 Minuten und funktionieren einmal. | "5 Minuten lang, einmal" is a calque |
| 37 | `notif_idle_offline` | Offline – verbinde neu | Offline – verbindet neu | "verbinde neu" reads as first person/imperative |
| 38 | `conv_allow_camera` | Erlaube die Kamera, um Fotos zu machen. | Erlaube den Kamerazugriff, um Fotos zu machen. | "Erlaube die Kamera" is not idiomatic |
| 39 | `conv_older_body` | Dieses Handy hat sie ab dem Tag, an dem du es zu nutzen begonnen hast. Frühere Tage laden, wenn ihr beide online seid. | Dieses Handy hat sie ab dem Tag, an dem du es zu nutzen begonnen hast. Frühere Tage werden geladen, wenn ihr beide online seid. | intransitive "laden" is colloquial |
| 40 | `conv_meta_sending` | Sende {percent} % · {time} | Senden {percent} % · {time} | "Sende" reads as an imperative |
| 41 | `groupsc_admin_revoked_you` | {by} hat dir die Admin-Rolle entzogen | {by} hat dich als Admin entfernt | same verb as the menu item "Als Admin entfernen"; "entziehen" sounds punitive |
| 42 | `groupsc_admin_revoked_other` | {by} hat {sub} die Admin-Rolle entzogen | {by} hat {sub} als Admin entfernt | as above |
| 43 | `groupsc_req_accepted_connecting` | {name} hat angenommen · verbinde | {name} hat angenommen · verbindet | "verbinde" reads as first person |
| 44 | `groupsc_left_group` | Du hast die Gruppe verlassen. Ihre Nachrichten bleiben hier. | Du hast die Gruppe verlassen. Die Nachrichten bleiben hier. | sentence-initial "Ihre" reads as Sie |
| 45 | `gcall_finding_host` | Suche einen Anruf-Host… | Anruf-Host wird gesucht… | "Suche einen …" reads as imperative/first person |
| 46 | `gcall_reconnecting` | Verbinde neu… | Neu verbinden… | "Verbinde neu…" reads as imperative; same infinitive style as "Verbinden…" |
| 47 | `gcall_the_host` | der Anruf-Host | das Host-Gerät | neuter stand-in so the following "es gibt den Ton weiter" agrees ("der Anruf-Host … es" did not) |
| 48 | `addcontact_line_connecting` | Verbinde | Verbindung wird aufgebaut | "Verbinde" reads as imperative |
| 49 | `addcontact_many_qr` | Mehr als ein QR-Code. Schneide auf den zu, den du möchtest. | Mehr als ein QR-Code. Schneide das Bild auf den Code zu, den du möchtest. | "auf den zu" is hard to parse |
| 50 | `addcontact_why_slow` | Versuche es weiter. Tinline muss dafür bei {name} offen und online sein. | Wird noch versucht. Tinline muss dafür bei {name} offen und online sein. | "Versuche es weiter" told the user to keep trying; English means the app is still trying |
| 51 | `addcontact_why_reaching` | Erreiche Tinline bei {name}… | Verbindung zu Tinline bei {name}… | "Erreiche …" reads as imperative |
| 52 | `call_flip` | Drehen | Wechseln | "Drehen" means rotate; the camera-switch caption is "Wechseln" in German phone UIs |
| 53 | `call_reconnecting_plain` | Verbinde neu | Neu verbinden | "Verbinde neu" reads as imperative |
| 54 | `call_about_reconnecting` | Über diesen Anruf. Verbinde neu. | Über diesen Anruf. Neu verbinden. | as above |
| 55 | `call_route_title` | Anruf abspielen über | Anruf hören über | "Anruf abspielen" sounds like playing a recording |
| 56 | `call_info_weak` | Die Leitung ist schwach, die Stimme von {name} kann abbrechen. | Die Leitung ist schwach, daher kann die Stimme von {name} stocken. | comma splice; "stocken" is the word for breaking-up audio |
| 57 | `call_mic_needed_body` | Ohne es hört man dich nicht. Tinline hört nur während Anrufen zu – nie im Hintergrund. | Ohne Mikrofon hört man dich nicht. Tinline hört nur während Anrufen zu – nie im Hintergrund. | "Ohne es" is ungrammatical |
| 58 | `call_status_calling` | Rufe an… | Anrufen… | "Rufe an…" reads as imperative/first person; infinitive like "Verbinden…" |
| 59 | `call_privacy_connecting` | Verbinde mit {name}. Sobald der Anruf verbunden ist, ist er Ende-zu-Ende-verschlüsselt. | Verbindung mit {name} wird aufgebaut. Sobald der Anruf verbunden ist, ist er Ende-zu-Ende-verschlüsselt. | "Verbinde mit" reads as imperative |
| 60 | `history_kind_away` | Während nicht erreichbar | Während deiner Pause | "Während nicht erreichbar" is ungrammatical; matches the "Pause machen" section |
| 61 | `home_first_waiting` | Wartet, bis gescannt wird – lass das hier offen. | Wartet aufs Scannen – lass das hier offen. | smoother than "Wartet, bis gescannt wird" |
| 62 | `main_told_blocked` | {name} erfährt, dass du blockiert hast | {name} erfährt von der Blockierung | "dass du blockiert hast" lacks its object |
| 63 | `group_callhosts_note_admin` | Ein Anruf-Host trägt den Anruf. Alle im Anruf verbinden sich mit ihm, und er gibt den Ton weiter, kann den Anruf also hören. Das ist getrennt vom Chat-Hosten: Ein Chat-Host ist kein Anruf-Host, außer du legst ihn auch als solchen fest. Anruf-Hosts sind Mitglieder, denen du vertraust, und jeder entscheidet selbst, ob er zusagt. Lege einen über das Menü eines Mitglieds fest oder hoste Anrufe auf diesem Handy. Ein Handy hostet nur Anrufe, die seine Besitzerin oder sein Besitzer startet. | Ein Anruf-Host trägt den Anruf. Alle im Anruf verbinden sich mit ihm, und er gibt den Ton weiter, kann den Anruf also hören. Das ist getrennt vom Chat-Hosten: Ein Chat-Host ist kein Anruf-Host, außer du legst ihn auch als solchen fest. Anruf-Hosts sind Mitglieder, denen du vertraust, und jeder entscheidet selbst, ob er zusagt. Lege einen über das Menü eines Mitglieds fest oder hoste Anrufe auf diesem Handy. Ein Handy hostet nur Anrufe, die auf ihm gestartet werden. | same neutral rephrase as the Android string |
| 64 | `group_sys_unadmin` | {by} hat {name} die Admin-Rolle entzogen | {by} hat {name} als Admin entfernt | same verb as the menu item |
| 65 | `group_flag_copy_final` | Deine wird Markierung {yours} von {total}. {name} wird sofort entfernt: Hosts nehmen keine neuen Nachrichten mehr an, und die Person sieht, dass sie entfernt wurde. | Das wird Markierung {yours} von {total}. {name} wird sofort entfernt: Hosts nehmen keine neuen Nachrichten mehr an, und die Person sieht, dass sie entfernt wurde. | "Deine wird Markierung" is ungrammatical |
| 66 | `group_flag_copy` | Deine wird Markierung {yours} von {total}. Markierungen aller Admins zählen, auch weitere von dir. Bei {total} Markierungen wird {name} sofort entfernt und sieht, dass die Entfernung passiert ist. | Das wird Markierung {yours} von {total}. Markierungen aller Admins zählen, auch weitere von dir. Bei {total} Markierungen wird {name} sofort entfernt und sieht das. | as above; simpler ending |
| 67 | `onboarding_linked_lead` | Deine Kontakte und Chats kommen von {name} und bleiben auf beiden im Gleichschritt. | Deine Kontakte und Chats kommen von {name} und bleiben auf beiden Geräten synchron. | "im Gleichschritt" has a marching connotation |
| 68 | `shell_flow_account_changed` | Konto hat gewechselt | Konto hat sich geändert | "hat gewechselt" needs the reflexive here |
| 69 | `common_decoder_for_their_video` | Decoder für ihr Video | Decoder für das Video der Gegenseite | same wording as "Video der Gegenseite"; "ihr Video" is ambiguous |
| 70 | `common_messages_only_calling_needs` | Nur Nachrichten: Anrufe brauchen ihre Erlaubnis | Nur Nachrichten: Anrufe brauchen die Erlaubnis der Person | "ihre" ambiguous (her/their) |
| 71 | `audio_disconnected_switching_to_the` | {label} getrennt, wechsle zum Systemstandard | {label} getrennt, Wechsel zum Systemstandard | "wechsle" reads as first person |
| 72 | `call_ignore_limits_heat_their` | Grenzen ignorieren (Wärme, ihr Decoder) | Grenzen ignorieren (Wärme, Decoder der Gegenseite) | "ihr Decoder" is ambiguous |
| 73 | `call_the_line_is_weak` |  Die Leitung ist schwach, die Stimme von {name} kann abbrechen. |  Die Leitung ist schwach, daher kann die Stimme von {name} stocken. | same fix as the Android string (leading space kept) |
| 74 | `call_your_network_changed_hold` | Dein Netzwerk hat gewechselt. Einen Moment – Tinline sucht die Leitung wieder. | Dein Netzwerk hat sich geändert. Einen Moment – Tinline sucht die Leitung wieder. | "hat gewechselt" needs the reflexive here |
| 75 | `call_your_size` | Deine Größe | Deine Bildgröße | "Deine Größe" reads as body height |
| 76 | `chat_older_messages_are_on` | Ältere Nachrichten sind auf dem Computer von {name}. Dieser Computer hat sie ab dem Tag, an dem du ihn zu nutzen begonnen hast. Frühere Tage laden, wenn ihr beide online seid. | Ältere Nachrichten sind auf dem Computer von {name}. Dieser Computer hat sie ab dem Tag, an dem du ihn zu nutzen begonnen hast. Frühere Tage werden geladen, wenn ihr beide online seid. | intransitive "laden" is colloquial |
| 77 | `chat_too_short_hold_on` | Zu kurz – halte ein bisschen länger. | Zu kurz – halte etwas länger gedrückt. | "halte ein bisschen länger" lacks "gedrückt" |
| 78 | `gate_anyone_who_sees_these` | Wer diese Wörter sieht, kann zu dir werden. Schreib sie auf und bewahre sie an einem sicheren Ort auf. | Wer diese Wörter sieht, kann dein Konto übernehmen. Schreib sie auf und bewahre sie an einem sicheren Ort auf. | "kann zu dir werden" is a calque; same meaning, plain |
| 79 | `gate_i_have_saved_it` | Hab ich gesichert | Ich hab sie gesichert | same ruling as the mobile "I've saved them" (refers to the words) |
| 80 | `gate_retry` | Nochmal | Nochmal versuchen | same label as every other Try again |
| 81 | `gate_this_computer_no_longer` | Dieser Computer hat den Schlüssel nicht mehr, der es öffnet, und sein Eintrag im Schlüsselbund fehlt. Sobald du wiederherstellst, kommen deine Anrufe wieder durch. | Dieser Computer hat den Schlüssel nicht mehr, der dein Konto öffnet, und sein Eintrag im Schlüsselbund fehlt. Sobald du wiederherstellst, kommen deine Anrufe wieder durch. | "der es öffnet" had no antecedent |
| 82 | `gate_trying` | Versuche... | Neuer Versuch... | "Versuche..." reads as imperative/first person |
| 83 | `gcall_the_call_host` | Der Anruf-Host | Das Host-Gerät | neuter so the following "es gibt den Ton weiter" agrees |
| 84 | `group_a_call_host_is` | Ein Anruf-Host ist der Computer oder das Handy eines Mitglieds, der oder das den Anruf trägt. Ein Admin kann ihn in der Gruppeninfo festlegen. | Ein Anruf-Host ist ein Computer oder Handy eines Mitglieds und trägt den Anruf. Ein Admin kann ihn in der Gruppeninfo festlegen. | "der oder das den Anruf trägt" is clumsy |
| 85 | `group_a_call_host_is_a` | Ein Anruf-Host ist der Computer oder das Handy eines Mitglieds, der oder das den Anruf trägt. Du kannst Anrufe auf diesem Computer hosten oder in der Gruppeninfo ein Mitglied festlegen. | Ein Anruf-Host ist ein Computer oder Handy eines Mitglieds und trägt den Anruf. Du kannst Anrufe auf diesem Computer hosten oder in der Gruppeninfo ein Mitglied festlegen. | as above |
| 86 | `group_flag_to_remove` | Zur Entfernung markieren | Zum Entfernen markieren | same wording as the mobile menu item |
| 87 | `group_remove_as_admin` | Admin-Rolle entziehen | Als Admin entfernen | same wording as the mobile menu item |
| 88 | `group_remove_as_admin_2` | {name} die Admin-Rolle entziehen? | {name} als Admin entfernen? | as above |
| 89 | `group_stop_device_carrying_calls` | Anrufe auf Gerät {value} beenden | Anruf-Hosten auf Gerät {value} beenden | "Anrufe … beenden" meant hanging up calls, not stopping call hosting |
| 90 | `group_youll_own_the_group` | Die Gruppe gehört dir. Dieser Computer hostet sie zuerst: Er hält die Gruppe und gibt Nachrichten weiter, während die Handys der Mitglieder schlafen, deshalb klappt es am besten auf einem Computer, der an bleibt. | Die Gruppe gehört dir. Dieser Computer hostet sie vorerst: Er hält die Gruppe und gibt Nachrichten weiter, während die Handys der Mitglieder schlafen, deshalb klappt es am besten auf einem Computer, der an bleibt. | "zuerst" -> "vorerst" (at first = for now) |
| 91 | `group_yours_will_be_flag` | Deine wird Markierung {yours} von {flags_needed}. Markierungen aller Admins zählen, auch weitere von dir. Bei {flags_needed} Markierungen wird {name} sofort entfernt und sieht, dass die Entfernung passiert ist. | Das wird Markierung {yours} von {flags_needed}. Markierungen aller Admins zählen, auch weitere von dir. Bei {flags_needed} Markierungen wird {name} sofort entfernt und sieht das. | "Deine wird Markierung" is ungrammatical |
| 92 | `group_yours_will_be_flag_of` | Deine wird Markierung {yours} von {flags_needed}. {name} wird sofort entfernt: Hosts nehmen keine neuen Nachrichten mehr an, und die Person sieht, dass sie entfernt wurde. | Das wird Markierung {yours} von {flags_needed}. {name} wird sofort entfernt: Hosts nehmen keine neuen Nachrichten mehr an, und die Person sieht, dass sie entfernt wurde. | as above |
| 93 | `link_a_linked_device_becomes` | Ein verknüpftes Gerät wird zu dir | Ein verknüpftes Gerät kann alles, was du kannst | "wird zu dir" is a calque |
| 94 | `link_show_this_to_the` | Zeig das dem Gerät, das du hinzufügen möchtest. Es wird zu dir, also verknüpfe nur deine eigenen. | Zeig das dem Gerät, das du hinzufügen möchtest. Es kann dann alles, was du kannst, also verknüpfe nur deine eigenen Geräte. | as above |
| 95 | `link_connecting_to_the_other` | Verbinde mit dem anderen Gerät… | Verbindung zum anderen Gerät… | "Verbinde mit" reads as imperative |
| 96 | `link_restore_with_recovery_phrase` | Lieber mit Wiederherstellungswörtern wiederherstellen | Lieber mit 24 Wörtern wiederherstellen | 53 -> 38 chars, no "Wiederherstellungs… wiederherstellen" repetition |
| 97 | `profile_anyone_who_still_has` |  Wer deine Wiederherstellungswörter noch hat oder {name} entsperren kann, kann weiter als du auftreten. Wenn es verloren oder gestohlen ist, hilft ganz nur ein neues Konto. |  Wer deine Wiederherstellungswörter noch hat oder {name} entsperren kann, kann weiter als du auftreten. Wenn es verloren oder gestohlen ist, ist ein neues Konto die einzige vollständige Lösung. | "hilft ganz nur" is ungrammatical |
| 98 | `profile_checking` | Prüfe… | Wird geprüft… | "Prüfe…" reads as imperative |
| 99 | `settings_checking` | Prüfe... | Wird geprüft... | as above |
| 100 | `profile_it_is_offline_so` | Es ist offline und erfährt es also, wenn es sich das nächste Mal verbindet. | Es ist offline und erfährt es, sobald es sich das nächste Mal verbindet. | smoother |
| 101 | `profile_selected_connecting` | Ausgewählt · verbinde | Ausgewählt · verbindet | "verbinde" reads as first person |
| 102 | `profile_told_you_blocked_them` | Weiß, dass du blockiert hast | Weiß von der Blockierung | "dass du blockiert hast" lacks its object |
| 103 | `settings_anyone_who_sees_these` | Wer diese Wörter sieht, kann zu dir werden. Achte darauf, dass niemand zuschaut, und zeig sie dann. | Wer diese Wörter sieht, kann dein Konto übernehmen. Achte darauf, dass niemand zuschaut, und zeig sie dann. | "kann zu dir werden" is a calque |
| 104 | `settings_anyone_who_sees_these_words` | Wer diese Wörter sieht, kann zu dir werden. Achte darauf, dass niemand zuschaut. | Wer diese Wörter sieht, kann dein Konto übernehmen. Achte darauf, dass niemand zuschaut. | as above |
| 105 | `shell_searching` | Suche… | Wird gesucht… | "Suche…" can read as imperative |
| 106 | `signup_tinline_connects_your_straight` | Tinline verbindet dein {noun} direkt mit dem der anderen. | Tinline verbindet deinen {noun} direkt mit dem der anderen. | {noun} is "computer" (masculine), so accusative "deinen" |
| 107 | `signup_your_account_is_its` | Dein Konto steckt in seinen Wiederherstellungswörtern, also kannst du es auf zwei Wegen auf dein {noun} holen. | Dein Konto steckt in seinen Wiederherstellungswörtern, also kannst du es auf zwei Wegen auf deinen {noun} holen. | as above |
| 108 | `notif_mute` | Stumm | Mikro aus | "Stumm" was also the label for the Muted state, so the button never changed; pairs with "Mikro an" (glossary option) |
| 109 | `onboarding_done_add_contact` | Meinen ersten Kontakt hinzufügen | Ersten Kontakt hinzufügen | 32 -> 25 chars on a primary button; infinitive keeps the user's voice |
| 110 | `onboarding_phrase_copy_key` | Meinen Schlüssel kopieren | Schlüssel kopieren | 25 -> 18 chars (English 11); same label as the recovery-backup page |
