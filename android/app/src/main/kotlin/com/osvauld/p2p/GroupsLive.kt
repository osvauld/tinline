package com.osvauld.p2p

import android.content.Intent
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Image
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.blur
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.semantics.stateDescription
import uniffi.p2pcore.GroupSummary
import uniffi.p2pcore.GroupChatRequest
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CheckboxDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.toggleableState
import androidx.compose.ui.state.ToggleableState
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.p2pcore.ChatRequestState
import uniffi.p2pcore.GroupDetail
import uniffi.p2pcore.GroupInvitation
import uniffi.p2pcore.GroupInvitePeek
import uniffi.p2pcore.GroupItemKind
import uniffi.p2pcore.GroupMessage
import uniffi.p2pcore.OnlineMember
import uniffi.p2pcore.GroupDelivery as CoreDelivery
import uniffi.p2pcore.GroupMember as CoreMember
import uniffi.p2pcore.GroupRole as CoreRole

/*
 * Group chat screens on the core (state: GroupsCore.kt), after the boards in docs/design/groups
 * (Main, AndroidGroupInfo, AndroidRemoval): the Groups tab (with invitations to us on top), a group's
 * chat, group info with member actions and the invitations we sent, add members (contact picker, G3),
 * new group, join (paste, scan or a tapped tinline://g/ link), the invite link with a large QR, and
 * why-removed. No "add my other device": linked devices get the group by own-device sync (G2).
 */

/** A DID as the user knows it: their contact name, else what the group calls them. */
private fun nameOf(app: P2pApp, did: String, fallback: String): String {
    // Per row while composing: the cached DID, never a core call on the main thread.
    if (did == app.groups.myDid) return "You"
    app.contacts.value.firstOrNull { it.did == did }?.let { return it.display() }
    return fallback.ifBlank { "Someone" }
}

/** Runs a group action off the main thread, then hands its outcome back on it. */
@Composable
private fun rememberRunner(): (suspend () -> Result<String>, (Result<String>) -> Unit) -> Unit {
    val scope = rememberCoroutineScope()
    return remember(scope) { { work, done -> scope.launch { done(withContext(Dispatchers.IO) { work() }) } } }
}

/** "X asked this phone to host <group>": Host here / Decline (G2 `host_requested_here`). */
@Composable
private fun HostAskCard(app: P2pApp, d: GroupDetail, say: (String) -> Unit) {
    val by = GroupText.hostAsker(d) ?: return
    val exec = rememberRunner()
    val c = Tin.c
    NoticeCard(c.warn, c.onWarn, "${firstName(by)} asked this phone to host ${d.name}",
        "A host keeps a full copy of the group, passes messages on while members’ phones sleep and learns members’ group addresses. " +
            "It needs to stay online and uses battery: a computer that stays on is a better host. Hosting doesn’t make you an admin.",
        Modifier.padding(horizontal = 16.dp)) {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            TinButton("Host here", { exec({ app.groups.acceptHost(d.id, true) }) { r -> say(r.fold({ it }, { it.message ?: "Couldn’t do that" })) } },
                Modifier.weight(1f), style = BtnStyle.Filled, height = 44.dp)
            TinButton("Decline", { exec({ app.groups.acceptHost(d.id, false).map { "Declined. The admins see no answer from this phone." } }) { r -> say(r.fold({ it }, { it.message ?: "Couldn’t do that" })) } },
                Modifier.weight(1f), style = BtnStyle.Outlined, height = 44.dp)
        }
    }
}

/**
 * Chat requests used to sit above the chats; the playground moved them to the Groups tab (GroupsList.svelte), so this block
 * is empty now and Home's `requestsBlock` slot can go when the shell is next touched.
 */
@Composable
@Suppress("UNUSED_PARAMETER")
fun GroupRequestsBlock(app: P2pApp, onChat: (String) -> Unit, say: (String) -> Unit) {}

// ------------------------------------------------------------------ the Groups tab (playground screens/groups/GroupsList.svelte)

private fun dayOnly(ms: Long): String = java.time.Instant.ofEpochMilli(ms).atZone(java.time.ZoneId.systemDefault())
    .format(java.time.format.DateTimeFormatter.ofPattern("d MMM", java.util.Locale.getDefault()))

/** A group call running in a group, as far as the list knows: whether this device is in it. */
private data class LiveCall(val joined: Boolean)

/**
 * The Groups tab: New group (and Join with a link), invitations to me, chat requests (incoming cards, then the sent ones),
 * my groups newest first, and the folded "Left or removed". Sections fold in and out in place; nothing else moves.
 */
@Composable
fun GroupsLiveTab(app: P2pApp, onOpen: (String) -> Unit, onNew: () -> Unit, onJoin: () -> Unit, say: (String) -> Unit = {}, onChat: (String) -> Unit = {}) {
    val c = Tin.c
    val list by app.groups.list.collectAsState()
    val invitations by app.groups.invitations.collectAsState()
    val incoming by app.groups.incoming.collectAsState()
    val sent by app.groups.outgoing.collectAsState()
    val contacts by app.contacts.collectAsState()
    val pendingInv = GroupText.pendingInvitations(invitations)
    val requestsIn = incoming.filter { it.state == ChatRequestState.PENDING }
    val exec = rememberRunner()
    var joining by remember { mutableStateOf<String?>(null) }
    var creating by remember { mutableStateOf(false) }
    var goneOpen by rememberSaveable { mutableStateOf(false) }
    val me = app.groups.myDid
    val now = System.currentTimeMillis()
    val active = list.filter { GroupText.active(it.myRole) }
    val gone = list.filter { !GroupText.active(it.myRole) }
    val done: (Result<String>) -> Unit = { r -> say(r.fold({ it }, { it.message ?: "Couldn’t do that" })) }
    LaunchedEffect(Unit) { withContext(Dispatchers.IO) { app.groups.refresh() } }
    // Group calls running in my groups, re-read when the core says something about calls changed.
    // The call UI keeps this current: Announced, Joined, Left and Ended all re-read or drop the group's call, so a
    // call that ended never leaves a Join pill behind.
    val gcs by app.groupCallUi.state.collectAsState()
    LaunchedEffect(active.map { it.id }) { withContext(Dispatchers.IO) { active.forEach { app.groupCallUi.refreshLive(it.id) } } }
    val calls: Map<String, LiveCall> = gcs.live.mapValues { LiveCall(it.value.joined) }.filterKeys { k -> active.any { it.id == k } }
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 96.dp)) {
        item(key = "new") {
            TopRow(GIcons.Plus, "New group", "You’ll be the owner") { creating = true }
        }
        item(key = "join") {
            TopRow(GIcons.Link, "Join with a link", "Paste or scan a group invite", onJoin)
        }
        // ---- invitations to me
        item(key = "inv") {
            GFold(pendingInv.isNotEmpty()) {
                GSec("Invitations to you", pendingInv.size)
                Column(Modifier.padding(horizontal = 8.dp)) {
                    pendingInv.forEach { inv ->
                        key(inv.id) {
                            val who = nameOf(app, inv.inviterDid, inv.inviterName)
                            val busy = joining == inv.id
                            ReqCard {
                                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                                    Avatar(who, inv.inviterDid, 40.dp)
                                    Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(1.dp)) {
                                        Text(bold(firstName(who), " invited you to ", inv.groupName), style = GType.body, color = c.ink)
                                        Text("Valid until ${dayOnly(inv.expires.toLong())}", style = GType.sub, color = c.ink2)
                                    }
                                }
                                GActs(top = 0.dp) {
                                    GBtn("Decline", { exec({ app.groups.declineInvitation(inv.id) }, done) }, GBtnKind.Quiet, enabled = !busy)
                                    GBtn(if (busy) "Joining…" else "Accept", {
                                        joining = inv.id
                                        exec({ app.groups.acceptInvitation(inv.id) }) { r -> joining = null; r.onSuccess { onOpen(it); say("You joined ${inv.groupName}.") }.onFailure { say(it.message ?: "Couldn’t join") } }
                                    }, GBtnKind.Primary, enabled = !busy)
                                }
                            }
                        }
                    }
                }
            }
        }
        // ---- chat requests, incoming then sent
        item(key = "req") {
            GFold(requestsIn.isNotEmpty() || sent.isNotEmpty()) {
                GSec("Chat requests", requestsIn.size.takeIf { it > 0 })
                Column(Modifier.padding(horizontal = 8.dp)) {
                    requestsIn.forEach { r ->
                        key(r.id) {
                            val name = nameOf(app, r.peerDid, r.peerName)
                            ReqCard {
                                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                                    Avatar(name, r.peerDid, 40.dp)
                                    Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(1.dp)) {
                                        Text(bold(name, " wants to chat"), style = GType.body, color = c.ink)
                                        Text("Member of ${r.groupName}", style = GType.sub, color = c.ink2)
                                    }
                                }
                                Text("Accepting adds ${firstName(name)} as a contact for messages only. You can allow calls later.", style = GType.sub, color = c.ink2)
                                GActs(top = 0.dp) {
                                    GBtn("Block", { exec({ app.groups.blockRequester(r.id, r.peerDid) }, done) }, GBtnKind.DangerQuiet)
                                    GBtn("Decline", { exec({ app.groups.decline(r.id) }, done) }, GBtnKind.Quiet)
                                    GBtn("Accept", { exec({ app.groups.accept(r.id) }, done) }, GBtnKind.Primary)
                                }
                            }
                        }
                    }
                }
                if (sent.isNotEmpty()) {
                    GSub2("Sent")
                    Column(Modifier.padding(horizontal = 8.dp)) {
                        sent.forEach { r ->
                            key(r.id) {
                                val name = nameOf(app, r.peerDid, r.peerName)
                                val (line, bad) = GroupText.sentLine(r)
                                val open = r.contactAdded && contacts.any { it.did == r.peerDid }
                                ListRow(onClick = if (open) ({ onChat(r.peerDid) }) else null) {
                                    Avatar(name, r.peerDid, 48.dp)
                                    Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                                        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                            Text(name, Modifier.weight(1f), style = GType.bodyStrong, color = c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                            Text(listTime(r.at.toLong(), now), style = GType.meta, color = c.ink2)
                                        }
                                        Text(line, style = GType.sub, color = if (bad) c.er else c.ink2, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        // ---- my groups
        item(key = "yours") { GSec("Your groups", active.size) }
        item(key = "empty") {
            GFold(active.isEmpty()) {
                Text("No groups yet. Create one, or accept an invitation.", Modifier.padding(horizontal = 20.dp, vertical = 8.dp), style = GType.note, color = c.ink2)
            }
        }
        items(active, key = { "g" + it.id }) { s ->
            Box(Modifier.animateItem(fadeInSpec = null, fadeOutSpec = null, placementSpec = tween(GMotion.ENTER, easing = GMotion.EaseOut)).padding(horizontal = 8.dp)) {
                GroupRow(s, me, now, calls[s.id]) { onOpen(s.id) }
            }
        }
        // ---- left or removed, folded
        if (gone.isNotEmpty()) item(key = "gone") {
            Column(Modifier.padding(top = 6.dp)) {
                val rot by animateFloatAsState(if (goneOpen) 180f else 0f, tween(GMotion.ENTER, easing = GMotion.EaseOut), label = "chev")
                Box(Modifier.fillMaxWidth().heightIn(min = 44.dp).gTap(label = if (goneOpen) "Fold" else "Unfold") { goneOpen = !goneOpen }
                    .semantics { stateDescription = if (goneOpen) "Expanded" else "Collapsed" }, contentAlignment = Alignment.CenterStart) {
                    GSec("Left or removed", gone.size) {
                        Icon(GIcons.Chev, null, tint = c.ink2, modifier = Modifier.size(18.dp).graphicsLayer { rotationZ = rot })
                    }
                }
                GCollapse(goneOpen) {
                    Column(Modifier.padding(horizontal = 8.dp)) {
                        gone.forEach { s ->
                            key(s.id) {
                                ListRow(onClick = { onOpen(s.id) }) {
                                    GroupAvatar(0, 48.dp, dim = true, gid = s.id)
                                    Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                                        Text(s.name, Modifier.alpha(.72f), style = GType.bodyStrong, color = c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                        val at = s.last?.at?.toLong()
                                        Text((if (s.myRole == CoreRole.REMOVED) "Removed" else "You left") + (at?.let { " · " + dayOnly(it) } ?: ""),
                                            style = GType.sub, color = c.ink2, maxLines = 1)
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    if (creating) NewGroupSheet(app, onDismiss = { creating = false }) { id -> creating = false; onOpen(id) }
}

/** "**a** b **c**": the playground's `<b>` runs inside a line (650 there, 600 here). */
private fun bold(vararg parts: String) = androidx.compose.ui.text.buildAnnotatedString {
    parts.forEachIndexed { i, p ->
        if (i % 2 == 0) { pushStyle(androidx.compose.ui.text.SpanStyle(fontWeight = FontWeight.SemiBold)); append(p); pop() } else append(p)
    }
}

/** The tab's top rows (`.wrap > .row`): a 48 dp accent-soft disc with a 22 dp glyph, a title and a sub-line, 64 dp. */
@Composable
private fun TopRow(icon: androidx.compose.ui.graphics.vector.ImageVector, title: String, sub: String, onClick: () -> Unit) {
    val c = Tin.c
    Row(
        Modifier.padding(horizontal = 8.dp).fillMaxWidth().heightIn(min = 64.dp).clip(SmoothCornerShape(14.dp)).gTap(onClick = onClick).padding(8.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Box(Modifier.size(48.dp).clip(CircleShape).background(c.accentSoft), contentAlignment = Alignment.Center) {
            Icon(icon, null, tint = c.accentInk, modifier = Modifier.size(22.dp))
        }
        Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(title, style = GType.bodyStrong, color = c.ink)
            Text(sub, style = GType.sub, color = c.ink2)
        }
    }
}

/** A list row (`.row`): 72 dp, 8 dp padding, radius 14, gap 12; tappable when [onClick] is set. */
@Composable
private fun ListRow(onClick: (() -> Unit)?, content: @Composable RowScope.() -> Unit) {
    Row(
        Modifier.fillMaxWidth().heightIn(min = 72.dp).clip(SmoothCornerShape(14.dp)).let { if (onClick != null) it.gTap(onClick = onClick) else it }.padding(8.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp), content = content,
    )
}

/** An invitation or request card (`.card`): surface, hairline, radius 14, padding 14, 10 between parts, 6 above and below. */
@Composable
private fun ReqCard(content: @Composable ColumnScope.() -> Unit) {
    GCard(Modifier.fillMaxWidth().padding(vertical = 6.dp)) {
        Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(10.dp), content = content)
    }
}

/** One of my groups: avatar, name and time on top; preview (or the call line, or "Waiting for a host"), Join and the badge below. */
@Composable
private fun GroupRow(s: GroupSummary, me: String, now: Long, call: LiveCall?, onClick: () -> Unit) {
    val c = Tin.c
    val at = s.last?.at?.toLong() ?: 0L
    val waiting = !s.hostedHere && !s.hostReachable
    val hot = s.unread > 0u
    ListRow(onClick) {
        GroupAvatar(0, 48.dp, gid = s.id)
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(s.name, Modifier.weight(1f), style = GType.bodyStrong, color = c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(if (at > 0) listTime(at, now) else "", style = GType.meta.copy(fontWeight = if (hot) FontWeight.Bold else FontWeight.Normal),
                    color = if (hot) c.accentInk else c.ink2)
            }
            Row(Modifier.heightIn(min = 18.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Box(Modifier.weight(1f)) {
                    when {
                        call != null -> GCallLine(if (call.joined) "You’re in the call" else "Call in progress")
                        waiting -> Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(3.dp)) {
                            Icon(GIcons.Clock, null, tint = c.goldInk, modifier = Modifier.size(13.dp))
                            Text("Waiting for a host", style = GType.sub.copy(fontWeight = FontWeight.SemiBold), color = c.goldInk, maxLines = 1)
                        }
                        else -> Text(GroupText.preview(s, me).ifEmpty { "No posts yet" }, style = GType.sub, color = c.ink2, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                }
                if (call != null && !call.joined) GJoinChip()
                GBadge(s.unread.toInt())
            }
        }
    }
}

/** New group (the playground's dialog; a sheet on Android): a name, then Create. This device is the group's first host. */
@Composable
private fun NewGroupSheet(app: P2pApp, onDismiss: () -> Unit, onCreated: (String) -> Unit) {
    var name by rememberSaveable { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var err by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    GSheet(onDismiss, "New group") { close ->
        GDialogBody("New group") {
            Spacer(Modifier.height(10.dp))
            GField(name, { name = it.take(60); err = null }, "Group name", "Name", error = err)
            GPara("You’ll be the owner. This device is the group’s first host, so it stores the group and passes posts on.", top = 10.dp)
            GActs {
                GBtn("Cancel", close, GBtnKind.Quiet)
                GBtn(if (busy) "Creating…" else "Create", {
                    busy = true
                    scope.launch {
                        val r = withContext(Dispatchers.IO) { app.groups.create(name) }
                        busy = false
                        r.onSuccess(onCreated).onFailure { err = it.message }
                    }
                }, GBtnKind.Primary, enabled = name.isNotBlank() && !busy)
            }
        }
    }
}

/** `.fld`: a 13 sp label over a 44 dp field, radius 14, 1 px line; focus turns the line accent with a soft halo. */
@Composable
private fun GField(value: String, onChange: (String) -> Unit, label: String, placeholder: String, error: String? = null, minLines: Int = 1) {
    val c = Tin.c
    var focused by remember { mutableStateOf(false) }
    val shape = SmoothCornerShape(14.dp)
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Text(label, style = GType.sub.copy(fontWeight = FontWeight.SemiBold), color = c.ink2)
        androidx.compose.foundation.text.BasicTextField(
            value, onChange,
            Modifier.fillMaxWidth().onFocusChanged { focused = it.isFocused }.semantics { contentDescription = label },
            textStyle = GType.body.copy(color = c.ink, lineHeight = 21.sp), cursorBrush = androidx.compose.ui.graphics.SolidColor(c.accentInk),
            singleLine = minLines == 1, minLines = minLines,
            decorationBox = { inner ->
                Box(
                    Modifier.fillMaxWidth().heightIn(min = 44.dp)
                        .let { if (focused) it.border(3.dp, c.accentInk.copy(alpha = .14f), shape) else it }
                        .padding(if (focused) 0.dp else 0.dp)
                        .clip(shape).background(c.sf).border(1.dp, if (error != null) c.er else if (focused) c.accentFill else c.ln, shape)
                        .padding(horizontal = 14.dp, vertical = 10.dp),
                    contentAlignment = Alignment.CenterStart,
                ) {
                    if (value.isEmpty()) Text(placeholder, style = GType.body, color = c.ink2)
                    inner()
                }
            },
        )
        if (error != null) Text(error, style = GType.sub, color = c.er)
    }
}

private fun tintOf(@Suppress("UNUSED_PARAMETER") id: String) = 0

// ------------------------------------------------------------------ chat

@Composable
fun GroupChatLive(app: P2pApp, id: String, onBack: () -> Unit, onInfo: () -> Unit, onRemoval: () -> Unit, say: (String) -> Unit) {
    val c = Tin.c
    val open by app.groups.open.collectAsState()
    val blocks by app.blocks.collectAsState()
    val groupLog by app.groupLog.collectAsState()
    val exec = rememberRunner()
    LaunchedEffect(id) { withContext(Dispatchers.IO) { app.groups.open(id) } }
    DisposableEffect(id) { onDispose { app.groups.close(id) } }
    // In front of the user: its posts don't notify, and what was shown for it goes away.
    val owner = androidx.lifecycle.compose.LocalLifecycleOwner.current
    val ctx = androidx.compose.ui.platform.LocalContext.current
    DisposableEffect(owner, id) {
        val o = androidx.lifecycle.LifecycleEventObserver { _, ev ->
            if (ev == androidx.lifecycle.Lifecycle.Event.ON_RESUME) { OpenChat.group = id; GroupNotifier.cancel(ctx, id) }
            if (ev == androidx.lifecycle.Lifecycle.Event.ON_PAUSE && OpenChat.group == id) OpenChat.group = null
        }
        owner.lifecycle.addObserver(o)
        onDispose { owner.lifecycle.removeObserver(o); if (OpenChat.group == id) OpenChat.group = null }
    }
    val o = open?.takeIf { it.id == id }
    if (o == null) { Page { TopBar("Group", onBack) ; Hint("Loading…", Modifier.padding(24.dp)) }; return }
    val d = o.detail
    val me = app.groups.myDid
    var draft by rememberSaveable(id) { mutableStateOf("") }
    var replyTo by remember(id) { mutableStateOf<String?>(null) }
    var sending by remember { mutableStateOf(false) }
    // The online list (public group §8): open, and whether I hide myself (read before it opens, so its note never pops in).
    var onlineSheet by remember(id) { mutableStateOf<Boolean?>(null) }
    val scope = rememberCoroutineScope()
    val list = rememberLazyListState()
    // The thread: posts and this group's call log (MessageRow `.callrec`) merged by time, oldest first.
    val rows = remember(o.messages, groupLog, id) {
        val calls = groupLog.filter { it.groupId == id }.map { ThreadRow.Call(it) }
        (o.messages.mapIndexed { i, m -> ThreadRow.Post(i, m) } + calls).sortedBy { it.at }.let { all ->
            // Calls go before a post of the same second; posts keep their own order.
            all.sortedWith(compareBy<ThreadRow> { it.at }.thenBy { if (it is ThreadRow.Call) 0 else 1 }.thenBy { (it as? ThreadRow.Post)?.i ?: 0 })
        }
    }
    LaunchedEffect(rows.size) { if (rows.isNotEmpty()) list.scrollToItem(rows.size - 1 + if (o.older != null) 1 else 0) }
    val waiting = GroupText.waitingForHost(d)
    // Unread posts when the group opened: where "n new messages" goes (the count is cleared as it opens).
    val unreadAtOpen = remember(id) { app.groups.list.value.firstOrNull { it.id == id }?.unread?.toInt() ?: 0 }
    val dividerId = remember(id, o.messages.isNotEmpty()) {
        if (unreadAtOpen > 0) o.messages.filter { !it.outgoing && it.kind == GroupItemKind.POST }.takeLast(unreadAtOpen).firstOrNull()?.id else null
    }
    val nowMs = System.currentTimeMillis()
    // Public group §5: the picture button posts a picked image (the draft is its caption); the core scales it.
    val picker = androidx.activity.compose.rememberLauncherForActivityResult(androidx.activity.result.contract.ActivityResultContracts.PickVisualMedia()) { uri ->
        if (uri == null || sending) return@rememberLauncherForActivityResult
        val caption = draft
        draft = ""; replyTo = null; sending = true
        exec({
            withContext(Dispatchers.IO) {
                val bytes = runCatching {
                    ctx.contentResolver.openInputStream(uri)?.use { s ->
                        val buf = java.io.ByteArrayOutputStream()
                        val chunk = ByteArray(64 * 1024)
                        while (true) {
                            val n = s.read(chunk)
                            if (n < 0) break
                            buf.write(chunk, 0, n)
                            if (buf.size() > GroupText.MAX_PICTURE_BYTES) break
                        }
                        buf.toByteArray()
                    }
                }.getOrNull() ?: return@withContext Result.failure(GroupFailure("Couldn’t read that picture."))
                app.groups.postImage(id, bytes, caption).map { "" }
            }
        }) { res ->
            sending = false
            res.onFailure { if (draft.isEmpty()) draft = caption; say(it.message ?: "Couldn’t send") }
        }
    }
    Page {
        // Conversation.svelte `.bar` for a group: back · the head (avatar 36, name 17/22, status 13/18; tap = info) · call · info.
        Row(Modifier.fillMaxWidth().height(56.dp).background(c.sf).padding(horizontal = 6.dp, vertical = 4.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(2.dp)) {
            PgIconBtn(Pg.Back, "Back to groups", onBack)
            Row(Modifier.weight(1f).heightIn(min = 44.dp).clip(SmoothCornerShape(12.dp)).clickable(onClick = onInfo).padding(start = 6.dp, end = 10.dp, top = 4.dp, bottom = 4.dp)
                .semantics(mergeDescendants = true) { contentDescription = "${d.name}. ${GroupText.status(d)}. Group info" },
                verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                GroupAvatar(0, 36.dp, dim = !GroupText.active(d.myRole), gid = d.id)
                Column(Modifier.weight(1f)) {
                    Text(d.name, style = PgBodyText.copy(fontSize = 17.sp, lineHeight = 22.sp, fontWeight = FontWeight.SemiBold), color = c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Text(GroupText.status(d), style = PgBodyText.copy(fontSize = 13.sp, lineHeight = 18.sp), color = if (d.myRole == CoreRole.REMOVED) c.er else c.ink2, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
            // Public group §8: "12 online" in an open group opens the online list. The label keeps the width of "500+ online",
            // so a changing count moves nothing beside it.
            if (GroupText.showsOnline(d)) OnlinePill(o.online.size) {
                scope.launch { onlineSheet = !withContext(Dispatchers.IO) { app.groups.presenceVisible() } }
            }
            // The call button: start, join the running call, or return to mine (accent while one runs); dimmed while the
            // group has no call host and no call runs (a tap says why).
            if (GroupText.active(d.myRole)) {
                val gs by app.groupCallUi.state.collectAsState()
                val running = gs.live.containsKey(d.id) || (gs.gid == d.id && gs.active)
                val noHost = d.callHosts.isEmpty() && !running
                Box(Modifier.alpha(if (noHost) .45f else 1f).then(if (running) Modifier.clip(CircleShape).background(c.accentSoft) else Modifier)) {
                    PgIconBtn(Pg.Phone, if (gs.gid == d.id && gs.active) "Return to group call" else if (running) "Join group call" else "Start group call",
                        { if (noHost) say("No call host yet: an admin can appoint one in Group info") else app.groupCallUi.call(d.id, say) })
                }
            }
            PgIconBtn(Pg.Info, "Group info", onInfo)
        }
        Box(Modifier.fillMaxWidth().height(1.dp).background(c.hairline))
        if (GroupText.active(d.myRole)) OngoingCallBanner(app, d.id, say)
        HostAskCard(app, d, say)
        Box(Modifier.weight(1f).fillMaxWidth().background(c.canvas)) {
            LazyColumn(Modifier.fillMaxSize(), state = list, contentPadding = PaddingValues(start = 10.dp, end = 10.dp, top = 6.dp + if (waiting) 33.dp else 0.dp, bottom = 19.dp)) {
                if (o.older != null) item {
                    Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
                        TinButton("Show earlier messages", { exec({ app.groups.older(); Result.success("") }) {} }, style = BtnStyle.Text, fill = false, height = 40.dp)
                    }
                }
                if (rows.isEmpty()) item { Column(Modifier.fillMaxWidth()) { SysLine("No messages yet") } }
                itemsIndexed(rows, key = { _, r -> r.key }) { k, row ->
                    val prevAt = rows.getOrNull(k - 1)?.at
                    val newDay = prevAt == null || !sameDay(prevAt, row.at)
                    Column(Modifier.fillMaxWidth()) {
                        if (newDay) PgDayPill(dayLabel(row.at, nowMs), Modifier.align(Alignment.CenterHorizontally).padding(vertical = 10.dp))
                        when (row) {
                            is ThreadRow.Call -> GroupCallPill(row.r, me, GroupText.active(d.myRole)) { app.groupCallUi.call(d.id, say) }
                            is ThreadRow.Post -> {
                                if (row.m.id == dividerId) PgUnreadDivider(unreadAtOpen)
                                GroupItemLive(app, o.messages, row.i, row.m, me, d, blocks.blockedByMe(row.m.authorDid), onRemoval, onReply = { replyTo = row.m.id }, top = if (newDay) 0.dp else null, receipt = o.receipts[row.m.id], say = say)
                            }
                        }
                    }
                }
            }
            // The strip over the top of the list (gold): waiting for a host. It overlays, so nothing moves.
            if (waiting) PgStrip("Waiting for a host. No group host is online; your posts are saved and send when one is.", Pg.Clock, warn = true, modifier = Modifier.align(Alignment.TopCenter))
        }
        when (d.myRole) {
            CoreRole.REMOVED -> BlockedFooter("You were removed and can’t send messages to this group.") {
                TinButton("Why?", onRemoval, Modifier.weight(1f), style = BtnStyle.Outlined, height = 48.dp)
            }
            CoreRole.LEFT -> BlockedFooter("You left this group. Its messages stay here, read-only.")
            else -> if (GroupText.adminsOnlyLocked(d)) AdminsOnlyLine() else {
                val r = replyTo?.let { rid -> o.messages.firstOrNull { it.id == rid } }
                PgComposer(draft, { draft = it }, "Message", r?.let {
                    ComposerCtx("Replying to " + (if (it.outgoing) "yourself" else firstName(nameOf(app, it.authorDid, it.authorName))), GroupText.postLine(it), "Cancel reply")
                }, editing = false, onCancelCtx = { replyTo = null }, onSend = {
                    if (sending || draft.isBlank()) return@PgComposer
                    val text = draft; val reply = replyTo
                    draft = ""; replyTo = null; sending = true
                    exec({ app.groups.post(id, text, reply).map { "" } }) { res ->
                        sending = false
                        res.onFailure { if (draft.isEmpty()) draft = text; say(it.message ?: "Couldn’t send") }
                    }
                }, onAttach = {
                    if (!sending) try { picker.launch(androidx.activity.result.PickVisualMediaRequest(androidx.activity.result.contract.ActivityResultContracts.PickVisualMedia.ImageOnly)) }
                        catch (_: Exception) { say("No photo picker on this phone.") }
                })
            }
        }
    }
    val hidden = onlineSheet
    if (hidden != null && GroupText.showsOnline(d)) OnlineSheet(app, d, o.online, hidden) { onlineSheet = null }
}

/** The top bar's "12 online" (dot + count, 44 dp tall); its label keeps the width of "500+ online". Tap = who's online. */
@Composable
private fun OnlinePill(n: Int, onClick: () -> Unit) {
    val c = Tin.c
    val style = PgBodyText.copy(fontSize = 13.sp, lineHeight = 18.sp, fontWeight = FontWeight.SemiBold)
    Row(Modifier.heightIn(min = 44.dp).clip(RoundedCornerShape(22.dp)).gTap(label = "Who’s online", onClick = onClick).padding(horizontal = 10.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        Box(Modifier.size(8.dp).clip(CircleShape).background(if (n > 0) c.accent else c.ink2))
        Box {
            Text(GroupText.onlineLabel(GroupText.ONLINE_CAP), Modifier.alpha(0f).clearAndSetSemantics {}, style = style, maxLines = 1)
            Text(GroupText.onlineLabel(n), style = style, color = c.ink2, maxLines = 1)
        }
    }
}

/**
 * Who is online in the open group (desktop's online dialog as a sheet): admins first then by name, an Admin tag, the lag note,
 * the cap note at 500, and a line when I hide myself.
 */
@Composable
private fun OnlineSheet(app: P2pApp, d: GroupDetail, online: List<OnlineMember>, hidden: Boolean, onDismiss: () -> Unit) {
    val c = Tin.c
    val rows = remember(online) { GroupText.onlineRows(online) }
    GSheet(onDismiss, "Who’s online in ${d.name}") { close ->
        Column(Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, top = 20.dp, bottom = 16.dp)) {
            Text(d.name, style = GType.sub, color = c.ink2, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(GroupText.onlineLabel(rows.size), Modifier.padding(top = 2.dp).semantics { heading() }, style = GType.title, color = c.ink)
            GPara("Members a host has seen in the last minute. It can be a few seconds behind.")
            if (rows.size >= GroupText.ONLINE_CAP) GPara("A host lists up to ${GroupText.ONLINE_CAP} people at a time, so there may be more.")
            if (hidden) GPara("You’re not shown to others here. You can change that in Settings › Privacy.")
            if (rows.isEmpty()) GPara("No one is online right now. People show here while they’re connected.", top = 14.dp)
            else Column(Modifier.padding(top = 10.dp)) {
                rows.forEach { m ->
                    key(m.did) {
                        val n = nameOf(app, m.did, m.name)
                        Row(Modifier.fillMaxWidth().heightIn(min = 52.dp).semantics(mergeDescendants = true) {},
                            verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                            Avatar(n, m.did, 32.dp)
                            Text(n, Modifier.weight(1f), style = GType.bodyStrong, color = c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            if (m.isAdmin) GPill("Admin", GTone.Admin)
                        }
                    }
                }
            }
            GActs { GBtn("Close", close, GBtnKind.Quiet) }
        }
    }
}

/** An admins-only group (a channel), for a member: one calm line in the composer's place at its height (8 + 48 + 10), so nothing moves when an admin switches it. */
@Composable
private fun AdminsOnlyLine() {
    Box(Modifier.fillMaxWidth().padding(start = 10.dp, end = 10.dp, top = 8.dp, bottom = 10.dp).heightIn(min = 48.dp), contentAlignment = Alignment.Center) {
        Text(GroupText.ADMINS_ONLY, style = PgBodyText.copy(fontSize = 14.sp, lineHeight = 20.sp), color = Tin.c.ink2, textAlign = TextAlign.Center)
    }
}

private sealed interface ThreadRow {
    val at: Long
    val key: String
    data class Post(val i: Int, val m: GroupMessage) : ThreadRow { override val at get() = m.at.toLong(); override val key get() = m.id }
    data class Call(val r: uniffi.p2pcore.GroupCallRecord) : ThreadRow { override val at get() = r.startedAt.toLong() * 1000; override val key get() = "call:" + r.callId }
}

/**
 * MessageRow `.callrec` for a group call (32 tall pill, radius 16, surface, hairline): "Group call · 19 min · 4 people", or
 * "Missed group call · 5 min" (calm, never red). On my side when I started it; tap = start or join a call in this group.
 */
@Composable
private fun GroupCallPill(r: uniffi.p2pcore.GroupCallRecord, me: String, enabled: Boolean, onCall: () -> Unit) {
    val c = Tin.c
    val text = uniffi.p2pcore.groupCallLabel(r)
    val time = java.time.Instant.ofEpochSecond(r.startedAt.toLong()).atZone(java.time.ZoneId.systemDefault()).toLocalTime().let { "%02d:%02d".format(it.hour, it.minute) }
    Row(Modifier.fillMaxWidth().padding(top = 10.dp), horizontalArrangement = if (!r.missed && r.startedBy == me) Arrangement.End else Arrangement.Start) {
        Row(Modifier.heightIn(min = 32.dp).clip(RoundedCornerShape(16.dp)).background(c.sf).border(1.dp, c.ln, RoundedCornerShape(16.dp))
            .clickable(enabled = enabled, onClick = onCall).padding(horizontal = 12.dp)
            .semantics(mergeDescendants = true) { contentDescription = "$text, $time. Start or join a call" },
            verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            Icon(Pg.Phone, null, Modifier.size(16.dp), tint = c.ink2)
            Text(text, style = PgBodyText.copy(fontSize = 13.sp, lineHeight = 18.sp, fontWeight = FontWeight.SemiBold), color = c.ink, maxLines = 1)
            Text(time, style = PgBodyText.copy(fontSize = 11.sp, lineHeight = 14.sp, fontFeatureSettings = "tnum"), color = c.ink2)
        }
    }
}

/**
 * One group item (MessageRow.svelte): a system line, my removal card, a folded post from someone I blocked, or a post as a
 * bubble (sender's first name in their colour on the first of their run, the quote, the inline time and tick; "Waiting for a
 * host" under my post while no host has it). Long-press replies.
 */
@OptIn(androidx.compose.foundation.ExperimentalFoundationApi::class)
@Composable
private fun ColumnScope.GroupItemLive(
    app: P2pApp, all: List<GroupMessage>, i: Int, m: GroupMessage, me: String, d: GroupDetail, authorBlocked: Boolean,
    onRemoval: () -> Unit, onReply: () -> Unit, top: androidx.compose.ui.unit.Dp? = null,
    receipt: uniffi.p2pcore.GroupReceiptStatus? = null, say: (String) -> Unit = {},
) {
    val c = Tin.c
    if (m.kind == GroupItemKind.REMOVED && m.subjectDid == me) {
        val r = d.removal
        if (r == null) SysLine("You were removed from this group")
        else NoticeCard(c.erc, c.onErc, "Removed after ${r.flags.size} admin flags",
            "Flagged by ${r.flags.map { nameOf(app, it.byDid, it.byName) }.distinct().let { if (it.size > 1) it.dropLast(1).joinToString(", ") + " and " + it.last() else it.joinToString() }}. " +
                "You can still read what you received before; nothing new will arrive.") {
            Text("See details", Modifier.heightIn(min = 48.dp).clickable(role = Role.Button, onClick = onRemoval).wrapContentHeight(Alignment.CenterVertically),
                style = TinType.label.copy(fontWeight = FontWeight.Bold, textDecoration = androidx.compose.ui.text.style.TextDecoration.Underline), color = c.onErc)
        }
        return
    }
    if (m.kind != GroupItemKind.POST) {
        // .sys: centred 12/17 muted, 6 above and below.
        Text(GroupText.sysText(m, me), Modifier.fillMaxWidth().padding(vertical = 6.dp, horizontal = 24.dp), style = PgBodyText.copy(fontSize = 12.sp, lineHeight = 17.sp),
            color = c.ink2, textAlign = androidx.compose.ui.text.style.TextAlign.Center)
        return
    }
    val name = nameOf(app, m.authorDid, m.authorName)
    var shown by remember(m.id) { mutableStateOf(false) }
    if (authorBlocked && !m.outgoing && !shown) {
        // A personal block never changes the group; the message is folded, not dropped.
        Row(
            Modifier.padding(top = 10.dp).align(Alignment.Start).heightIn(min = 48.dp).clip(SmoothCornerShape(18.dp)).background(c.sf2)
                .clickable(role = Role.Button, onClickLabel = "Show message") { shown = true }.padding(horizontal = 12.dp),
            verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Icon(PgIcons.Off, null, tint = c.ink2, modifier = Modifier.size(18.dp))
            Text("Message from ${firstName(name)}, who you blocked · Show", style = TinType.bodyM, color = c.ink2)
        }
        return
    }
    // Runs: same author, same day, within 5 minutes (Conversation.svelte `join`).
    fun joined(a: GroupMessage?, b: GroupMessage?) = a != null && b != null && a.kind == GroupItemKind.POST && b.kind == GroupItemKind.POST &&
        a.outgoing == b.outgoing && (a.outgoing || a.authorDid == b.authorDid) && sameDay(a.at.toLong(), b.at.toLong()) && kotlin.math.abs(b.at.toLong() - a.at.toLong()) <= 5 * 60_000
    val p = joined(all.getOrNull(i - 1), m); val n = joined(m, all.getOrNull(i + 1))
    val pos = if (p && n) BubblePos.Mid else if (p) BubblePos.Last else if (n) BubblePos.First else BubblePos.Solo
    val qr = m.replyTo?.let { rid -> all.firstOrNull { it.id == rid } }
    val qWho = qr?.let { if (it.outgoing) "You" else firstName(nameOf(app, it.authorDid, it.authorName)) }
    val qColor = if (qr?.outgoing == true) c.accentInk else qr?.let { authorColor(it.authorDid) } ?: c.accentInk
    val quote: (@Composable () -> Unit)? = if (m.replyTo == null) null else ({ PgQuote(qWho, qColor, qr?.let { GroupText.postLine(it) } ?: "Original message unavailable", null) })
    // Waiting for a host = the slack string; sent = one tick; every eligible member signed delivered = two ticks; every one
    // read = the read ink (core's GroupReceiptStatus). Host storage is never members' delivery/read.
    // An open group sends no member receipts (public group §4): "Delivered 0/N" would mislead, so no n/N there.
    val lines = receipt?.takeIf { it.eligible > 0u && !d.open }?.let { groupReceiptLines(it) }
    val tick = if (!m.outgoing) null else if (m.delivery == CoreDelivery.WAITING) MetaReceipt(TickVis.SLACK, m.id, "Waiting for a host")
        else {
            val vis = receipt?.let { tickVisOf(it.tick, false) } ?: TickVis.ONE
            MetaReceipt(vis, m.id, lines?.take(2)?.joinToString(", ") ?: "Sent")
        }
    // No details view for group posts yet: a tap on my post shows the n/N lines and who under it (asked for, so it may move).
    var info by remember(m.id) { mutableStateOf(false) }
    val meta = BubbleMeta(msClock(m.at.toLong()), receipt = tick)
    Column(Modifier.padding(top = top ?: gapAbove(pos)).fillMaxWidth(), horizontalAlignment = if (m.outgoing) Alignment.End else Alignment.Start) {
        m.image?.let { img ->
            if (!m.outgoing && !p) Text(firstName(name) + if (m.beforeRemoval) " · removed" else "", Modifier.padding(start = 4.dp, bottom = 2.dp),
                style = PgBodyText.copy(fontSize = 13.sp, lineHeight = 18.sp, fontWeight = FontWeight.Bold), color = if (m.beforeRemoval) c.ink2 else authorColor(m.authorDid))
            GroupPhotoBox(app, d.id, m, img, if (m.text.isEmpty()) pos else BubblePos.First, m.outgoing, say, onReply)
            if (m.text.isEmpty()) Text(msClock(m.at.toLong()), Modifier.padding(top = 3.dp, start = 4.dp, end = 4.dp), style = PgBodyText.copy(fontSize = 11.sp, lineHeight = 14.sp), color = c.ink2)
        }
        if (m.image == null || m.text.isNotEmpty()) Box(Modifier.fillMaxWidth(.78f), contentAlignment = if (m.outgoing) Alignment.CenterEnd else Alignment.CenterStart) {
            PgTextBubble(androidx.compose.ui.text.AnnotatedString(m.text), pos, m.outgoing, meta,
                Modifier.combinedClickable(enabled = GroupText.active(d.myRole) || lines != null, onLongClickLabel = "Reply", onLongClick = onReply,
                    onClickLabel = if (lines != null) "Who has it" else null, onClick = { if (lines != null) info = !info }),
                sender = if (m.outgoing || p || m.image != null) null else firstName(name) + if (m.beforeRemoval) " · removed" else "",
                senderColor = if (m.beforeRemoval) c.ink2 else authorColor(m.authorDid), quote = quote)
        }
        if (m.outgoing && m.delivery == CoreDelivery.WAITING) Row(Modifier.padding(top = 3.dp, end = 2.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            Icon(Pg.Clock, null, Modifier.size(12.dp), tint = c.goldInk)
            Text("Waiting for a host", style = PgBodyText.copy(fontSize = 12.sp, lineHeight = 16.sp, fontWeight = FontWeight.SemiBold), color = c.goldInk)
        }
        if (m.beforeRemoval) Text("Written before ${firstName(name)} was removed", Modifier.padding(top = 3.dp), style = PgBodyText.copy(fontSize = 11.sp), color = c.ink2)
        if (info && lines != null) Text(lines.joinToString("\n"), Modifier.padding(top = 3.dp, end = 2.dp), style = PgBodyText.copy(fontSize = 12.sp, lineHeight = 16.sp), color = c.ink2,
            textAlign = androidx.compose.ui.text.style.TextAlign.End)
    }
}

/**
 * A post's picture (public group §5), drawn like a 1:1 photo (PhotoBox): its box at the picture's shape from the start,
 * the core's colour preview until it is here, then the picture. Not downloaded (the auto-download setting said wait): its
 * size and "Load photo"; on its way: a ring; failed: retry. A long press replies, as on the text bubble.
 */
@OptIn(androidx.compose.foundation.ExperimentalFoundationApi::class)
@Composable
private fun GroupPhotoBox(app: P2pApp, gid: String, m: GroupMessage, img: uniffi.p2pcore.GroupImage, pos: BubblePos, out: Boolean, say: (String) -> Unit, onLong: () -> Unit) {
    val c = Tin.c
    val scope = rememberCoroutineScope()
    val ready = img.state == uniffi.p2pcore.GroupImageState.READY
    val bmp by produceState<androidx.compose.ui.graphics.ImageBitmap?>(null, m.id, ready) {
        value = if (!ready) null else withContext(Dispatchers.IO) {
            app.groups.imagePath(gid, m.id)?.let { runCatching { ChatMedia.bitmap(java.io.File(it), 720) }.getOrNull() }
        }
    }
    val pv = remember(m.id) { previewBitmap(img.preview) }
    val (w, h) = photoBox(img.width, img.height)
    fun load() = scope.launch { withContext(Dispatchers.IO) { app.groups.fetchImage(gid, m.id) }.onFailure { say(it.message ?: "Couldn’t load the picture") } }
    Box(Modifier.size(w, h).clip(bubbleShape(pos, out, 3.dp)).background(c.sf3)
        .combinedClickable(role = Role.Button, onLongClickLabel = "Reply", onLongClick = onLong, onClickLabel = if (ready) null else "Load photo", onClick = { if (!ready) load() })
        .semantics { contentDescription = if (m.text.isEmpty()) "Photo" else "Photo: ${m.text}" },
        contentAlignment = Alignment.Center) {
        pv?.let {
            Image(it, null, Modifier.fillMaxSize().graphicsLayer { scaleX = 1.24f; scaleY = 1.24f }.blur(14.dp),
                contentScale = androidx.compose.ui.layout.ContentScale.FillBounds, filterQuality = androidx.compose.ui.graphics.FilterQuality.High)
        }
        bmp?.let { Image(it, null, Modifier.fillMaxSize(), contentScale = androidx.compose.ui.layout.ContentScale.Crop) }
        when (img.state) {
            uniffi.p2pcore.GroupImageState.REMOTE -> Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)) {
                PhotoChip("Photo · ${sizeText(img.size.toLong())}")
                Row(Modifier.clip(RoundedCornerShape(18.dp)).background(c.accent).clickable(role = Role.Button) { load() }.padding(horizontal = 16.dp, vertical = 7.dp),
                    verticalAlignment = Alignment.CenterVertically) {
                    Text("Load photo", style = TinType.caption.copy(fontSize = 13.sp, fontWeight = FontWeight.SemiBold), color = c.onAccent)
                }
            }
            uniffi.p2pcore.GroupImageState.LOADING -> Ring {
                androidx.compose.material3.CircularProgressIndicator(Modifier.size(40.dp), color = Color.White, trackColor = Color.White.copy(alpha = .28f), strokeWidth = 3.dp)
            }
            uniffi.p2pcore.GroupImageState.FAILED -> Ring { Icon(PgIcons.Retry, "Try loading again", tint = Color.White) }
            uniffi.p2pcore.GroupImageState.READY -> if (bmp == null && pv == null) Icon(PgIcons.Image, null, tint = c.ink2)
        }
    }
}

/** "Delivered n/N", "Read n/N", then who (eligible members by level; kept statements of people no longer counted). Same words as desktop. */
private fun groupReceiptLines(r: uniffi.p2pcore.GroupReceiptStatus): List<String> {
    fun who(m: uniffi.p2pcore.GroupMemberReceipts) = if (m.name.isEmpty()) "Someone" else firstName(m.name)
    val out = mutableListOf("Delivered ${r.delivered}/${r.eligible}", "Read ${r.read}/${r.eligible}")
    for ((label, want) in listOf("Read" to uniffi.p2pcore.ReceiptLevel.READ, "On their device" to uniffi.p2pcore.ReceiptLevel.DELIVERED, "Not yet" to uniffi.p2pcore.ReceiptLevel.UNCONFIRMED)) {
        val n = r.members.filter { it.eligible && it.receipts.level == want }.map { who(it) }
        if (n.isNotEmpty()) out += "$label: ${n.joinToString(", ")}"
    }
    val gone = r.members.filter { !it.eligible }.map { who(it) }
    if (gone.isNotEmpty()) out += "Also confirmed: ${gone.joinToString(", ")}"
    return out
}

// ------------------------------------------------------------------ group info (playground screens/groups/GroupInfo.svelte)

private sealed interface InfoSheet {
    data class Member(val did: String) : InfoSheet
    data object Invite : InfoSheet
    data object Leave : InfoSheet
    /** Open the group to anyone with the link (confirm first: receipts go off, an online list shows). */
    data object OpenGroup : InfoSheet
}

/** A public-group setting whose change is on its way to the core (its switch shows the asked-for value meanwhile). */
private enum class PubSetting { Open, AdminsOnly, Link }

/** "n members" / "1 member". */
private fun plural(n: Int, one: String, many: String = one + "s") = "$n ${if (n == 1) one else many}"

/**
 * Group info: header, flags on me, invites (admins), hosts, call hosts, members, leave. What I can do depends on my role; the
 * owner can't leave. A removed group shows the removal view instead.
 */
@Composable
fun GroupInfoLive(app: P2pApp, id: String, onBack: () -> Unit, onLink: () -> Unit, onAddMembers: () -> Unit, onRemoval: () -> Unit, say: (String) -> Unit, onChat: (String) -> Unit = {}) {
    val c = Tin.c
    val open by app.groups.open.collectAsState()
    val sent by app.groups.sent.collectAsState()
    val outgoing by app.groups.outgoing.collectAsState()
    val blocks by app.blocks.collectAsState()
    val exec = rememberRunner()
    LaunchedEffect(id) { withContext(Dispatchers.IO) { if (app.groups.open.value?.id != id) app.groups.open(id) } }
    val d = open?.takeIf { it.id == id }?.detail ?: run { InfoPage("Group info", onBack) {}; return }
    var sheet by remember { mutableStateOf<InfoSheet?>(null) }
    var pending by remember(id) { mutableStateOf<Pair<PubSetting, Boolean>?>(null) }
    val admin = GroupText.isAdmin(d.myRole)
    val done: (Result<String>) -> Unit = { r -> r.onSuccess { if (it.isNotEmpty()) say(it) }.onFailure { say(it.message ?: "Couldn’t do that") } }
    val name: (String, String) -> String = { did, fb -> nameOf(app, did, fb).let { if (it == "You") app.node.profile()?.name?.ifBlank { null } ?: "You" else it } }
    InfoPage(if (d.myRole == CoreRole.REMOVED) "Removed" else "Group info", onBack) {
        item(key = "hero") {
            Column(Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, top = 16.dp, bottom = 8.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                GroupAvatar(0, 72.dp, dim = !GroupText.active(d.myRole), gid = d.id)
                Text(d.name, Modifier.padding(top = 12.dp).semantics { heading() }, style = GType.hero, color = c.ink, textAlign = TextAlign.Center)
                Text(
                    when (d.myRole) {
                        CoreRole.OWNER -> "${plural(d.members.size, "member")} · You are the owner"
                        CoreRole.ADMIN -> "${plural(d.members.size, "member")} · You are an admin"
                        CoreRole.MEMBER -> "${plural(d.members.size, "member")} · You are a member"
                        CoreRole.LEFT -> "You left this group"
                        CoreRole.REMOVED -> "You’re no longer a member"
                    },
                    Modifier.padding(top = 2.dp), style = GType.note, color = c.ink2, textAlign = TextAlign.Center,
                )
            }
        }
        when (d.myRole) {
            CoreRole.REMOVED -> item(key = "removal") { RemovalBody(app, d.removal, onBack) }
            CoreRole.LEFT -> item(key = "left") {
                GNote("You can still read the posts on this device. To come back, someone in the group has to invite you again.", Modifier.padding(horizontal = 20.dp, vertical = 24.dp))
            }
            else -> {
                val mine = d.members.firstOrNull { it.isMe }
                if (mine != null && mine.flags > 0u && !mine.isOwner) item(key = "flags") { FlagsOnMe(mine.flags.toInt()) }
                if (admin) item(key = "invites") {
                    val shownSent = sent.filter { it.groupId == id && d.members.none { m -> m.did == it.inviteeDid } }
                    val noHost = d.hosts.isEmpty() && !d.amHost
                    InfoSec("Invites") {
                        GCard(Modifier.fillMaxWidth()) {
                            ActRow(GIcons.UserPlus, "Invite contacts", "A private invite to each person", trailing = GIcons.ChevR) { sheet = InfoSheet.Invite }
                            GRule()
                            ActRow(GIcons.Link, "Create invite link", if (noHost) "Appoint a host first. A link names one host." else "Valid for 7 days",
                                trailing = GIcons.Plus, enabled = !noHost, onClick = onLink)
                        }
                        if (shownSent.isNotEmpty()) Column(Modifier.padding(top = 6.dp)) {
                            shownSent.forEach { inv ->
                                key(inv.id) {
                                    val n = name(inv.inviteeDid, inv.inviteeName)
                                    val (line, bad) = GroupText.sentInvitationLine(inv)
                                    MRow {
                                        Avatar(n, inv.inviteeDid, 40.dp)
                                        TwoLine(n, line, subColor = if (bad) c.er else c.ink2)
                                    }
                                }
                            }
                        }
                    }
                }
                // Public group (§8): the open link, and who may post.
                if (admin) item(key = "public") {
                    PublicGroupSection(app, d, pending, { pending = it }, exec, done, say) { sheet = InfoSheet.OpenGroup }
                }
                item(key = "hosts") {
                    HostsSection(app, d, admin, name, exec, done)
                }
                item(key = "callhosts") {
                    CallHostsSection(app, d, admin, name, exec, done)
                }
                item(key = "members") {
                    val order = { m: CoreMember -> if (m.isOwner) 0 else if (m.isAdmin) 1 else 2 }
                    val sorted = d.members.sortedWith(compareBy<CoreMember>({ order(it) }, { !it.isMe }))
                    InfoSec("Members", count = d.members.size) {
                        sorted.forEach { m ->
                            key(m.did) {
                                val n = name(m.did, m.name)
                                val req = outgoing.filter { it.peerDid == m.did }.maxByOrNull { it.at }
                                val reqNote = when (req?.state) {
                                    ChatRequestState.PENDING -> "Chat request sent"
                                    ChatRequestState.DECLINED -> "Declined your chat request"
                                    ChatRequestState.BLOCKED -> "Blocked"
                                    else -> null
                                }
                                val since = m.invitedBy?.let { "Invited by ${firstName(name(it, ""))} · ${dayOnly(m.joinedAt.toLong())}" } ?: "Since ${dayOnly(m.joinedAt.toLong())}"
                                val showFlags = (admin || m.isMe) && m.flags > 0u && !m.isOwner
                                val desc = "$n${if (m.isMe) " (you)" else ""}, ${if (m.isOwner) "owner" else if (m.isAdmin) "admin" else "member"}" +
                                    (if (m.isHost) ", host" else "") + (if (d.callHosts.any { it.did == m.did }) ", call host" else "") + ". Open actions"
                                MRow(onClick = { sheet = InfoSheet.Member(m.did) }, desc = desc) {
                                    Avatar(n, m.did, 40.dp)
                                    Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(1.dp)) {
                                        Text(if (m.isMe) "$n (you)" else n, style = GType.bodyStrong, color = c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                        Text(androidx.compose.ui.text.buildAnnotatedString {
                                            append(since)
                                            if (showFlags) { pushStyle(androidx.compose.ui.text.SpanStyle(color = c.er, fontWeight = FontWeight.SemiBold)); append(" · ${m.flags} of ${GroupText.FLAGS_TO_REMOVE} flags"); pop() }
                                            if (blocks.blockedByMe(m.did)) append(" · Blocked by you")
                                            else if (reqNote != null) append(" · $reqNote")
                                        }, style = GType.sub, color = c.ink2, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                    }
                                    Row(horizontalArrangement = Arrangement.spacedBy(4.dp), verticalAlignment = Alignment.CenterVertically) {
                                        if (m.isOwner) GPill("Owner", GTone.Owner) else if (m.isAdmin) GPill("Admin", GTone.Admin)
                                        if (m.isHost) GMark(GIcons.Server, "Host")
                                        if (d.callHosts.any { it.did == m.did }) GMark(GIcons.Phone, "Call host", call = true)
                                    }
                                }
                            }
                        }
                    }
                }
                item(key = "leave") {
                    Column(Modifier.padding(start = 16.dp, end = 16.dp, top = 24.dp)) {
                        GCard(Modifier.fillMaxWidth()) {
                            if (d.myRole != CoreRole.OWNER) ActRow(GIcons.Leave, "Leave group", danger = true) { sheet = InfoSheet.Leave }
                            else ActRow(GIcons.Leave, "Leave group", "The owner can’t leave yet", enabled = false, off = true) {}
                        }
                    }
                }
            }
        }
    }
    when (val k = sheet) {
        null -> {}
        is InfoSheet.Member -> d.members.firstOrNull { it.did == k.did }?.let { m ->
            MemberSheet(app, d, m, name(m.did, m.name), outgoing.filter { it.peerDid == m.did }.maxByOrNull { it.at }, blocks.blockedByMe(m.did),
                exec, done, onChat, onDismiss = { sheet = null })
        } ?: run { sheet = null }
        InfoSheet.Invite -> InviteSheet(app, d, sent, exec, say, onDismiss = { sheet = null })
        InfoSheet.Leave -> GSheet({ sheet = null }, "Leave group") { close ->
            GDialogBody("Leave ${d.name}?") {
                GPara("You’ll stop getting posts. What’s already on this device stays, and you can read it. To come back, someone in the group has to invite you again.", top = 4.dp)
                if (d.myRole == CoreRole.ADMIN) GPara("You’re an admin. The owner and the other admins stay.")
                GActs {
                    GBtn("Cancel", close, GBtnKind.Quiet)
                    GBtn("Leave", { exec({ app.groups.leave(id) }) { r -> done(r); r.onSuccess { sheet = null; onBack() } } }, GBtnKind.Danger)
                }
            }
        }
        InfoSheet.OpenGroup -> GSheet({ sheet = null }, "Open the group") { close ->
            val busy = pending?.first == PubSetting.Open
            GDialogBody("Open ${d.name} to anyone with the link?") {
                GPara("Anyone holding the link can join, with no invite and no expiry. Share it where you’d like people to find the group.", top = 4.dp)
                GPara("While it’s open, no read receipts are sent in this group, and members see who is online. Each person can hide themselves in their settings.")
                GPara("You can close the link any time; everyone already in stays.")
                GActs {
                    GBtn("Cancel", close, GBtnKind.Quiet)
                    GBtn(if (busy) "Opening…" else "Open the group", {
                        pending = PubSetting.Open to true
                        exec({ app.groups.openLink(id).map { "Your group is open. Copy the link to share it." } }) { r -> pending = null; done(r); if (r.isSuccess) close() }
                    }, GBtnKind.Primary, enabled = !busy)
                }
            }
        }
    }
}

/** Info pages: the 60 dp surface bar with a hairline under it, then a scrolling column capped at 640 dp. */
@Composable
private fun InfoPage(title: String, onBack: () -> Unit, content: androidx.compose.foundation.lazy.LazyListScope.() -> Unit) {
    val c = Tin.c
    Column(Modifier.fillMaxSize().background(c.bg).systemBarsPadding()) {
        Row(Modifier.fillMaxWidth().background(c.sf).heightIn(min = 60.dp).padding(start = 8.dp, end = 12.dp, top = 8.dp, bottom = 8.dp),
            verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            Box(Modifier.size(44.dp).clip(CircleShape).gTap(label = null, onClick = onBack).semantics { contentDescription = "Back" }, contentAlignment = Alignment.Center) {
                Icon(GIcons.Back, null, tint = c.ink, modifier = Modifier.size(22.dp))
            }
            Text(title, style = GType.title, color = c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        GRule()
        LazyColumn(Modifier.weight(1f).fillMaxWidth().widthIn(max = 640.dp), contentPadding = PaddingValues(bottom = 32.dp), content = content)
    }
}

/** A section of the info page (`.sec`): 16 dp sides, 24 above, the head inset 4 more with 8 below. */
@Composable
private fun InfoSec(title: String, count: Int? = null, countText: String? = null, countColor: Color? = null, content: @Composable ColumnScope.() -> Unit) {
    Column(Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 24.dp)) {
        GSec(title, count, countText = countText, countColor = countColor, padding = PaddingValues(start = 4.dp, end = 4.dp, bottom = 8.dp))
        content()
    }
}

/** A row in a card (`.act`): 20 dp glyph, title and sub, an optional trailing glyph; 60 dp, 16 dp sides, 14 gap. */
@Composable
private fun ActRow(icon: androidx.compose.ui.graphics.vector.ImageVector, title: String, sub: String? = null, trailing: androidx.compose.ui.graphics.vector.ImageVector? = null,
                   enabled: Boolean = true, danger: Boolean = false, off: Boolean = false, onClick: () -> Unit) {
    val c = Tin.c
    val tint = if (danger) c.er else c.ink2
    Row(
        Modifier.fillMaxWidth().heightIn(min = 60.dp).let { if (!off) it.gTap(enabled = enabled, onClick = onClick) else it }.alpha(if (!enabled && !off) .55f else 1f)
            .padding(horizontal = 16.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        Icon(icon, null, tint = tint, modifier = Modifier.size(20.dp))
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(1.dp)) {
            Text(title, style = GType.bodyStrong, color = if (danger) c.er else if (off) c.ink2 else c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (sub != null) Text(sub, style = GType.sub, color = c.ink2, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        if (trailing != null) Icon(trailing, null, tint = c.ink2, modifier = Modifier.size(18.dp))
    }
}

/** A member / host row (`.mrow`): 60 dp, 12 gap; tappable rows get the wash with 8 dp sides. */
@Composable
private fun MRow(onClick: (() -> Unit)? = null, desc: String? = null, content: @Composable RowScope.() -> Unit) {
    Row(
        Modifier.fillMaxWidth().heightIn(min = 60.dp)
            .let { if (onClick != null) it.padding(horizontal = 0.dp).clip(SmoothCornerShape(14.dp)).gTap(onClick = onClick).let { m -> if (desc != null) m.semantics(mergeDescendants = true) { contentDescription = desc } else m } else it }
            .padding(horizontal = 4.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp), content = content,
    )
}

@Composable
private fun RowScope.TwoLine(title: String, sub: String, subColor: Color = Tin.c.ink2, dot: Boolean? = null) {
    val c = Tin.c
    Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(1.dp)) {
        Text(title, style = GType.bodyStrong, color = c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
        Row(verticalAlignment = Alignment.CenterVertically) {
            if (dot != null) { GDot(dot); Spacer(Modifier.width(6.dp)) }
            Text(sub, style = GType.sub, color = subColor, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
    }
}

/** A status line whose three wordings share one cell (`.status` + `.stk`), so it changing never moves what is below. */
@Composable
private fun StatusLine(lines: List<String>, current: Int, on: Boolean) {
    val c = Tin.c
    val col by androidx.compose.animation.animateColorAsState(if (on) c.ink else c.onWarn, tween(GMotion.ENTER, easing = GMotion.EaseOut), label = "st")
    Row(Modifier.padding(start = 4.dp, end = 4.dp, bottom = 6.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Box(Modifier.padding(top = 6.dp)) { GDot(on) }
        GStack(lines, current, GType.note, col, Modifier.weight(1f))
    }
}

/**
 * Public group (owner and admins; desktop's Group info › Public group): the Open link switch (turning it on asks first), Copy
 * link (the link this phone holds, else fetched from the hosts), the Admins only switch, and what it means. A switch shows the
 * asked-for value while its call runs; results and errors are said like the other actions here.
 */
@Composable
private fun PublicGroupSection(app: P2pApp, d: GroupDetail, pending: Pair<PubSetting, Boolean>?, onPending: (Pair<PubSetting, Boolean>?) -> Unit,
                               exec: (suspend () -> Result<String>, (Result<String>) -> Unit) -> Unit, done: (Result<String>) -> Unit,
                               say: (String) -> Unit, onAskOpen: () -> Unit) {
    val ctx = LocalContext.current
    val held by app.groups.openLink.collectAsState()
    val openOn = pending?.takeIf { it.first == PubSetting.Open }?.second ?: d.open
    val aoOn = pending?.takeIf { it.first == PubSetting.AdminsOnly }?.second ?: d.adminsOnly
    val fetching = pending?.first == PubSetting.Link
    val link = held?.takeIf { it.first == d.id && d.open }?.second
    val copy: (String) -> Unit = { copyToClipboard(ctx, "Group link", it); say(GroupText.OPEN_LINK_COPIED) }
    InfoSec("Public group") {
        GCard(Modifier.fillMaxWidth()) {
            SwitchRow(GIcons.Link, "Open link", "Anyone with the link can join", "Only people you invite can join", openOn) { want ->
                if (pending == null) {
                    if (want) onAskOpen()
                    else { onPending(PubSetting.Open to false); exec({ app.groups.closeLink(d.id) }) { r -> onPending(null); done(r) } }
                }
            }
            GRule()
            ActRow(PgIcons.Copy, "Copy link", when {
                fetching -> "Getting the link…"
                link != null -> link
                openOn -> "A link that doesn’t expire"
                else -> "Turn on Open link to share it"
            }, enabled = openOn && !fetching) {
                if (link != null) copy(link)
                else if (pending == null && d.open) {
                    onPending(PubSetting.Link to true)
                    exec({ app.groups.openLink(d.id) }) { r -> onPending(null); r.onSuccess(copy).onFailure { say(it.message ?: "Couldn’t get the link") } }
                }
            }
            GRule()
            SwitchRow(GIcons.Shield, "Admins only", "Only admins post; everyone reads", "Everyone in the group can post", aoOn) { want ->
                if (pending == null) { onPending(PubSetting.AdminsOnly to want); exec({ app.groups.setAdminsOnly(d.id, want) }) { r -> onPending(null); done(r) } }
            }
        }
        GNote("The open link doesn’t expire. Closing it stops new joins; everyone already in stays. While the group is open, no read receipts are sent in it and members see who is online.",
            Modifier.padding(start = 4.dp, end = 4.dp, top = 10.dp))
    }
}

/** A card row with a switch (`.act` + Switch): the row toggles too; its sub-line keeps the height of its longer wording, so flipping moves nothing. */
@Composable
private fun SwitchRow(icon: androidx.compose.ui.graphics.vector.ImageVector, title: String, subOn: String, subOff: String, on: Boolean, onToggle: (Boolean) -> Unit) {
    val c = Tin.c
    Row(
        Modifier.fillMaxWidth().heightIn(min = 60.dp).gTap(role = Role.Switch) { onToggle(!on) }.padding(start = 16.dp, end = 8.dp, top = 8.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        Icon(icon, null, tint = c.ink2, modifier = Modifier.size(20.dp))
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(1.dp)) {
            Text(title, style = GType.bodyStrong, color = c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Box {
                Text(if (on) subOff else subOn, Modifier.alpha(0f).clearAndSetSemantics {}, style = GType.sub)
                Text(if (on) subOn else subOff, style = GType.sub, color = c.ink2)
            }
        }
        PgSwitch(on, onToggle, title)
    }
}

/** Hosts: the status line, the hosts with pending asks and offers, then what a host is. */
@Composable
private fun HostsSection(app: P2pApp, d: GroupDetail, admin: Boolean, name: (String, String) -> String,
                         exec: (suspend () -> Result<String>, (Result<String>) -> Unit) -> Unit, done: (Result<String>) -> Unit) {
    val noHost = d.hosts.isEmpty()
    val up = d.amHost || d.hosts.any { it.online }
    val online = d.hosts.count { it.online || it.isMe }
    val line = if (noHost) 0 else if (up) 1 else 2
    val dev = { h: uniffi.p2pcore.GroupHost -> if (h.isMe) "This device" else name(h.did, h.name) + if (d.members.firstOrNull { it.did == h.did }?.deviceIds?.size ?: 1 > 1) " · ${GroupText.shortDev(h.deviceId)}" else "" }
    InfoSec("Hosts") {
        StatusLine(listOf(
            "This group has no host. Posts wait until a host is appointed.",
            "${plural(online.coerceAtLeast(1), "host")} online. Posts go through right away.",
            "No host is online. Your posts are saved and send when one is.",
        ), line, line == 1)
        d.hosts.forEach { h ->
            key("h" + h.did + h.deviceId) {
                MRow {
                    Avatar(name(h.did, h.name), h.did, 40.dp)
                    val on = h.isMe || h.online
                    TwoLine(dev(h), (if (on) "Online" else "Offline") + if (d.callHosts.any { it.did == h.did && it.deviceId == h.deviceId }) " · also hosts calls" else "", dot = on)
                    if (admin) TrailBtn("Remove", "Remove ${dev(h)} as host") { exec({ app.groups.revoke(d.id, h.did, h.deviceId) }, done) }
                }
            }
        }
        d.hostRequests.forEach { h ->
            key("q" + h.did + h.deviceId) {
                MRow {
                    Avatar(name(h.did, h.name), h.did, 40.dp)
                    TwoLine(dev(h), "Waiting to accept")
                    if (h.isMe && d.hostRequestedHere) {
                        GBtn("Decline", { exec({ app.groups.acceptHost(d.id, false) }, done) }, GBtnKind.Quiet)
                        GBtn("Accept", { exec({ app.groups.acceptHost(d.id, true) }, done) }, GBtnKind.Primary)
                    } else if (admin) TrailBtn("Cancel", "Cancel host request to ${dev(h)}") { exec({ app.groups.revoke(d.id, h.did, h.deviceId) }, done) }
                }
            }
        }
        d.hostOffers.forEach { h ->
            key("o" + h.did + h.deviceId) {
                MRow {
                    Avatar(name(h.did, h.name), h.did, 40.dp)
                    TwoLine(dev(h), "Offered to host · needs an admin")
                    if (admin) GBtn("Appoint", { exec({ app.groups.appoint(d.id, h.did, h.deviceId) }, done) }, GBtnKind.Primary)
                }
            }
        }
        GNote("Hosts store the group and pass posts on when others are offline. They are members you trust: a host can read everything posted in this group." +
            if (admin) " Appoint a host from a member’s menu; they have to accept. Removing a host changes the group’s address within 2 days, so that device can no longer reach it." else "",
            Modifier.padding(start = 4.dp, end = 4.dp, top = 10.dp))
    }
}

/** Call hosts: a separate permit from chat hosting (the core's call_hosts / call_host_requests). */
@Composable
private fun CallHostsSection(app: P2pApp, d: GroupDetail, admin: Boolean, name: (String, String) -> String,
                             exec: (suspend () -> Result<String>, (Result<String>) -> Unit) -> Unit, done: (Result<String>) -> Unit) {
    val ready = d.callHosts.count { it.online || it.isMe }
    // This phone is said in words, not listed as a row: "Stop hosting calls on this phone" below stands for it.
    val onlyMe = d.amCallHost && d.callHosts.size == 1
    val line = if (d.callHosts.isEmpty()) 0 else if (onlyMe) 3 else if (ready > 0) 1 else 2
    val dev = { h: uniffi.p2pcore.GroupHost -> if (h.isMe) "This device" else name(h.did, h.name) }
    InfoSec("Call hosts") {
        StatusLine(listOf(
            "This group has no call host. Nobody can start a call until an admin appoints one.",
            "${plural(ready.coerceAtLeast(1), "call host")} ready. Calls start through ${if (ready == 1) "it" else "them"}.",
            "No call host is available right now. A call can’t start until one is back.",
            "This phone carries the calls you start.",
        ), line, line == 1 || line == 3)
        d.callHosts.filter { !it.isMe }.forEach { h ->
            key("c" + h.did + h.deviceId) {
                MRow {
                    Avatar(name(h.did, h.name), h.did, 40.dp)
                    val on = h.isMe || h.online
                    TwoLine(dev(h), if (on) "Online" else "Offline", dot = on)
                    if (admin) TrailBtn("Remove", "Remove ${dev(h)} as call host") { exec({ app.groups.revokeCall(d.id, h.did, h.deviceId) }, done) }
                }
            }
        }
        d.callHostRequests.forEach { h ->
            key("cq" + h.did + h.deviceId) {
                MRow {
                    Avatar(name(h.did, h.name), h.did, 40.dp)
                    TwoLine(dev(h), "Waiting to accept")
                    if (h.isMe && d.callHostRequestedHere) {
                        GBtn("Decline", { exec({ app.groups.acceptCallHost(d.id, false) }, done) }, GBtnKind.Quiet)
                        GBtn("Accept", { exec({ app.groups.acceptCallHost(d.id, true) }, done) }, GBtnKind.Primary)
                    } else if (admin) TrailBtn("Cancel", "Cancel call host request to ${dev(h)}") { exec({ app.groups.revokeCall(d.id, h.did, h.deviceId) }, done) }
                }
            }
        }
        // This phone: carry the calls it starts (admin: appoint + accept in one go; asked: the Accept above; already: stop).
        if (d.amCallHost) ActRow(GIcons.Phone, "Stop hosting calls on this phone") { exec({ app.groups.acceptCallHost(d.id, false) }, done) }
        else if (admin && !d.callHostRequestedHere && GroupText.active(d.myRole))
            ActRow(GIcons.Phone, "Host calls on this phone", "Carries the calls you start here. A computer that stays on can carry everyone’s.") {
                exec({ app.groups.hostCallsHere(d.id, requested = false) }, done)
            }
        GNote("A call host carries the call. Everyone in a call connects to it, and it passes the audio on, so it can hear the call. That is separate from chat hosting: " +
            "a chat host is not a call host unless you also appoint it as one. Call hosts are members you trust, and each one chooses to accept." +
            if (admin) " Appoint one from a member’s menu, or host calls on this phone. A phone only hosts calls that its owner starts." else "",
            Modifier.padding(start = 4.dp, end = 4.dp, top = 10.dp))
    }
}

/** A trailing quiet text button whose label lines up with the pills' right edge (`margin-inline-end: -11px`, weight 500). */
@Composable
private fun TrailBtn(text: String, desc: String, onClick: () -> Unit) {
    val c = Tin.c
    Box(Modifier.offset(x = 11.dp).heightIn(min = 44.dp).clip(RoundedCornerShape(22.dp)).gTap(onClick = onClick).semantics { contentDescription = desc }
        .padding(horizontal = 18.dp), contentAlignment = Alignment.Center) {
        Text(text, style = GType.body.copy(fontWeight = FontWeight.Medium), color = c.ink)
    }
}

/** Flags on me while I'm still a member (FlagsOnMe.svelte). The core gives the count; who and when come only with a removal. */
@Composable
private fun FlagsOnMe(n: Int) {
    val total = GroupText.FLAGS_TO_REMOVE
    val left = (total - n).coerceAtLeast(0)
    InfoSec("Flags on you", countText = "$n of $total", countColor = Tin.c.er) {
        Box(Modifier.padding(start = 4.dp, end = 4.dp, bottom = 4.dp, top = 2.dp)) { FlagMeter(n, total, desc = "$n of $total flags") }
        GNote((if (left == 1) "One more flag" else "$left more flags") + " from any admin removes you from the group. Flags can’t be taken back. You can still read and post until then.",
            Modifier.padding(start = 4.dp, end = 4.dp, top = 6.dp))
    }
}

/** The removed person's view (Removal.svelte): who flagged, when, and what it means. */
@Composable
private fun RemovalBody(app: P2pApp, r: uniffi.p2pcore.GroupRemoval?, onBack: () -> Unit) {
    val c = Tin.c
    val ord = listOf("First flag", "Second flag", "Third flag")
    Column(Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, top = 8.dp, bottom = 32.dp)) {
        Text("Removed after ${GroupText.FLAGS_TO_REMOVE} admin flags", Modifier.padding(top = 8.dp, bottom = 2.dp).semantics { heading() }, style = GType.hero, color = c.ink)
        if (r != null && r.at > 0u) Text(dayAndTime(r.at.toLong(), System.currentTimeMillis()), style = GType.note, color = c.ink2)
        RemSec("Who flagged you")
        r?.flags?.forEachIndexed { i, f ->
            val who = nameOf(app, f.byDid, f.byName)
            Row(Modifier.fillMaxWidth().heightIn(min = 60.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Avatar(who, f.byDid, 40.dp)
                Column(verticalArrangement = Arrangement.spacedBy(1.dp)) {
                    Text(who, style = GType.bodyStrong, color = c.ink)
                    Text(ord.getOrElse(i) { "Flag" } + if (f.at > 0u) " · " + dayAndTime(f.at.toLong(), System.currentTimeMillis()) else "", style = GType.sub.copy(fontFeatureSettings = "tnum"), color = c.ink2)
                }
            }
            if (i < r.flags.lastIndex) GRule()
        }
        GNote("Each flag was made by an admin of the group, and this device checked them before showing this.", Modifier.padding(top = 6.dp))
        RemSec("What this means")
        Text("You keep the posts already on this device, and you can read them. You can’t post, and the group’s hosts no longer accept anything from you. Removal is final: to come back, an admin has to invite you again.",
            style = GType.body.copy(lineHeight = 22.sp), color = c.ink)
        Box(Modifier.padding(top = 20.dp)) { GBtn("Read the posts", onBack, GBtnKind.Outline) }
    }
}

@Composable
private fun RemSec(text: String) = Text(text.uppercase(), Modifier.padding(top = 24.dp, bottom = 6.dp).semantics { heading() }, style = GType.sec, color = Tin.c.ink2)

/** Invite contacts (InviteSheet.svelte): pick contacts who aren't in the group and have no invite waiting. */
@Composable
private fun InviteSheet(app: P2pApp, d: GroupDetail, sent: List<GroupInvitation>, exec: (suspend () -> Result<String>, (Result<String>) -> Unit) -> Unit,
                        say: (String) -> Unit, onDismiss: () -> Unit) {
    val c = Tin.c
    val contacts by app.contacts.collectAsState()
    val blocks by app.blocks.collectAsState()
    val choices = remember(contacts, d, sent, blocks) {
        GroupText.pickRows(contacts.map { InviteCandidate(it.did, it.display()) }, d, sent, { blocks.blockedByMe(it) }, { blocks.blockedMeAt(it) != null }).filter { it.why == null }
    }
    val picked = remember { mutableStateListOf<String>() }
    var busy by remember { mutableStateOf(false) }
    GSheet(onDismiss, "Invite contacts") { close ->
        GDialogBody("Invite to ${d.name}") {
            if (choices.isNotEmpty()) {
                Column(Modifier.padding(top = 8.dp).offset(x = (-8).dp).fillMaxWidth().padding(end = 0.dp)) {
                    choices.forEach { p ->
                        key(p.did) {
                            val on = p.did in picked
                            Row(
                                Modifier.fillMaxWidth().heightIn(min = 56.dp).clip(SmoothCornerShape(14.dp))
                                    .gTap(role = Role.Checkbox) { if (on) picked.remove(p.did) else picked.add(p.did) }
                                    .semantics(mergeDescendants = true) { toggleableState = ToggleableState(on) }.padding(horizontal = 10.dp, vertical = 6.dp),
                                verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp),
                            ) {
                                Avatar(p.name, p.did, 40.dp)
                                Text(p.name, Modifier.weight(1f), style = GType.bodyStrong, color = c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                CheckBox(on)
                            }
                        }
                    }
                }
                GPara("Each person gets a private invite that works for 7 days. Anyone you invite can read the group’s posts once they join.", top = 10.dp)
            } else GPara("Everyone in your contacts is already in this group or has an invite waiting.", top = 10.dp)
            GActs {
                GBtn("Cancel", close, GBtnKind.Quiet)
                GBtn(if (busy) "Sending…" else if (picked.isEmpty()) "Invite" else "Invite ${plural(picked.size, "person", "people")}", {
                    busy = true
                    val dids = picked.toList()
                    exec({ app.groups.inviteContacts(d.id, dids) }) { r -> busy = false; r.onSuccess { say(it); close() }.onFailure { say(it.message ?: "Couldn’t invite") } }
                }, GBtnKind.Primary, enabled = picked.isNotEmpty() && !busy)
            }
        }
    }
}

/** `.box`: 22 dp, radius 7, a 2 px line ring; checked fills accent with a 16 dp check (120 ms). */
@Composable
private fun CheckBox(on: Boolean) {
    val c = Tin.c
    val shape = RoundedCornerShape(7.dp)
    val bg by androidx.compose.animation.animateColorAsState(if (on) c.accentFill else Color.Transparent, tween(GMotion.SWITCH, easing = GMotion.EaseOut), label = "box")
    Box(Modifier.size(22.dp).clip(shape).background(bg).let { if (!on) it.border(2.dp, c.ln, shape) else it }, contentAlignment = Alignment.Center) {
        if (on) Icon(GIcons.Check, null, tint = c.onAccent, modifier = Modifier.size(16.dp))
    }
}

private enum class MView { Actions, Flag, Ask }

/**
 * A member's sheet (MemberSheet.svelte): per-member actions by my role, then the two confirm steps (flag, chat request),
 * cross-faded in place. Request admission and cooldown remain the core's responsibility.
 */
@Composable
private fun MemberSheet(app: P2pApp, d: GroupDetail, m: CoreMember, n: String, req: GroupChatRequest?, blockedByMe: Boolean,
                        exec: (suspend () -> Result<String>, (Result<String>) -> Unit) -> Unit, done: (Result<String>) -> Unit,
                        onChat: (String) -> Unit, onDismiss: () -> Unit) {
    val c = Tin.c
    var view by remember { mutableStateOf(MView.Actions) }
    val fn = firstName(n)
    val admin = GroupText.isAdmin(d.myRole)
    val contact = app.contacts.value.any { it.did == m.did }
    val acts = GroupText.actions(d, m)
    val several = m.deviceIds.size > 1
    GSheet(onDismiss, if (m.isMe) "Your role in this group" else "$n in ${d.name}") { close ->
        androidx.compose.animation.Crossfade(view, animationSpec = tween(GMotion.CROSSFADE, easing = GMotion.EaseOut), label = "view") { v ->
            when (v) {
                MView.Actions -> Column {
                    Row(Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, top = 18.dp, bottom = 10.dp), verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                        Avatar(n, m.did, 48.dp)
                        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(1.dp)) {
                            Text(if (m.isMe) "$n (you)" else n, style = GType.title.copy(lineHeight = 22.sp), color = c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            Text(m.invitedBy?.let { "Invited by ${firstName(nameOf(app, it, ""))} · ${dayOnly(m.joinedAt.toLong())}" } ?: "Member since ${dayOnly(m.joinedAt.toLong())}",
                                style = GType.sub, color = c.ink2, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        }
                        Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                            GPill(if (m.isOwner) "Owner" else if (m.isAdmin) "Admin" else "Member", if (m.isOwner) GTone.Owner else if (m.isAdmin) GTone.Admin else GTone.Muted)
                            if (m.isHost) GPill("Host", GTone.Host)
                            if (d.callHosts.any { it.did == m.did }) GPill("Call host", GTone.Call)
                        }
                    }
                    if (admin && !m.isMe && !m.isOwner) Text("${m.flags} of ${GroupText.FLAGS_TO_REMOVE} flags", Modifier.padding(start = 20.dp, end = 20.dp, bottom = 4.dp),
                        style = GType.sub.copy(fontFeatureSettings = "tnum"), color = c.ink2)
                    Column(Modifier.padding(start = 8.dp, end = 8.dp, top = 4.dp)) {
                        if (!m.isMe) when {
                            contact && !blockedByMe -> MItem(GIcons.Chat, "Message $fn") { onDismiss(); onChat(m.did) }
                            blockedByMe -> MItem(GIcons.Off, "You blocked $fn", "They stay in the group. Unblock them in Contacts to chat.", off = true)
                            req?.state == ChatRequestState.PENDING -> MItem(GIcons.Clock, "Request sent", "Waiting for $fn to answer", off = true)
                            req?.state == ChatRequestState.BLOCKED -> MItem(GIcons.Off, "Blocked", "$fn can’t receive requests from you.", off = true)
                            MemberAction.Message in acts -> MItem(GIcons.Chat, if (req?.state == ChatRequestState.DECLINED) "Ask $fn again" else "Ask $fn for a private chat",
                                if (req?.state == ChatRequestState.DECLINED) "$fn declined your last request" else "Sends a chat request through the group’s host") { view = MView.Ask }
                        }
                        acts.forEach { a ->
                            when (a) {
                                MemberAction.MakeAdmin -> MItem(GIcons.Shield, "Make admin") { exec({ app.groups.setAdmin(d.id, m.did, true) }, done); close() }
                                MemberAction.RevokeAdmin -> MItem(GIcons.Shield, "Remove admin") { exec({ app.groups.setAdmin(d.id, m.did, false) }, done); close() }
                                is MemberAction.Appoint -> MItem(GIcons.Server, if (several) "Make device ${GroupText.shortDev(a.deviceId)} a host" else "Make host",
                                    "Asks $fn to host. They have to accept.") { exec({ app.groups.appoint(d.id, m.did, a.deviceId) }, done); close() }
                                is MemberAction.Revoke -> MItem(GIcons.Server, if (several) "Stop device ${GroupText.shortDev(a.deviceId)} hosting" else "Remove as host") {
                                    exec({ app.groups.revoke(d.id, m.did, a.deviceId) }, done); close()
                                }
                                else -> {}
                            }
                        }
                        if (admin && !m.isMe) m.deviceIds.forEach { dev ->
                            val isCh = d.callHosts.any { it.did == m.did && it.deviceId == dev }
                            val pend = d.callHostRequests.any { it.did == m.did && it.deviceId == dev }
                            val suffix = if (several) " · ${GroupText.shortDev(dev)}" else ""
                            when {
                                pend -> MItem(GIcons.Phone, "Cancel call host request$suffix", "Waiting for $fn’s device to accept") { exec({ app.groups.revokeCall(d.id, m.did, dev) }, done); close() }
                                isCh -> MItem(GIcons.Phone, "Remove as call host$suffix") { exec({ app.groups.revokeCall(d.id, m.did, dev) }, done); close() }
                                else -> MItem(GIcons.Phone, "Make call host$suffix", "Asks $fn to carry group calls. They have to accept.") { exec({ app.groups.appointCall(d.id, m.did, dev) }, done); close() }
                            }
                        }
                        if (m.isMe && GroupText.active(d.myRole)) {
                            if (d.amHost) MItem(GIcons.Server, "Stop hosting on this device") { exec({ app.groups.acceptHost(d.id, false) }, done); close() }
                            else if (d.hostOffers.none { it.isMe }) MItem(GIcons.Server, "Host on this device", "An admin has to appoint it too. A computer that stays on is a better host.") {
                                exec({ app.groups.acceptHost(d.id, true) }, done); close()
                            }
                            if (d.amCallHost) MItem(GIcons.Phone, "Stop hosting calls on this device") { exec({ app.groups.acceptCallHost(d.id, false) }, done); close() }
                        }
                        if (MemberAction.Flag in acts) MItem(GIcons.Flag, "Flag for removal", "${GroupText.FLAGS_TO_REMOVE} flags remove $fn from the group", danger = true) { view = MView.Flag }
                        else if (m.isOwner && !m.isMe) MItem(GIcons.Flag, "The owner can’t be flagged", off = true)
                    }
                    Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 8.dp, bottom = 14.dp), horizontalArrangement = Arrangement.End) {
                        GBtn("Close", close, GBtnKind.Quiet)
                    }
                }
                MView.Flag -> {
                    val (next, lead) = GroupText.flagCopy(n, m.flags.toInt())
                    Column(Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, top = 20.dp, bottom = 16.dp)) {
                        Text("Flag $fn for removal?", Modifier.padding(bottom = 12.dp).semantics { heading() }, style = GType.title, color = c.ink)
                        FlagMeter(m.flags.toInt(), GroupText.FLAGS_TO_REMOVE, yours = true, desc = "Flag $next of ${GroupText.FLAGS_TO_REMOVE}")
                        GPara(lead, top = 12.dp, color = c.ink, style = GType.body.copy(lineHeight = 21.sp))
                        GPara("Flags can’t be taken back, and removal is final. The same admin can flag more than once; each flag counts.")
                        GPara("Flagging is a decision for the group. It is separate from blocking, which only affects you. $fn sees each flag: who made it and when.")
                        GActs {
                            GBtn("Back", { view = MView.Actions }, GBtnKind.Quiet)
                            GBtn("Flag $fn", { exec({ app.groups.flag(d.id, m.did) }, done); close() }, if (next >= GroupText.FLAGS_TO_REMOVE) GBtnKind.Danger else GBtnKind.Outline)
                        }
                    }
                }
                MView.Ask -> Column(Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, top = 20.dp, bottom = 16.dp)) {
                    Text("Ask $fn for a private chat", Modifier.padding(bottom = 4.dp).semantics { heading() }, style = GType.title, color = c.ink)
                    GPara("$fn sees your name and the group. The request goes through the group’s host, sealed, so the host can’t read it.")
                    GPara("If $fn accepts, you can message each other directly. Calls stay off until one of you allows them. A request expires after 7 days.")
                    GActs {
                        GBtn("Back", { view = MView.Actions }, GBtnKind.Quiet)
                        GBtn("Send request", { exec({ app.groups.request(d.id, m.did) }, done); close() }, GBtnKind.Primary)
                    }
                }
            }
        }
    }
}

/** A sheet menu item (`.mi`): 56 dp, 6/12 padding, radius 14, 20 dp glyph, 16 gap; off items are muted and inert. */
@Composable
private fun MItem(icon: androidx.compose.ui.graphics.vector.ImageVector, title: String, sub: String? = null, danger: Boolean = false, off: Boolean = false, onClick: () -> Unit = {}) {
    val c = Tin.c
    Row(
        Modifier.fillMaxWidth().heightIn(min = 56.dp).clip(SmoothCornerShape(14.dp)).let { if (!off) it.gTap(onClick = onClick) else it.semantics(mergeDescendants = true) {} }
            .padding(horizontal = 12.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Icon(icon, null, tint = if (danger) c.er else c.ink2, modifier = Modifier.size(20.dp))
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(1.dp)) {
            Text(title, style = GType.bodyStrong, color = if (danger) c.er else if (off) c.ink2 else c.ink)
            if (sub != null) Text(sub, style = GType.sub.copy(lineHeight = 17.sp), color = c.ink2)
        }
    }
}

// ------------------------------------------------------------------ removal

/** "Why you were removed" (the route from the conversation): the same view as a removed group's info. */
@Composable
fun GroupRemovalLive(app: P2pApp, id: String, onBack: () -> Unit) {
    val open by app.groups.open.collectAsState()
    LaunchedEffect(id) { withContext(Dispatchers.IO) { if (app.groups.open.value?.id != id) app.groups.open(id) } }
    val r = open?.takeIf { it.id == id }?.detail?.removal
    InfoPage("Removed", onBack) { item { RemovalBody(app, r, onBack) } }
}

// ------------------------------------------------------------------ new group

@Composable
fun NewGroupLive(app: P2pApp, onBack: () -> Unit, onCreated: (String) -> Unit) {
    val c = Tin.c
    var name by rememberSaveable { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var err by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    Page {
        TopBar("New group", onBack)
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(horizontal = 20.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            TinField(name, { name = it.take(100) }, "Group name", state = if (err != null) FieldState.Error else FieldState.Normal, hint = err)
            InfoCard("This phone hosts the group at first: it keeps the group and passes messages on. A computer that stays on is a better host — appoint one from Group info once it has joined.",
                icon = PgIcons.Server, kind = BannerKind.Warn)
            Hint("You’ll own the group. Invite people from Group info.")
        }
        Box(Modifier.padding(20.dp)) {
            TinButton(if (busy) "Making…" else "Create group", {
                busy = true; err = null
                scope.launch {
                    val r = withContext(Dispatchers.IO) { app.groups.create(name) }
                    busy = false
                    r.onSuccess(onCreated).onFailure { err = it.message }
                }
            }, enabled = name.isNotBlank() && !busy)
        }
    }
}

// ------------------------------------------------------------------ add members (G3)

/**
 * Pick contacts to invite: blocked contacts (either direction) are not offered; members and people with
 * a pending invitation show why they can't be picked. Each invitation goes over the 1:1 chat.
 */
@Composable
fun AddMembersLive(app: P2pApp, id: String, onBack: () -> Unit, say: (String) -> Unit) {
    val c = Tin.c
    val open by app.groups.open.collectAsState()
    val sent by app.groups.sent.collectAsState()
    val contacts by app.contacts.collectAsState()
    val blocks by app.blocks.collectAsState()
    LaunchedEffect(id) { withContext(Dispatchers.IO) { if (app.groups.open.value?.id != id) app.groups.open(id) } }
    val d = open?.takeIf { it.id == id }?.detail ?: run { Page { TopBar("Add members", onBack) }; return }
    val picked = remember { mutableStateListOf<String>() }
    var busy by remember { mutableStateOf(false) }
    val exec = rememberRunner()
    val rows = remember(contacts, d, sent, blocks) {
        GroupText.pickRows(contacts.map { InviteCandidate(it.did, it.display()) }, d, sent, { blocks.blockedByMe(it) }, { blocks.blockedMeAt(it) != null })
    }
    Page {
        TopBar("Add to ${d.name}", onBack)
        LazyColumn(Modifier.weight(1f), contentPadding = PaddingValues(horizontal = 20.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            item { SectionLabel("CONTACTS · ${picked.size} SELECTED", Modifier.padding(start = 0.dp)) }
            if (rows.isEmpty()) item { Hint("No contacts to add. People you block, or who block you, aren’t listed.") }
            items(rows, key = { it.did }) { r ->
                val on = r.did in picked
                val can = r.why == null
                Row(
                    Modifier.fillMaxWidth().heightIn(min = 56.dp).clip(SmoothCornerShape(12.dp))
                        .let { if (can) it.clickable(role = Role.Checkbox) { if (on) picked.remove(r.did) else picked.add(r.did) }.semantics(mergeDescendants = true) { toggleableState = ToggleableState(on) } else it },
                    verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp),
                ) {
                    PersonAvatar(r.name, r.did, 40.dp)
                    Column(Modifier.weight(1f)) {
                        Text(r.name, style = TinType.bodyL.copy(fontSize = 15.sp), color = if (can) c.ink else c.ink2, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        val sub = r.why ?: r.invited?.let { GroupText.sentInvitationLine(it).first }
                        if (sub != null) Text(sub, style = TinType.bodyM, color = c.ink2)
                    }
                    if (can) Checkbox(on, null, colors = CheckboxDefaults.colors(checkedColor = c.pr, uncheckedColor = c.ln2, checkmarkColor = c.onPr))
                }
            }
            item { Hint("Each person gets the invitation in your 1:1 chat, and joins when they accept. It is valid for 7 days.", Modifier.padding(top = 8.dp)) }
        }
        Box(Modifier.padding(20.dp)) {
            TinButton(if (busy) "Sending…" else if (picked.size > 1) "Invite ${picked.size} people" else "Invite", {
                busy = true
                val dids = picked.toList()
                exec({ app.groups.inviteContacts(id, dids) }) { r -> busy = false; r.onSuccess { say(it); onBack() }.onFailure { say(it.message ?: "Couldn’t invite") } }
            }, enabled = picked.isNotEmpty() && !busy)
        }
    }
}

// ------------------------------------------------------------------ join

@Composable
fun JoinGroupLive(app: P2pApp, initial: String? = null, onBack: () -> Unit, onJoined: (String) -> Unit) {
    val c = Tin.c
    val ctx = LocalContext.current
    var token by rememberSaveable { mutableStateOf(initial ?: "") }
    var scanning by remember { mutableStateOf(false) }
    var busy by remember { mutableStateOf(false) }
    var err by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    var peek by remember { mutableStateOf<GroupInvitePeek?>(null) }
    // The details come from the group's host over the network: wait for typing to settle, show progress.
    var checking by remember { mutableStateOf(false) }
    LaunchedEffect(token) {
        peek = null
        if (token.isBlank()) { checking = false; return@LaunchedEffect }
        checking = true
        kotlinx.coroutines.delay(400)
        peek = withContext(Dispatchers.IO) { app.groups.peek(token) }
        checking = false
    }
    if (scanning) {
        Page {
            TopBar("Scan the invite", { scanning = false })
            QrCameraBox(Modifier.weight(1f).fillMaxWidth(), "Point at the group invite", false, {}) { t -> token = t.trim(); scanning = false }
        }
        return
    }
    val bad = token.isNotBlank() && !checking && peek == null
    Page {
        TopBar("Join a group", onBack)
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(horizontal = 20.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
            TinField(token, { token = it.trim(); err = null }, "Invite link or code", mono = true, singleLine = false, minHeight = 72.dp, placeholder = "tinline://g/…",
                state = if (bad) FieldState.Error else FieldState.Normal,
                hint = when { checking -> "Checking with the group’s host…"; bad -> "That link didn’t work: it isn’t a group invite, it expired, or its host is offline."; else -> null })
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                TinButton("Paste", {
                    val cm = ctx.getSystemService(android.content.ClipboardManager::class.java)
                    cm?.primaryClip?.takeIf { it.itemCount > 0 }?.getItemAt(0)?.coerceToText(ctx)?.toString()?.let { token = it.trim() }
                }, Modifier.weight(1f), style = BtnStyle.Outlined, icon = PgIcons.Paste)
                TinButton("Scan", { scanning = true }, Modifier.weight(1f), style = BtnStyle.Outlined, icon = PgIcons.Scan)
            }
            peek?.let { p ->
                CardBox(Modifier.fillMaxWidth(), radius = 20.dp) {
                    Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                            GroupAvatar(tintOf(p.groupId), 44.dp, radius = 14.dp, gid = p.groupId)
                            Text(p.name, style = TinType.titleM, color = c.ink)
                        }
                        Text(p.issuerDid?.let { "Invited by ${nameOf(app, it, "someone who isn’t in your contacts")}" } ?: "A link for your own account", style = TinType.bodyM, color = c.ink2)
                        Text("Owner: ${nameOf(app, p.ownerDid, "someone who isn’t in your contacts")}", style = TinType.bodyM, color = c.ink2)
                        p.expires?.let { Text("Valid until ${whenFull(it.toLong())}", style = TinType.bodyM, color = c.ink2) }
                    }
                }
                Hint("Joining connects this phone to the group’s hosts. Members see your name; only hosts learn how to reach this phone. It can take up to a minute.")
            }
            err?.let { Text(it, style = TinType.bodyM, color = c.er) }
        }
        Box(Modifier.padding(20.dp)) {
            TinButton(if (busy) "Joining…" else "Join", {
                busy = true; err = null
                scope.launch {
                    val r = withContext(Dispatchers.IO) { app.groups.join(token) }
                    busy = false
                    r.onSuccess(onJoined).onFailure { err = it.message }
                }
            }, enabled = peek != null && !busy)
        }
    }
}

// ------------------------------------------------------------------ invite link (G3: short, large QR)

@Composable
fun GroupTokenLive(app: P2pApp, id: String, onBack: () -> Unit) {
    val c = Tin.c
    val ctx = LocalContext.current
    val name = app.groups.open.collectAsState().value?.takeIf { it.id == id }?.detail?.name ?: "the group"
    var token by remember { mutableStateOf<String?>(null) }
    var err by remember { mutableStateOf<String?>(null) }
    var full by remember { mutableStateOf(false) }
    LaunchedEffect(id) {
        val r = withContext(Dispatchers.IO) { app.groups.invite(id) }
        r.onSuccess { token = it }.onFailure { err = it.message }
    }
    Page {
        TopBar("Invite link", onBack)
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(horizontal = 20.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(14.dp)) {
            Hint("Anyone with this link or QR can join $name for 7 days. Share it over an app you trust; you see who joined and can flag them out. To invite your contacts directly, use Add members.")
            val t = token
            // Large, high contrast, the full width: easy to scan from another phone. Tap for full screen.
            Box(Modifier.fillMaxWidth().aspectRatio(1f).clip(SmoothCornerShape(16.dp)).background(Color.White)
                .let { m -> if (t != null) m.clickable(role = Role.Button, onClickLabel = "Show full screen") { full = true } else m },
                contentAlignment = Alignment.Center) {
                when {
                    t != null -> {
                        val bmp = remember(t) { qrBitmap(t, 1024, fg = 0xFF000000.toInt()) }
                        Image(bmp.asImageBitmap(), "QR code of the group invite link", Modifier.fillMaxSize().padding(12.dp))
                    }
                    err != null -> Text(err!!, Modifier.padding(16.dp), style = TinType.bodyM, color = c.er, textAlign = TextAlign.Center)
                    else -> Text("Making it…", style = TinType.bodyM, color = Color(0xFF4D5853))
                }
            }
            t?.let { Text(it, style = TinType.bodyM.copy(fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace), color = c.ink2, textAlign = TextAlign.Center) }
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                TinButton("Share", {
                    token?.let { tk -> runCatching { ctx.startActivity(Intent.createChooser(Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, "Join $name on Tinline: $tk"), "Share group invite")) } }
                }, Modifier.weight(1f), style = BtnStyle.Tonal, icon = PgIcons.Share, enabled = token != null)
                CopyButton(token, Modifier.weight(1f))
            }
        }
    }
    val t = token
    if (full && t != null) {
        Dialog({ full = false }, DialogProperties(usePlatformDefaultWidth = false)) {
            Box(Modifier.fillMaxSize().background(Color.White).clickable(role = Role.Button, onClickLabel = "Close") { full = false }.padding(16.dp), contentAlignment = Alignment.Center) {
                val bmp = remember(t) { qrBitmap(t, 1024, fg = 0xFF000000.toInt()) }
                Image(bmp.asImageBitmap(), "QR code of the group invite link, full screen", Modifier.fillMaxWidth().aspectRatio(1f))
            }
        }
    }
}
