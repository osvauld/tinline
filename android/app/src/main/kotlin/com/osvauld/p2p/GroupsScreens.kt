package com.osvauld.p2p

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.Chat
import androidx.compose.material.icons.automirrored.rounded.Logout
import androidx.compose.material.icons.rounded.*
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CheckboxDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.layout.layout
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.toggleableState
import androidx.compose.ui.state.ToggleableState
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/*
 * PLACEHOLDER screens for groups (boards Main, AndroidGroupInfo, AndroidRemoval). Backed by
 * [FakeGroupsPrivacy] through [PreviewApp]; reachable only while [PreviewFlags] is on.
 */

private fun first(name: String) = name.trim().substringBefore(' ')
private fun Outcome.reason() = (this as? Outcome.Refused)?.reason

// ------------------------------------------------------------------ chats list

private enum class ChatFilter { All, People, Groups }

private data class ChatEntry(
    val key: String, val group: Boolean, val name: String, val preview: String, val atMs: Long, val unread: Int,
    val dim: Boolean = false, val tint: Int = 0, val previewColor: Boolean = false, val open: PRoute,
)

private fun groupEntry(g: Group): ChatEntry {
    val last = g.items.lastOrNull()
    val preview = when {
        g.removal != null -> "You were removed from this group"
        !g.hostReachable -> "Waiting for a host"
        last is GroupItem.Msg -> (if (last.outgoing) "You" else first(last.authorName)) + ": " + last.text
        last is GroupItem.Sys -> last.text
        else -> ""
    }
    return ChatEntry(g.id, true, g.name, preview, g.lastActivityMs, g.unread, dim = g.removal != null, tint = g.tint, open = PRoute.GroupChat(g.id))
}

/** Chats with groups mixed in, filter chips, chat requests above (boards Main + AndroidChatRequest). */
@Composable
fun PChatsScreen(src: FakeGroupsPrivacy, nav: PNav) {
    val c = Tin.c
    val groups by src.groups.collectAsState()
    val people by src.people.collectAsState()
    val reqs by src.requests.collectAsState()
    val blocked by src.blocked.collectAsState()
    var filter by rememberSaveable { mutableStateOf(ChatFilter.All) }
    var newMenu by remember { mutableStateOf(false) }
    var simMenu by remember { mutableStateOf(false) }
    var searching by remember { mutableStateOf(false) }
    var query by remember { mutableStateOf("") }
    val now = System.currentTimeMillis()
    LaunchedEffect(Unit) { src.expireDue() }

    val rows = remember(groups, people, blocked) {
        groups.map(::groupEntry) + people.map { p ->
            val last = p.items.lastOrNull { it is DmItem.Text }
            val isBlocked = blocked.any { it.did == p.did }
            val preview = when {
                isBlocked -> "Blocked"
                p.blockedMe != null -> "${first(p.name)} blocked you"
                p.lost -> "Can’t reach ${first(p.name)}"
                last is DmItem.Text -> (if (last.outgoing) "You: " else "") + last.text
                else -> p.items.lastOrNull()?.let { (it as? DmItem.Sys)?.text } ?: ""
            }
            ChatEntry(p.did, false, p.name, preview, p.lastActivityMs, p.unread, previewColor = isBlocked || p.blockedMe != null, open = PRoute.Dm(p.did))
        }
    }.sortedByDescending { it.atMs }
        .filter { when (filter) { ChatFilter.All -> true; ChatFilter.People -> !it.group; ChatFilter.Groups -> it.group } }
        .filter { query.isBlank() || it.name.contains(query.trim(), ignoreCase = true) }
    val list = rememberLazyListState()
    val fabWide by remember { derivedStateOf { list.firstVisibleItemIndex == 0 && list.firstVisibleItemScrollOffset == 0 } }

    PreviewPage {
        if (searching) SearchBar(query, { query = it }, onClose = { searching = false; query = "" }, hint = "Search chats")
        else Row(Modifier.fillMaxWidth().heightIn(min = 64.dp).padding(start = 20.dp, end = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            Text("Chats", Modifier.weight(1f).semantics { heading() }, style = TinType.h1, color = c.ink)
            IconBtn(Icons.Rounded.Search, "Search", { searching = true })
            Box {
                IconBtn(Icons.Rounded.MoreVert, "Preview controls", { simMenu = true })
                SimulateMenu(src, nav, simMenu) { simMenu = false }
            }
        }
        Row(Modifier.padding(start = 20.dp, end = 20.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            ChatFilter.entries.forEach { f -> FilterChipPill(f.name, filter == f) { filter = f } }
        }
        Box(Modifier.weight(1f)) {
            LazyColumn(Modifier.fillMaxSize(), state = list, contentPadding = PaddingValues(bottom = 96.dp)) {
                if (filter != ChatFilter.Groups) item { RequestsRow(reqs) { nav.go(PRoute.Requests) } }
                items(rows, key = { (if (it.group) "g" else "p") + it.key }) { r -> ChatListRow(r, now) { nav.go(r.open) } }
            }
            Box(Modifier.align(Alignment.BottomEnd)) {
                ShrinkFab(Icons.Rounded.Add, "New", fabWide) { newMenu = true }
                DropdownMenu(newMenu, { newMenu = false }, containerColor = c.sfRaised, shape = SmoothCornerShape(14.dp)) {
                    DropdownMenuItem({ Text("New group", style = TinType.bodyL, color = c.ink) }, { newMenu = false; nav.go(PRoute.NewGroup) },
                        leadingIcon = { Icon(Icons.Rounded.GroupAdd, null, tint = c.ink2) })
                    DropdownMenuItem({ Text("Add contact", style = TinType.bodyL, color = c.ink) }, { newMenu = false; nav.addContact() },
                        leadingIcon = { Icon(Icons.Rounded.QrCode, null, tint = c.ink2) })
                }
            }
        }
    }
}

@Composable
private fun ChatListRow(r: ChatEntry, now: Long, onClick: () -> Unit) {
    val c = Tin.c
    Row(
        Modifier.fillMaxWidth().heightIn(min = 72.dp).clickable(role = Role.Button, onClick = onClick).padding(horizontal = 20.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        if (r.group) GroupAvatar(r.tint, 48.dp, dim = r.dim, radius = 16.dp) else PersonAvatar(r.name, r.key, 48.dp)
        Column(Modifier.weight(1f)) {
            Row(verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(r.name, Modifier.weight(1f), style = TinType.bodyL.copy(fontWeight = FontWeight.SemiBold), color = if (r.dim) c.ink2 else c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(listTime(r.atMs, now), style = TinType.caption.copy(fontWeight = FontWeight.Normal), color = c.ink2)
            }
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(r.preview, Modifier.weight(1f), style = TinType.bodyM, color = if (r.previewColor) c.er else c.ink2, maxLines = 1, overflow = TextOverflow.Ellipsis)
                if (r.unread > 0) Box(Modifier.defaultMinSize(22.dp, 22.dp).clip(RoundedCornerShape(50)).background(c.pr).padding(horizontal = 6.dp)
                    .semantics { contentDescription = "${r.unread} unread" }, contentAlignment = Alignment.Center) {
                    Text("${r.unread}", style = TinType.caption.copy(fontWeight = FontWeight.Bold), color = c.onPr)
                }
            }
        }
    }
}

/** Overflow menu: stand-ins for what other people do, so every state can be reached. */
@Composable
private fun SimulateMenu(src: FakeGroupsPrivacy, nav: PNav, open: Boolean, onDismiss: () -> Unit) {
    val c = Tin.c
    val people by src.people.collectAsState()
    val reqs by src.requests.collectAsState()
    val groups by src.groups.collectAsState()
    val meera = people.firstOrNull { it.did == FakeGroupsPrivacy.MEERA }
    val leoWaiting = reqs.sent.firstOrNull { it.toDid == FakeGroupsPrivacy.LEO && it.state == SentRequestState.Waiting }
    val hikeUp = groups.firstOrNull { it.id == FakeGroupsPrivacy.HIKE }?.hostReachable == true
    val familyUp = groups.firstOrNull { it.id == FakeGroupsPrivacy.FAMILY }?.hostReachable == true
    @Composable fun item(t: String, f: () -> Unit) = DropdownMenuItem({ Text(t, style = TinType.bodyM, color = c.ink) }, { onDismiss(); f() })
    DropdownMenu(open, onDismiss, containerColor = c.sfRaised, shape = SmoothCornerShape(14.dp)) {
        Text("SIMULATE (PLACEHOLDER)", Modifier.padding(horizontal = 16.dp, vertical = 8.dp), style = TinType.caption, color = c.ink2)
        if (leoWaiting != null) {
            item("Leo accepts your request") { src.simulateRequestAnswered(leoWaiting.id, true); nav.say("Leo accepted: new messages-only chat") }
            item("Leo declines your request") { src.simulateRequestAnswered(leoWaiting.id, false) }
        }
        item("Priya sends a chat request") { src.simulateIncomingRequest() }
        if (meera != null) item(if (meera.blockedMe != null) "Meera unblocks you" else "Meera blocks you") { src.simulateBlockedMe(meera.did, meera.blockedMe == null) }
        if (src.isBlocked(FakeGroupsPrivacy.ARJUN)) item("Arjun tries to message you") { src.simulateBlockedAttempt(FakeGroupsPrivacy.ARJUN) }
        if (people.any { it.did == FakeGroupsPrivacy.PRIYA }) item("Priya asks to call you") { src.simulateCallAsk(FakeGroupsPrivacy.PRIYA) }
        item(if (hikeUp) "Weekend hike hosts go offline" else "Weekend hike hosts come back") { src.simulateHostsOnline(FakeGroupsPrivacy.HIKE, !hikeUp) }
        item(if (familyUp) "Family hosts go offline" else "Family hosts come back") { src.simulateHostsOnline(FakeGroupsPrivacy.FAMILY, !familyUp) }
        if (people.any { it.did == FakeGroupsPrivacy.ARJUN && !it.lost }) item("Lose contact with Arjun") { src.simulateLost(FakeGroupsPrivacy.ARJUN) }
        item("Reset sample data") { src.reset(); nav.say("Sample data reset") }
        item("Leave preview") { nav.exit() }
    }
}

// ------------------------------------------------------------------ group chat

/** Group chat, removed person's view, group's view of a removal, "Waiting for a host" (boards Main, AndroidRemoval). */
@Composable
fun PGroupChatScreen(src: FakeGroupsPrivacy, nav: PNav, id: String) {
    val c = Tin.c
    val groups by src.groups.collectAsState()
    val blocked by src.blocked.collectAsState()
    val g = groups.firstOrNull { it.id == id } ?: run { LaunchedEffect(Unit) { nav.back() }; return }
    var draft by rememberSaveable { mutableStateOf("") }
    val expanded = remember { mutableStateListOf<String>() }
    var flagsOf by remember { mutableStateOf<GroupMember?>(null) }
    val list = rememberLazyListState()
    LaunchedEffect(id) { src.markGroupRead(id) }
    LaunchedEffect(g.items.size) { if (g.items.isNotEmpty()) list.scrollToItem(g.items.lastIndex) }
    val removal = g.removal
    val waiting = removal == null && !g.hostReachable
    val hostCount = g.activeHosts.size
    PreviewPage {
        ChatHeader(
            g.name, nav.back,
            sub = when {
                removal != null -> "You’re no longer a member"
                waiting -> "Waiting for a host"
                else -> "${g.activeMembers.size} members · " + if (hostCount == 1) "hosted by 1 desktop" else "hosted by $hostCount desktops"
            },
            subColor = if (waiting) c.threadText else c.ink2,
            avatar = { GroupAvatar(g.tint, 40.dp, dim = removal != null, radius = 12.dp) },
            onTitle = if (removal == null) ({ nav.go(PRoute.GroupInfo(id)) }) else null,
        ) {
            if (removal == null) IconBtn(Icons.Rounded.Info, "Group info", { nav.go(PRoute.GroupInfo(id)) })
        }
        if (waiting) InfoCard(
            if (hostCount == 0) "This group has no host yet. Your messages are saved and will send when a member’s desktop hosts it."
            else if (hostCount == 1) "The host desktop is offline. Your messages are saved and will send when it comes back."
            else "${if (hostCount == 2) "Both" else "All"} host desktops are offline. Your messages are saved and will send when one comes back.",
            Modifier.padding(horizontal = 16.dp, vertical = 4.dp), icon = Icons.Rounded.CloudOff, kind = BannerKind.Warn,
        )
        LazyColumn(Modifier.weight(1f).fillMaxWidth(), state = list, contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            items(g.items, key = { it.id }) { item ->
                Column(Modifier.fillMaxWidth()) {
                    when (item) {
                        is GroupItem.Sys -> {
                            // Admins can open a removal line to see who flagged.
                            val removedMember = g.members.firstOrNull { m -> m.removed && !m.isMe && item.text.startsWith(first(m.name) + " was removed") }
                            SysLine(item.text, onClick = if (removedMember != null && g.iAmAdmin) ({ flagsOf = removedMember }) else null)
                            if (removal != null && item.atMs == removal.atMs) RemovedCard(removal) { nav.go(PRoute.RemovalDetails(id)) }
                        }
                        is GroupItem.Msg -> {
                            val authorBlocked = !item.outgoing && blocked.any { it.did == item.authorDid }
                            val authorRemoved = g.members.firstOrNull { it.did == item.authorDid }?.removed == true && !item.outgoing
                            if (authorBlocked && item.id !in expanded) CollapsedMessage(item.authorName) { expanded.add(item.id) }
                            else Bubble(
                                item.text, item.outgoing,
                                author = if (item.outgoing) null else item.authorName + if (authorRemoved) " · removed" else "",
                                authorColor = authorColor(item.authorDid),
                                tick = when {
                                    item.writtenBeforeAuthorRemoved -> "Written before ${item.authorName} was removed"
                                    item.delivery == GroupDelivery.WaitingForHost -> "Waiting for a host"
                                    item.delivery == GroupDelivery.WrittenBeforeRemoval ->
                                        "Written offline before removal · sending until ${whenDate(removal?.hostsRefreshAtMs ?: item.atMs)}"
                                    else -> null
                                },
                                faded = item.delivery == GroupDelivery.WrittenBeforeRemoval,
                            )
                        }
                    }
                }
            }
        }
        when {
            removal != null && !removal.archived -> BlockedFooter("You can’t send messages to this group.") {
                TinButton("Delete group", { src.deleteGroup(id); nav.back() }, Modifier.weight(1f), style = BtnStyle.Outlined, height = 48.dp)
                TinButton("Keep as archive", { src.keepAsArchive(id) }, Modifier.weight(1f), style = BtnStyle.Text, height = 48.dp)
            }
            removal != null -> BlockedFooter("You can’t send messages to this group. Kept as an archive.")
            else -> Composer(draft, { draft = it }, "Message the group", onSend = {
                src.sendGroupText(id, draft).reason()?.let(nav.say); draft = ""
            }, onAttach = { nav.say("Attachments aren’t part of the preview") })
        }
    }
    flagsOf?.let { m ->
        TinDialog("Why ${first(m.name)} was removed", { flagsOf = null }, "OK", { flagsOf = null }, dismiss = null) {
            m.flags.forEach { f -> DialogText("${f.byName} · ${f.byRole.name.lowercase()} · ${whenFull(f.atMs)}") }
        }
    }
}

@Composable
private fun ColumnScope.CollapsedMessage(author: String, onShow: () -> Unit) {
    val c = Tin.c
    Row(
        Modifier.align(Alignment.Start).heightIn(min = 48.dp).clip(SmoothCornerShape(18.dp)).border(1.dp, c.ln, SmoothCornerShape(18.dp))
            .clickable(role = Role.Button, onClickLabel = "Show message", onClick = onShow).padding(horizontal = 12.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Icon(Icons.Rounded.Block, null, tint = c.ink2, modifier = Modifier.size(18.dp))
        Text("Message from ${first(author)}, who you blocked · Show", style = TinType.bodyM, color = c.ink2)
    }
}

@Composable
private fun RemovedCard(r: Removal, onDetails: () -> Unit) {
    val c = Tin.c
    NoticeCard(c.erc, c.onErc, "Removed after ${r.flags.size} admin flags",
        "Flagged by ${r.flags.map { it.byName }.distinct().let { if (it.size > 1) it.dropLast(1).joinToString(", ") + " and " + it.last() else it.joinToString() }}. " +
            "You can still read what you received before; nothing new will arrive.") {
        Text("See details", Modifier.heightIn(min = 48.dp).clickable(role = Role.Button, onClick = onDetails).wrapContentHeight(Alignment.CenterVertically),
            style = TinType.label.copy(fontWeight = FontWeight.Bold, textDecoration = androidx.compose.ui.text.style.TextDecoration.Underline), color = c.onErc)
    }
}

// ------------------------------------------------------------------ new group

/** New group from a phone, with the "needs a host" card (board Main). */
@Composable
fun PNewGroupScreen(src: FakeGroupsPrivacy, nav: PNav) {
    val c = Tin.c
    val people by src.people.collectAsState()
    val blocked by src.blocked.collectAsState()
    // Only contacts can be added directly; not people I blocked or who blocked me.
    val pickable = people.filter { p -> p.blockedMe == null && blocked.none { it.did == p.did } }
    var name by rememberSaveable { mutableStateOf("") }
    val picked = remember { mutableStateListOf<String>() }
    var host by rememberSaveable { mutableStateOf<Boolean?>(null) } // true = my desktop, false = ask later
    PreviewPage {
        Row(Modifier.fillMaxWidth().heightIn(min = 64.dp).padding(start = 4.dp, end = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            IconBtn(Icons.Rounded.Close, "Close", nav.back)
            Text("New group", style = TinType.titleM.copy(fontSize = 20.sp), color = c.ink)
        }
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(horizontal = 20.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            TinField(name, { name = it }, "Group name")
            Text("ADD PEOPLE · ${picked.size} SELECTED", Modifier.padding(top = 4.dp), style = TinType.caption.copy(fontSize = 13.sp, fontWeight = FontWeight.SemiBold, letterSpacing = 0.04.sp), color = c.ink2)
            if (pickable.isEmpty()) Hint("No contacts to add yet.")
            pickable.forEach { p ->
                val on = p.did in picked
                Row(
                    Modifier.fillMaxWidth().heightIn(min = 56.dp).clip(SmoothCornerShape(12.dp))
                        .clickable(role = Role.Checkbox, onClick = { if (on) picked.remove(p.did) else picked.add(p.did) })
                        .semantics(mergeDescendants = true) { toggleableState = ToggleableState(on) },
                    verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp),
                ) {
                    PersonAvatar(p.name, p.did, 40.dp)
                    Text(p.name, Modifier.weight(1f), style = TinType.bodyL.copy(fontSize = 15.sp), color = c.ink)
                    Checkbox(on, null, colors = CheckboxDefaults.colors(checkedColor = c.pr, uncheckedColor = c.ln2, checkmarkColor = c.onPr))
                }
            }
            Column(Modifier.fillMaxWidth().clip(SmoothCornerShape(20.dp)).background(c.warn).border(1.dp, c.thread.copy(alpha = 0.45f), SmoothCornerShape(20.dp)).padding(16.dp),
                verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Text(when (host) { true -> "Your desktop will host"; false -> "You’ll ask a member to host"; null -> "This group needs a host" },
                    style = TinType.bodyL.copy(fontSize = 15.sp, fontWeight = FontWeight.Bold), color = c.onWarn, modifier = Modifier.semantics { heading() })
                Text(
                    if (host == false) "Until a member’s desktop hosts it, messages wait (“Waiting for a host”). Ask from Group info › Hosts after the group is made."
                    else "A host is a desktop that keeps the group running when phones sleep. Your phone can’t host. Link your desktop, or ask a member to host from theirs after the group is made.",
                    style = TinType.bodyM, color = c.onWarn,
                )
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    HostChoice("Use my desktop", host == true) { host = true }
                    HostChoice("Ask later", host == false) { host = false }
                }
            }
            Hint("You become the owner. Each person gets an invite in your 1:1 chat.")
        }
        Box(Modifier.padding(start = 20.dp, end = 20.dp, bottom = 16.dp)) {
            TinButton("Create group", {
                val gid = src.createGroup(name, picked.toList(), useMyDesktop = host == true)
                nav.replace(PRoute.GroupChat(gid))
            }, enabled = name.isNotBlank())
        }
    }
}

@Composable
private fun HostChoice(text: String, on: Boolean, onClick: () -> Unit) {
    val c = Tin.c
    Row(
        Modifier.heightIn(min = 48.dp).clip(RoundedCornerShape(50)).background(if (on) c.onWarn.copy(alpha = 0.12f) else androidx.compose.ui.graphics.Color.Transparent)
            .border(1.dp, if (on) c.onWarn else c.ln2, RoundedCornerShape(50))
            .clickable(role = Role.RadioButton, onClick = onClick).semantics { selected = on }.padding(horizontal = 16.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        if (on) Icon(Icons.Rounded.Check, null, tint = c.onWarn, modifier = Modifier.size(18.dp))
        Text(text, style = TinType.label, color = c.onWarn)
    }
}

// ------------------------------------------------------------------ group info + member sheet + flag

/** Group info: hosts first, members with role pills; tap a member for actions (board AndroidGroupInfo). */
@Composable
fun PGroupInfoScreen(src: FakeGroupsPrivacy, nav: PNav, id: String) {
    val c = Tin.c
    val groups by src.groups.collectAsState()
    val g = groups.firstOrNull { it.id == id } ?: run { LaunchedEffect(Unit) { nav.back() }; return }
    var sheetFor by remember { mutableStateOf<String?>(null) }
    var leaving by remember { mutableStateOf(false) }
    val now = System.currentTimeMillis()
    val me = g.me
    PreviewPage {
        TopBar(null, nav.back) {
            if (g.iAmAdmin) IconBtn(Icons.Rounded.Edit, "Edit group", { nav.say("Editing name, photo and who can invite isn’t in the preview") })
        }
        LazyColumn(Modifier.weight(1f)) {
            item {
                Column(Modifier.fillMaxWidth().padding(horizontal = 24.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    GroupAvatar(g.tint, 80.dp, radius = 24.dp)
                    Text(g.name, style = TinType.h1.copy(fontSize = 26.sp), color = c.ink, textAlign = TextAlign.Center, modifier = Modifier.semantics { heading() })
                    val role = when { me == null -> ""; GroupRole.Owner in me.roles -> " · you are the owner"; GroupRole.Admin in me.roles -> " · you are an admin"; else -> "" }
                    Text("${g.activeMembers.size} members$role", style = TinType.bodyM, color = c.ink2)
                    Row(Modifier.padding(top = 8.dp), horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.CenterHorizontally)) {
                        InfoAction(Icons.Rounded.PersonAdd, "Invite", c.pr) { nav.say("Inviting from here isn’t in the preview: use New group") }
                        InfoAction(if (g.muted) Icons.Rounded.NotificationsOff else Icons.Rounded.Notifications, if (g.muted) "Unmute" else "Mute", c.pr) { src.setMuted(id, !g.muted) }
                        InfoAction(Icons.AutoMirrored.Rounded.Logout, "Leave", c.er) { leaving = true }
                    }
                }
            }
            item { SectionLabel("HOSTS · KEEP THE GROUP RUNNING") }
            if (g.hosts.isEmpty()) item { Hint("No host yet. Messages wait until a member’s desktop hosts the group.", Modifier.padding(horizontal = 20.dp)) }
            items(g.hosts, key = { it.id }) { h ->
                Row(Modifier.fillMaxWidth().heightIn(min = 72.dp).padding(start = 20.dp, end = 8.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp)) {
                    Dot(if (h.online) c.pr else c.ln2, 8.dp)
                    Column(Modifier.weight(1f)) {
                        Text(h.label, style = TinType.bodyL.copy(fontWeight = FontWeight.SemiBold), color = c.ink)
                        Text(hostStatus(h, now, info = true), style = TinType.bodyM, color = c.ink2)
                    }
                    IconBtn(Icons.Rounded.MoreVert, "Host options", { nav.go(PRoute.Hosts(id)) })
                }
            }
            item { SectionLabel("MEMBERS · ${g.activeMembers.size}") }
            items(g.activeMembers.sortedBy { m -> if (GroupRole.Owner in m.roles) 0 else if (m.isAdmin) 1 else 2 }, key = { it.did }) { m ->
                Row(
                    Modifier.fillMaxWidth().heightIn(min = 64.dp).let { if (!m.isMe) it.clickable(role = Role.Button, onClickLabel = "Member actions", onClick = { sheetFor = m.did }) else it }
                        .padding(horizontal = 20.dp, vertical = 6.dp),
                    verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    PersonAvatar(m.name, m.did, 40.dp, contact = src.people.value.any { it.did == m.did }, isMe = m.isMe)
                    Column(Modifier.weight(1f).padding(start = 4.dp)) {
                        Text(if (m.isMe) "You" else m.name, style = TinType.bodyL.copy(fontWeight = FontWeight.SemiBold), color = c.ink)
                        // Admins see flag counts; other members don't.
                        if (g.iAmAdmin && m.flags.isNotEmpty()) Text(if (m.flags.size == 1) "1 flag" else "${m.flags.size} flags", style = TinType.bodyM, color = c.ink2)
                    }
                    listOf(GroupRole.Owner, GroupRole.Admin, GroupRole.Host).filter { it in m.roles }.forEach { RolePill(it) }
                }
            }
        }
    }
    sheetFor?.let { did -> g.members.firstOrNull { it.did == did }?.let { m -> MemberSheet(src, nav, g, m) { sheetFor = null } } }
    if (leaving) TinDialog("Leave ${g.name}?", { leaving = false }, "Leave", { leaving = false; src.leaveGroup(id); nav.replace(PRoute.Chats) }, destructive = true, icon = Icons.AutoMirrored.Rounded.Logout) {
        DialogText("You stop getting its messages. To come back, an admin has to invite you again.")
    }
}

private fun hostStatus(h: GroupHost, now: Long, info: Boolean): String = when {
    h.pending -> "Asked ${h.label.substringBefore('’')} · waiting to accept"
    h.online -> if (info) "Online · relaying now" else (h.appointedBy?.let { "Appointed by $it · online" } ?: "Owner’s host · online")
    else -> {
        val off = h.offlineSinceMs?.let { durationWords(now - it) }
        if (info) "Offline${off?.let { " for $it" } ?: ""} · others carry on"
        else (h.appointedBy?.let { "Appointed by $it" } ?: "Owner’s host") + " · offline${off?.let { " $it" } ?: ""}"
    }
}

@Composable
private fun InfoAction(icon: androidx.compose.ui.graphics.vector.ImageVector, label: String, tint: androidx.compose.ui.graphics.Color, onClick: () -> Unit) {
    val c = Tin.c
    Column(
        Modifier.width(88.dp).clip(SmoothCornerShape(16.dp)).clickable(role = Role.Button, onClick = onClick).padding(vertical = 4.dp),
        horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Box(Modifier.size(52.dp).clip(CircleShape).background(c.sf2), contentAlignment = Alignment.Center) { Icon(icon, null, tint = tint) }
        Text(label, style = TinType.caption.copy(fontSize = 13.sp), color = c.ink)
    }
}

/** Member actions bottom sheet: everyone sees Message privately and Block; admins also Make admin / host and Flag. */
@Composable
private fun MemberSheet(src: FakeGroupsPrivacy, nav: PNav, g: Group, m: GroupMember, onDismiss: () -> Unit) {
    val c = Tin.c
    val people by src.people.collectAsState()
    val blocked by src.blocked.collectAsState()
    var flagging by remember { mutableStateOf(false) }
    var asking by remember { mutableStateOf(false) }
    var blocking by remember { mutableStateOf(false) }
    val fn = first(m.name)
    val isContact = people.any { it.did == m.did }
    val isBlocked = blocked.any { it.did == m.did }
    val admin = g.iAmAdmin
    if (!flagging && !asking && !blocking) TinSheet(onDismiss) {
        Row(Modifier.padding(vertical = 12.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp)) {
            PersonAvatar(m.name, m.did, 56.dp, contact = isContact)
            Column {
                Text(m.name, style = TinType.titleL.copy(fontSize = 20.sp), color = c.ink)
                val parts = listOfNotNull("Member since ${whenDate(m.memberSinceMs)}", m.invitedBy?.let { "invited by ${first(it)}" },
                    if (admin) "${m.flags.size} of ${FakeGroupsPrivacy.FLAGS_TO_REMOVE} flags" else null)
                Text(parts.joinToString(" · "), style = TinType.bodyM, color = c.ink2)
            }
        }
        SheetItemFlush(Icons.AutoMirrored.Rounded.Chat, "Message privately", if (isContact) "Opens your chat with $fn" else "Sends $fn a chat request") {
            if (isContact) { onDismiss(); nav.go(PRoute.Dm(m.did)) } else asking = true
        }
        if (admin && !m.isAdmin) SheetItemFlush(Icons.Rounded.AdminPanelSettings, "Make admin") {
            src.makeAdmin(g.id, m.did).reason()?.let(nav.say); onDismiss()
        }
        if (admin && g.hosts.none { it.ownerDid == m.did }) SheetItemFlush(Icons.Rounded.Computer, "Make host", "Only for one of $fn’s desktops") {
            src.askToHost(g.id, m.did).reason()?.let(nav.say) ?: nav.say("Asked $fn to host from a desktop"); onDismiss()
        }
        if (isBlocked) SheetItemFlush(Icons.Rounded.Block, "Unblock $fn", "They can message you again") { src.unblock(m.did); onDismiss() }
        else SheetItemFlush(Icons.Rounded.Block, "Block $fn", "Just for you. $fn stays in the group") { blocking = true }
        if (admin) SheetItemFlush(Icons.Rounded.Flag, "Flag to remove", "${FakeGroupsPrivacy.FLAGS_TO_REMOVE} flags remove $fn from the group", c.er) { flagging = true }
    }
    if (flagging) FlagDialog(m, onDismiss = { flagging = false; onDismiss() }) {
        val removedNow = m.flags.size + 1 >= FakeGroupsPrivacy.FLAGS_TO_REMOVE
        src.flagMember(g.id, m.did).reason()?.let(nav.say) ?: nav.say(if (removedNow) "$fn was removed from ${g.name}" else "Flag ${m.flags.size + 1} of ${FakeGroupsPrivacy.FLAGS_TO_REMOVE}")
        flagging = false; onDismiss()
    }
    if (asking) AskPrivateChatSheet(src.myName, m.name, g.name, onDismiss = { asking = false; onDismiss() }) { note ->
        src.sendRequest(m.did, m.name, g.name, note).reason()?.let(nav.say) ?: nav.say("Request sent to $fn")
        asking = false; onDismiss()
    }
    if (blocking) BlockDialog(m.name, sharedGroups = src.groups.value.filter { gr -> gr.removal == null && gr.activeMembers.any { it.did == m.did } }.map { it.name },
        onDismiss = { blocking = false; onDismiss() }) {
        src.block(m.did, m.name).reason()?.let(nav.say); blocking = false; onDismiss()
    }
}

/** [SheetItem] without the sheet's own 24 dp side padding doubling up. */
@Composable
private fun SheetItemFlush(icon: androidx.compose.ui.graphics.vector.ImageVector, title: String, sub: String? = null, color: androidx.compose.ui.graphics.Color = Tin.c.ink, onClick: () -> Unit) {
    Box(Modifier.layout { measurable, constraints ->
        // Stretch 24 dp past the sheet's padding on both sides so the ripple spans the sheet like the board's .mi rows.
        val extra = 24.dp.roundToPx()
        val p = measurable.measure(constraints.copy(minWidth = constraints.maxWidth + 2 * extra, maxWidth = constraints.maxWidth + 2 * extra))
        layout(constraints.maxWidth, p.height) { p.place(-extra, 0) }
    }) { SheetItem(icon, title, sub, color, onClick) }
}

/** Flag dialog with the three-segment meter (board AndroidGroupInfo). */
@Composable
fun FlagDialog(m: GroupMember, onDismiss: () -> Unit, onFlag: () -> Unit) {
    val fn = first(m.name)
    val n = m.flags.size + 1
    val total = FakeGroupsPrivacy.FLAGS_TO_REMOVE
    TinDialog("Flag $fn for removal?", onDismiss, "Flag", onFlag, destructive = true, icon = Icons.Rounded.Flag) {
        FlagMeter(n.coerceAtMost(total), total)
        DialogText(
            if (n >= total) "Yours will be flag $total of $total. $fn is removed right away: new messages from $fn are refused and $fn is told about the removal."
            else "Yours will be flag $n of $total. At $total flags $fn is removed right away: new messages from $fn are refused and $fn is told about the removal."
        )
        DialogText("Flags can’t be taken back. $fn can see who flagged.")
    }
}

// ------------------------------------------------------------------ hosts

/** Hosts list for the owner and admins; others see it read-only (board AndroidGroupInfo). */
@Composable
fun PHostsScreen(src: FakeGroupsPrivacy, nav: PNav, id: String) {
    val c = Tin.c
    val groups by src.groups.collectAsState()
    val g = groups.firstOrNull { it.id == id } ?: run { LaunchedEffect(Unit) { nav.back() }; return }
    var removing by remember { mutableStateOf<GroupHost?>(null) }
    var adding by remember { mutableStateOf(false) }
    val now = System.currentTimeMillis()
    PreviewPage {
        TopBar("Hosts", nav.back)
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(horizontal = 20.dp, vertical = 4.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
            Text("Hosts are members’ desktops that store the whole group and pass messages on when phones are asleep. They are trusted: they can read the group and see who is online.",
                style = TinType.bodyM, color = c.ink2)
            if (g.hosts.isNotEmpty()) CardBox(Modifier.fillMaxWidth(), radius = 20.dp) {
                g.hosts.forEachIndexed { i, h ->
                    if (i > 0) Box(Modifier.fillMaxWidth().height(1.dp).background(c.ln))
                    Row(Modifier.fillMaxWidth().heightIn(min = 72.dp).padding(start = 20.dp, end = 8.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp)) {
                        Dot(if (h.online) c.pr else c.ln2, 8.dp)
                        Column(Modifier.weight(1f)) {
                            Text(h.label, style = TinType.bodyL.copy(fontWeight = FontWeight.SemiBold), color = c.ink)
                            Text(hostStatus(h, now, info = false), style = TinType.bodyM, color = c.ink2)
                        }
                        if (g.iAmAdmin) Box(Modifier.heightIn(min = 48.dp).clip(RoundedCornerShape(50)).clickable(role = Role.Button, onClickLabel = "Remove ${h.label}", onClick = { removing = h })
                            .padding(horizontal = 12.dp), contentAlignment = Alignment.Center) {
                            Text("Remove", style = TinType.button.copy(fontSize = 15.sp), color = c.er)
                        }
                    }
                }
            } else Hint("No hosts yet.")
            if (g.iAmAdmin) TinButton("Add a host", { adding = true }, style = BtnStyle.Outlined, icon = Icons.Rounded.Add)
            InfoCard("changes every member’s group address within 2 days, so the removed desktop can’t reach the group again. Messages it already stored stay with it.",
                kind = BannerKind.Warn, bold = "Removing a host")
            Hint("Two or more hosts keep the group working when one is off. Phones can’t host.")
        }
    }
    removing?.let { h ->
        TinDialog("Remove ${h.label}?", { removing = null }, "Remove", {
            src.removeHost(id, h.id).reason()?.let(nav.say); removing = null
        }, destructive = true, icon = Icons.Rounded.Computer) {
            DialogText("Every member’s group address changes within 2 days, so ${h.label} can’t reach the group again. Messages it already stored stay with it.")
        }
    }
    if (adding) {
        val candidates = g.activeMembers.filter { m -> !m.isMe && g.hosts.none { it.ownerDid == m.did } }
        TinSheet({ adding = false }) {
            Text("Add a host", style = TinType.titleL.copy(fontSize = 20.sp), color = c.ink, modifier = Modifier.padding(vertical = 12.dp))
            Hint("Pick a member. They’re asked to host from one of their linked desktops and must accept.")
            Gap(h = 8.dp)
            if (candidates.isEmpty()) Hint("Every member already hosts.")
            candidates.forEach { m ->
                SheetItemFlush(Icons.Rounded.Computer, "${first(m.name)}’s desktop", "Ask ${first(m.name)} to host") {
                    src.askToHost(id, m.did).reason()?.let(nav.say) ?: nav.say("Asked ${first(m.name)} to host"); adding = false
                }
            }
        }
    }
}

// ------------------------------------------------------------------ removal details

/** "Why you were removed": each signed flag, what continues and what stops (board AndroidRemoval). */
@Composable
fun PRemovalDetailsScreen(src: FakeGroupsPrivacy, nav: PNav, id: String) {
    val c = Tin.c
    val groups by src.groups.collectAsState()
    val people by src.people.collectAsState()
    val g = groups.firstOrNull { it.id == id }
    val r = g?.removal ?: run { LaunchedEffect(Unit) { nav.back() }; return }
    // A private message is offered only to a flagger who is already a contact.
    val contactFlagger = r.flags.firstOrNull { f -> people.any { it.did == f.byDid && it.blockedMe == null } }
    PreviewPage {
        TopBar("Why you were removed", nav.back)
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(horizontal = 20.dp, vertical = 4.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
            CardBox(Modifier.fillMaxWidth(), radius = 20.dp) {
                r.flags.forEachIndexed { i, f ->
                    Row(Modifier.fillMaxWidth().heightIn(min = 64.dp).padding(horizontal = 16.dp, vertical = 6.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp)) {
                        PersonAvatar(f.byName, f.byDid, 36.dp, contact = people.any { it.did == f.byDid })
                        Column(Modifier.weight(1f)) {
                            Text("${f.byName} · ${f.byRole.name.lowercase()}", style = TinType.bodyL.copy(fontSize = 15.sp, fontWeight = FontWeight.SemiBold), color = c.ink)
                            Hint("Flagged ${whenFull(f.atMs)}" + if (i == FakeGroupsPrivacy.FLAGS_TO_REMOVE - 1) " · 3rd flag" else "")
                        }
                    }
                }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                Icon(Icons.Rounded.VerifiedUser, null, tint = c.ink2, modifier = Modifier.size(22.dp))
                Text("Each flag is signed by the admin who made it and was checked by your phone. Nobody else could have added them.", style = TinType.bodyM, color = c.ink2)
            }
            Text("What happens now", style = TinType.bodyL.copy(fontSize = 15.sp, fontWeight = FontWeight.Bold), color = c.ink, modifier = Modifier.padding(top = 6.dp).semantics { heading() })
            Text("You keep the messages already on your phone. The group’s hosts stop accepting your new messages and calls now, and refresh their addresses on ${whenDate(r.hostsRefreshAtMs)}, after which your phone can’t reach them at all.",
                style = TinType.bodyM.copy(lineHeight = 21.sp), color = c.ink2)
            Text("To rejoin, an admin has to invite you again.", style = TinType.bodyM.copy(lineHeight = 21.sp), color = c.ink2)
        }
        if (contactFlagger != null) Box(Modifier.padding(start = 20.dp, end = 20.dp, bottom = 16.dp)) {
            TinButton("Message ${first(contactFlagger.byName)} privately", { nav.go(PRoute.Dm(contactFlagger.byDid)) }, style = BtnStyle.Outlined)
        }
    }
}

// ------------------------------------------------------------------ Home's Groups tab + requests row

/** One compact row for all chat requests ("1 new · 3 sent"); nothing when there are none. */
@Composable
fun RequestsRow(reqs: Requests, onClick: () -> Unit) {
    val c = Tin.c
    val new = reqs.incoming.count { it.state == IncomingRequestState.Pending }
    val sent = reqs.sent.size
    if (new == 0 && sent == 0) return
    Row(
        Modifier.fillMaxWidth().heightIn(min = 56.dp).clickable(role = Role.Button, onClick = onClick).padding(start = 20.dp, end = 12.dp, top = 6.dp, bottom = 6.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        Box(Modifier.size(40.dp).clip(CircleShape).background(if (new > 0) c.prc else c.sf2), contentAlignment = Alignment.Center) {
            Icon(Icons.Rounded.PersonAddAlt, null, tint = if (new > 0) c.onPrc else c.ink2)
        }
        Text(
            listOfNotNull("Chat requests", if (new > 0) "$new new" else null, if (sent > 0) "$sent sent" else null).joinToString(" · "),
            Modifier.weight(1f), style = TinType.bodyL.copy(fontWeight = if (new > 0) FontWeight.SemiBold else FontWeight.Normal), color = c.ink,
            maxLines = 1, overflow = TextOverflow.Ellipsis,
        )
        Icon(Icons.Rounded.ChevronRight, null, tint = c.ink2)
    }
}

