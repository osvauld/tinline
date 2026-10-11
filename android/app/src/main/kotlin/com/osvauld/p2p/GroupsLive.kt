package com.osvauld.p2p

import android.content.Intent
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.Logout
import androidx.compose.material.icons.automirrored.rounded.Reply
import androidx.compose.material.icons.rounded.*
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
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.p2pcore.GroupDetail
import uniffi.p2pcore.GroupInvitePeek
import uniffi.p2pcore.GroupItemKind
import uniffi.p2pcore.GroupMessage
import uniffi.p2pcore.GroupDelivery as CoreDelivery
import uniffi.p2pcore.GroupMember as CoreMember
import uniffi.p2pcore.GroupRole as CoreRole

/*
 * Group chat screens on the core (state: GroupsCore.kt), after the boards in docs/design/groups
 * (Main, AndroidGroupInfo, AndroidRemoval): the Groups tab, a group's chat, group info with member
 * actions, new group, join (paste or scan), the invite / own-device token, and why-removed.
 */

/** A DID as the user knows it: their contact name, else what the group calls them. */
private fun nameOf(app: P2pApp, did: String, fallback: String): String {
    if (did == app.currentDid()) return "You"
    app.contacts.value.firstOrNull { it.did == did }?.let { return it.display() }
    return fallback.ifBlank { "Someone" }
}

/** Runs a group action off the main thread, then hands its outcome back on it. */
@Composable
private fun rememberRunner(): (suspend () -> Result<String>, (Result<String>) -> Unit) -> Unit {
    val scope = rememberCoroutineScope()
    return remember(scope) { { work, done -> scope.launch { done(withContext(Dispatchers.IO) { work() }) } } }
}

// ------------------------------------------------------------------ the Groups tab

@Composable
fun GroupsLiveTab(app: P2pApp, onOpen: (String) -> Unit, onNew: () -> Unit, onJoin: () -> Unit) {
    val c = Tin.c
    val list by app.groups.list.collectAsState()
    val me = app.groups.myDid
    val now = System.currentTimeMillis()
    val state = rememberLazyListState()
    val wide by remember { derivedStateOf { state.firstVisibleItemIndex == 0 && state.firstVisibleItemScrollOffset == 0 } }
    var menu by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) { withContext(Dispatchers.IO) { app.groups.refresh() } }
    Box(Modifier.fillMaxSize()) {
        LazyColumn(Modifier.fillMaxSize(), state = state, contentPadding = PaddingValues(bottom = 96.dp)) {
            item { SectionLabel("GROUPS · ${list.size}") }
            if (list.isEmpty()) item {
                Hint("No groups yet. Make one, or join with an invite someone sent you.", Modifier.padding(horizontal = 20.dp, vertical = 8.dp))
            }
            items(list, key = { it.id }) { s ->
                val gone = !GroupText.active(s.myRole)
                val at = s.last?.at?.toLong() ?: 0L
                Row(
                    Modifier.fillMaxWidth().heightIn(min = 72.dp).clickable(role = Role.Button) { onOpen(s.id) }.padding(horizontal = 20.dp, vertical = 6.dp),
                    verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp),
                ) {
                    GroupAvatar(tintOf(s.id), 48.dp, dim = gone, radius = 16.dp)
                    Column(Modifier.weight(1f)) {
                        Row(verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            Text(s.name, Modifier.weight(1f), style = TinType.bodyL.copy(fontWeight = if (s.unread > 0u && !gone) FontWeight.Bold else FontWeight.SemiBold),
                                color = if (gone) c.ink2 else c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            if (at > 0) Text(listTime(at, now), style = TinType.caption.copy(fontWeight = FontWeight.Normal), color = c.ink2)
                        }
                        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                            if (!gone && !s.hostedHere && !s.hostReachable) Icon(Icons.Rounded.Schedule, "Waiting for a host", tint = c.ink2, modifier = Modifier.size(14.dp))
                            Text(GroupText.preview(s, me), Modifier.weight(1f), style = TinType.bodyM, color = c.ink2, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            if (s.unread > 0u && !gone) Box(Modifier.defaultMinSize(22.dp, 22.dp).clip(RoundedCornerShape(50)).background(c.pr).padding(horizontal = 6.dp)
                                .semantics { contentDescription = "${s.unread} unread" }, contentAlignment = Alignment.Center) {
                                Text("${s.unread}", style = TinType.caption.copy(fontWeight = FontWeight.Bold), color = c.onPr)
                            }
                        }
                    }
                }
            }
        }
        Box(Modifier.align(Alignment.BottomEnd)) {
            ShrinkFab(Icons.Rounded.GroupAdd, "Group", wide) { menu = true }
            DropdownMenu(menu, { menu = false }, containerColor = c.sf) {
                DropdownMenuItem({ Text("New group", style = TinType.bodyL, color = c.ink) }, { menu = false; onNew() },
                    leadingIcon = { Icon(Icons.Rounded.GroupAdd, null, tint = c.ink2) })
                DropdownMenuItem({ Text("Join with an invite", style = TinType.bodyL, color = c.ink) }, { menu = false; onJoin() },
                    leadingIcon = { Icon(Icons.Rounded.QrCodeScanner, null, tint = c.ink2) })
            }
        }
    }
}

private fun tintOf(id: String) = (id.hashCode() and 0x7fffffff) % 2

// ------------------------------------------------------------------ chat

@Composable
fun GroupChatLive(app: P2pApp, id: String, onBack: () -> Unit, onInfo: () -> Unit, onRemoval: () -> Unit, say: (String) -> Unit) {
    val c = Tin.c
    val open by app.groups.open.collectAsState()
    val blocks by app.blocks.collectAsState()
    val exec = rememberRunner()
    LaunchedEffect(id) { withContext(Dispatchers.IO) { app.groups.open(id) } }
    DisposableEffect(id) { onDispose { app.groups.close(id) } }
    val o = open?.takeIf { it.id == id }
    if (o == null) { Page { TopBar("Group", onBack) ; Hint("Loading…", Modifier.padding(24.dp)) }; return }
    val d = o.detail
    val me = app.groups.myDid
    var draft by rememberSaveable(id) { mutableStateOf("") }
    var replyTo by remember(id) { mutableStateOf<String?>(null) }
    var sending by remember { mutableStateOf(false) }
    val list = rememberLazyListState()
    LaunchedEffect(o.messages.size) { if (o.messages.isNotEmpty()) list.scrollToItem(o.messages.size - 1 + if (o.older != null) 1 else 0) }
    val waiting = GroupText.waitingForHost(d)
    Page {
        ChatHeader(
            d.name, onBack, sub = GroupText.status(d), subColor = if (d.myRole == CoreRole.REMOVED) c.er else if (waiting) c.threadText else c.ink2,
            avatar = { GroupAvatar(tintOf(d.id), 40.dp, dim = !GroupText.active(d.myRole), radius = 12.dp) }, onTitle = onInfo,
        ) { IconBtn(Icons.Rounded.Info, "Group info", onInfo) }
        if (waiting) InfoCard(
            if (d.hosts.isEmpty()) "This group has no host yet. Your messages are saved and send when a member’s computer hosts it."
            else "No host is online. Your messages are saved and send when one comes back.",
            Modifier.padding(horizontal = 16.dp, vertical = 4.dp), icon = Icons.Rounded.CloudOff, kind = BannerKind.Warn,
        )
        LazyColumn(Modifier.weight(1f).fillMaxWidth(), state = list, contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            if (o.older != null) item {
                Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
                    TinButton("Show earlier messages", { exec({ app.groups.older(); Result.success("") }) {} }, style = BtnStyle.Text, fill = false, height = 40.dp)
                }
            }
            if (o.messages.isEmpty()) item { Column(Modifier.fillMaxWidth()) { SysLine("No messages yet") } }
            items(o.messages, key = { it.id }) { m ->
                Column(Modifier.fillMaxWidth()) { GroupItemLive(app, o.messages, m, me, d, blocks.blockedByMe(m.authorDid), onRemoval, onReply = { replyTo = m.id }) }
            }
        }
        when (d.myRole) {
            CoreRole.REMOVED -> BlockedFooter("You were removed and can’t send messages to this group.") {
                TinButton("Why?", onRemoval, Modifier.weight(1f), style = BtnStyle.Outlined, height = 48.dp)
            }
            CoreRole.LEFT -> BlockedFooter("You left this group. Its messages stay here, read-only.")
            else -> Column {
                replyTo?.let { rid ->
                    val r = o.messages.firstOrNull { it.id == rid }
                    Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        Icon(Icons.AutoMirrored.Rounded.Reply, null, tint = c.ink2, modifier = Modifier.size(16.dp))
                        Text("Replying to ${r?.let { firstName(nameOf(app, it.authorDid, it.authorName)) + ": " + it.text } ?: "a message"}", Modifier.weight(1f),
                            style = TinType.bodyM, color = c.ink2, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        IconBtn(Icons.Rounded.Close, "Cancel reply", { replyTo = null }, tint = c.ink2)
                    }
                }
                Composer(draft, { draft = it }, "Message the group", onSend = {
                    if (sending) return@Composer
                    val text = draft; val reply = replyTo
                    draft = ""; replyTo = null; sending = true
                    exec({ app.groups.post(id, text, reply).map { "" } }) { r ->
                        sending = false
                        r.onFailure { if (draft.isEmpty()) draft = text; say(it.message ?: "Couldn’t send") }
                    }
                })
            }
        }
    }
}

@Composable
private fun ColumnScope.GroupItemLive(
    app: P2pApp, all: List<GroupMessage>, m: GroupMessage, me: String, d: GroupDetail, authorBlocked: Boolean,
    onRemoval: () -> Unit, onReply: () -> Unit,
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
    if (m.kind != GroupItemKind.POST) { SysLine(GroupText.sysText(m, me)); return }
    val name = nameOf(app, m.authorDid, m.authorName)
    var shown by remember(m.id) { mutableStateOf(false) }
    if (authorBlocked && !m.outgoing && !shown) {
        // A personal block never changes the group; the message is folded, not dropped.
        Row(
            Modifier.align(Alignment.Start).heightIn(min = 48.dp).clip(RoundedCornerShape(18.dp)).background(c.sf2)
                .clickable(role = Role.Button, onClickLabel = "Show message") { shown = true }.padding(horizontal = 12.dp),
            verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Icon(Icons.Rounded.Block, null, tint = c.ink2, modifier = Modifier.size(18.dp))
            Text("Message from ${firstName(name)}, who you blocked · Show", style = TinType.bodyM, color = c.ink2)
        }
        return
    }
    m.replyTo?.let { rid ->
        val r = all.firstOrNull { it.id == rid }
        Text(
            "↪ " + (r?.let { firstName(nameOf(app, it.authorDid, it.authorName)) + ": " + it.text } ?: "Reply to an earlier message"),
            Modifier.align(if (m.outgoing) Alignment.End else Alignment.Start).widthIn(max = 280.dp).padding(horizontal = 8.dp),
            style = TinType.caption, color = c.ink2, maxLines = 1, overflow = TextOverflow.Ellipsis,
        )
    }
    val tick = when {
        m.beforeRemoval -> "Written before ${firstName(name)} was removed"
        else -> GroupText.deliveryWord(m)
    }
    val waitIcon: (@Composable () -> Unit)? =
        if (m.outgoing && m.delivery == CoreDelivery.WAITING) { { Icon(Icons.Rounded.Schedule, null, tint = c.onPrc, modifier = Modifier.size(12.dp)) } } else null
    Box(Modifier.align(if (m.outgoing) Alignment.End else Alignment.Start).clickable(role = Role.Button, onClickLabel = "Reply", enabled = GroupText.active(d.myRole), onClick = onReply)) {
        Column {
            Bubble(
                m.text, m.outgoing,
                author = if (m.outgoing) null else firstName(name) + if (m.beforeRemoval) " · removed" else "",
                authorColor = authorColor(m.authorDid), tick = tick,
                tickIcon = waitIcon,
                faded = m.beforeRemoval,
            )
        }
    }
}

// ------------------------------------------------------------------ group info

@Composable
fun GroupInfoLive(app: P2pApp, id: String, onBack: () -> Unit, onToken: (device: Boolean) -> Unit, onRemoval: () -> Unit, say: (String) -> Unit) {
    val c = Tin.c
    val open by app.groups.open.collectAsState()
    val blocks by app.blocks.collectAsState()
    val exec = rememberRunner()
    LaunchedEffect(id) { withContext(Dispatchers.IO) { if (app.groups.open.value?.id != id) app.groups.open(id) } }
    val d = open?.takeIf { it.id == id }?.detail ?: run { Page { TopBar(null, onBack) }; return }
    var sheetFor by remember { mutableStateOf<String?>(null) }
    var confirm by remember { mutableStateOf<GroupConfirm?>(null) }
    val admin = GroupText.isAdmin(d.myRole)
    val active = GroupText.active(d.myRole)
    val done: (Result<String>) -> Unit = { r -> r.onSuccess { if (it.isNotEmpty()) say(it) }.onFailure { say(it.message ?: "Couldn’t do that") } }
    Page {
        TopBar(null, onBack)
        LazyColumn(Modifier.weight(1f), contentPadding = PaddingValues(bottom = 24.dp)) {
            item {
                Column(Modifier.fillMaxWidth().padding(horizontal = 24.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    GroupAvatar(tintOf(d.id), 80.dp, dim = !active, radius = 24.dp)
                    Text(d.name, style = TinType.h1.copy(fontSize = 26.sp), color = c.ink, textAlign = TextAlign.Center, modifier = Modifier.semantics { heading() })
                    val role = when (d.myRole) {
                        CoreRole.OWNER -> " · you are the owner"; CoreRole.ADMIN -> " · you are an admin"; CoreRole.MEMBER -> ""
                        CoreRole.REMOVED -> " · you were removed"; CoreRole.LEFT -> " · you left"
                    }
                    Text("${d.members.size} members$role", style = TinType.bodyM, color = c.ink2)
                    if (active) Row(Modifier.padding(top = 8.dp), horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.CenterHorizontally)) {
                        if (admin) InfoActionLive(Icons.Rounded.PersonAdd, "Invite", c.pr) { onToken(false) }
                        InfoActionLive(Icons.Rounded.Devices, "My device", c.pr) { onToken(true) }
                        if (d.myRole != CoreRole.OWNER) InfoActionLive(Icons.AutoMirrored.Rounded.Logout, "Leave", c.er) { confirm = GroupConfirm.Leave }
                    }
                    if (d.removal != null) TinButton("Why was I removed?", onRemoval, style = BtnStyle.Outlined, fill = false, height = 44.dp)
                }
            }
            item { SectionLabel("HOSTS · KEEP THE GROUP RUNNING") }
            if (d.hosts.isEmpty()) item { Hint("No host yet. Messages wait until a member’s device hosts the group; a computer that stays on is best.", Modifier.padding(horizontal = 20.dp)) }
            items(d.hosts, key = { "h" + it.did + it.deviceId }) { h ->
                HostRow(if (h.isMe) "This phone" else "${firstName(nameOf(app, h.did, h.name))}’s device ${GroupText.shortDev(h.deviceId)}",
                    if (h.isMe) "Hosting now" else if (h.online) "Connected" else "Not connected", if (h.online) c.pr else c.ln2)
            }
            if (admin && d.hostOffers.isNotEmpty()) {
                item { SectionLabel("OFFERED TO HOST") }
                items(d.hostOffers, key = { "o" + it.did + it.deviceId }) { h ->
                    HostRow(if (h.isMe) "This phone" else "${firstName(nameOf(app, h.did, h.name))}’s device ${GroupText.shortDev(h.deviceId)}", "Agreed to host; needs an admin", c.thread) {
                        TinButton("Appoint", { exec({ app.groups.appoint(id, h.did, h.deviceId) }, done) }, style = BtnStyle.Tonal, fill = false, height = 40.dp)
                    }
                }
            }
            if (active) item {
                val offered = d.hostOffers.any { it.isMe }
                Column(Modifier.padding(horizontal = 20.dp, vertical = 6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    if (d.amHost || offered) TinButton(if (d.amHost) "Stop hosting on this phone" else "Withdraw host offer", { confirm = GroupConfirm.Host(false) }, style = BtnStyle.Outlined, height = 44.dp)
                    else {
                        TinButton("Host from this phone", { confirm = GroupConfirm.Host(true) }, style = BtnStyle.Outlined, height = 44.dp)
                        Hint("A computer that stays on is a better host: a phone sleeps and runs on battery.")
                    }
                }
            }
            item { SectionLabel("MEMBERS · ${d.members.size}") }
            items(d.members.sortedBy { m -> if (m.isOwner) 0 else if (m.isAdmin) 1 else 2 }, key = { it.did }) { m ->
                val acts = GroupText.actions(d, m)
                Row(
                    Modifier.fillMaxWidth().heightIn(min = 64.dp).let { if (acts.isNotEmpty()) it.clickable(role = Role.Button, onClickLabel = "Member actions") { sheetFor = m.did } else it }
                        .padding(horizontal = 20.dp, vertical = 6.dp),
                    verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    val n = nameOf(app, m.did, m.name)
                    PersonAvatar(n, m.did, 40.dp, contact = app.contacts.value.any { it.did == m.did }, isMe = m.isMe)
                    Column(Modifier.weight(1f).padding(start = 4.dp)) {
                        Text(n, style = TinType.bodyL.copy(fontWeight = FontWeight.SemiBold), color = c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        val sub = listOfNotNull(
                            if (admin && m.flags > 0u) (if (m.flags == 1u) "1 flag" else "${m.flags} flags") else null,
                            if (blocks.blockedByMe(m.did)) "Blocked by you" else null,
                        )
                        if (sub.isNotEmpty()) Text(sub.joinToString(" · "), style = TinType.bodyM, color = if (m.flags > 0u) c.er else c.ink2)
                    }
                    if (m.isOwner) RolePill(GroupRole.Owner) else if (m.isAdmin) RolePill(GroupRole.Admin)
                    if (m.isHost) RolePill(GroupRole.Host)
                }
            }
        }
    }
    sheetFor?.let { did -> d.members.firstOrNull { it.did == did }?.let { m ->
        val name = nameOf(app, m.did, m.name)
        TinSheet({ sheetFor = null }) {
            Text(name, style = TinType.titleM, color = c.ink)
            Text(buildString {
                append("Member since ${whenDate(m.joinedAt.toLong())}")
                m.invitedBy?.let { append(" · invited by ${firstName(nameOf(app, it, ""))}") }
                if (admin) append(" · ${m.flags} of ${GroupText.FLAGS_TO_REMOVE} flags")
            }, Modifier.padding(vertical = 4.dp), style = TinType.bodyM, color = c.ink2)
            val several = m.deviceIds.size > 1
            GroupText.actions(d, m).forEach { a ->
                when (a) {
                    MemberAction.MakeAdmin -> SheetItem(Icons.Rounded.AdminPanelSettings, "Make admin", "Can invite, flag members and appoint hosts") { sheetFor = null; confirm = GroupConfirm.Admin(m.did, name, true) }
                    MemberAction.RevokeAdmin -> SheetItem(Icons.Rounded.AdminPanelSettings, "Remove as admin", "${firstName(name)} stays a member") { sheetFor = null; confirm = GroupConfirm.Admin(m.did, name, false) }
                    is MemberAction.Appoint -> SheetItem(Icons.Rounded.Dns, if (several) "Make device ${GroupText.shortDev(a.deviceId)} a host" else "Make host",
                        "Hosts once ${firstName(name)} agrees on that device; a computer that stays on is best") { sheetFor = null; exec({ app.groups.appoint(id, m.did, a.deviceId) }, done) }
                    is MemberAction.Revoke -> SheetItem(Icons.Rounded.Dns, if (several) "Stop device ${GroupText.shortDev(a.deviceId)} hosting" else "Stop hosting",
                        "Members stop going through it") { sheetFor = null; exec({ app.groups.revoke(id, m.did, a.deviceId) }, done) }
                    MemberAction.Flag -> SheetItem(Icons.Rounded.Flag, "Flag to remove", "${GroupText.FLAGS_TO_REMOVE} flags remove ${firstName(name)} from the group", color = c.er) {
                        sheetFor = null; confirm = GroupConfirm.Flag(m.did, name, m.flags.toInt())
                    }
                }
            }
            Spacer(Modifier.height(16.dp))
        }
    } }
    when (val k = confirm) {
        null -> {}
        is GroupConfirm.Flag -> {
            val (yours, text) = GroupText.flagCopy(k.name, k.flags)
            TinDialog("Flag ${firstName(k.name)} for removal?", { confirm = null }, "Flag", { confirm = null; exec({ app.groups.flag(id, k.did) }, done) }, destructive = true, icon = Icons.Rounded.Flag) {
                FlagMeter(yours, GroupText.FLAGS_TO_REMOVE)
                DialogText(text)
                DialogText("A flag is signed by you and can’t be taken back. ${firstName(k.name)} can see who flagged them and when.")
            }
        }
        is GroupConfirm.Admin -> TinDialog(if (k.admin) "Make ${firstName(k.name)} an admin?" else "Remove ${firstName(k.name)} as admin?", { confirm = null },
            if (k.admin) "Make admin" else "Remove", { confirm = null; exec({ app.groups.setAdmin(id, k.did, k.admin) }, done) }) {
            DialogText(if (k.admin) "${firstName(k.name)} will be able to invite people, flag members (3 flags remove someone) and appoint hosts. Only you, the owner, can make or remove admins."
                else "${firstName(k.name)} stays in the group as a member. Flags they already made still count.")
        }
        GroupConfirm.Leave -> TinDialog("Leave ${d.name}?", { confirm = null }, "Leave", { confirm = null; exec({ app.groups.leave(id) }, done) }, destructive = true, icon = Icons.AutoMirrored.Rounded.Logout) {
            DialogText("Members see that you left. The messages stay on this phone, read-only. To come back, an admin has to invite you again.")
        }
        is GroupConfirm.Host -> if (k.accept) TinDialog("Host ${d.name} on this phone?", { confirm = null }, "Host here", { confirm = null; exec({ app.groups.acceptHost(id, true) }, done) }, icon = Icons.Rounded.Dns) {
            DialogText("A host keeps a full copy of the group, passes messages on while members’ phones sleep and learns members’ group addresses. It needs to stay online and uses battery.")
            DialogText("A computer that stays on is a better host. This phone starts hosting once an admin appoints it.")
        } else TinDialog("Stop hosting ${d.name} here?", { confirm = null }, "Stop hosting", { confirm = null; exec({ app.groups.acceptHost(id, false) }, done) }, destructive = true) {
            DialogText("This phone stops passing the group’s messages on. You stay a member." + if (d.amHost && d.hosts.size == 1) " The group has no other host: messages wait until another device hosts it." else "")
        }
    }
}

private sealed interface GroupConfirm {
    data class Flag(val did: String, val name: String, val flags: Int) : GroupConfirm
    data class Admin(val did: String, val name: String, val admin: Boolean) : GroupConfirm
    data object Leave : GroupConfirm
    data class Host(val accept: Boolean) : GroupConfirm
}

@Composable
private fun HostRow(title: String, sub: String, dot: Color, trailing: @Composable () -> Unit = {}) {
    val c = Tin.c
    Row(Modifier.fillMaxWidth().heightIn(min = 64.dp).padding(start = 20.dp, end = 16.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp)) {
        Dot(dot, 8.dp)
        Column(Modifier.weight(1f)) {
            Text(title, style = TinType.bodyL.copy(fontWeight = FontWeight.SemiBold), color = c.ink)
            Text(sub, style = TinType.bodyM, color = c.ink2)
        }
        trailing()
    }
}

@Composable
private fun InfoActionLive(icon: androidx.compose.ui.graphics.vector.ImageVector, label: String, tint: Color, onClick: () -> Unit) {
    val c = Tin.c
    Column(
        Modifier.width(88.dp).clip(RoundedCornerShape(16.dp)).clickable(role = Role.Button, onClick = onClick).padding(vertical = 4.dp),
        horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Box(Modifier.size(52.dp).clip(RoundedCornerShape(50)).background(c.sf2), contentAlignment = Alignment.Center) { Icon(icon, null, tint = tint) }
        Text(label, style = TinType.caption.copy(fontSize = 13.sp), color = c.ink)
    }
}

// ------------------------------------------------------------------ removal

@Composable
fun GroupRemovalLive(app: P2pApp, id: String, onBack: () -> Unit) {
    val c = Tin.c
    val open by app.groups.open.collectAsState()
    LaunchedEffect(id) { withContext(Dispatchers.IO) { if (app.groups.open.value?.id != id) app.groups.open(id) } }
    val r = open?.takeIf { it.id == id }?.detail?.removal
    Page {
        TopBar("Why you were removed", onBack)
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(horizontal = 20.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
            if (r == null) { Hint("Nothing to show."); return@Column }
            r.flags.forEachIndexed { i, f ->
                val who = nameOf(app, f.byDid, f.byName)
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    PersonAvatar(who, f.byDid, 40.dp, contact = app.contacts.value.any { it.did == f.byDid })
                    Column {
                        Text(who, style = TinType.bodyL.copy(fontWeight = FontWeight.SemiBold), color = c.ink)
                        Text("Flagged ${whenFull(f.at.toLong())}" + if (i == r.flags.lastIndex) " · flag ${i + 1}" else "", style = TinType.bodyM, color = c.ink2)
                    }
                }
            }
            InfoCard("Each flag is signed by the admin who made it and was checked on this phone. Nobody else could have added them.", icon = Icons.Rounded.VerifiedUser)
            Text("What happens now", style = TinType.titleM, color = c.ink)
            DialogText("You keep the messages already on this phone. The group’s hosts refuse your new messages. To rejoin, an admin has to invite you again.")
        }
    }
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
                icon = Icons.Rounded.Dns, kind = BannerKind.Warn)
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

// ------------------------------------------------------------------ join

@Composable
fun JoinGroupLive(app: P2pApp, onBack: () -> Unit, onJoined: (String) -> Unit) {
    val c = Tin.c
    val ctx = LocalContext.current
    var token by rememberSaveable { mutableStateOf("") }
    var scanning by remember { mutableStateOf(false) }
    var busy by remember { mutableStateOf(false) }
    var err by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    var peek by remember { mutableStateOf<GroupInvitePeek?>(null) }
    LaunchedEffect(token) { peek = withContext(Dispatchers.IO) { app.groups.peek(token) } }
    if (scanning) {
        Page {
            TopBar("Scan the invite", { scanning = false })
            QrCameraBox(Modifier.weight(1f).fillMaxWidth(), "Point at the group invite", false, {}) { t -> token = t.trim(); scanning = false }
        }
        return
    }
    Page {
        TopBar("Join a group", onBack)
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(horizontal = 20.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
            TinField(token, { token = it.trim(); err = null }, "Invite", mono = true, singleLine = false, minHeight = 96.dp, placeholder = "Paste the invite you were sent",
                state = if (token.isNotBlank() && peek == null) FieldState.Error else FieldState.Normal,
                hint = if (token.isNotBlank() && peek == null) "That isn’t a group invite. Paste the whole text you were sent." else null)
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                TinButton("Paste", {
                    val cm = ctx.getSystemService(android.content.ClipboardManager::class.java)
                    cm?.primaryClip?.takeIf { it.itemCount > 0 }?.getItemAt(0)?.coerceToText(ctx)?.toString()?.let { token = it.trim() }
                }, Modifier.weight(1f), style = BtnStyle.Outlined, icon = Icons.Rounded.ContentPaste)
                TinButton("Scan", { scanning = true }, Modifier.weight(1f), style = BtnStyle.Outlined, icon = Icons.Rounded.QrCodeScanner)
            }
            peek?.let { p ->
                CardBox(Modifier.fillMaxWidth(), radius = 20.dp) {
                    Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                            GroupAvatar(tintOf(p.groupId), 44.dp, radius = 14.dp)
                            Text(p.name, style = TinType.titleM, color = c.ink)
                        }
                        Text(p.issuerDid?.let { "Invited by ${nameOf(app, it, "someone who isn’t in your contacts")}" } ?: "A link for your own other device", style = TinType.bodyM, color = c.ink2)
                        Text("Owner: ${nameOf(app, p.ownerDid, "someone who isn’t in your contacts")}", style = TinType.bodyM, color = c.ink2)
                        p.expires?.let { Text("Valid until ${whenFull(it.toLong() * 1000)}", style = TinType.bodyM, color = c.ink2) }
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

// ------------------------------------------------------------------ invite / own-device token

@Composable
fun GroupTokenLive(app: P2pApp, id: String, device: Boolean, onBack: () -> Unit) {
    val c = Tin.c
    val ctx = LocalContext.current
    val name = app.groups.open.collectAsState().value?.takeIf { it.id == id }?.detail?.name ?: "the group"
    var token by remember { mutableStateOf<String?>(null) }
    var err by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(id, device) {
        val r = withContext(Dispatchers.IO) { if (device) app.groups.deviceToken(id) else app.groups.invite(id) }
        r.onSuccess { token = it }.onFailure { err = it.message }
    }
    Page {
        TopBar(if (device) "Add my other device" else "Invite to $name", onBack)
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(horizontal = 20.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(14.dp)) {
            Hint(if (device) "On your other device (same account), open Groups › Join with an invite and scan or paste this. It joins as you, not as a new member."
                else "Send this to the people you want in $name, over an app you trust. Anyone holding it can join for 7 days; you see who joined and can flag them out.")
            val t = token
            Box(Modifier.size(260.dp).clip(RoundedCornerShape(14.dp)).background(Color.White), contentAlignment = Alignment.Center) {
                when {
                    t != null -> {
                        val bmp = remember(t) { qrBitmap(t, 720, fg = 0xFF17201D.toInt()) }
                        Image(bmp.asImageBitmap(), if (device) "QR code to add your other device" else "QR code of the group invite", Modifier.fillMaxSize().padding(10.dp))
                    }
                    err != null -> Text(err!!, Modifier.padding(16.dp), style = TinType.bodyM, color = c.er, textAlign = TextAlign.Center)
                    else -> Text("Making it…", style = TinType.bodyM, color = Color(0xFF4D5853))
                }
            }
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                TinButton("Share", {
                    token?.let { tk -> runCatching { ctx.startActivity(Intent.createChooser(Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, tk), "Share group invite")) } }
                }, Modifier.weight(1f), style = BtnStyle.Tonal, icon = Icons.Rounded.Share, enabled = token != null)
                CopyButton(token, Modifier.weight(1f))
            }
        }
    }
}
