package com.osvauld.p2p

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
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
        val dark = intent.getBooleanExtra("dark", false)
        setContent { TinlineTheme(dark) { Gallery(which, app, this) } }
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
private fun Gallery(which: String, app: P2pApp, act: ComponentActivity) {
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
        "voice" -> VoiceDemo()
        "settings" -> SettingsScreen(app, needs, none, none, none, none, none, none)
        "battery" -> BatteryScreen(needs, none) {}
        "gate" -> PhraseGateScreen(app, none) {}
        "shown" -> PhraseShownScreen(PHRASE, none)
        "changepass" -> ChangePassphraseScreen(app, none)
        "about" -> AboutScreen(none, none)
        "licences" -> LicencesScreen(none)
        "fakechat" -> {
            // The whole app on the fake chat backend (debug only): opens the real Home with seeded chats.
            LaunchedEffect(Unit) {
                ChatBackend.override = FakeChatSource(app)
                act.startActivity(android.content.Intent(act, MainActivity::class.java).addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK))
                act.finish()
            }
        }
        "chats", "chats_waiting", "chats_empty", "calls", "contacts" -> {
            val fake = remember { FakeChatSource(app).also { if (which == "chats_waiting") it.setLink(FakeChatSource.did("Lena"), Link.Offline) } }
            val chats by fake.chats.collectAsState()
            val links by fake.links.collectAsState()
            val rows = if (which == "chats_empty") emptyList() else chatRows(chats, emptyList(), links, true, System.currentTimeMillis())
            val tab = when (which) { "calls" -> HomeTab.Calls; "contacts" -> HomeTab.Contacts; else -> HomeTab.Chats }
            HomeContent(contacts, true, false, emptyList(), {}, {}, none, {}, {}, none, recents, null, subLines = subs, tab = tab, chatRows = rows,
                chatBadge = rows.sumOf { it.unread }, callBadge = if (which == "calls") 0 else 1)
        }
        "newchat" -> NewChatScreen(contacts, mapOf(contacts[0].did to "Last message 12:41", contacts[1].did to "Last message 11:20"), none, {}, none)
        else -> if (which.startsWith("conv") || which in setOf("attach", "files", "receive", "viewer")) {
            val files = which in setOf("files", "receive", "viewer")
            val fake = remember { FakeChatSource(app, files).also { ChatBackend.override = it } }
            val peer = when (which) { "files", "viewer" -> "Jonas"; "receive" -> "Rosa"; else -> "Arjun" }
            val did = FakeChatSource.did(peer)
            val name = FakeChatSource.people.first { it.startsWith(peer) }
            remember {
                when (which) {
                    "conv_offline" -> fake.setLink(did, Link.Offline)
                    "conv_old" -> fake.seedOlder(did)
                }
            }
            val preview = when (which) {
                "conv_actions" -> ConvPreview(actionsFor = "perfect")
                "conv_edit" -> ConvPreview(editing = "perfect", draft = "Perfect, talk then at 6:30")
                "conv_reply" -> ConvPreview(replying = "perfect")
                "attach" -> ConvPreview(attach = true)
                "viewer" -> ConvPreview(viewer = "outphoto")
                else -> null
            }
            ConversationScreen(fake, did, name, true, none, none, preview)
        } else HomeContent(contacts, true, false, emptyList(), {}, {}, none, {}, {}, none, emptyList(), null)
    }
}

/** Record with the mic button; each recording lands as a bubble you can play and seek. Two made-up messages to start. */
@Composable
private fun VoiceDemo() {
    val ctx = androidx.compose.ui.platform.LocalContext.current
    val msgs = remember { mutableStateListOf<RecordedVoice>() }
    LaunchedEffect(Unit) {
        // A synthetic warbling "speech" file through the real Rust recorder, so playback needs no mic.
        val f = java.io.File(ctx.cacheDir, "demo.opus")
        val rec = uniffi.p2pcore.VoiceRecorder.start(f.absolutePath)
        rec.push(List(16000 * 6) { i -> (kotlin.math.sin(i * 2 * Math.PI * (200 + 80 * kotlin.math.sin(i / 3000.0)) / 16000) * 9000 * kotlin.math.abs(kotlin.math.sin(i / 2500.0))).toInt().toShort() })
        val info = rec.finish()
        msgs.add(RecordedVoice(f.absolutePath, info.durationMs.toInt(), info.waveform))
    }
    val state = rememberVoiceRecState { msgs.add(it) }
    val c = Tin.c
    Column(Modifier.fillMaxSize().background(c.bg).statusBarsPadding().navigationBarsPadding()) {
        Column(Modifier.weight(1f).padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp, Alignment.Bottom)) {
            msgs.forEachIndexed { i, m ->
                val out = i % 2 == 1
                Box(
                    Modifier.align(if (out) Alignment.End else Alignment.Start)
                        .background(if (out) c.prc else c.sf, RoundedCornerShape(18.dp)).padding(start = 12.dp, end = 12.dp, top = 8.dp, bottom = 8.dp),
                ) { VoiceBubbleContent("demo$i", m.path, m.durationMs, m.waveform, out) }
            }
        }
        Row(Modifier.padding(8.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            if (state.recording) VoiceRecordingBar(state, Modifier.weight(1f))
            else Text("Message", color = c.ink2, modifier = Modifier.weight(1f).background(c.sf2, RoundedCornerShape(24.dp)).padding(16.dp))
            VoiceMicButton(state)
        }
    }
}
