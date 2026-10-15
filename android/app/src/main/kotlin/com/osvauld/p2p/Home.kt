package com.osvauld.p2p

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import uniffi.p2pcore.Contact
import uniffi.p2pcore.CallRecord

private fun String.matchesQuery(q: String) = q.isBlank() || contains(q.trim(), ignoreCase = true)

@Composable
fun HomeScreen(
    app: P2pApp, missing: List<Need>, onFix: (Need) -> Unit,
    onAdd: (scan: Boolean) -> Unit, onSettings: () -> Unit, onContact: (Contact) -> Unit, onCall: (Contact) -> Unit,
    onChat: (String) -> Unit = {}, onNewChat: () -> Unit = {}, callError: String? = null, onPaste: () -> Unit = {},
    /** Hoisted tab, so coming back from a chat or group lands on the same tab. Null keeps it local. */
    homeTab: MutableState<HomeTab>? = null,
    /** Opens a PLACEHOLDER preview screen (Settings › Developer). */
    onPreview: (PRoute) -> Unit = {},
    /** The Groups tab (real groups, GroupsLive.kt). */
    onGroup: (String) -> Unit = {}, onGroupInfo: (String) -> Unit = {}, onNewGroup: () -> Unit = {}, onJoinGroup: () -> Unit = {}, say: (String) -> Unit = {},
    /** Opens the full-screen search (Search.kt). */
    onSearch: () -> Unit = {},
) {
    val ctx = androidx.compose.ui.platform.LocalContext.current
    val status by app.status.collectAsState()
    val contacts by app.contacts.collectAsState()
    val history by app.history.collectAsState()
    val avail by app.availability.collectAsState()
    val chats by app.chat.chats.collectAsState()
    val links by app.chat.links.collectAsState()
    var grace by remember { mutableStateOf(true) }
    var sheet by remember { mutableStateOf(false) }
    val localTab = rememberSaveable { mutableStateOf(HomeTab.Chats) }
    var tab by (homeTab ?: localTab)
    val groupList by app.groups.list.collectAsState()
    val groupLog by app.groupLog.collectAsState()
    val groupCalls = remember(groupLog, groupList) {
        GroupCallsUi(groupLog, groupList.filter { GroupText.active(it.myRole) }.associate { it.id to it.name }, onGroupInfo) { gid -> app.groupCallUi.call(gid, say) }
    }
    val groupInvitations by app.groups.invitations.collectAsState()
    val groupBadge = GroupText.badge(groupList, groupInvitations)
    val requestsIn by app.groups.incoming.collectAsState()
    val waitingIn by app.requestsIn.collectAsState()
    val waitingOut by app.requestsOut.collectAsState()
    // "Not now" on a Contacts card hides it for now; the request stays in the core (and comes back next time).
    var hiddenRequests by remember { mutableStateOf(emptySet<String>()) }
    val pendingRequests = requestsIn.count { it.state == uniffi.p2pcore.ChatRequestState.PENDING }
    // Relative times ("4 min ago") go stale: refresh them once a minute.
    var now by remember { mutableLongStateOf(System.currentTimeMillis() / 1000) }
    LaunchedEffect(Unit) { delay(6000); grace = false }
    LaunchedEffect(Unit) { while (true) { delay(60_000); now = System.currentTimeMillis() / 1000 } }
    // The red count on Calls is missed calls since that tab was last open.
    val prefs = remember { ctx.getSharedPreferences("chat_ui", android.content.Context.MODE_PRIVATE) }
    var callsSeen by remember { mutableLongStateOf(prefs.getLong("calls_seen", 0L)) }
    LaunchedEffect(tab, history) {
        if (tab == HomeTab.Calls) { callsSeen = System.currentTimeMillis() / 1000; prefs.edit().putLong("calls_seen", callsSeen).apply() }
    }
    val byDid = remember(contacts) { contacts.associateBy { it.did } }
    val recents = remember(history, byDid, now) { history.take(200).map { it.toRecent(byDid, now) } }
    val blocks by app.blocks.collectAsState()
    val subs = remember(history, contacts, now, blocks) { contacts.associate { it.did to (blocks.status(it.did) ?: contactSubLine(it.did, history, now)) } }
    val online = status?.online == true
    val myDid = remember(chats) { app.currentDid() }
    val rows = remember(chats, contacts, links, online, now, myDid, blocks, groupList) {
        val you = myDid
        val all = chatRows(chats, contacts, links, online, now * 1000, myDid, blocks) + groupChatRows(groupList, app.groups.myDid, now * 1000)
        // "You" stays pinned first; the rest newest first, 1:1 and groups together.
        all.sortedWith(compareBy<ChatRowUi> { it.did != you || it.group }.thenByDescending { it.atMs })
    }
    val inCall by app.calls.ui.collectAsState()
    val callPhase = when (val u = inCall) {
        null -> CallPhase.None
        else -> when (u.state) {
            is uniffi.p2pcore.CallState.Dialing -> CallPhase.Outgoing
            is uniffi.p2pcore.CallState.Ringing -> if (u.info.incoming) CallPhase.Incoming else CallPhase.Outgoing
            is uniffi.p2pcore.CallState.Active -> if (u.stats?.reconnecting == true) CallPhase.Reconnecting else CallPhase.Active
            else -> CallPhase.None
        }
    }
    HomeContent(
        contacts = contacts, online = online, connectingGrace = grace, missing = missing,
        onFix = onFix, onAdd = onAdd, onSettings = onSettings, onContact = onContact, onCall = onCall,
        onAvailability = { sheet = true }, recents = recents, callError = callError,
        available = avail.available, onTurnOn = { app.setAvailable(true) }, subLines = subs,
        tab = tab, onTab = { tab = it }, chatRows = rows, myDid = myDid, onChat = onChat, onNewChat = onNewChat,
        chatBadge = rows.sumOf { it.unread } + pendingRequests, callBadge = history.count { it.missed && it.startedAt.toLong() > callsSeen },
        firstRun = { FirstRunAdd(app, onScan = { onAdd(true) }, onPaste = onPaste) },
        groupsBody = { GroupsLiveTab(app, onGroup, onNewGroup, onJoinGroup, say, onChat) }, groupBadge = groupBadge,
        requestsBlock = { GroupRequestsBlock(app, onChat, say) },
        onSearch = onSearch, history = history, groupCalls = groupCalls,
        canCall = { did -> blocks.callGate(did, byDid.containsKey(did)) == CallGate.Ok && byDid.containsKey(did) },
        isBlocked = { did -> blocks.of(did).blockedByMe }, onGroup = onGroup, onNewGroup = onNewGroup, callLook = callPhase,
        waitingIn = waitingIn.filter { it.did !in hiddenRequests }, waitingOut = waitingOut,
        onAcceptRequest = { did -> app.scope.launch { app.acceptRequest(did) } }, onNotNowRequest = { did -> hiddenRequests = hiddenRequests + did },
    )
    if (sheet) AvailabilitySheet(avail.available, avail.until?.toLong(), app.node.profile()?.name ?: "", onDismiss = { sheet = false }) { available, until ->
        sheet = false
        app.scope.launch { app.setAvailable(available, until) }
    }
}

/** Banner priority: offline, mic, notifications, background (battery, lock-screen), then availability. Only one shows. */
@Composable
private fun TopBanner(online: Boolean, grace: Boolean, missing: List<Need>, onFix: (Need) -> Unit, available: Boolean, onTurnOn: () -> Unit) {
    when {
        !online && !grace -> Banner(BannerKind.Error, PgIcons.Off, "You’re offline", "Your calls come through again once you’re back online.")
        Need.Mic in missing -> Banner(BannerKind.Error, PgIcons.Micoff, "Microphone is off", "People won’t hear you on calls.", "Allow") { onFix(Need.Mic) }
        Need.Notifications in missing -> Banner(BannerKind.Warn, PgIcons.Mute, "Calls won’t ring", "Notifications are off for Tinline.", "Turn on") { onFix(Need.Notifications) }
        Need.Battery in missing -> Banner(BannerKind.Warn, PgIcons.Battery, "Calls may be missed when idle", "Android is limiting Tinline in the background.", "Fix") { onFix(Need.Battery) }
        Need.FullScreen in missing -> Banner(BannerKind.Warn, PgIcons.Lock, "Calls won’t show on the lock screen", "Allow full-screen calls so Tinline rings like the phone app.", "Allow") { onFix(Need.FullScreen) }
        !available -> Banner(BannerKind.Neutral, PgIcons.Callend, "You’re not available", "Calls won’t ring until you turn it back on.", "Turn on", onTurnOn)
        // "Set a passphrase" slots in after this; it is a blocking screen today.
    }
}

/**
 * Home (screens/shell/AndroidShell.svelte): the 56 dp app bar with the live mark, the tab's title and list (fading through
 * on a tab change, each tab keeping its scroll), the search dock on Chats and Groups, and the 4-tab bar.
 */
@Composable
fun HomeContent(
    contacts: List<Contact>, online: Boolean, connectingGrace: Boolean, missing: List<Need>, onFix: (Need) -> Unit,
    onAdd: (scan: Boolean) -> Unit, onSettings: () -> Unit, onContact: (Contact) -> Unit, onCall: (Contact) -> Unit,
    onAvailability: () -> Unit, recents: List<RecentCall>, callError: String?, startSearching: Boolean = false, startQuery: String = "",
    available: Boolean = true, onTurnOn: () -> Unit = {}, subLines: Map<String, String> = emptyMap(),
    tab: HomeTab = HomeTab.Contacts, onTab: (HomeTab) -> Unit = {}, chatRows: List<ChatRowUi> = emptyList(), myDid: String? = null,
    onChat: (String) -> Unit = {}, onNewChat: () -> Unit = {}, chatBadge: Int = 0, callBadge: Int = 0,
    firstRun: (@Composable () -> Unit)? = null,
    /** The Groups tab's content; null hides the tab (the gallery and tests). */
    groupsBody: (@Composable () -> Unit)? = null, groupBadge: Int = 0,
    /** Group chat requests (U3), above the chats; draws nothing when there are none. */
    requestsBlock: (@Composable () -> Unit)? = null,
    /** Opens the full-screen search. */
    onSearch: () -> Unit = {},
    /** The call log for the Calls tab (newest first). */
    history: List<CallRecord> = emptyList(),
    /** Group calls for the Calls tab (HomeLists.kt). */
    groupCalls: GroupCallsUi? = null,
    canCall: (String) -> Boolean = { true },
    isBlocked: (String) -> Boolean = { false },
    onGroup: (String) -> Unit = {}, onNewGroup: () -> Unit = {},
    callLook: CallPhase = CallPhase.None,
    /** Contact requests: made with my code (Contacts › "Waiting for you"), and mine waiting for theirs. */
    waitingIn: List<uniffi.p2pcore.ContactRequest> = emptyList(), waitingOut: List<uniffi.p2pcore.ContactRequest> = emptyList(),
    onAcceptRequest: (String) -> Unit = {}, onNotNowRequest: (String) -> Unit = {},
) {
    val c = Tin.c
    val banner: @Composable () -> Unit = { TopBanner(online, connectingGrace, missing, onFix, available, onTurnOn) }
    val byDid = remember(contacts) { contacts.associateBy { it.did } }
    // No contacts yet: Chats and Contacts are the add screen (my code, Copy, Share, Scan, Paste).
    // The old first-run add screen is replaced by the one empty state (EmptyLine.kt): the hero on Chats and Calls.
    val firstRunShown = false
    val hero: @Composable () -> Unit = { EmptyHero(contacts, onAdd = { onAdd(false) }, onMessage = onChat, onNewChat = onNewChat, sent = waitingOut) }
    val dock = tab == HomeTab.Chats || (tab == HomeTab.Groups && groupsBody != null)
    val look = statusLook(call = callLook, conn = when { online -> ConnState.Online; connectingGrace -> ConnState.Connecting; else -> ConnState.Offline })
    Column(Modifier.fillMaxSize().background(c.bg).statusBarsPadding()) {
        HomeAppBar(look, showSearch = !dock, onSearch = onSearch, onSettings = onSettings)
        TabPanel(tab, Modifier.weight(1f).fillMaxWidth()) { t ->
            Column(Modifier.fillMaxSize()) {
                TabTitle(t.title)
                val state = HomeScroll.states.getValue(t)
                Box(Modifier.weight(1f).fillMaxWidth()) {
                    if (firstRunShown && t != HomeTab.Calls && t != HomeTab.Groups) Column(Modifier.fillMaxSize()) { banner(); firstRun?.invoke() }
                    else when (t) {
                        HomeTab.Chats -> ChatsTabBody(if (chatRows.any { it.group || (it.did != myDid && it.atMs > 0) }) chatRows else emptyList(), state, onOpen = { r -> if (r.group) onGroup(r.did) else onChat(r.did) },
                            top = { Column { banner(); requestsBlock?.invoke() } }, empty = hero)
                        HomeTab.Groups -> groupsBody?.invoke()
                        HomeTab.Calls -> CallsTabBody(
                            history, groupCalls = groupCalls, nameOf = { r -> byDid[r.peerDid]?.display() ?: r.peerName.ifBlank { "Unknown" } }, canCall = canCall, state = state,
                            onProfile = { did -> byDid[did]?.let(onContact) }, onCall = { did -> byDid[did]?.let(onCall) },
                            top = {
                                Column {
                                    banner()
                                    if (callError != null) Text(callError, Modifier.padding(horizontal = 16.dp, vertical = 8.dp), style = TinFs.sub, color = c.danger)
                                }
                            },
                            empty = if (contacts.isEmpty()) hero else null,
                        )
                        HomeTab.Contacts -> ContactsTabBody(
                            contacts, blocked = isBlocked, sub = { did -> if (isBlocked(did)) "Blocked" else subLines[did]?.takeIf { it.startsWith("Blocked") }.orEmpty() },
                            state = state, onOpen = onContact, onAdd = { onAdd(false) }, top = { banner() },
                            waitingIn = waitingIn, waitingOut = waitingOut, onAccept = onAcceptRequest, onNotNow = onNotNowRequest,
                        )
                    }
                }
            }
        }
        if (dock) SearchDock(if (tab == HomeTab.Groups) "New group" else "New chat", onSearch, if (tab == HomeTab.Groups) onNewGroup else onNewChat)
        TinTabBar(tab, onTab, chatBadge, showGroups = groupsBody != null, groupBadge = groupBadge)
    }
}

fun recentIcon(k: RecentCall.Kind): ImageVector = when (k) {
    RecentCall.Kind.Incoming -> PgIcons.CallIn
    RecentCall.Kind.Missed -> PgIcons.CallIn
    else -> PgIcons.CallOut
}

/** The full call log (Home "See all"). */
@Composable
fun HistoryScreen(app: P2pApp, onBack: () -> Unit, onContact: (Contact) -> Unit) {
    val history by app.history.collectAsState()
    val contacts by app.contacts.collectAsState()
    val now = System.currentTimeMillis() / 1000
    val byDid = remember(contacts) { contacts.associateBy { it.did } }
    val rows = remember(history, byDid) { history.map { it.toRecent(byDid, now) } }
    val c = Tin.c
    Page {
        TopBar("Recent calls", onBack)
        if (rows.isEmpty()) Hint("No calls yet. History stays on this phone only.", Modifier.padding(24.dp))
        else LazyColumn(Modifier.weight(1f), contentPadding = PaddingValues(bottom = 24.dp)) {
            items(rows, key = { it.callId + it.whenLong }) { r ->
                val red = r.kind == RecentCall.Kind.Missed
                RowItem(r.name, r.did, subIcon = recentIcon(r.kind), sub = r.whenText, nameColor = if (red) c.er else c.ink, subColor = if (red) c.er else c.ink2,
                    onClick = { byDid[r.did]?.let(onContact) }, onCall = null)
            }
            item { Hint("History stays on this phone only.", Modifier.padding(horizontal = 20.dp, vertical = 12.dp)) }
        }
    }
}

@Composable
fun SearchBar(query: String, onChange: (String) -> Unit, onClose: () -> Unit, hint: String = "Search contacts") {
    val c = Tin.c
    val focus = remember { FocusRequester() }
    LaunchedEffect(Unit) { runCatching { focus.requestFocus() } }
    Row(Modifier.fillMaxWidth().heightIn(min = 64.dp).padding(start = 4.dp, end = 8.dp), verticalAlignment = Alignment.CenterVertically) {
        IconBtn(PgIcons.Back, "Close search", onClose)
        Box(Modifier.weight(1f).heightIn(min = 48.dp), contentAlignment = Alignment.CenterStart) {
            if (query.isEmpty()) Text(hint, style = TinType.bodyL.copy(fontSize = 18.sp), color = c.ink2)
            BasicTextField(
                query, onChange, Modifier.fillMaxWidth().focusRequester(focus), singleLine = true,
                textStyle = TinType.bodyL.copy(fontSize = 18.sp, color = c.ink), cursorBrush = SolidColor(c.pr),
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search), keyboardActions = KeyboardActions(),
            )
        }
        if (query.isNotEmpty()) IconBtn(PgIcons.X, "Clear", { onChange("") })
    }
    Box(Modifier.fillMaxWidth().height(1.dp).background(c.ln))
}

@Composable
private fun RowItem(
    name: String, key: String, sub: String? = null, subIcon: ImageVector? = null, highlight: String = "",
    nameColor: Color = Tin.c.ink, subColor: Color = Tin.c.ink2, onClick: () -> Unit, onCall: (() -> Unit)?,
) {
    val c = Tin.c
    Row(
        Modifier.fillMaxWidth().heightIn(min = 72.dp).clickable(role = Role.Button, onClick = onClick).padding(start = 20.dp, end = 8.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        Avatar(name, key, 44.dp)
        Column(Modifier.weight(1f)) {
            val hi = highlight.trim()
            val i = if (hi.isEmpty()) -1 else name.indexOf(hi, ignoreCase = true)
            Text(
                if (i < 0) androidx.compose.ui.text.AnnotatedString(name) else buildAnnotatedString {
                    append(name.substring(0, i))
                    withStyle(SpanStyle(background = c.prc, color = c.onPrc)) { append(name.substring(i, i + hi.length)) }
                    append(name.substring(i + hi.length))
                },
                style = TinType.bodyL.copy(fontWeight = FontWeight.SemiBold, lineHeight = 22.sp), color = nameColor, maxLines = 1, overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis,
            )
            if (sub != null) Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                if (subIcon != null) Icon(subIcon, null, tint = subColor, modifier = Modifier.size(16.dp))
                Text(sub, style = TinType.bodyM, color = subColor)
            }
        }
        if (onCall != null) IconBtn(PgIcons.Phone, "Call $name", onCall, tint = c.pr) else Spacer(Modifier.width(12.dp))
    }
}

/** First run: my code with Copy and Share under it, Scan their code, Paste their card. One screen, nothing to open. */
@Composable
private fun FirstRunAdd(app: P2pApp, onScan: () -> Unit, onPaste: () -> Unit) {
    val c = Tin.c
    val meName = remember { app.node.profile()?.name ?: "" }
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(start = 24.dp, end = 24.dp, top = 8.dp, bottom = 24.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp), horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        H1("Add your first contact", align = TextAlign.Center)
        Lead("Show your code to a friend, or copy it and send it. Or scan theirs.", Modifier.widthIn(max = 320.dp), align = TextAlign.Center)
        MyCodeBlock(app, meName, qr = 220.dp, showNewCode = false)
        TinButton("Scan their code", onScan, icon = PgIcons.Scan)
        TinButton("Paste their card", onPaste, style = BtnStyle.Outlined, icon = PgIcons.Paste)
        Row(
            Modifier.fillMaxWidth().clip(SmoothCornerShape(14.dp)).background(c.sf2).padding(horizontal = 14.dp, vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Dot(c.pr, 10.dp)
            Text("Waiting for them to scan — keep this open.", style = TinType.bodyM, color = c.ink2)
        }
    }
}
