# Finnish (fi) review

Language: fi · File: i18n/tm/fi.toml (applies to android, ios, desktop) · Entries: 2053 ·
ok 1877 · fixed 78 · native 8 (+5 overlapping fixed/restructure) · restructure 90

Reviewer: `i18n-review-fi` (agent, 2026-10-25). Not a native speaker's sign-off.

**Verdict: ship as draft.** The translation is good: correct sinä register throughout,
correct partitive after numerals, plurals complete, placeholders intact
(`i18n_gen.py --check` passes), glossary terms used consistently, no exclamation marks
or alarm words. The fixes below are calques ("erillään", "yhdistävät siihen",
"lukitse ulos"), a few agreement slips, and wording that read like a log. The
remaining risk is the glued and slotted English strings (restructure list), which
no Finnish wording can make fully grammatical.

## Rulings on the translator's open questions

| Question | Ruling |
|---|---|
| host = "isäntälaite", verb "isännöidä" | Keep. Hosts are devices, so "isäntälaite" is accurate and avoids "palvelin"; "isännöidä" is the normal verb. One soft spot: the role pill on a *member* row (`groupsb_host`) says "Isäntälaite" about a person (native check). |
| call host = "puheluisäntä" | Keep. "puheluisäntälaite" is too long for pills; the shorter compound reads fine. I made "isäntä" alone into "puheluisäntä" where it stood for the call host. |
| admin = "ylläpitäjä" | Keep. For the role as a thing you remove I replaced "ylläpitäjyys" with "ylläpito-oikeudet" (everyday word). |
| online = "paikalla" (people) / "verkossa" (devices) | Keep the split; it is what Finnish messengers do. Problem: `groupsb_online`/`groupsb_offline` (host device rows) share the English with the people label, so host rows show "Paikalla/Poissa" (new restructure item R-B). |
| `common_call` = "Puhelu" | Keep "Puhelu" for now. Half its uses are titles (notification fallback, history fallback), and as a button it pairs with "Viesti" and "Video" the way Finnish contact screens do. The real fix is the split already in RESTRUCTURE §9; the button should then be "Soita". |
| "palautusavain" / "palautussanat" | Keep. They are the readable forms of the root secret, never called "salasana"; "juurisalaisuus" is used where the root secret itself is meant. |
| flag = "merkintä" | Acceptable but weak: "merkintä" is a neutral "mark/entry". Native speaker to choose; candidate "poistomerkintä" (clear, still neutral). Not "ilmianto" (too strong). |
| partitive "Tinlinea" | Keep. Finnish inflects foreign names by pronunciation; /tinlain/ has a back vowel, so Tinlinen, Tinlinessa, Tinlinea, Tinlinesta are consistent (all 27 uses). Native to confirm nobody reads it as "tin-li-ne" (which would give Tinlinessä). |
| Left in English: "Ev", "Cut", "Cut+probe", "Trend+probe" | Keep. "Ev" is a debug format string (RESTRUCTURE §10); the others are developer video-tuning modes on desktop only. |

## Changes made (78 entries)

`(+n)` = the same text is used by n more keys.

| Key | Before | After | Why |
|---|---|---|---|
| `android groups_list_left_or_removed` (+2) | Poistuttu tai poistettu | Entiset ryhmät | "Poistuttu tai poistettu" is two impersonal participles with no subject; "Entiset ryhmät" (former groups) says the same thing naturally. |
| `android groups_chat_waiting_strip` (+2) | Odotetaan isäntälaitetta. Yksikään ryhmän isäntälaite ei ole verkossa; viestisi on tallessa ja lähtee, kun jokin on. | Odotetaan isäntälaitetta. Yksikään ryhmän isäntälaite ei ole verkossa; viestisi ovat tallessa ja lähtevät, kun jokin tulee verkkoon. | "viestisi on … lähtee" read as one message; plural agreement; "kun jokin on" was elliptic. |
| `android groupsb_hosts_offline` (+2) | Yksikään isäntälaite ei ole verkossa. Viestisi on tallessa ja lähtee, kun jokin on. | Yksikään isäntälaite ei ole verkossa. Viestisi ovat tallessa ja lähtevät, kun jokin tulee verkkoon. | Same plural agreement and elliptic ending as the strip. |
| `android groupsb_callhosts_note` (+1) | Puheluisäntä välittää puhelun. Kaikki puhelussa olevat yhdistävät siihen, ja se välittää äänen eteenpäin, joten se voi kuulla puhelun. Tämä on erillään keskustelun isännöinnistä: keskustelun isäntälaite ei ole puheluisäntä, ellet nimeä sitä myös sellaiseksi. Puheluisännät ovat jäseniä, joihin luotat, ja jokainen päättää itse hyväksyä tehtävän. | Puheluisäntä välittää puhelun. Kaikki puhelussa olevat ovat yhteydessä siihen, ja se välittää äänen eteenpäin, joten se voi kuulla puhelun. Tämä on eri asia kuin keskustelun isännöinti: keskustelun isäntälaite ei ole puheluisäntä, ellet nimeä sitä myös sellaiseksi. Puheluisännät ovat jäseniä, joihin luotat, ja jokainen päättää itse, ottaako tehtävän vastaan. | "yhdistävät siihen" is transitive without object (calque of connect); "erillään" is a calque of separate; "päättää hyväksyä" stiff. |
| `android gcall_about_is_carrying` (+1) | välittää tätä puhelua. Kaikki puhelussa olevat yhdistävät siihen, ja se välittää äänen eteenpäin ja valitsee, kuka kuuluu, kun useampi puhuu yhtä aikaa. | välittää tätä puhelua. Kaikki puhelussa olevat ovat yhteydessä siihen, ja se välittää äänen eteenpäin ja valitsee, kuka kuuluu, kun useampi puhuu yhtä aikaa. | "yhdistävät siihen" -> "ovat yhteydessä siihen". |
| `android gcall_about_carries` (+1) | välittää tämän puhelun. Kaikki puhelussa olevat yhdistävät siihen, ja se välittää äänen eteenpäin ja valitsee, kuka kuuluu, kun useampi puhuu yhtä aikaa. | välittää tämän puhelun. Kaikki puhelussa olevat ovat yhteydessä siihen, ja se välittää äänen eteenpäin ja valitsee, kuka kuuluu, kun useampi puhuu yhtä aikaa. | "yhdistävät siihen" -> "ovat yhteydessä siihen". |
| `ios group_callhosts_note_admin` | Puheluisäntä välittää puhelun. Kaikki puhelussa olevat yhdistävät siihen, ja se välittää äänen eteenpäin, joten se voi kuulla puhelun. Tämä on erillään keskustelun isännöinnistä: keskustelun isäntälaite ei ole puheluisäntä, ellet nimeä sitä myös sellaiseksi. Puheluisännät ovat jäseniä, joihin luotat, ja jokainen päättää itse hyväksyä tehtävän. Nimeä sellainen jäsenen valikosta tai isännöi puheluita tällä puhelimella. Puhelin isännöi vain omistajansa aloittamia puheluita. | Puheluisäntä välittää puhelun. Kaikki puhelussa olevat ovat yhteydessä siihen, ja se välittää äänen eteenpäin, joten se voi kuulla puhelun. Tämä on eri asia kuin keskustelun isännöinti: keskustelun isäntälaite ei ole puheluisäntä, ellet nimeä sitä myös sellaiseksi. Puheluisännät ovat jäseniä, joihin luotat, ja jokainen päättää itse, ottaako tehtävän vastaan. Nimeä sellainen jäsenen valikosta tai isännöi puheluita tällä puhelimella. Puhelin isännöi vain omistajansa aloittamia puheluita. | Same fixes as groupsb_callhosts_note (iOS combined string). |
| `desktop gcall_this_call_everyone_in` |  {value} tätä puhelua. Kaikki puhelussa olevat yhdistävät siihen, ja se välittää äänen eteenpäin ja valitsee, kuka kuuluu, kun useampi puhuu yhtä aikaa. |  {value} tätä puhelua. Kaikki puhelussa olevat ovat yhteydessä siihen, ja se välittää äänen eteenpäin ja valitsee, kuka kuuluu, kun useampi puhuu yhtä aikaa. | "yhdistävät siihen" -> "ovat yhteydessä siihen". |
| `desktop group_a_call_host_carries` | Puheluisäntä välittää puhelun. Kaikki puhelussa olevat yhdistävät siihen, ja se välittää äänen eteenpäin, joten se voi kuulla puhelun. Tämä on erillään keskustelun isännöinnistä: keskustelun isäntälaite ei ole puheluisäntä, ellet nimeä sitä myös sellaiseksi. Puheluisännät ovat jäseniä, joihin luotat, ja jokainen päättää itse hyväksyä tehtävän.{value} | Puheluisäntä välittää puhelun. Kaikki puhelussa olevat ovat yhteydessä siihen, ja se välittää äänen eteenpäin, joten se voi kuulla puhelun. Tämä on eri asia kuin keskustelun isännöinti: keskustelun isäntälaite ei ole puheluisäntä, ellet nimeä sitä myös sellaiseksi. Puheluisännät ovat jäseniä, joihin luotat, ja jokainen päättää itse, ottaako tehtävän vastaan.{value} | Same fixes as groupsb_callhosts_note (desktop). |
| `android gcall_about_who` (+2) | Puheluisäntä on laite, jonka ryhmän ylläpitäjä valitsi ja joka suostui siihen. Se on erillään keskustelun isännöinnistä. Ylläpitäjät voivat vaihtaa puheluisäntiä ryhmän tiedoissa. | Puheluisäntä on laite, jonka ryhmän ylläpitäjä valitsi ja joka suostui siihen. Se on eri asia kuin keskustelun isännöinti. Ylläpitäjät voivat vaihtaa puheluisäntiä ryhmän tiedoissa. | "erillään" calque -> "eri asia kuin". |
| `android groupsb_flag_note2` (+1) | Merkitseminen on ryhmän päätös. Se on erillään estämisestä, joka koskee vain sinua. {name} näkee jokaisen merkinnän: kuka sen teki ja milloin. | Merkitseminen on ryhmän päätös. Se on eri asia kuin estäminen, joka koskee vain sinua. {name} näkee jokaisen merkinnän: kuka sen teki ja milloin. | "erillään estämisestä" calque -> "eri asia kuin estäminen". |
| `android groupsb_remove_admin` (+1) | Poista ylläpitäjyys | Poista ylläpito-oikeudet | "ylläpitäjyys" is a rare coinage; "ylläpito-oikeudet" is the everyday word for the admin role. |
| `desktop group_remove_as_admin` | Poista ylläpitäjyys | Poista ylläpito-oikeudet | Same as groupsb_remove_admin. |
| `desktop group_remove_as_admin_2` | Poistetaanko ylläpitäjyys: {name}? | Poistetaanko ylläpito-oikeudet: {name}? | Same term change. |
| `android groupsc_admin_revoked_you` (+2) | {by} poisti ylläpitäjyytesi | {by} poisti ylläpito-oikeutesi | Same term change. |
| `android groupsc_admin_revoked_other` | {by} poisti ylläpitäjyyden: {sub} | {by} poisti ylläpito-oikeudet: {sub} | Same term change. |
| `ios group_sys_unadmin` (+1) | {by} poisti ylläpitäjyyden: {name} | {by} poisti ylläpito-oikeudet: {name} | Same term change. |
| `android groupsb_blocked_stay` (+1) | Hän pysyy ryhmässä. Poista esto yhteystiedoista keskustellaksesi. | Hän pysyy ryhmässä. Voit poistaa eston yhteystiedoissa, jos haluat keskustella. | "keskustellaksesi" after an imperative reads like an order; gentler "Voit …, jos haluat". |
| `android groupsb_waiting_device` (+1) | Odotetaan, että laite hyväksyy: {name} | Odotetaan, että henkilön {name} laite hyväksyy | "Odotetaan, että laite hyväksyy: Maya" split the possessor from the device; "henkilön {name} laite" keeps the name nominative. |
| `android groupsb_add_note` (+1) | Jokainen saa kutsun teidän kahdenkeskiseen keskusteluunne ja liittyy hyväksyessään sen. Kutsu on voimassa 7 päivää. | Jokainen saa kutsun teidän kahdenkeskisessä keskustelussanne ja liittyy, kun hyväksyy sen. Kutsu on voimassa 7 päivää. | Invitation arrives in the chat (inessive), not into it; "liittyy, kun hyväksyy" smoother. |
| `desktop group_each_person_gets_the` | Jokainen saa kutsun teidän kahdenkeskiseen keskusteluunne ja liittyy hyväksyessään sen. Kutsu on voimassa 7 päivää. | Jokainen saa kutsun teidän kahdenkeskisessä keskustelussanne ja liittyy, kun hyväksyy sen. Kutsu on voimassa 7 päivää. | Same as groupsb_add_note. |
| `android groupsb_point_invite` (+1) | Osoita ryhmäkutsua | Osoita kameralla ryhmäkutsua | "Osoita ryhmäkutsua" lacked the instrument; "Osoita kameralla" is what Finnish camera hints say. |
| `android groupsb_link_note` | Kuka tahansa, jolla on tämä linkki tai QR-koodi, voi liittyä ryhmään {name} 7 päivän ajan. Jaa se sovelluksella, johon luotat; näet, kuka liittyi, ja voit merkitä hänet pois. Jos haluat kutsua yhteystietojasi suoraan, käytä kohtaa Lisää jäseniä. | Kuka tahansa, jolla on tämä linkki tai QR-koodi, voi liittyä ryhmään {name} 7 päivän ajan. Jaa se sovelluksella, johon luotat; näet, kuka liittyi, ja voit merkitä hänet poistettavaksi. Jos haluat kutsua yhteystietojasi suoraan, käytä kohtaa Lisää jäseniä. | "merkitä hänet pois" is not Finnish; "merkitä poistettavaksi" matches the Flag for removal action. |
| `ios group_link_hint` | Kuka tahansa, jolla on tämä linkki tai QR-koodi, voi liittyä ryhmään {group} 7 päivän ajan. Jaa se sovelluksella, johon luotat; näet, kuka liittyi, ja voit merkitä hänet pois. Jos haluat kutsua yhteystietojasi suoraan, käytä kohtaa Lisää jäseniä. | Kuka tahansa, jolla on tämä linkki tai QR-koodi, voi liittyä ryhmään {group} 7 päivän ajan. Jaa se sovelluksella, johon luotat; näet, kuka liittyi, ja voit merkitä hänet poistettavaksi. Jos haluat kutsua yhteystietojasi suoraan, käytä kohtaa Lisää jäseniä. | Same. |
| `desktop group_anyone_with_this_link` | Kuka tahansa, jolla on tämä linkki tai QR-koodi, voi liittyä 7 päivän ajan. Jaa se sovelluksella, johon luotat; näet, kuka liittyi, ja voit merkitä hänet pois. Jos haluat kutsua yhteystietojasi suoraan, käytä kohtaa Lisää jäseniä. | Kuka tahansa, jolla on tämä linkki tai QR-koodi, voi liittyä 7 päivän ajan. Jaa se sovelluksella, johon luotat; näet, kuka liittyi, ja voit merkitä hänet poistettavaksi. Jos haluat kutsua yhteystietojasi suoraan, käytä kohtaa Lisää jäseniä. | Same. |
| `android groupsc_flag_recorded` (+2) | Merkintä tallennettu. Se on allekirjoitettu sinun nimissäsi, ja jäsen näkee sen. | Merkintä tallennettu. Allekirjoitit sen, ja jäsen näkee sen. | "sinun nimissäsi" means on your behalf, not signed by you; "Allekirjoitit sen". |
| `desktop group_a_flag_is_signed` | Merkintä allekirjoitetaan sinun nimissäsi, eikä sitä voi perua. {name} näkee, kuka hänet merkitsi ja milloin. Viestit, jotka {name} kirjoitti ennen poistoa, näkyvät yhä merkittyinä. | Allekirjoitat merkinnän, eikä sitä voi perua. {name} näkee, kuka hänet merkitsi ja milloin. Viestit, jotka {name} kirjoitti ennen poistoa, näkyvät yhä merkittyinä. | Same: the admin signs the flag. |
| `android gcall_ended_host_lost` (+2) | Puhelu päättyi — isäntä katosi | Puhelu päättyi — puheluisäntä katosi | "isäntä" alone; glossary term is "puheluisäntä". |
| `desktop group_a_host_keeps_a` | Isäntälaite pitää täyden kopion ryhmästä ja välittää viestit eteenpäin jäsenten puhelinten nukkuessa. Jäsenet yhdistävät isäntälaitteisiin, eivät toisiinsa; vain isäntälaitteet saavat tietää jäsenten ryhmäosoitteet. Tinline jatkaa isännöintiä, vaikka ikkuna on suljettu (se pysyy ilmaisinalueella). | Isäntälaite pitää täyden kopion ryhmästä ja välittää viestit eteenpäin jäsenten puhelinten nukkuessa. Jäsenet ovat yhteydessä isäntälaitteisiin, eivät toisiinsa; vain isäntälaitteet saavat tietää jäsenten ryhmäosoitteet. Tinline jatkaa isännöintiä, vaikka ikkuna on suljettu (se pysyy ilmaisinalueella). | "yhdistävät isäntälaitteisiin" -> "ovat yhteydessä isäntälaitteisiin". |
| `desktop group_members_stop_going_through` | Jäsenet lakkaavat kulkemasta sen kautta | Jäsenten yhteydet eivät enää kulje sen kautta | "Jäsenet lakkaavat kulkemasta sen kautta" said members walk through it; it is their connections. |
| `ios group_done_request_accepted` | Hyväksytty. {name} lisättiin vain viestejä varten ja näkyy Keskusteluissa, kun hänen laitteensa yhdistää; puhelut pysyvät pois päältä, kunnes sallit ne. | Hyväksytty. {name} lisättiin vain viestejä varten ja näkyy Keskusteluissa, kun hänen laitteensa saa yhteyden; puhelut pysyvät pois päältä, kunnes sallit ne. | "laitteensa yhdistää" (transitive, no object) -> "saa yhteyden". |
| `android link_point_new` (+1) | Osoita toisen laitteesi koodia | Osoita kameralla toisen laitteesi koodia | Added "kameralla", as in link_point_existing. |
| `android addcontact_point_camera` (+1) | Osoita hänen Tinline-koodiaan. | Osoita kameralla hänen Tinline-koodiaan. | Added "kameralla". |
| `android link_show_body` (+1) | Valitse uudella laitteella ”Linkitä laitteeseen”, sitten ”Skannaa sen sijaan” ja osoita tätä koodia. | Valitse uudella laitteella ”Linkitä laitteeseen”, sitten ”Skannaa sen sijaan”, ja osoita sillä tätä koodia. | "osoita tätä koodia" lacked what to point with; "osoita sillä". |
| `android unlock_keylost_notice` (+2) | Et asettanut salalausetta, joten 24 palautussanaa ovat ainoa tie takaisin. Yhteystietosi säilyvät. | Et asettanut salalausetta, joten 24 palautussanaa on ainoa tie takaisin. Yhteystietosi säilyvät. | Numeral subject takes a singular verb: "24 palautussanaa on". |
| `android profile_other_info_note` (+2) | Osa tiedoista tuli toiselta laitteelta tai sovellusversiolta. Ne näytetään tallennettuina ja säilyvät, kun muokkaat muuta. | Osa tiedoista tuli toiselta laitteelta tai sovellusversiolta. Ne näytetään sellaisinaan ja säilyvät, kun muokkaat muuta. | "näytetään tallennettuina" = shown as saved items; meaning is "as stored" = "sellaisinaan". |
| `android profile_dismiss` (+3) | Ohita | Sulje | Closes a floating error: "Sulje", not "Ohita" (skip/ignore). |
| `android onboarding_welcome_title` (+2) | Suora linja ihmisiisi | Suora linja läheisiisi | "ihmisiisi" reads odd; "läheisiisi" (your close ones) is the natural phrase for "your people". Native check of tone. |
| `android onboarding_tip_devices_body` (+2) | Linkitä toinen laite, kun ehdit, niin yhden menettäminen ei koskaan lukitse sinua ulos. | Linkitä toinen laite, kun ehdit, niin pääset tilillesi, vaikka yksi laite katoaisi. | "lukitse sinua ulos" is a calque of locks you out; say what stays possible (VOICE: say when things come back). |
| `android onboarding_tip_groups_body` (+1) | Aloittaaksesi ryhmän lisää Tinline tietokoneelle, joka voi olla jatkuvasti päällä. | Jos haluat aloittaa ryhmän, asenna Tinline tietokoneelle, joka voi olla jatkuvasti päällä. | Fronted "Aloittaaksesi ryhmän" is translationese; "add Tinline on a computer" = install it. |
| `android onboarding_phrase_lead_key` (+1) | Palautusavaimesi on sama kuin 24 sanaasi, yhtenä rivinä.\nLiitä se johonkin turvalliseen paikkaan. Salasanojen hallinta sopii hyvin. | Palautusavaimesi on sama kuin 24 sanaasi, yhtenä rivinä.\nLiitä se johonkin turvalliseen paikkaan. Salasanojen hallintasovellus sopii hyvin. | "Salasanojen hallinta" is the activity, not the app; "salasanojen hallintasovellus". |
| `desktop signup_paste_it_somewhere_safe` | Liitä se johonkin turvalliseen paikkaan. Salasanojen hallinta sopii hyvin. | Liitä se johonkin turvalliseen paikkaan. Salasanojen hallintasovellus sopii hyvin. | Same password manager fix. |
| `desktop profile_your_recovery_key_is` | Palautusavaimesi on sama kuin 24 sanaasi, yhtenä rivinä. Salasanojen hallinta on hyvä paikka sille. | Palautusavaimesi on sama kuin 24 sanaasi, yhtenä rivinä. Salasanojen hallintasovellus on hyvä paikka sille. | Same password manager fix. |
| `desktop common_paste_only_into_a` | Liitä vain luotettuun salasanojen hallintaan; leikepöydällä on tilisi avain. | Liitä vain luotettuun salasanojen hallintasovellukseen; leikepöydällä on tilisi avain. | Same password manager fix. |
| `android onboarding_phrase_lead_words` (+1) | Jos joskus vaihdat laitetta, nämä 24 sanaa tuovat tilisi mukanasi.\nTallenna kopio turvalliseen paikkaan. Paperi toimii hyvin. | Jos joskus vaihdat laitetta, nämä 24 sanaa tuovat tilisi mukaan.\nTallenna kopio turvalliseen paikkaan. Paperi toimii hyvin. | "tuovat tilisi mukanasi" (with you, locative) -> "tuovat tilisi mukaan" (along). |
| `desktop signup_if_you_ever_switch` | Jos joskus vaihdat laitetta, nämä 24 sanaa tuovat tilisi mukanasi. | Jos joskus vaihdat laitetta, nämä 24 sanaa tuovat tilisi mukaan. | Same. |
| `android notif_muted_note` (+2) | Mykistetty keskustelu ei koskaan ilmoita sinulle, vaikka täällä olisi mitä päällä. Saman henkilön vastaamattomat puhelut kootaan yhteen ilmoitukseen. | Mykistetty keskustelu ei koskaan ilmoita sinulle, riippumatta siitä, mitä täällä on päällä. Saman henkilön vastaamattomat puhelut kootaan yhteen ilmoitukseen. | "vaikka täällä olisi mitä päällä" is colloquial and clumsy; "riippumatta siitä, mitä täällä on päällä". |
| `android settings_quiet_sub` (+2) | Ei ääniä Tinlinesta: soittoa, puheluääniä eikä liittymisääniä | Tinline on äänetön: ei soittoääntä, puheluääniä eikä liittymisääniä | "Ei ääniä Tinlinesta: soittoa…" mixed cases in the list; restated as one clause. |
| `android settings_presence_off` (+1) | Et näy heidän paikallaololistoissaan | Et näy heille paikalla olevana | "paikallaololistoissaan" is an awkward coinage; "Et näy heille paikalla olevana". |
| `android settings_calls_allowed_note` (+2) | Sallimasi henkilö voi soittaa sinulle; vain viestejä lähettävä voi yhä kirjoittaa. Estämäsi ihmiset eivät voi kumpaakaan. | Sallimasi henkilöt voivat soittaa sinulle; ne, joilla on vain viestit, voivat silti kirjoittaa. Estämäsi henkilöt eivät voi kumpaakaan. | "vain viestejä lähettävä" misread as a person who only sends messages; plural, clearer. |
| `android settings_passphrase_why` (+1) | Tilisi on vain tällä puhelimella. Salalause salaa sen, joten kukaan puhelimesi haltija ei voi kopioida sitä. | Tilisi on vain tällä puhelimella. Salalause salaa sen, joten kukaan, jolla puhelimesi on käsissään, ei voi kopioida sitä. | "puhelimesi haltija" is legal wording (holder/owner); "jolla puhelimesi on käsissään". |
| `android devices_rings` (+1) | Soi puheluille | Soi puheluissa | "Soi puheluille" is not idiomatic; "Soi puheluissa". |
| `android devices_unlink_warn` (+2) | Linkityksen poisto ei voi lukita sitä täältä. Kuka tahansa, joka saa sen auki, voi yhä käyttää tiliäsi. Ainoa täysi katkaisu on uusi tili. | Linkityksen poisto ei voi lukita sitä täältä. Kuka tahansa, joka saa sen auki, voi yhä käyttää tiliäsi. Ainoa tapa katkaista kaikki kokonaan on uusi tili. | "Ainoa täysi katkaisu" is a calque of full cut-off. |
| `android notif_ch_ongoing_desc` | Näyttää nykyisen puhelusi ja antaa palata siihen tai lopettaa sen | Näyttää nykyisen puhelusi, ja voit palata siihen tai lopettaa sen | "antaa palata" lacked the subject; "voit palata". |
| `android profile_availability_note` (+2) | Yhteystietosi näkevät tämän, ja se on sama kaikilla laitteillasi. Se vaikuttaa siihen, tulevatko puhelut perille, ja on erillään siitä, oletko verkossa. | Yhteystietosi näkevät tämän, ja se on sama kaikilla laitteillasi. Se vaikuttaa siihen, tulevatko puhelut perille, eikä se riipu siitä, oletko verkossa. | "on erillään siitä" calque -> "eikä se riipu siitä". |
| `android gcall_cant_connect` (+4) | Yhteys ei onnistunut | Yhdistäminen ei onnistunut | "Yhteys ei onnistunut" (the connection did not succeed) -> "Yhdistäminen ei onnistunut". |
| `android gcall_mic_permission` (+2) | Mikrofonin lupa tarvitaan - sinua ei kuulla | Mikrofonille tarvitaan lupa - sinua ei kuulla | "Mikrofonin lupa tarvitaan" is word-for-word; "Mikrofonille tarvitaan lupa". |
| `android video_camera_permission` (+1) | Kameran lupa tarvitaan - sinut kuullaan silti | Kameralle tarvitaan lupa - sinut kuullaan silti | Same pattern for the camera. |
| `ios call_mic_permission_needed_place` | Soittamiseen tarvitaan mikrofonin lupa | Soittamiseen tarvitaan mikrofonin käyttölupa | "mikrofonin lupa" -> "mikrofonin käyttölupa". |
| `android addcontact_code_expired` (+1) | Tämä koodi on vanhentunut (lähettäjä {name}). Pyydä häneltä uusi. | Henkilön {name} koodi on vanhentunut. Pyydä häneltä uusi. | "(lähettäjä {name})" in brackets read like a log line; "Henkilön {name} koodi". |
| `desktop signup_that_code_from_has` | Tämä koodi on vanhentunut (lähettäjä {value}). Koodit ovat voimassa 7 päivää, joten pyydä häneltä uusi. | Henkilön {value} koodi on vanhentunut. Koodit ovat voimassa 7 päivää, joten pyydä häneltä uusi. | Same. |
| `android addcontact_why_blocked_them` (+2) | Et voi lisätä henkilöä {name} juuri nyt. Hän on valinnut olla olematta yhteydessä täällä. | Et voi lisätä henkilöä {name} juuri nyt. Hän on päättänyt olla pitämättä yhteyttä täällä. | "on valinnut olla olematta yhteydessä" is a double-negative calque. |
| `android people_off` (+4) | Pois | Pois päältä | Spoken switch state: "Pois päältä" pairs with "Päällä"; bare "Pois" means away. |
| `android call_route_title` (+1) | Toista puhelu laitteella | Puhelun ääni | "Toista puhelu laitteella" means replay the call; sheet title is where the audio plays. |
| `android call_route_headset` (+1) | Kuulokemikrofoni | Kuulokkeet | Wired headset: Android Finnish says "Kuulokkeet"; "Kuulokemikrofoni" is long and technical. |
| `android video_camera_off` (+1) | Kamera on pois päältä: {name} | Henkilön {name} kamera on pois päältä | "Kamera on pois päältä: Maya" -> "Henkilön Maya kamera…", same name pattern as the rest of the file. |
| `android call_info_reconnect_body` (+2) | Yhteys katkesi hetkeksi. Tinline etsii tietä takaisin: {name}. | Yhteys katkesi hetkeksi. Tinline etsii yhteyttä takaisin henkilöön {name}. | "etsii tietä takaisin: {name}" was broken; "etsii yhteyttä takaisin henkilöön {name}". |
| `android call_unreachable_body` (+1) | {name} voi olla poissa verkosta, kuuluvuusalueen ulkopuolella tai ei juuri nyt ota puheluita vastaan. Tinline ei voi jättää viestejä. | {name} ei ehkä ole verkossa tai kuuluvuusalueella, tai hän ei ota puheluita vastaan juuri nyt. Tinline ei voi jättää viestejä. | "voi olla …, tai ei juuri nyt ota" mixed two verb frames; split into two clauses. |
| `ios call_unreachable_body_blank` | Hän voi olla poissa verkosta, kuuluvuusalueen ulkopuolella tai ei juuri nyt ota puheluita vastaan. Tinline ei voi jättää viestejä. | Hän ei ehkä ole verkossa tai kuuluvuusalueella, tai hän ei ota puheluita vastaan juuri nyt. Tinline ei voi jättää viestejä. | Same for the unnamed variant. |
| `android home_banner_battery_title` | Puheluita voi jäädä huomaamatta laitteen ollessa levossa | Lepotilassa puheluita voi jäädä väliin | 51 characters for a 29-character banner title; shorter natural wording. |
| `desktop profile_it_is_offline_so` | Se ei ole verkossa, joten se saa tiedon, kun se seuraavan kerran yhdistää. | Se ei ole verkossa, joten se saa tiedon, kun se seuraavan kerran saa yhteyden. | "yhdistää" without object -> "saa yhteyden". |
| `ios onboarding_terms_consent` | Jatkamalla hyväksyt [käyttöehdot]({terms_url}) ja vahvistat lukeneesi [tietosuojakäytännön]({privacy_url}). | Jatkamalla hyväksyt [käyttöehdot]({terms_url}) ja olet lukenut [tietosuojakäytännön]({privacy_url}). | "vahvistat lukeneesi" adds "you confirm"; English says "have read". Same as Android. |
| `desktop signup_and_have_read_the` |  ja vahvistat lukeneesi dokumentin  |  ja olet lukenut dokumentin  | Same, desktop piece. |
| `desktop common_a_device_name_is` | Laitteen nimessä on 1–64 merkkiä. | Laitteen nimen pituus on 1–64 merkkiä. | "Laitteen nimessä on 1–64 merkkiä" states a fact about this name; it is a rule: "nimen pituus on". |
| `desktop call_camera_off` | Kamera pois | Kamera pois päältä | "Kamera pois" -> "Kamera pois päältä", pairs with "Kamera päällä". |
| `desktop chat_delete_for_everyone_asks` | Poista kaikilta pyytää henkilön {name} laitteita poistamaan sen myös. Muualla olevia kopioita ei voi kutsua takaisin. | ”Poista kaikilta” pyytää henkilön {name} laitteita poistamaan sen myös. Muualla olevia kopioita ei voi kutsua takaisin. | Button name used as subject needs quotes in Finnish. |
| `desktop chat_is_offline_delete_for` | {name} ei ole verkossa. Poista kaikilta ulottuu hänen laitteisiinsa, kun olette molemmat verkossa. | {name} ei ole verkossa. ”Poista kaikilta” ulottuu hänen laitteisiinsa, kun olette molemmat verkossa. | Same. |
| `desktop link_keep_both_devices_online` | Pidä molemmat laitteet verkossa. Jos toinen putoaa, se jatkaa siitä, mihin jäi, kun molemmat ovat taas verkossa. | Pidä molemmat laitteet verkossa. Jos toisen yhteys katkeaa, se jatkaa siitä, mihin jäi, kun molemmat ovat taas verkossa. | "Jos toinen putoaa" (if one falls) is a calque of drops off. |

## Needs a native speaker

| Key | Current | Concern / candidate |
|---|---|---|
| all "Tinline" inflections | Tinlinen, Tinlinessa, Tinlinea | Back-vowel harmony from the English pronunciation; confirm. |
| `groupsb_flag_n` and all flag strings | merkintä | Neutral but vague; candidate "poistomerkintä". Decide once, then replace throughout. |
| `groupsb_host` (+1) | Isäntälaite | Pill on a member (a person) row; "Isäntä" may read better there. |
| `common_call` (+8) | Puhelu | Noun on a button; see ruling. "Soita" once split. |
| `groupsb_online` / `groupsb_offline` (+shared) | Paikalla / Poissa | Fine for people, odd for host devices (R-B). |
| `onboarding_welcome_title` (+2) | Suora linja läheisiisi | Changed from "ihmisiisi"; confirm the warmth fits the brand line. |
| `app_ticket` | Lippu: {error} | iroh "ticket"; "Lippu" is literal. Candidate "Yhteyskoodi: {error}". |
| `unlock_keylost_restore` (+3), `settings_add_restore` (+1) | Palauta palautussanoilla | Repetitive "palauta palautus-"; candidate "Palauta tili sanoilla". |
| `settings_quiet` (+2) | Hiljainen | Switch title; "Hiljainen tila" (used in the note) may be clearer. |
| `onboarding_step_nickname` (+3) vs `profile_nickname` | Kutsumanimesi / Lempinimi | Own name vs your name for a contact; confirm the two words are wanted. |
| `call_route_headset` (+1) | Kuulokkeet | Changed from "Kuulokemikrofoni"; confirm against Android's own Finnish label. |
| `groupsb_share_text` | Liity ryhmään {name} Tinlinessa: {link} | Message other people read outside the app; check tone. |
| `onboarding_name_title` (+2) | Millä nimellä ihmiset voivat kutsua sinua? | Fine; "saavat" is the alternative if "voivat" sounds like ability. |

## Restructure: items already in RESTRUCTURE.md

All of §1–§6, §8, §9 and §10 affect Finnish (90 entries). The translator's workarounds
are the best available and are kept: names stay nominative via a colon or "henkilön/henkilölle
{name}" (reads naturally; slightly formal in titles, never wrong), host strings with a
`{noun}` slot use "laite ({noun})", glued desktop pieces use "dokumentin". The worst
remaining outputs are `kit_tick_their` ("Henkilön hänen laitteella", ungrammatical) and
`{by}` = "Sinä" ("Sinä nimesi …", wrong verb person).

## Restructure: new items (not in RESTRUCTURE.md)

**R-A. Desktop `group_flag` gets an English ordinal.** `group_live_view.rs:1258` passes
`ordinal(n)` ("1st", "2nd", … from `groups_view.rs:74`) into " · {value} flag", so Finnish shows
" · merkintä 2nd". Suggested English: " · flag {n}" with a plain number (Finnish
" · merkintä {n}"), or one plural-style string per position if the ordinal must stay.

**R-B. Host rows share "Online"/"Offline" with people.** `groupsb_online`, `groupsb_offline`
(and desktop/iOS copies) use the same English as `conv_online`/`common_online`. Finnish uses
"paikalla" for people and "verkossa" for devices. Suggested English: separate keys
`groupsb_host_online` "Online" / `groupsb_host_offline` "Offline" with notes saying they
describe a device.

**R-C. Desktop weekday/date + time slots.** `profile_text` "{value} {time}" and `call_text`
"{day} {value}" glue a weekday or date word to a time, the same problem as RESTRUCTURE §8,
but these two desktop keys are not listed there. Suggested fix: format the whole phrase with
the platform date formatter, or one string per relative day ("tomorrow at {time}").

## Checks run

- `python3 scripts/i18n_tm.py apply fi` and `python3 scripts/i18n_gen.py --check`: pass.
- Scans: no "!", no formal "Te" register, no "virhe/epäonnistui/virheellinen".
