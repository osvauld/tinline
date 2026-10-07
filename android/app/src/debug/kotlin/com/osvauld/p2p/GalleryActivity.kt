package com.osvauld.p2p

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.Composable
import uniffi.p2pcore.Contact

/**
 * Debug-only screen gallery for design review and screenshots:
 * `adb shell am start -n com.osvauld.tinline/com.osvauld.p2p.GalleryActivity --es screen home`.
 * Renders the stateless screens with made-up data; never shipped (debug source set).
 */
class GalleryActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        val which = intent.getStringExtra("screen") ?: "home"
        val app = P2pApp.get(this)
        setContent { TinlineTheme { Gallery(which, app) } }
    }
}

private val maya = "Maya"
private val arjun = Contact("did:key:z6MkvVQqpbLsMmYnER7zdMsTvrD4s", "Arjun Oommen", "dev1", 0UL, "Arjun", true)
private val names = listOf("Arjun Oommen", "Dad", "Joanna Ostrowska", "Jonas Krüger", "Lena Park", "Rosa Silva")
private val contacts = names.mapIndexed { i, n -> Contact("did:key:z6Mk$n$i", n, "d$i", 0UL, null, false) }
private val subs = contacts.associate { it.did to "Not called yet" } + mapOf(contacts[3].did to "Last call yesterday", contacts[5].did to "Missed call · 11:02")
private const val PHRASE = "harbor velvet orbit candle meadow lunar ticket gravel salmon whisper copper anchor fabric jungle pepper quarter ripple saddle timber unveil walnut yellow zebra mosaic"
private val recents = listOf(
    RecentCall(contacts[0].did, "Arjun Oommen", RecentCall.Kind.Incoming, "Incoming · 4 min ago", "Incoming · 18 min", "Today, 12:31", true),
    RecentCall(contacts[5].did, "Rosa Silva", RecentCall.Kind.Missed, "Missed · 11:02", "Missed", "Today, 11:02"),
    RecentCall(contacts[3].did, "Jonas Krüger", RecentCall.Kind.Outgoing, "Outgoing · Yesterday · 08:15", "Outgoing · 3 min", "Yesterday, 08:15", false),
    RecentCall(contacts[4].did, "Lena Park", RecentCall.Kind.Unreached, "Couldn’t reach · 2 h ago", "Couldn’t reach", "Yesterday, 18:50"),
)

@Composable
private fun Gallery(which: String, app: P2pApp) {
    val none = {}
    val needs = listOf(Need.Battery)
    when (which) {
        "welcome" -> WelcomeScreen(none, none)
        "name" -> NameScreen("Maya", {}, none, 1, none)
        "pass" -> PassphraseStep("correct-horse-lamp-river", {}, false, null, 2, "Add a passphrase", "Continue", none, none, none)
        "key_lost" -> KeyLostContent("Maya", false, none, none)
        "phrase" -> PhraseScreen(PHRASE, none)
        "terms" -> TermsScreen(4, null, none)
        "perms" -> PermissionsScreen(listOf(Need.Mic, Need.FullScreen, Need.Battery), {}, none)
        "restore" -> RestoreScreen(none, null, false, "Restoring replaces whatever account is on this phone. Next, you’ll choose a new passphrase.") {}
        "unlock" -> UnlockContent("Maya", "correct-horse-lamp", {}, false, null, none, none)
        "unlock_error" -> UnlockContent("Maya", "correct-horse-lump", {}, false, "That didn’t match. Check the spelling and spaces.", none, none)
        "home" -> HomeContent(contacts, true, false, needs, {}, {}, none, {}, {}, none, recents, null, subLines = subs)
        "home_away" -> HomeContent(contacts, true, false, emptyList(), {}, {}, none, {}, {}, none, recents, null, available = false, subLines = subs)
        "avail" -> { HomeContent(contacts, true, false, emptyList(), {}, {}, none, {}, {}, none, recents, null, subLines = subs); AvailabilitySheet(true, null, maya, none) { _, _ -> } }
        "avail_off" -> { HomeContent(contacts, true, false, emptyList(), {}, {}, none, {}, {}, none, recents, null, available = false, subLines = subs); AvailabilitySheet(false, System.currentTimeMillis() / 1000 + 3600, maya, none) { _, _ -> } }
        "history" -> HistoryScreen(app, none) {}
        "reconnecting" -> InCallContent("Arjun Oommen", arjun.did, maya, true, false, 771, true, 3, false, false, null, {}, {}, {}, {}, reconnecting = true)
        "home_plain" -> HomeContent(contacts, true, false, emptyList(), {}, {}, none, {}, {}, none, emptyList(), null)
        "home_search" -> HomeContent(contacts, true, false, emptyList(), {}, {}, none, {}, {}, none, emptyList(), null, startSearching = true, startQuery = "jo")
        "home_empty" -> HomeContent(emptyList(), true, false, emptyList(), {}, {}, none, {}, {}, none, emptyList(), null)
        "home_offline" -> HomeContent(contacts, false, false, listOf(Need.Mic), {}, {}, none, {}, {}, none, emptyList(), null)
        "mycode" -> AddContactScreen(app, false, none, {}, {})
        "scan" -> AddContactScreen(app, true, none, {}, {})
        "paste" -> PasteCardScreen("OSVC2:MFRGGZDFMZTWQ2LKNNWG23TPOBYXE43UOV3HO6DZPE======", {}, none, none)
        "adding" -> AddingScreen(maya, none)
        "added" -> AddedScreen(maya, arjun, none, none, none)
        "failed" -> FailedScreen(maya, null, none, none)
        "contact" -> ContactScreen(arjun, none, none, none)
        "contact_full" -> ContactScreen(arjun, none, none, none, history = recents)
        "verify" -> VerifyScreen(maya, arjun, "41203 88127 05519 73360 29841 66012 90475 13398 57206 84431 20987 36654".split(" "), none, none, none)
        "rename" -> RenameSheet("Arjun (work)", "Arjun", true, none) {}
        "incoming" -> IncomingContent("Arjun Oommen", arjun.did, none, none)
        "calling" -> InCallContent("Arjun Oommen", arjun.did, maya, false, false, 0, null, null, false, false, null, {}, {}, {}, {})
        "incall" -> InCallContent("Arjun Oommen", arjun.did, maya, true, false, 252, true, 4, true, false, null, {}, {}, {}, {})
        "incall_relayed" -> InCallContent("Arjun Oommen", arjun.did, maya, true, false, 767, false, 3, false, true, null, {}, {}, {}, {}, startWithSheet = true)
        "ended" -> EndedContent("Arjun Oommen", arjun.did, maya, endReasonText("hangup_remote", "Arjun"), 1084, true, 3, {}, {}, autoClose = false)
        "unreach" -> UnreachableContent("Lena", contacts[4].did, maya, none, none)
        "mic" -> MicNeededScreen(none, none)
        "settings" -> SettingsScreen(app, needs, none, none, none, none, none, none)
        "battery" -> BatteryScreen(needs, none) {}
        "gate" -> PhraseGateScreen(app, none) {}
        "shown" -> PhraseShownScreen(PHRASE, none)
        "changepass" -> ChangePassphraseScreen(app, none)
        "about" -> AboutScreen(none, none)
        "licences" -> LicencesScreen(none)
        else -> HomeContent(contacts, true, false, emptyList(), {}, {}, none, {}, {}, none, emptyList(), null)
    }
}
