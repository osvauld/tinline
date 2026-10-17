package com.osvauld.p2p

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.ime
import androidx.compose.foundation.layout.union
import androidx.compose.foundation.layout.consumeWindowInsets
import androidx.compose.foundation.layout.WindowInsets
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
        // `<screen>+strip`: that screen with a minimised 1:1 call at the bottom (CallStrip.kt), laid out as MainActivity does.
        // `<screen>+frame`: the same MainActivity frame with no call (the strip area must take no room then).
        val strip = which.endsWith("+strip")
        val frame = strip || which.endsWith("+frame")
        val base = which.removeSuffix("+strip").removeSuffix("+frame")
        if (strip) app.calls.showForGallery(CallUi(uniffi.p2pcore.CallInfo("gallery", FakeChatSource.did("arjun"), arjun.name, false, false), uniffi.p2pcore.CallState.Active,
            activeSinceMs = System.currentTimeMillis() - 15_000))
        setContent { TinlineTheme(dark) {
            if (!frame && base != "gcall_strip") Gallery(base, app, this)
            else androidx.compose.foundation.layout.Column(Modifier.fillMaxSize()) {
                val on = callStripOn(app)
                Box(Modifier.weight(1f).fillMaxWidth().then(if (on) Modifier.consumeWindowInsets(WindowInsets.navigationBars.union(WindowInsets.ime)) else Modifier)) { Gallery(base, app, this@GalleryActivity) }
                CallStrips(app)
            }
        } }
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
        "groups_live" -> Page { GroupsLiveTab(app, {}, none, none) }
        "welcome" -> WelcomeScreen(none, none)
        // sign-up port (2026-10-13): the stepped gate, Add contact sheet and the first screen
        "g-welcome", "g-choose", "g-name", "g-pass", "g-pass-typed", "g-phrase", "g-phrase-copied", "g-confirm", "g-device", "g-terms", "g-perms", "g-done" ->
            GateStepPreview(which.removePrefix("g-"))
        // the one empty state: a new account's Chats / Calls (hero), one contact (open), Add contact over Chats
        "first-chats", "first-calls", "first-open", "first-add", "first-contacts" -> {
            val people = if (which == "first-open") listOf(arjun) else emptyList()
            val tab = when (which) { "first-calls" -> HomeTab.Calls; "first-contacts" -> HomeTab.Contacts; else -> HomeTab.Chats }
            HomeContent(people, true, false, emptyList(), {}, {}, none, {}, {}, none, emptyList(), null, tab = tab, chatRows = emptyList())
            if (which == "first-add") AddContactSheet(app, onClose = none, onMessage = {}, onProfile = {})
        }
        // § 6f, both sides: the owner asked / joined, the joiner told they accepted, the joiner waiting, Contacts with requests.
        "first-request", "first-request-joined", "first-accepted" -> {
            val tara = uniffi.p2pcore.ContactRequest("did:key:z6MkTara", "Tara Iyer", 0UL, 0UL)
            androidx.compose.runtime.DisposableEffect(which) {
                app.incoming.value = IncomingSheet(tara.did, tara.name, joined = which != "first-request", theirs = which == "first-accepted")
                onDispose { app.incoming.value = null }
            }
            HomeContent(emptyList(), true, false, emptyList(), {}, {}, none, {}, {}, none, emptyList(), null, tab = HomeTab.Chats, chatRows = emptyList())
            AddContactSheet(app, onClose = none, onMessage = {}, onProfile = {})
        }
        "first-waiting", "first-contacts-requests" -> {
            val tara = uniffi.p2pcore.ContactRequest("did:key:z6MkTara", "Tara Iyer", 0UL, 0UL)
            val nikhil = uniffi.p2pcore.ContactRequest("did:key:z6MkNikhil", "Nikhil Das", 0UL, 0UL)
            val inReq = if (which == "first-contacts-requests") listOf(tara) else emptyList()
            val outReq = if (which == "first-contacts-requests") listOf(nikhil) else listOf(tara)
            HomeContent(emptyList(), true, false, emptyList(), {}, {}, none, {}, {}, none, emptyList(), null,
                tab = if (which == "first-waiting") HomeTab.Chats else HomeTab.Contacts, chatRows = emptyList(), waitingIn = inReq, waitingOut = outReq)
        }
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
        "search" -> SearchScreen(app, none, {}, {}, {})
        "home_search" -> HomeContent(contacts, true, false, emptyList(), {}, {}, none, {}, {}, none, emptyList(), null, startSearching = true, startQuery = "jo")
        "home_empty" -> HomeContent(emptyList(), true, false, emptyList(), {}, {}, none, {}, {}, none, emptyList(), null)
        "home_offline" -> HomeContent(contacts, false, false, listOf(Need.Mic), {}, {}, none, {}, {}, none, emptyList(), null)
        "mycode" -> AddContactScreen(app, false, onClose = none, onCall = {}, onVerify = {})
        "scan" -> AddContactScreen(app, true, onClose = none, onCall = {}, onVerify = {})
        "paste" -> PasteCardScreen(app, "OSVC2:MFRGGZDFMZTWQ2LKNNWG23TPOBYXE43UOV3HO6DZPE======", {}, none, {})
        "adding" -> AddingScreen(maya, none)
        "added" -> AddedScreen(maya, arjun, none, none, none)
        "failed" -> FailedScreen(maya, null, none, none)
        "contact" -> ContactScreen(arjun, none, none, none)
        "contact_full" -> ContactScreen(arjun, none, none, none, history = recents)
        "contact_blocked" -> ContactScreen(arjun, none, none, none, block = BlockView(blockedByMe = true, blockedAtSecs = System.currentTimeMillis() / 1000))
        "block" -> { ContactScreen(arjun, none, none, none); BlockDialog(arjun.display(), none) {} }
        "verify" -> VerifyScreen(maya, arjun, "41203 88127 05519 73360 29841 66012 90475 13398 57206 84431 20987 36654".split(" "), none, none, none)
        "rename" -> RenameSheet("Arjun (work)", "Arjun", true, none) {}
        "incoming" -> IncomingContent("Arjun Oommen", arjun.did, none, none)
        "calling" -> InCallContent("Arjun Oommen", arjun.did, maya, false, false, 0, null, null, false, false, null, {}, {}, {}, {})
        "incall" -> InCallContent("Arjun Oommen", arjun.did, maya, true, false, 252, true, 4, true, false, null, {}, {}, {}, {}, onMessage = {})
        "group_chat", "group_chat_waiting" -> {
            // The real group conversation over the seed's "Weekend hike" (FakeGroupApi), not the core.
            remember { app.groupsForGallery(GroupsController(FakeGroupApi(app.currentDid() ?: "did:me", waiting = which.endsWith("waiting"))) { app.currentDid() }); app.groups.refresh() }
            GroupChatLive(app, "hike", onBack = none, onInfo = none, onRemoval = none, say = {})
        }
        "gcall", "gcall_about", "gcall_strip" -> {
            remember {
                val me = app.currentDid() ?: "did:me"
                app.groupCallUi.showForGallery(GcState(gid = "g", phase = GcPhase.Active, parts = listOf(me) + contacts.take(3).map { it.did },
                    sinceMs = System.currentTimeMillis() - 252_000, host = contacts[0].did, about = which == "gcall_about", minimized = which == "gcall_strip"))
            }
            Box(Modifier.fillMaxSize()) { HomeContent(contacts, true, false, emptyList(), {}, {}, none, {}, {}, none, recents, null, subLines = subs); GroupCallLayer(app) }
        }
        "incall_about" -> InCallContent("Arjun Oommen", arjun.did, maya, true, false, 252, true, 4, false, false, null, {}, {}, {}, {}, rttMs = 42, lossPct = .2, startWithInfo = true)
        "incall_relay_about" -> InCallContent("Arjun Oommen", arjun.did, maya, true, false, 252, false, 3, false, false, null, {}, {}, {}, {}, rttMs = 140, lossPct = 1.4, startWithInfo = true)
        "incall_weak_about" -> InCallContent("Arjun Oommen", arjun.did, maya, true, false, 252, true, 1, false, false, null, {}, {}, {}, {}, rttMs = 520, lossPct = 14.0, startWithInfo = true)
        "reconnecting_about" -> InCallContent("Arjun Oommen", arjun.did, maya, true, false, 771, true, 3, false, false, null, {}, {}, {}, {}, reconnecting = true, startWithInfo = true)
        "incall_relayed" -> InCallContent("Arjun Oommen", arjun.did, maya, true, false, 767, false, 3, false, true, null, {}, {}, {}, {}, startWithSheet = true)
        "incall_bt" -> {
            val bt = AudioRoute(AudioRoute.Kind.Bluetooth, "Pixel Buds", "bt")
            val r = RouteState(bt, listOf(AudioRoute(AudioRoute.Kind.Earpiece, "", "ear"), AudioRoute(AudioRoute.Kind.Speaker, "", "spk"), bt))
            InCallContent("Arjun Oommen", arjun.did, maya, true, false, 252, true, 4, false, false, null, {}, {}, {}, {}, routes = r)
        }
        "incall_bt_sheet" -> {
            val bt = AudioRoute(AudioRoute.Kind.Bluetooth, Routes.GENERIC_BT, "bt")
            val r = RouteState(bt, listOf(AudioRoute(AudioRoute.Kind.Earpiece, "", "ear"), AudioRoute(AudioRoute.Kind.Speaker, "", "spk"), bt))
            InCallContent("Arjun Oommen", arjun.did, maya, true, false, 252, true, 4, false, false, null, {}, {}, {}, {},
                startWithSheet = true, routes = r, onAllowBtNames = {})
        }
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
        // PLACEHOLDER groups & privacy preview: `--es screen pv_hike` (keys in PRoute.parse).
        "pv", "pv_chats", "pv_hike", "pv_family", "pv_book", "pv_removal", "pv_newgroup", "pv_info", "pv_hosts", "pv_arjun", "pv_meera",
        "pv_sam", "pv_contact", "pv_blocked", "pv_address", "pv_requests" ->
            PreviewApp(PreviewBackend.get(maya), PRoute.parse(which.substringAfter("pv_", "chats")), onExit = { act.finish() }, onShowMyQr = none)
        else -> if (which.startsWith("pf_")) ProfileGallery(which, act) else when (which) {
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
                    "conv_new" -> fake.setLink(did, Link.Unknown)
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
            // A finished call yesterday evening, shown as a pill among the messages.
            val yesterday = (System.currentTimeMillis() / 1000 - 86400).let { it - it % 86400 + 16 * 3600 }
            val calls = if (which == "conv") listOf(uniffi.p2pcore.CallRecord("g-call", did, name, false, yesterday.toULong(), 742u, "hangup_local", true, false, false)) else emptyList()
            ConversationScreen(fake, did, name, true, none, none, preview, addedAtSecs = System.currentTimeMillis() / 1000 - (if (which == "conv_new") 0 else 30 * 86400),
                calls = calls, onVideo = none)
        } else HomeContent(contacts, true, false, emptyList(), {}, {}, none, {}, {}, none, emptyList(), null)
        }
    }
}

/** Playground-ported profile / settings pages on the fake profile adapter (`--es screen pf_me`, keys below). */
@Composable
private fun ProfileGallery(which: String, act: ComponentActivity) {
    val none = {}
    val fake = remember { FakeProfileStore(java.io.File(act.filesDir, "pg")).also { ProfileBackend.override = it } }
    DisposableEffect(fake) {
        onDispose { if (ProfileBackend.override === fake) ProfileBackend.override = null }
    }
    val now = System.currentTimeMillis()
    remember(which) {
        if (which.contains("busy")) fake.put(FakeProfileStore.ME, fake.fields().put("availability", org.json.JSONObject().put("available", false).put("until", (now + 3_600_000L) / 1000)))
        if (which.contains("busyopen")) fake.put(FakeProfileStore.ME, fake.fields().put("availability", org.json.JSONObject().put("available", false)))
        if (which in setOf("pf_me_pic", "pf_me_remove", "pf_me_pic_replace", "pf_settings_pic")) fake.put(FakeProfileStore.ME, fake.fields(), picture = "me")
        if (which.contains("extra")) fake.put(FakeProfileStore.ME, fake.fields().put("website", "https://maya.example/")
            .put("work", org.json.JSONObject().put("title", "Interior architect").put("studio", org.json.JSONObject().put("name", "Tin & Lime").put("city", "Kochi"))
                .put("tools", org.json.JSONArray(listOf("Rhino", "Figma", "Pencil")))))
        if (which.contains("offline")) fake.online.value = false
        0
    }
    val pickBmp = remember { java.io.File(act.filesDir, "pg/pick.png").takeIf { it.exists() }?.let { android.graphics.BitmapFactory.decodeFile(it.path) } ?: sample("pick", 512, 640, 480) }
    when (which.removeSuffix("_offline")) {
        "pf_me", "pf_me_busy", "pf_me_busyopen", "pf_me_pic", "pf_me_extra" -> MyProfileScreen(fake, "Maya Kurian", none)
        "pf_me_edit_status" -> MyProfileScreen(fake, "Maya Kurian", none, MyProfilePreview(editField = "status"))
        "pf_me_edit_name" -> MyProfileScreen(fake, "Maya Kurian", none, MyProfilePreview(editField = "name"))
        "pf_me_edit_about" -> MyProfileScreen(fake, "Maya Kurian", none, MyProfilePreview(editField = "about"))
        "pf_me_add" -> MyProfileScreen(fake, "Maya Kurian", none, MyProfilePreview(addOpen = true))
        "pf_me_add_custom" -> MyProfileScreen(fake, "Maya Kurian", none, MyProfilePreview(addOpen = true, addPick = ""))
        "pf_me_add_website" -> MyProfileScreen(fake, "Maya Kurian", none, MyProfilePreview(addOpen = true, addPick = "website"))
        "pf_me_picture" -> MyProfileScreen(fake, "Maya Kurian", none, MyProfilePreview(picked = Picked(pickBmp)))
        "pf_me_pic_replace" -> MyProfileScreen(fake, "Maya Kurian", none, MyProfilePreview(picked = Picked(pickBmp)))
        "pf_me_remove" -> MyProfileScreen(fake, "Maya Kurian", none, MyProfilePreview(removeAsk = true))
        "pf_me_link" -> MyProfileScreen(fake, "Maya Kurian", none, MyProfilePreview(link = "https://arjun.example/notes"))
        "pf_me_picerr" -> MyProfileScreen(fake, "Maya Kurian", none, MyProfilePreview(picError = "Tinline can use PNG, JPEG or WebP pictures. That file is a different kind of file."))
        "pf_me_pending" -> MyProfileScreen(fake, "Maya Kurian", none, MyProfilePreview(pending = true))
        "pf_settings", "pf_settings_busy", "pf_settings_pic" -> SettingsLanding(fake, "Maya Kurian", listOf(AccountRow(FakeProfileStore.ME, "Maya Kurian", true)), emptyList(),
            none, none, {}, none, none, none, none, none)
        "pf_settings_perm" -> SettingsLanding(fake, "Maya Kurian", listOf(AccountRow(FakeProfileStore.ME, "Maya Kurian", true)), permProblems(listOf(Need.Notifications)),
            none, none, {}, none, none, none, none, none)
        "pf_account" -> AccountProfileScreen(fake, "Maya Kurian", none, none, none, none)
        "pf_code" -> MyContactCodeScreen(fake, "Maya Kurian", "tinline-gallery-code-0000", false, none, none)
        "pf_code_failed" -> MyContactCodeScreen(fake, "Maya Kurian", null, true, none, none)
        "pf_notifications" -> NotificationsPage(emptyList(), none) {}
        "pf_notifications_off" -> NotificationsPage(listOf(Need.Notifications), none) {}
        "pf_notifications_battery" -> NotificationsPage(listOf(Need.Battery), none) {}
        "pf_sounds" -> SoundsPage(none)
        "pf_privacy" -> PrivacyLanding(1, 1, true, none, none, none, none, none)
        "pf_calls" -> CallsAllowedPage(listOf(CallsRow(FakeProfileStore.did("Arjun"), "Arjun Oommen", true), CallsRow(FakeProfileStore.did("Dev"), "Dev Patel", false),
            CallsRow(FakeProfileStore.did("Meera"), "Meera Nair", true)), none) { _, _ -> }
        else -> MyProfileScreen(fake, "Maya Kurian", none)
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
