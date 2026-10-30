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
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
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
/** "Invited by {name}" for a contact, "...by you" for me, a whole-sentence variant for someone outside my contacts. */
@Composable
private fun personLine(app: P2pApp, did: String, @androidx.annotation.StringRes named: Int, @androidx.annotation.StringRes you: Int, @androidx.annotation.StringRes unknown: Int): String = when {
    did == app.groups.myDid -> stringResource(you)
    else -> app.contacts.value.firstOrNull { it.did == did }?.let { stringResource(named, it.display()) } ?: stringResource(unknown)
}

private fun nameOf(app: P2pApp, did: String, fallback: String): String {
    // Per row while composing: the cached DID, never a core call on the main thread.
    if (did == app.groups.myDid) return appString(R.string.common_you)
    app.contacts.value.firstOrNull { it.did == did }?.let { return it.display() }
    return fallback.ifBlank { appString(R.string.common_someone) }
}

/** Runs a group action off the main thread, then hands its outcome back on it. */
@Composable
private fun rememberRunner(): (suspend () -> Result<String>, (Result<String>) -> Unit) -> Unit {
    val scope = rememberCoroutineScope()
    return remember(scope) { { work, done -> scope.launch { done(withContext(Dispatchers.IO) { work() }) } } }
}

/** Permission state for one hosting device. */
private fun hostingStatus(d: GroupDetail, did: String, dev: String): String {
    fun has(hosts: List<uniffi.p2pcore.GroupHost>) = hosts.any { it.did == did && it.deviceId == dev }
    fun state(active: Boolean, pending: Boolean, offered: Boolean) = appString(when {
        active -> R.string.group_hosting_active
        pending -> R.string.group_hosting_waiting_device
        offered -> R.string.group_hosting_waiting_admin
        else -> R.string.group_hosting_not_enabled
    })
    return "${appString(R.string.group_hosting_messages)}: ${state(has(d.hosts), has(d.hostRequests), has(d.hostOffers))} · " +
        "${appString(R.string.group_hosting_calls)}: ${state(has(d.callHosts), has(d.callHostRequests), has(d.callHostOffers))}"
}

/** One permission editor, used by admins and by the device owner giving consent. */
@Composable
private fun HostingSheet(app: P2pApp, d: GroupDetail, did: String, dev: String, local: Boolean,
    exec: (suspend () -> Result<String>, (Result<String>) -> Unit) -> Unit, done: (Result<String>) -> Unit, onDismiss: () -> Unit) {
    fun has(hosts: List<uniffi.p2pcore.GroupHost>) = hosts.any { it.did == did && it.deviceId == dev }
    var messages by remember(d.id, did, dev) { mutableStateOf(has(d.hosts) || has(d.hostRequests) || has(d.hostOffers)) }
    var calls by remember(d.id, did, dev) { mutableStateOf(has(d.callHosts) || has(d.callHostRequests) || has(d.callHostOffers)) }
    var busy by remember { mutableStateOf(false) }
    GSheet(onDismiss, stringResource(R.string.group_hosting_permissions)) { close ->
        GDialogBody(stringResource(R.string.group_hosting_permissions)) {
            GPara("${nameOf(app, did, d.members.firstOrNull { it.did == did }?.name.orEmpty())} · ${GroupText.shortDev(dev)}", top = 4.dp)
            Row(Modifier.fillMaxWidth().clickable(enabled = !busy, role = Role.Checkbox) { messages = !messages }
                .semantics { toggleableState = ToggleableState(messages) }, verticalAlignment = Alignment.CenterVertically) {
                Checkbox(messages, null, enabled = !busy)
                Text(stringResource(R.string.group_hosting_messages), color = Tin.c.ink)
            }
            Row(Modifier.fillMaxWidth().clickable(enabled = !busy, role = Role.Checkbox) { calls = !calls }
                .semantics { toggleableState = ToggleableState(calls) }, verticalAlignment = Alignment.CenterVertically) {
                Checkbox(calls, null, enabled = !busy)
                Text(stringResource(R.string.group_hosting_calls), color = Tin.c.ink)
            }
            GPara(stringResource(if (local) R.string.group_hosting_consent else R.string.group_hosting_admin))
            if (local && calls) GPara(stringResource(R.string.groupsb_host_calls_phone_sub))
            GActs {
                GBtn(stringResource(R.string.common_cancel), { if (!busy) close() }, GBtnKind.Quiet, enabled = !busy)
                GBtn(stringResource(R.string.group_hosting_save), {
                    busy = true
                    exec({ app.groups.configureHosting(d.id, did, dev, local, messages, calls) }) { result ->
                        busy = false; done(result); if (result.isSuccess) close()
                    }
                }, GBtnKind.Primary, enabled = !busy)
            }
        }
    }
}

/** "X asked this phone to host <group>": Host here / Decline (G2 `host_requested_here`). */
@Composable
private fun HostAskCard(app: P2pApp, d: GroupDetail, say: (String) -> Unit) {
    val by = GroupText.hostAsker(d) ?: return
    val exec = rememberRunner()
    val c = Tin.c
    val cant = stringResource(R.string.groups_cant_do)
    val declined = stringResource(R.string.groups_host_declined)
    var review by remember(d.id) { mutableStateOf(false) }
    if (review) HostingSheet(app, d, app.groups.myDid, app.groups.hostingDeviceId(), true, exec,
        { r -> say(r.fold({ it }, { it.message ?: cant })) }, { review = false })
    NoticeCard(c.warn, c.onWarn, stringResource(R.string.groups_host_ask_title, firstName(by), d.name),
        hostingStatus(d, app.groups.myDid, app.groups.hostingDeviceId()),
        Modifier.padding(horizontal = 16.dp)) {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            TinButton(stringResource(R.string.groups_host_here), { review = true },
                Modifier.weight(1f), style = BtnStyle.Filled, height = 44.dp)
            TinButton(stringResource(R.string.common_decline), { exec({ app.groups.configureHosting(d.id, app.groups.myDid, app.groups.hostingDeviceId(), true, d.amHost, d.amCallHost).map { declined } }) { r -> say(r.fold({ it }, { it.message ?: cant })) } },
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
    val ctx = LocalContext.current
    val cant = stringResource(R.string.groups_cant_do)
    val done: (Result<String>) -> Unit = { r -> say(r.fold({ it }, { it.message ?: cant })) }
    LaunchedEffect(Unit) { withContext(Dispatchers.IO) { app.groups.refresh() } }
    // Group calls running in my groups, re-read when the core says something about calls changed.
    // The call UI keeps this current: Announced, Joined, Left and Ended all re-read or drop the group's call, so a
    // call that ended never leaves a Join pill behind.
    val gcs by app.groupCallUi.state.collectAsState()
    LaunchedEffect(active.map { it.id }) { withContext(Dispatchers.IO) { active.forEach { app.groupCallUi.refreshLive(it.id) } } }
    val calls: Map<String, LiveCall> = gcs.live.mapValues { LiveCall(it.value.joined) }.filterKeys { k -> active.any { it.id == k } }
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 96.dp)) {
        item(key = "new") {
            TopRow(GIcons.Plus, stringResource(R.string.groups_list_new_title), stringResource(R.string.groups_list_new_sub)) { creating = true }
        }
        item(key = "join") {
            TopRow(GIcons.Link, stringResource(R.string.groups_list_join_title), stringResource(R.string.groups_list_join_sub), onJoin)
        }
        // ---- invitations to me
        item(key = "inv") {
            GFold(pendingInv.isNotEmpty()) {
                GSec(stringResource(R.string.groups_list_invitations), pendingInv.size)
                Column(Modifier.padding(horizontal = 8.dp)) {
                    pendingInv.forEach { inv ->
                        key(inv.id) {
                            val who = nameOf(app, inv.inviterDid, inv.inviterName)
                            val busy = joining == inv.id
                            ReqCard {
                                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                                    Avatar(who, inv.inviterDid, 40.dp)
                                    Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(1.dp)) {
                                        Text(boldText(stringResource(R.string.groups_list_invited, mark(firstName(who)), mark(inv.groupName))), style = GType.body, color = c.ink)
                                        Text(stringResource(R.string.groups_list_valid_until, dayOnly(inv.expires.toLong())), style = GType.sub, color = c.ink2)
                                    }
                                }
                                GActs(top = 0.dp) {
                                    GBtn(stringResource(R.string.common_decline), { exec({ app.groups.declineInvitation(inv.id) }, done) }, GBtnKind.Quiet, enabled = !busy)
                                    GBtn(if (busy) stringResource(R.string.groups_list_joining) else stringResource(R.string.common_accept), {
                                        joining = inv.id
                                        exec({ app.groups.acceptInvitation(inv.id) }) { r -> joining = null; r.onSuccess { onOpen(it); say(ctx.getString(R.string.groups_list_joined, inv.groupName)) }.onFailure { say(it.message ?: ctx.getString(R.string.groups_list_cant_join)) } }
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
                GSec(stringResource(R.string.groups_list_requests), requestsIn.size.takeIf { it > 0 })
                Column(Modifier.padding(horizontal = 8.dp)) {
                    requestsIn.forEach { r ->
                        key(r.id) {
                            val name = nameOf(app, r.peerDid, r.peerName)
                            ReqCard {
                                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                                    Avatar(name, r.peerDid, 40.dp)
                                    Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(1.dp)) {
                                        Text(boldText(stringResource(R.string.groups_list_wants_chat, mark(name))), style = GType.body, color = c.ink)
                                        Text(stringResource(R.string.groups_list_member_of, r.groupName), style = GType.sub, color = c.ink2)
                                    }
                                }
                                Text(stringResource(R.string.groups_list_accepting_adds, firstName(name)), style = GType.sub, color = c.ink2)
                                GActs(top = 0.dp) {
                                    GBtn(stringResource(R.string.groups_list_block), { exec({ app.groups.blockRequester(r.id, r.peerDid) }, done) }, GBtnKind.DangerQuiet)
                                    GBtn(stringResource(R.string.common_decline), { exec({ app.groups.decline(r.id) }, done) }, GBtnKind.Quiet)
                                    GBtn(stringResource(R.string.common_accept), { exec({ app.groups.accept(r.id) }, done) }, GBtnKind.Primary)
                                }
                            }
                        }
                    }
                }
                if (sent.isNotEmpty()) {
                    GSub2(stringResource(R.string.groups_list_sent_requests))
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
        item(key = "yours") { GSec(stringResource(R.string.groups_list_yours), active.size) }
        item(key = "empty") {
            GFold(active.isEmpty()) {
                Text(stringResource(R.string.groups_list_empty), Modifier.padding(horizontal = 20.dp, vertical = 8.dp), style = GType.note, color = c.ink2)
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
                val expandedText = stringResource(R.string.groups_list_expanded)
                val collapsedText = stringResource(R.string.groups_list_collapsed)
                Box(Modifier.fillMaxWidth().heightIn(min = 44.dp).gTap(label = if (goneOpen) stringResource(R.string.groups_list_fold) else stringResource(R.string.groups_list_unfold)) { goneOpen = !goneOpen }
                    .semantics { stateDescription = if (goneOpen) expandedText else collapsedText }, contentAlignment = Alignment.CenterStart) {
                    GSec(stringResource(R.string.groups_list_left_or_removed), gone.size) {
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
                                        Text((if (s.myRole == CoreRole.REMOVED) stringResource(R.string.groups_list_removed) else stringResource(R.string.groups_list_left)) + (at?.let { " · " + dayOnly(it) } ?: ""),
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
                        call != null -> GCallLine(if (call.joined) stringResource(R.string.groups_list_in_call) else stringResource(R.string.groups_list_call_running))
                        waiting -> Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(3.dp)) {
                            Icon(GIcons.Clock, null, tint = c.goldInk, modifier = Modifier.size(13.dp))
                            Text(stringResource(R.string.groups_waiting_host), style = GType.sub.copy(fontWeight = FontWeight.SemiBold), color = c.goldInk, maxLines = 1)
                        }
                        else -> Text(GroupText.preview(s, me).ifEmpty { stringResource(R.string.groups_list_no_posts) }, style = GType.sub, color = c.ink2, maxLines = 1, overflow = TextOverflow.Ellipsis)
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
    GSheet(onDismiss, stringResource(R.string.groups_list_new_title)) { close ->
        GDialogBody(stringResource(R.string.groups_list_new_title)) {
            Spacer(Modifier.height(10.dp))
            GField(name, { name = it.take(60); err = null }, stringResource(R.string.groups_new_field_label), stringResource(R.string.groups_new_field_hint), error = err)
            GPara(stringResource(R.string.groups_new_note), top = 10.dp)
            GActs {
                GBtn(stringResource(R.string.common_cancel), close, GBtnKind.Quiet)
                GBtn(if (busy) stringResource(R.string.groups_new_creating) else stringResource(R.string.groups_new_create), {
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
    if (o == null) { Page { TopBar(stringResource(R.string.groups_chat_title), onBack) ; Hint(stringResource(R.string.groups_chat_loading), Modifier.padding(24.dp)) }; return }
    val d = o.detail
    val me = app.groups.myDid
    var draft by rememberSaveable(id) { mutableStateOf("") }
    var replyTo by remember(id) { mutableStateOf<String?>(null) }
    var sending by remember { mutableStateOf(false) }
    // The online list (public group §8): open, and whether I hide myself (read before it opens, so its note never pops in).
    var onlineSheet by remember(id) { mutableStateOf<Boolean?>(null) }
    // The post whose picture is open full-screen (the 1:1 viewer).
    var viewer by remember(id) { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    val list = rememberLazyListState()
    val cantSend = stringResource(R.string.groups_chat_cant_send)
    val cantReadPicture = stringResource(R.string.groups_chat_cant_read_picture)
    val noPicker = stringResource(R.string.groups_chat_no_picker)
    val voiceFailed = stringResource(R.string.conv_voice_send_failed)
    // The thread: posts and this group's call log (MessageRow `.callrec`) merged by time, oldest first.
    val rows = remember(o.messages, groupLog, id) {
        val calls = groupLog.filter { it.groupId == id }.map { ThreadRow.Call(it) }
        (o.messages.mapIndexed { i, m -> ThreadRow.Post(i, m) } + calls).sortedBy { it.at }.let { all ->
            // Calls go before a post of the same second; posts keep their own order.
            all.sortedWith(compareBy<ThreadRow> { it.at }.thenBy { if (it is ThreadRow.Call) 0 else 1 }.thenBy { (it as? ThreadRow.Post)?.i ?: 0 })
        }
    }
    // The thread is a reversed list (newest = item 0, at the bottom), as in a 1:1 chat: the keyboard and a growing composer
    // shorten it from below without hiding the newest post, earlier posts load in above without moving anything, and a new
    // post is followed when it is mine or I'm reading the bottom (ChatFollow.kt).
    val newestRow = rows.lastOrNull()
    FollowNewest(list, newestRow?.key, mine = when (newestRow) { is ThreadRow.Post -> newestRow.m.outgoing; is ThreadRow.Call -> newestRow.r.startedBy == me; null -> false })
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
                }.getOrNull() ?: return@withContext Result.failure(GroupFailure(cantReadPicture))
                app.groups.postImage(id, bytes, caption).map { "" }
            }
        }) { res ->
            sending = false
            res.onFailure { if (draft.isEmpty()) draft = caption; say(it.message ?: cantSend) }
        }
    }
    // Group files: the attach sheet's File picks any document (OpenDocument); it is copied into the app's cache, posted (the
    // core seals it from there, keeping its own copy) and the copy is deleted. The draft goes with it as its caption.
    var attachOpen by remember(id) { mutableStateOf(false) }
    val cantReadFile = stringResource(R.string.conv_read_file_failed)
    val noFilePicker = stringResource(R.string.groups_file_no_picker)
    val filePicker = androidx.activity.compose.rememberLauncherForActivityResult(androidx.activity.result.contract.ActivityResultContracts.OpenDocument()) { uri ->
        if (uri == null || sending) return@rememberLauncherForActivityResult
        val caption = draft
        draft = ""; replyTo = null; sending = true
        exec({
            withContext(Dispatchers.IO) {
                val picked = runCatching { ChatMedia.copyIn(ctx, uri) }.getOrNull() ?: return@withContext Result.failure(GroupFailure(cantReadFile))
                val (tmp, mime) = picked
                val name = runCatching { ChatMedia.displayName(ctx, uri) }.getOrNull() ?: tmp.name
                try { app.groups.postFile(id, tmp.path, name, mime, caption).map { "" } } finally { tmp.parentFile?.deleteRecursively() }
            }
        }) { res ->
            sending = false
            res.onFailure { if (draft.isEmpty()) draft = caption; say(it.message ?: cantSend) }
        }
    }
    Box(Modifier.fillMaxSize()) {
    Page {
        val headDesc = stringResource(R.string.groups_chat_head_desc, d.name, GroupText.status(d))
        // Conversation.svelte `.bar` for a group: back · the head (avatar 36, name 17/22, status 13/18; tap = info) · call · info.
        Row(Modifier.fillMaxWidth().height(56.dp).background(c.sf).padding(horizontal = 6.dp, vertical = 4.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(2.dp)) {
            PgIconBtn(Pg.Back, stringResource(R.string.groups_chat_back), onBack)
            Row(Modifier.weight(1f).heightIn(min = 44.dp).clip(SmoothCornerShape(12.dp)).clickable(onClick = onInfo).padding(start = 6.dp, end = 10.dp, top = 4.dp, bottom = 4.dp)
                .semantics(mergeDescendants = true) { contentDescription = headDesc },
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
                    PgIconBtn(Pg.Phone, if (gs.gid == d.id && gs.active) stringResource(R.string.groups_chat_call_return) else if (running) stringResource(R.string.groups_chat_call_join) else stringResource(R.string.groups_chat_call_start),
                        { if (noHost) { say(appString(R.string.group_hosting_calls_missing)); onInfo() } else app.groupCallUi.call(d.id, say) })
                }
            }
            PgIconBtn(Pg.Info, stringResource(R.string.groups_chat_info), onInfo)
        }
        Box(Modifier.fillMaxWidth().height(1.dp).background(c.hairline))
        if (GroupText.active(d.myRole)) OngoingCallBanner(app, d.id, say)
        HostAskCard(app, d, say)
        Box(Modifier.weight(1f).fillMaxWidth().background(c.canvas)) {
            LazyColumn(Modifier.fillMaxSize(), state = list, reverseLayout = true, contentPadding = PaddingValues(start = 10.dp, end = 10.dp, top = 6.dp + if (waiting) 33.dp else 0.dp, bottom = 19.dp)) {
                // Reversed: emitted newest first, so the first item sits at the bottom and "Earlier messages" (last) at the top.
                if (rows.isEmpty()) item { Column(Modifier.fillMaxWidth()) { SysLine(stringResource(R.string.groups_chat_no_messages)) } }
                items(rows.size, key = { j -> rows[rows.size - 1 - j].key }) { j ->
                    val k = rows.size - 1 - j
                    val row = rows[k]
                    val prevAt = rows.getOrNull(k - 1)?.at
                    val newDay = prevAt == null || !sameDay(prevAt, row.at)
                    Column(Modifier.fillMaxWidth()) {
                        if (newDay) PgDayPill(dayLabel(row.at, nowMs), Modifier.align(Alignment.CenterHorizontally).padding(vertical = 10.dp))
                        when (row) {
                            is ThreadRow.Call -> GroupCallPill(row.r, me, GroupText.active(d.myRole)) { app.groupCallUi.call(d.id, say) }
                            is ThreadRow.Post -> {
                                if (row.m.id == dividerId) PgUnreadDivider(unreadAtOpen)
                                GroupItemLive(app, o.messages, row.i, row.m, me, d, blocks.blockedByMe(row.m.authorDid), onRemoval, onReply = { replyTo = row.m.id }, top = if (newDay) 0.dp else null, receipt = o.receipts[row.m.id], say = say,
                                    onOpenPhoto = { viewer = row.m.id })
                            }
                        }
                    }
                }
                if (o.older != null) item(key = "older") {
                    Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
                        TinButton(stringResource(R.string.groups_chat_earlier), { exec({ app.groups.older(); Result.success("") }) {} }, style = BtnStyle.Text, fill = false, height = 40.dp)
                    }
                }
            }
            // The strip over the top of the list (gold): waiting for a host. It overlays, so nothing moves.
            if (waiting) PgStrip(stringResource(R.string.groups_chat_waiting_strip), Pg.Clock, warn = true, modifier = Modifier.align(Alignment.TopCenter))
        }
        when (d.myRole) {
            CoreRole.REMOVED -> BlockedFooter(stringResource(R.string.groups_chat_removed_footer)) {
                TinButton(stringResource(R.string.groups_chat_why), onRemoval, Modifier.weight(1f), style = BtnStyle.Outlined, height = 48.dp)
            }
            CoreRole.LEFT -> BlockedFooter(stringResource(R.string.groups_chat_left_footer))
            else -> if (GroupText.adminsOnlyLocked(d)) AdminsOnlyLine() else {
                val r = replyTo?.let { rid -> o.messages.firstOrNull { it.id == rid } }
                // Hold to record, as in a 1:1 chat; the core checks and seals the file, which then goes (it keeps its own copy).
                val voice = rememberVoiceRecState { rec ->
                    exec({ app.groups.postVoice(id, rec.path, rec.waveform).map { "" }.also { java.io.File(rec.path).delete() } }) { res ->
                        res.onFailure { say(it.message ?: voiceFailed) }
                    }
                }
                PgComposer(draft, { draft = it }, stringResource(R.string.groups_chat_composer_hint), r?.let {
                    ComposerCtx(if (it.outgoing) stringResource(R.string.groups_chat_replying_self) else stringResource(R.string.groups_chat_replying, firstName(nameOf(app, it.authorDid, it.authorName))), GroupText.postLine(it), stringResource(R.string.groups_chat_cancel_reply))
                }, editing = false, onCancelCtx = { replyTo = null }, onSend = {
                    if (sending || draft.isBlank()) return@PgComposer
                    val text = draft; val reply = replyTo
                    draft = ""; replyTo = null; sending = true
                    exec({ app.groups.post(id, text, reply).map { "" } }) { res ->
                        sending = false
                        res.onFailure { if (draft.isEmpty()) draft = text; say(it.message ?: cantSend) }
                    }
                }, onAttach = { if (!sending) attachOpen = true }, field = if (voice.recording) { modifier -> VoiceRecordingBar(voice, modifier) } else null,
                    trailing = { VoiceMicButton(voice) })
            }
        }
    }
    if (attachOpen) AttachSheet(stringResource(R.string.groups_attach_title, d.name), stringResource(R.string.groups_attach_hint), onDismiss = { attachOpen = false },
        onPhotos = {
            attachOpen = false
            try { picker.launch(androidx.activity.result.PickVisualMediaRequest(androidx.activity.result.contract.ActivityResultContracts.PickVisualMedia.ImageOnly)) }
            catch (_: Exception) { say(noPicker) }
        }, onCamera = null,
        onFile = { attachOpen = false; try { filePicker.launch(arrayOf("*/*")) } catch (_: Exception) { say(noFilePicker) } },
        photosLabel = stringResource(R.string.groups_attach_photo))
    val vm = viewer?.let { v -> o.messages.firstOrNull { it.id == v } }
    val vImg = vm?.image?.takeIf { it.state == uniffi.p2pcore.GroupImageState.READY }
    if (vm != null && vImg != null) {
        val savedLine = stringResource(R.string.conv_saved_to_phone)
        val saveFailed = stringResource(R.string.conv_save_failed)
        val pictureName = ChatMedia.groupPictureName(vm.id, vImg.mime)
        fun file() = app.groups.imagePath(d.id, vm.id)?.let { java.io.File(it) }
        PhotoViewer(vm.id, if (vm.outgoing) stringResource(R.string.common_you) else nameOf(app, vm.authorDid, vm.authorName), vm.at.toLong(),
            if (vm.text.isEmpty()) stringResource(R.string.groups_photo_desc) else stringResource(R.string.groups_photo_desc_caption, vm.text),
            load = { file()?.let { ChatMedia.bitmap(it, 2048) } }, onClose = { viewer = null },
            onSave = {
                scope.launch {
                    val ok = withContext(Dispatchers.IO) { runCatching { file()?.let { ChatMedia.saveToPhone(ctx, it, pictureName, vImg.mime) } ?: false }.getOrDefault(false) }
                    say(if (ok) savedLine else saveFailed)
                }
            },
            onShare = {
                scope.launch(Dispatchers.IO) { runCatching { file()?.let { ChatMedia.share(ctx, ChatMedia.cacheCopy(ctx, it, pictureName), vImg.mime) } } }
            })
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
    Row(Modifier.heightIn(min = 44.dp).clip(RoundedCornerShape(22.dp)).gTap(label = stringResource(R.string.groups_online_pill), onClick = onClick).padding(horizontal = 10.dp),
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
    GSheet(onDismiss, stringResource(R.string.groups_online_title, d.name)) { close ->
        Column(Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, top = 20.dp, bottom = 16.dp)) {
            Text(d.name, style = GType.sub, color = c.ink2, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(GroupText.onlineLabel(rows.size), Modifier.padding(top = 2.dp).semantics { heading() }, style = GType.title, color = c.ink)
            GPara(stringResource(R.string.groups_online_lag))
            if (rows.size >= GroupText.ONLINE_CAP) GPara(pluralStringResource(R.plurals.groups_online_cap, GroupText.ONLINE_CAP, GroupText.ONLINE_CAP))
            if (hidden) GPara(stringResource(R.string.groups_online_hidden))
            if (rows.isEmpty()) GPara(stringResource(R.string.groups_online_none), top = 14.dp)
            else Column(Modifier.padding(top = 10.dp)) {
                rows.forEach { m ->
                    key(m.did) {
                        val n = nameOf(app, m.did, m.name)
                        Row(Modifier.fillMaxWidth().heightIn(min = 52.dp).semantics(mergeDescendants = true) {},
                            verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                            Avatar(n, m.did, 32.dp)
                            Text(n, Modifier.weight(1f), style = GType.bodyStrong, color = c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            if (m.isAdmin) GPill(stringResource(R.string.groups_online_admin), GTone.Admin)
                        }
                    }
                }
            }
            GActs { GBtn(stringResource(R.string.common_close), close, GBtnKind.Quiet) }
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
    val pillDesc = stringResource(R.string.groups_call_pill_desc, text, time)
    Row(Modifier.fillMaxWidth().padding(top = 10.dp), horizontalArrangement = if (!r.missed && r.startedBy == me) Arrangement.End else Arrangement.Start) {
        Row(Modifier.heightIn(min = 32.dp).clip(RoundedCornerShape(16.dp)).background(c.sf).border(1.dp, c.ln, RoundedCornerShape(16.dp))
            .clickable(enabled = enabled, onClick = onCall).padding(horizontal = 12.dp)
            .semantics(mergeDescendants = true) { contentDescription = pillDesc },
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
    receipt: uniffi.p2pcore.GroupReceiptStatus? = null, say: (String) -> Unit = {}, onOpenPhoto: () -> Unit = {},
) {
    val c = Tin.c
    val ctx = LocalContext.current
    if (m.kind == GroupItemKind.REMOVED && m.subjectDid == me) {
        val r = d.removal
        if (r == null) SysLine(stringResource(R.string.groups_msg_removed_system))
        else NoticeCard(c.erc, c.onErc, ctx.resources.getQuantityString(R.plurals.groups_msg_removed_title, r.flags.size, r.flags.size),
            stringResource(R.string.groups_msg_removed_by, r.flags.map { nameOf(app, it.byDid, it.byName) }.distinct().let(::listText))) {
            Text(stringResource(R.string.groups_msg_see_details), Modifier.heightIn(min = 48.dp).clickable(role = Role.Button, onClick = onRemoval).wrapContentHeight(Alignment.CenterVertically),
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
                .clickable(role = Role.Button, onClickLabel = stringResource(R.string.groups_msg_show)) { shown = true }.padding(horizontal = 12.dp),
            verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Icon(PgIcons.Off, null, tint = c.ink2, modifier = Modifier.size(18.dp))
            Text(stringResource(R.string.groups_msg_blocked_folded, firstName(name)), style = TinType.bodyM, color = c.ink2)
        }
        return
    }
    // Runs: same author, same day, within 5 minutes (Conversation.svelte `join`).
    fun joined(a: GroupMessage?, b: GroupMessage?) = a != null && b != null && a.kind == GroupItemKind.POST && b.kind == GroupItemKind.POST &&
        a.outgoing == b.outgoing && (a.outgoing || a.authorDid == b.authorDid) && sameDay(a.at.toLong(), b.at.toLong()) && kotlin.math.abs(b.at.toLong() - a.at.toLong()) <= 5 * 60_000
    val p = joined(all.getOrNull(i - 1), m); val n = joined(m, all.getOrNull(i + 1))
    val pos = if (p && n) BubblePos.Mid else if (p) BubblePos.Last else if (n) BubblePos.First else BubblePos.Solo
    val qr = m.replyTo?.let { rid -> all.firstOrNull { it.id == rid } }
    val qWho = qr?.let { if (it.outgoing) stringResource(R.string.common_you) else firstName(nameOf(app, it.authorDid, it.authorName)) }
    val qColor = if (qr?.outgoing == true) c.accentInk else qr?.let { authorColor(it.authorDid) } ?: c.accentInk
    val quote: (@Composable () -> Unit)? = if (m.replyTo == null) null else ({ PgQuote(qWho, qColor, qr?.let { GroupText.postLine(it) } ?: stringResource(R.string.groups_msg_original_missing), null) })
    // Waiting for a host = the slack string; sent = one tick; every eligible member signed delivered = two ticks; every one
    // read = the read ink (core's GroupReceiptStatus). Host storage is never members' delivery/read.
    // An open group sends no member receipts (public group §4): "Delivered 0/N" would mislead, so no n/N there.
    val lines = receipt?.takeIf { it.eligible > 0u && !d.open }?.let { groupReceiptLines(it) }
    val tick = if (!m.outgoing) null else if (m.delivery == CoreDelivery.WAITING || m.delivery == CoreDelivery.FAILED) MetaReceipt(TickVis.SLACK, m.id, GroupText.deliveryWord(m) ?: stringResource(R.string.groups_waiting_host))
        else {
            val vis = receipt?.let { tickVisOf(it.tick, false) } ?: TickVis.ONE
            MetaReceipt(vis, m.id, lines?.take(2)?.joinToString(", ") ?: stringResource(R.string.groups_msg_sent))
        }
    // No details view for group posts yet: a tap on my post shows the n/N lines and who under it (asked for, so it may move).
    var info by remember(m.id) { mutableStateOf(false) }
    val meta = BubbleMeta(msClock(m.at.toLong()), receipt = tick)
    val senderName = if (m.beforeRemoval) stringResource(R.string.groups_msg_name_removed, firstName(name)) else firstName(name)
    val replyLabel = stringResource(R.string.groups_msg_reply)
    Column(Modifier.padding(top = top ?: gapAbove(pos)).fillMaxWidth(), horizontalAlignment = if (m.outgoing) Alignment.End else Alignment.Start) {
        m.image?.let { img ->
            if (!m.outgoing && !p) Text(senderName, Modifier.padding(start = 4.dp, bottom = 2.dp),
                style = PgBodyText.copy(fontSize = 13.sp, lineHeight = 18.sp, fontWeight = FontWeight.Bold), color = if (m.beforeRemoval) c.ink2 else authorColor(m.authorDid))
            GroupPhotoBox(app, d.id, m, img, if (m.text.isEmpty()) pos else BubblePos.First, m.outgoing, say, onReply, onOpenPhoto)
            if (m.text.isEmpty()) Text(msClock(m.at.toLong()), Modifier.padding(top = 3.dp, start = 4.dp, end = 4.dp), style = PgBodyText.copy(fontSize = 11.sp, lineHeight = 14.sp), color = c.ink2)
        }
        m.voice?.let { v ->
            Box(Modifier.fillMaxWidth(.78f), contentAlignment = if (m.outgoing) Alignment.CenterEnd else Alignment.CenterStart) {
                GroupVoiceBubble(app, d.id, m, v, pos, meta, if (m.outgoing || p) null else senderName, if (m.beforeRemoval) c.ink2 else authorColor(m.authorDid), quote, say,
                    Modifier.combinedClickable(enabled = GroupText.active(d.myRole) || lines != null, onLongClickLabel = replyLabel, onLongClick = onReply,
                        onClickLabel = if (lines != null) stringResource(R.string.groups_msg_who_has_it) else null, onClick = { if (lines != null) info = !info }))
            }
        }
        m.file?.let { f ->
            Box(Modifier.fillMaxWidth(.78f), contentAlignment = if (m.outgoing) Alignment.CenterEnd else Alignment.CenterStart) {
                GroupFileBubble(app, d, m, f, me, pos, meta, if (m.outgoing || p) null else senderName,
                    if (m.beforeRemoval) c.ink2 else authorColor(m.authorDid), quote, say, onLong = onReply)
            }
        }
        // A file post's caption sits in its own bubble (GroupFileBubble).
        if (m.file == null && ((m.image == null && m.voice == null) || m.text.isNotEmpty())) Box(Modifier.fillMaxWidth(.78f), contentAlignment = if (m.outgoing) Alignment.CenterEnd else Alignment.CenterStart) {
            PgTextBubble(androidx.compose.ui.text.AnnotatedString(m.text), pos, m.outgoing, meta,
                Modifier.combinedClickable(enabled = GroupText.active(d.myRole) || lines != null, onLongClickLabel = replyLabel, onLongClick = onReply,
                    onClickLabel = if (lines != null) stringResource(R.string.groups_msg_who_has_it) else null, onClick = { if (lines != null) info = !info }),
                sender = if (m.outgoing || p || m.image != null) null else senderName,
                senderColor = if (m.beforeRemoval) c.ink2 else authorColor(m.authorDid), quote = quote)
        }
        if (m.outgoing && m.delivery == CoreDelivery.WAITING) Row(Modifier.padding(top = 3.dp, end = 2.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            Icon(Pg.Clock, null, Modifier.size(12.dp), tint = c.goldInk)
            Text(stringResource(R.string.groups_waiting_host), style = PgBodyText.copy(fontSize = 12.sp, lineHeight = 16.sp, fontWeight = FontWeight.SemiBold), color = c.goldInk)
        }
        if (GroupText.isFailed(m)) Row(Modifier.padding(top = 3.dp, end = 2.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            Icon(Pg.Warning, null, Modifier.size(12.dp), tint = c.danger)
            Text(GroupText.deliveryWord(m) ?: "", style = PgBodyText.copy(fontSize = 12.sp, lineHeight = 16.sp, fontWeight = FontWeight.SemiBold), color = c.danger)
        }
        if (m.beforeRemoval) Text(stringResource(R.string.groups_msg_written_before, firstName(name)), Modifier.padding(top = 3.dp), style = PgBodyText.copy(fontSize = 11.sp), color = c.ink2)
        if (info && lines != null) Text(lines.joinToString("\n"), Modifier.padding(top = 3.dp, end = 2.dp), style = PgBodyText.copy(fontSize = 12.sp, lineHeight = 16.sp), color = c.ink2,
            textAlign = androidx.compose.ui.text.style.TextAlign.End)
    }
}

/**
 * A post's picture (public group §5), drawn like a 1:1 photo (PhotoBox): its box at the picture's shape from the start,
 * the core's colour preview until it is here, then the picture. Not downloaded (the auto-download setting said wait): its
 * size and "Load photo"; on its way: a ring; failed: retry. Here, a tap opens it full-screen (the 1:1 PhotoViewer, with zoom
 * and Save to phone). A long press replies, as on the text bubble.
 */
@OptIn(androidx.compose.foundation.ExperimentalFoundationApi::class)
@Composable
private fun GroupPhotoBox(app: P2pApp, gid: String, m: GroupMessage, img: uniffi.p2pcore.GroupImage, pos: BubblePos, out: Boolean, say: (String) -> Unit, onLong: () -> Unit, onOpen: () -> Unit) {
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
    val cantLoad = stringResource(R.string.groups_photo_cant_load)
    val photoDesc = if (m.text.isEmpty()) stringResource(R.string.groups_photo_desc) else stringResource(R.string.groups_photo_desc_caption, m.text)
    fun load() = scope.launch { withContext(Dispatchers.IO) { app.groups.fetchImage(gid, m.id) }.onFailure { say(it.message ?: cantLoad) } }
    Box(Modifier.size(w, h).clip(bubbleShape(pos, out, 3.dp)).background(c.sf3)
        .combinedClickable(role = Role.Button, onLongClickLabel = stringResource(R.string.groups_msg_reply), onLongClick = onLong, onClickLabel = stringResource(if (ready) R.string.profile_view_photo else R.string.groups_photo_load), onClick = { if (ready) onOpen() else load() })
        .semantics { contentDescription = photoDesc },
        contentAlignment = Alignment.Center) {
        pv?.let {
            Image(it, null, Modifier.fillMaxSize().graphicsLayer { scaleX = 1.24f; scaleY = 1.24f }.blur(14.dp),
                contentScale = androidx.compose.ui.layout.ContentScale.FillBounds, filterQuality = androidx.compose.ui.graphics.FilterQuality.High)
        }
        bmp?.let { Image(it, null, Modifier.fillMaxSize(), contentScale = androidx.compose.ui.layout.ContentScale.Crop) }
        when (img.state) {
            uniffi.p2pcore.GroupImageState.REMOTE -> Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)) {
                PhotoChip(stringResource(R.string.groups_photo_size, sizeText(img.size.toLong())))
                Row(Modifier.clip(RoundedCornerShape(18.dp)).background(c.accent).clickable(role = Role.Button) { load() }.padding(horizontal = 16.dp, vertical = 7.dp),
                    verticalAlignment = Alignment.CenterVertically) {
                    Text(stringResource(R.string.groups_photo_load), style = TinType.caption.copy(fontSize = 13.sp, fontWeight = FontWeight.SemiBold), color = c.onAccent)
                }
            }
            uniffi.p2pcore.GroupImageState.LOADING -> Ring {
                androidx.compose.material3.CircularProgressIndicator(Modifier.size(40.dp), color = Color.White, trackColor = Color.White.copy(alpha = .28f), strokeWidth = 3.dp)
            }
            uniffi.p2pcore.GroupImageState.FAILED -> Ring { Icon(PgIcons.Retry, stringResource(R.string.groups_photo_retry), tint = Color.White) }
            uniffi.p2pcore.GroupImageState.READY -> if (bmp == null && pv == null) Icon(PgIcons.Image, null, tint = c.ink2)
        }
    }
}

/**
 * A voice post: the 1:1 voice bubble's inside (player, waveform, time) in a group bubble, with the sender's name and the
 * quote above it. Before the recording is here: load it (auto-download said wait), wait while it comes, or try again.
 */
@Composable
private fun GroupVoiceBubble(
    app: P2pApp, gid: String, m: GroupMessage, v: uniffi.p2pcore.GroupVoice, pos: BubblePos, meta: BubbleMeta, sender: String?, senderColor: Color,
    quote: (@Composable () -> Unit)?, say: (String) -> Unit, modifier: Modifier,
) {
    val scope = rememberCoroutineScope()
    val ready = v.state == uniffi.p2pcore.GroupImageState.READY
    val path by produceState<String?>(null, m.id, ready) {
        value = if (!ready) null else withContext(Dispatchers.IO) { app.groups.voicePath(gid, m.id) }
    }
    val cantLoad = stringResource(R.string.common_try_again)
    fun load() = scope.launch { withContext(Dispatchers.IO) { app.groups.fetchVoice(gid, m.id) }.onFailure { say(it.message ?: cantLoad) } }
    PgBubble(pos, m.outgoing, modifier) {
        Column {
            sender?.let { Text(it, Modifier.padding(bottom = 1.dp), style = PgBodyText.copy(fontSize = 13.sp, lineHeight = 18.sp, fontWeight = FontWeight.Bold),
                color = senderColor, maxLines = 1, overflow = TextOverflow.Ellipsis) }
            quote?.invoke()
            val p = path
            if (p != null) VoiceBubbleContent(m.id, p, v.durationMs.toInt(), v.waveform, m.outgoing)
            else VoicePendingContent(v.durationMs.toInt(), remote = v.state == uniffi.p2pcore.GroupImageState.REMOTE,
                failed = v.state == uniffi.p2pcore.GroupImageState.FAILED, outgoing = m.outgoing, onDownload = { load() })
            PgMeta(meta, bubbleMetaColor(m.outgoing), Modifier.align(Alignment.End))
        }
    }
}

/**
 * A file post (docs/design/group-files.md): the 1:1 file row in a group bubble, with the sender's name and the quote above
 * it, and under the bubble who else has the file. Not downloaded: its size and Download; on its way: progress and Cancel;
 * here: a tap opens it in another app (FileProvider; a type no app should open is saved instead), Save to phone (Downloads)
 * and Delete my copy (not on a host, which keeps it for the group); failed: "Not available right now" and Try again. The
 * card keeps one size in every state and the holder line keeps its room, so nothing moves when either changes.
 */
@Composable
private fun GroupFileBubble(
    app: P2pApp, d: GroupDetail, m: GroupMessage, f: uniffi.p2pcore.GroupFile, me: String, pos: BubblePos, meta: BubbleMeta, sender: String?, senderColor: Color,
    quote: (@Composable () -> Unit)?, say: (String) -> Unit, onLong: () -> Unit,
) {
    val c = Tin.c
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    val gid = d.id
    val st = f.state
    val size = sizeText(f.size.toLong())
    val plan = remember(f.name, f.mime) { FileOpen.plan(f.name, f.mime) }
    val canOpen = plan.kind != OpenKind.NONE
    val type = f.name.substringAfterLast('.', "").takeIf { it.length in 1..5 }?.uppercase() ?: f.mime.substringAfter('/', "").uppercase().take(5)
    val sub = when (st) {
        uniffi.p2pcore.GroupImageState.READY -> stringResource(if (canOpen) R.string.conv_file_tap_open else R.string.conv_file_tap_save, size, type)
        uniffi.p2pcore.GroupImageState.LOADING -> stringResource(R.string.conv_file_downloading, sizeText(f.done.toLong()), size)
        uniffi.p2pcore.GroupImageState.FAILED -> stringResource(R.string.groups_file_not_available)
        uniffi.p2pcore.GroupImageState.REMOTE -> stringResource(R.string.conv_file_not_downloaded, size)
    }
    val frac = if (st == uniffi.p2pcore.GroupImageState.LOADING && f.size > 0UL) f.done.toFloat() / f.size.toFloat() else null
    val icon = when {
        f.mime.startsWith("image/") -> PgIcons.Image
        f.mime.startsWith("video/") -> PgIcons.Video
        f.mime.startsWith("audio/") -> PgIcons.Mic
        else -> PgIcons.File
    }
    val tryAgain = stringResource(R.string.common_try_again)
    val savedLine = stringResource(R.string.conv_saved_to_phone)
    val saveFailed = stringResource(R.string.conv_save_failed)
    val noApp = stringResource(R.string.conv_no_app_opens, FileOpen.extOf(f.name))
    fun io(block: () -> Result<Unit>) = scope.launch { withContext(Dispatchers.IO) { block() }.onFailure { say(it.message ?: tryAgain) } }
    fun local(): java.io.File? = app.groups.filePath(gid, m.id)?.let { java.io.File(it) }?.takeIf { it.exists() }
    fun save() = scope.launch {
        val ok = withContext(Dispatchers.IO) { runCatching { local()?.let { ChatMedia.saveToPhone(ctx, it, f.name, f.mime, downloads = true) } ?: false }.getOrDefault(false) }
        say(if (ok) savedLine else saveFailed)
    }
    fun open() {
        if (!canOpen) { save(); return }
        scope.launch {
            val ok = withContext(Dispatchers.IO) {
                runCatching { local()?.let { FileOpen.openExternally(ctx, ChatMedia.cacheCopy(ctx, it, "${m.id.take(12)}_${f.name}"), plan.mime) } ?: false }.getOrDefault(false)
            }
            if (!ok) say(noApp)
        }
    }
    val fetch = { io { app.groups.fetchFile(gid, m.id) } }
    Column(horizontalAlignment = if (m.outgoing) Alignment.End else Alignment.Start) {
        PgBubble(pos, m.outgoing, Modifier.fillMaxWidth()) {
            Column {
                sender?.let { Text(it, Modifier.padding(bottom = 1.dp), style = PgBodyText.copy(fontSize = 13.sp, lineHeight = 18.sp, fontWeight = FontWeight.Bold),
                    color = senderColor, maxLines = 1, overflow = TextOverflow.Ellipsis) }
                quote?.invoke()
                FileCard(f.name, icon, sub, frac, m.outgoing, Modifier.fillMaxWidth(), reserveBar = true,
                    onTap = { when (st) { uniffi.p2pcore.GroupImageState.READY -> open(); uniffi.p2pcore.GroupImageState.LOADING -> {}; else -> fetch() } }, onLong = onLong) {
                    val ink = if (m.outgoing) c.onPrc else c.ink2
                    when (st) {
                        uniffi.p2pcore.GroupImageState.REMOTE -> IconBtn(PgIcons.Download, stringResource(R.string.conv_download_name, f.name), { fetch() }, tint = c.pr)
                        uniffi.p2pcore.GroupImageState.LOADING -> IconBtn(PgIcons.X, stringResource(R.string.conv_cancel_download_name, f.name), { io { app.groups.cancelFile(gid, m.id) } }, tint = ink)
                        uniffi.p2pcore.GroupImageState.FAILED -> IconBtn(PgIcons.Retry, stringResource(R.string.groups_file_try_again_name, f.name), { fetch() }, tint = c.pr)
                        uniffi.p2pcore.GroupImageState.READY -> {
                            IconBtn(PgIcons.Save, stringResource(R.string.conv_save_name_to_phone, f.name), { save() }, tint = ink)
                            if (!d.amHost) IconBtn(PgIcons.Trash, stringResource(R.string.groups_file_delete_copy_name, f.name), { io { app.groups.deleteFileCopy(gid, m.id) } }, tint = ink)
                        }
                    }
                }
                if (m.text.isNotEmpty()) Text(m.text, Modifier.padding(top = 6.dp), style = PgBodyText, color = bubbleInk(m.outgoing))
                PgMeta(meta, bubbleMetaColor(m.outgoing), Modifier.align(Alignment.End))
            }
        }
        // Who else has it; the line keeps its room while nobody else does, so it never pushes the thread when it appears.
        Text(GroupText.holdersLine(f, me) { h -> nameOf(app, h.did, h.name) } ?: "", Modifier.padding(top = 3.dp, start = 4.dp, end = 4.dp),
            style = PgBodyText.copy(fontSize = 12.sp, lineHeight = 16.sp), color = c.ink2, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

/** "Delivered n/N", "Read n/N", then who (eligible members by level; kept statements of people no longer counted). Same words as desktop. */
private fun groupReceiptLines(r: uniffi.p2pcore.GroupReceiptStatus): List<String> {
    fun who(m: uniffi.p2pcore.GroupMemberReceipts) = if (m.name.isEmpty()) appString(R.string.common_someone) else firstName(m.name)
    val out = mutableListOf(appString(R.string.groups_receipt_delivered, "${r.delivered}", "${r.eligible}"), appString(R.string.groups_receipt_read, "${r.read}", "${r.eligible}"))
    for ((label, want) in listOf(appString(R.string.groups_receipt_level_read) to uniffi.p2pcore.ReceiptLevel.READ, appString(R.string.groups_receipt_level_device) to uniffi.p2pcore.ReceiptLevel.DELIVERED, appString(R.string.groups_receipt_level_not_yet) to uniffi.p2pcore.ReceiptLevel.UNCONFIRMED)) {
        val n = r.members.filter { it.eligible && it.receipts.level == want }.map { who(it) }
        if (n.isNotEmpty()) out += appString(R.string.groups_receipt_who, label, n.joinToString(", "))
    }
    val gone = r.members.filter { !it.eligible }.map { who(it) }
    if (gone.isNotEmpty()) out += appString(R.string.groups_receipt_also, gone.joinToString(", "))
    return out
}

// ------------------------------------------------------------------ group info (playground screens/groups/GroupInfo.svelte)

private sealed interface InfoSheet {
    data class Hosting(val did: String, val dev: String, val local: Boolean) : InfoSheet
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
 * Group info: header, flags on me, invites (admins), hosting permissions, members, leave. What I can do depends on my role; the
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
    val d = open?.takeIf { it.id == id }?.detail ?: run { InfoPage(stringResource(R.string.groupsb_info_title), onBack) {}; return }
    val you = stringResource(R.string.common_you)
    val cantDo = stringResource(R.string.groupsb_cant_do)
    val openedMsg = stringResource(R.string.groupsb_opened)
    var sheet by remember { mutableStateOf<InfoSheet?>(null) }
    var pending by remember(id) { mutableStateOf<Pair<PubSetting, Boolean>?>(null) }
    val admin = GroupText.isAdmin(d.myRole)
    val done: (Result<String>) -> Unit = { r -> r.onSuccess { if (it.isNotEmpty()) say(it) }.onFailure { say(it.message ?: cantDo) } }
    val name: (String, String) -> String = { did, fb -> nameOf(app, did, fb).let { if (it == you) app.node.profile()?.name?.ifBlank { null } ?: you else it } }
    InfoPage(if (d.myRole == CoreRole.REMOVED) stringResource(R.string.groupsb_removed_title) else stringResource(R.string.groupsb_info_title), onBack) {
        item(key = "hero") {
            Column(Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, top = 16.dp, bottom = 8.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                GroupAvatar(0, 72.dp, dim = !GroupText.active(d.myRole), gid = d.id)
                Text(d.name, Modifier.padding(top = 12.dp).semantics { heading() }, style = GType.hero, color = c.ink, textAlign = TextAlign.Center)
                Text(
                    when (d.myRole) {
                        CoreRole.OWNER -> pluralStringResource(R.plurals.groupsb_hero_owner, d.members.size, d.members.size)
                        CoreRole.ADMIN -> pluralStringResource(R.plurals.groupsb_hero_admin, d.members.size, d.members.size)
                        CoreRole.MEMBER -> pluralStringResource(R.plurals.groupsb_hero_member, d.members.size, d.members.size)
                        CoreRole.LEFT -> stringResource(R.string.groupsb_hero_left)
                        CoreRole.REMOVED -> stringResource(R.string.groupsb_hero_removed)
                    },
                    Modifier.padding(top = 2.dp), style = GType.note, color = c.ink2, textAlign = TextAlign.Center,
                )
            }
        }
        when (d.myRole) {
            CoreRole.REMOVED -> item(key = "removal") { RemovalBody(app, d.removal, onBack) }
            CoreRole.LEFT -> item(key = "left") {
                GNote(stringResource(R.string.groupsb_left_note), Modifier.padding(horizontal = 20.dp, vertical = 24.dp))
            }
            else -> {
                val mine = d.members.firstOrNull { it.isMe }
                if (mine != null && mine.flags > 0u && !mine.isOwner) item(key = "flags") { FlagsOnMe(mine.flags.toInt()) }
                if (admin) item(key = "invites") {
                    val shownSent = sent.filter { it.groupId == id && d.members.none { m -> m.did == it.inviteeDid } }
                    val noHost = d.hosts.isEmpty() && !d.amHost
                    InfoSec(stringResource(R.string.groupsb_invites_sec)) {
                        GCard(Modifier.fillMaxWidth()) {
                            ActRow(GIcons.UserPlus, stringResource(R.string.groupsb_invite_contacts), stringResource(R.string.groupsb_invite_contacts_sub), trailing = GIcons.ChevR) { sheet = InfoSheet.Invite }
                            GRule()
                            ActRow(GIcons.Link, stringResource(R.string.groupsb_create_link), if (noHost) stringResource(R.string.groupsb_create_link_nohost) else stringResource(R.string.groupsb_create_link_valid),
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
                    HostsSection(app, d, admin, name) { did, dev, local -> sheet = InfoSheet.Hosting(did, dev, local) }
                }
                item(key = "members") {
                    val order = { m: CoreMember -> if (m.isOwner) 0 else if (m.isAdmin) 1 else 2 }
                    val sorted = d.members.sortedWith(compareBy<CoreMember>({ order(it) }, { !it.isMe }))
                    InfoSec(stringResource(R.string.groupsb_members_sec), count = d.members.size) {
                        sorted.forEach { m ->
                            key(m.did) {
                                val n = name(m.did, m.name)
                                val req = outgoing.filter { it.peerDid == m.did }.maxByOrNull { it.at }
                                val reqNote = when (req?.state) {
                                    ChatRequestState.PENDING -> stringResource(R.string.groupsb_req_pending)
                                    ChatRequestState.DECLINED -> stringResource(R.string.groupsb_req_declined)
                                    ChatRequestState.BLOCKED -> stringResource(R.string.common_blocked)
                                    else -> null
                                }
                                val since = m.invitedBy?.let { stringResource(R.string.groupsb_invited_by, firstName(name(it, "")), dayOnly(m.joinedAt.toLong())) } ?: stringResource(R.string.groupsb_since, dayOnly(m.joinedAt.toLong()))
                                val flagsTxt = pluralStringResource(R.plurals.groupsb_flags_of, GroupText.FLAGS_TO_REMOVE, m.flags.toInt(), GroupText.FLAGS_TO_REMOVE)
                                val blockedByYou = stringResource(R.string.groupsb_blocked_by_you)
                                val showFlags = (admin || m.isMe) && m.flags > 0u && !m.isOwner
                                val nameYou = if (m.isMe) stringResource(R.string.groupsb_name_you, n) else n
                                val desc = stringResource(R.string.groupsb_member_desc, listText(listOfNotNull(nameYou,
                                    if (m.isOwner) stringResource(R.string.groupsb_role_owner_lc) else if (m.isAdmin) stringResource(R.string.groupsb_role_admin_lc) else stringResource(R.string.groupsb_role_member_lc),
                                    if (m.isHost || d.callHosts.any { it.did == m.did }) stringResource(R.string.groupsb_role_host_lc) else null)))
                                MRow(onClick = { sheet = InfoSheet.Member(m.did) }, desc = desc) {
                                    Avatar(n, m.did, 40.dp)
                                    Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(1.dp)) {
                                        Text(nameYou, style = GType.bodyStrong, color = c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                        Text(androidx.compose.ui.text.buildAnnotatedString {
                                            append(since)
                                            if (showFlags) { pushStyle(androidx.compose.ui.text.SpanStyle(color = c.er, fontWeight = FontWeight.SemiBold)); append(" · $flagsTxt"); pop() }
                                            if (blocks.blockedByMe(m.did)) append(" · $blockedByYou")
                                            else if (reqNote != null) append(" · $reqNote")
                                        }, style = GType.sub, color = c.ink2, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                    }
                                    Row(horizontalArrangement = Arrangement.spacedBy(4.dp), verticalAlignment = Alignment.CenterVertically) {
                                        if (m.isOwner) GPill(stringResource(R.string.groupsb_pill_owner), GTone.Owner) else if (m.isAdmin) GPill(stringResource(R.string.groupsb_pill_admin), GTone.Admin)
                                        if (m.isHost || d.callHosts.any { it.did == m.did }) GMark(GIcons.Server, stringResource(R.string.groupsb_host))
                                    }
                                }
                            }
                        }
                    }
                }
                item(key = "leave") {
                    Column(Modifier.padding(start = 16.dp, end = 16.dp, top = 24.dp)) {
                        GCard(Modifier.fillMaxWidth()) {
                            if (d.myRole != CoreRole.OWNER) ActRow(GIcons.Leave, stringResource(R.string.groupsb_leave_group), danger = true) { sheet = InfoSheet.Leave }
                            else ActRow(GIcons.Leave, stringResource(R.string.groupsb_leave_group), stringResource(R.string.groupsb_owner_cant_leave), enabled = false, off = true) {}
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
        is InfoSheet.Hosting -> HostingSheet(app, d, k.did, k.dev, k.local, exec, done) { sheet = null }
        InfoSheet.Invite -> InviteSheet(app, d, sent, exec, say, onDismiss = { sheet = null })
        InfoSheet.Leave -> GSheet({ sheet = null }, stringResource(R.string.groupsb_leave_group)) { close ->
            GDialogBody(stringResource(R.string.groupsb_leave_title, d.name)) {
                GPara(stringResource(R.string.groupsb_leave_body), top = 4.dp)
                if (d.myRole == CoreRole.ADMIN) GPara(stringResource(R.string.groupsb_leave_admin))
                GActs {
                    GBtn(stringResource(R.string.common_cancel), close, GBtnKind.Quiet)
                    GBtn(stringResource(R.string.groupsb_leave), { exec({ app.groups.leave(id) }) { r -> done(r); r.onSuccess { sheet = null; onBack() } } }, GBtnKind.Danger)
                }
            }
        }
        InfoSheet.OpenGroup -> GSheet({ sheet = null }, stringResource(R.string.groupsb_open_group)) { close ->
            val busy = pending?.first == PubSetting.Open
            GDialogBody(stringResource(R.string.groupsb_open_title, d.name)) {
                GPara(stringResource(R.string.groupsb_open_body1), top = 4.dp)
                GPara(stringResource(R.string.groupsb_open_body2))
                GPara(stringResource(R.string.groupsb_open_body3))
                GActs {
                    GBtn(stringResource(R.string.common_cancel), close, GBtnKind.Quiet)
                    GBtn(if (busy) stringResource(R.string.groupsb_opening) else stringResource(R.string.groupsb_open_group), {
                        pending = PubSetting.Open to true
                        exec({ app.groups.openLink(id).map { openedMsg } }) { r -> pending = null; done(r); if (r.isSuccess) close() }
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
    val backLabel = stringResource(R.string.common_back)
    Column(Modifier.fillMaxSize().background(c.bg).systemBarsPadding()) {
        Row(Modifier.fillMaxWidth().background(c.sf).heightIn(min = 60.dp).padding(start = 8.dp, end = 12.dp, top = 8.dp, bottom = 8.dp),
            verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            Box(Modifier.size(44.dp).clip(CircleShape).gTap(label = null, onClick = onBack).semantics { contentDescription = backLabel }, contentAlignment = Alignment.Center) {
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
private fun RowScope.TwoLine(title: String, sub: String, subColor: Color = Tin.c.ink2, dot: Boolean? = null, subLines: Int = 1) {
    val c = Tin.c
    Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(1.dp)) {
        Text(title, style = GType.bodyStrong, color = c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
        Row(verticalAlignment = Alignment.CenterVertically) {
            if (dot != null) { GDot(dot); Spacer(Modifier.width(6.dp)) }
            Text(sub, style = GType.sub, color = subColor, maxLines = subLines, overflow = TextOverflow.Ellipsis)
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
    val cantGetLink = stringResource(R.string.groupsb_cant_get_link)
    val copy: (String) -> Unit = { copyToClipboard(ctx, ctx.getString(R.string.groupsb_group_link), it, sensitive = false); say(GroupText.OPEN_LINK_COPIED) }
    InfoSec(stringResource(R.string.groupsb_public_sec)) {
        GCard(Modifier.fillMaxWidth()) {
            SwitchRow(GIcons.Link, stringResource(R.string.groupsb_public_link), stringResource(R.string.groupsb_open_link_on), stringResource(R.string.groupsb_open_link_off), openOn) { want ->
                if (pending == null) {
                    if (want) onAskOpen()
                    else { onPending(PubSetting.Open to false); exec({ app.groups.closeLink(d.id) }) { r -> onPending(null); done(r) } }
                }
            }
            GRule()
            ActRow(PgIcons.Copy, stringResource(R.string.groupsb_copy_link), when {
                fetching -> stringResource(R.string.groupsb_getting_link)
                link != null -> link
                openOn -> stringResource(R.string.groupsb_link_never_expires)
                else -> stringResource(R.string.groupsb_turn_on_open)
            }, enabled = openOn && !fetching) {
                if (link != null) copy(link)
                else if (pending == null && d.open) {
                    onPending(PubSetting.Link to true)
                    exec({ app.groups.openLink(d.id) }) { r -> onPending(null); r.onSuccess(copy).onFailure { say(it.message ?: cantGetLink) } }
                }
            }
            GRule()
            SwitchRow(GIcons.Shield, stringResource(R.string.groupsb_admins_only), stringResource(R.string.groupsb_admins_only_on), stringResource(R.string.groupsb_admins_only_off), aoOn) { want ->
                if (pending == null) { onPending(PubSetting.AdminsOnly to want); exec({ app.groups.setAdminsOnly(d.id, want) }) { r -> onPending(null); done(r) } }
            }
        }
        GNote(stringResource(R.string.groupsb_public_note),
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
    edit: (String, String, Boolean) -> Unit) {
    val devices = (d.hosts + d.callHosts + d.hostRequests + d.callHostRequests + d.hostOffers + d.callHostOffers)
        .distinctBy { it.did to it.deviceId }
    InfoSec(stringResource(R.string.groupsb_hosts_sec)) {
        if (devices.isEmpty()) GNote(stringResource(R.string.groupsb_hosts_none))
        devices.forEach { h ->
            key(h.did + h.deviceId) {
                MRow {
                    Avatar(name(h.did, h.name), h.did, 40.dp)
                    TwoLine((if (h.isMe) stringResource(R.string.groupsb_this_device) else name(h.did, h.name)) + " · " + GroupText.shortDev(h.deviceId),
                        (if (h.online || h.isMe) stringResource(R.string.groupsb_host_online) else stringResource(R.string.groupsb_host_offline)) + " · " + hostingStatus(d, h.did, h.deviceId), subLines = Int.MAX_VALUE)
                    if (admin || h.isMe) TrailBtn(stringResource(R.string.group_hosting_edit), stringResource(R.string.group_hosting_permissions)) { edit(h.did, h.deviceId, h.isMe) }
                }
            }
        }
        if (devices.none { it.isMe }) {
            ActRow(GIcons.Server, stringResource(R.string.group_hosting_this_device)) { edit(app.groups.myDid, app.groups.hostingDeviceId(), true) }
        }
        GNote(stringResource(R.string.group_hosting_note), Modifier.padding(start = 4.dp, end = 4.dp, top = 10.dp))
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
    InfoSec(stringResource(R.string.groupsb_flags_on_you), countText = stringResource(R.string.groupsb_of_total, n.toString(), total.toString()), countColor = Tin.c.er) {
        Box(Modifier.padding(start = 4.dp, end = 4.dp, bottom = 4.dp, top = 2.dp)) { FlagMeter(n, total, desc = pluralStringResource(R.plurals.groupsb_flags_of, total, n, total)) }
        GNote(pluralStringResource(R.plurals.groupsb_flags_left_note, left, left),
            Modifier.padding(start = 4.dp, end = 4.dp, top = 6.dp))
    }
}

/** The removed person's view (Removal.svelte): who flagged, when, and what it means. */
@Composable
private fun RemovalBody(app: P2pApp, r: uniffi.p2pcore.GroupRemoval?, onBack: () -> Unit) {
    val c = Tin.c
    val ord = listOf(stringResource(R.string.groupsb_flag_first), stringResource(R.string.groupsb_flag_second), stringResource(R.string.groupsb_flag_third))
    val flagFallback = stringResource(R.string.groupsb_flag_n)
    Column(Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, top = 8.dp, bottom = 32.dp)) {
        Text(pluralStringResource(R.plurals.groupsb_removed_after, GroupText.FLAGS_TO_REMOVE, GroupText.FLAGS_TO_REMOVE), Modifier.padding(top = 8.dp, bottom = 2.dp).semantics { heading() }, style = GType.hero, color = c.ink)
        if (r != null && r.at > 0u) Text(dayAndTime(r.at.toLong(), System.currentTimeMillis()), style = GType.note, color = c.ink2)
        RemSec(stringResource(R.string.groupsb_who_flagged))
        r?.flags?.forEachIndexed { i, f ->
            val who = nameOf(app, f.byDid, f.byName)
            Row(Modifier.fillMaxWidth().heightIn(min = 60.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Avatar(who, f.byDid, 40.dp)
                Column(verticalArrangement = Arrangement.spacedBy(1.dp)) {
                    Text(who, style = GType.bodyStrong, color = c.ink)
                    Text(ord.getOrElse(i) { flagFallback } + if (f.at > 0u) " · " + dayAndTime(f.at.toLong(), System.currentTimeMillis()) else "", style = GType.sub.copy(fontFeatureSettings = "tnum"), color = c.ink2)
                }
            }
            if (i < r.flags.lastIndex) GRule()
        }
        GNote(stringResource(R.string.groupsb_flags_checked_note), Modifier.padding(top = 6.dp))
        RemSec(stringResource(R.string.groupsb_what_means))
        Text(stringResource(R.string.groupsb_removal_body),
            style = GType.body.copy(lineHeight = 22.sp), color = c.ink)
        Box(Modifier.padding(top = 20.dp)) { GBtn(stringResource(R.string.groupsb_read_posts), onBack, GBtnKind.Outline) }
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
    val cantInvite = stringResource(R.string.groupsb_cant_invite)
    GSheet(onDismiss, stringResource(R.string.groupsb_invite_contacts)) { close ->
        GDialogBody(stringResource(R.string.groupsb_invite_to, d.name)) {
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
                GPara(stringResource(R.string.groupsb_invite_sheet_note), top = 10.dp)
            } else GPara(stringResource(R.string.groupsb_invite_none), top = 10.dp)
            GActs {
                GBtn(stringResource(R.string.common_cancel), close, GBtnKind.Quiet)
                GBtn(if (busy) stringResource(R.string.groupsb_sending) else if (picked.isEmpty()) stringResource(R.string.groupsb_invite) else pluralStringResource(R.plurals.groupsb_invite_people, picked.size, picked.size), {
                    busy = true
                    val dids = picked.toList()
                    exec({ app.groups.inviteContacts(d.id, dids) }) { r -> busy = false; r.onSuccess { say(it); close() }.onFailure { say(it.message ?: cantInvite) } }
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
    val nameYou = if (m.isMe) stringResource(R.string.groupsb_name_you, n) else n
    var hostingDev by remember(d.id, m.did) { mutableStateOf<String?>(null) }
    hostingDev?.let { dev ->
        HostingSheet(app, d, m.did, dev, m.isMe, exec, done) { hostingDev = null }
    }
    GSheet(onDismiss, if (m.isMe) stringResource(R.string.groupsb_member_sheet_me) else stringResource(R.string.groupsb_member_in_group, n, d.name)) { close ->
        androidx.compose.animation.Crossfade(view, animationSpec = tween(GMotion.CROSSFADE, easing = GMotion.EaseOut), label = "view") { v ->
            when (v) {
                MView.Actions -> Column {
                    Row(Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, top = 18.dp, bottom = 10.dp), verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                        Avatar(n, m.did, 48.dp)
                        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(1.dp)) {
                            Text(nameYou, style = GType.title.copy(lineHeight = 22.sp), color = c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            Text(m.invitedBy?.let { stringResource(R.string.groupsb_invited_by, firstName(nameOf(app, it, "")), dayOnly(m.joinedAt.toLong())) } ?: stringResource(R.string.groupsb_member_since, dayOnly(m.joinedAt.toLong())),
                                style = GType.sub, color = c.ink2, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        }
                        Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                            GPill(if (m.isOwner) stringResource(R.string.groupsb_pill_owner) else if (m.isAdmin) stringResource(R.string.groupsb_pill_admin) else stringResource(R.string.groupsb_pill_member), if (m.isOwner) GTone.Owner else if (m.isAdmin) GTone.Admin else GTone.Muted)
                            if (m.isHost || d.callHosts.any { it.did == m.did }) GPill(stringResource(R.string.groupsb_host), GTone.Host)
                        }
                    }
                    if (admin && !m.isMe && !m.isOwner) Text(pluralStringResource(R.plurals.groupsb_flags_of, GroupText.FLAGS_TO_REMOVE, m.flags.toInt(), GroupText.FLAGS_TO_REMOVE), Modifier.padding(start = 20.dp, end = 20.dp, bottom = 4.dp),
                        style = GType.sub.copy(fontFeatureSettings = "tnum"), color = c.ink2)
                    Column(Modifier.padding(start = 8.dp, end = 8.dp, top = 4.dp)) {
                        if (!m.isMe) when {
                            contact && !blockedByMe -> MItem(GIcons.Chat, stringResource(R.string.groupsb_message_fn, fn)) { onDismiss(); onChat(m.did) }
                            blockedByMe -> MItem(GIcons.Off, stringResource(R.string.groupsb_you_blocked, fn), stringResource(R.string.groupsb_blocked_stay), off = true)
                            req?.state == ChatRequestState.PENDING -> MItem(GIcons.Clock, stringResource(R.string.groupsb_request_sent), stringResource(R.string.groupsb_waiting_answer, fn), off = true)
                            req?.state == ChatRequestState.BLOCKED -> MItem(GIcons.Off, stringResource(R.string.common_blocked), stringResource(R.string.groupsb_cant_receive, fn), off = true)
                            MemberAction.Message in acts -> MItem(GIcons.Chat, if (req?.state == ChatRequestState.DECLINED) stringResource(R.string.groupsb_ask_again, fn) else stringResource(R.string.groupsb_ask_chat, fn),
                                if (req?.state == ChatRequestState.DECLINED) stringResource(R.string.groupsb_declined_last, fn) else stringResource(R.string.groupsb_sends_request)) { view = MView.Ask }
                        }
                        acts.forEach { a ->
                            when (a) {
                                is MemberAction.Hosting -> MItem(GIcons.Server,
                                    stringResource(R.string.group_hosting_permissions) + if (several) " · ${GroupText.shortDev(a.deviceId)}" else "",
                                    hostingStatus(d, m.did, a.deviceId)) { hostingDev = a.deviceId }
                                MemberAction.MakeAdmin -> MItem(GIcons.Shield, stringResource(R.string.groupsb_make_admin)) { exec({ app.groups.setAdmin(d.id, m.did, true) }, done); close() }
                                MemberAction.RevokeAdmin -> MItem(GIcons.Shield, stringResource(R.string.groupsb_remove_admin)) { exec({ app.groups.setAdmin(d.id, m.did, false) }, done); close() }
                                else -> {}
                            }
                        }
                        if (m.isMe && GroupText.active(d.myRole)) {
                            MItem(GIcons.Server, stringResource(R.string.group_hosting_this_device)) { hostingDev = app.groups.hostingDeviceId() }
                        }
                        if (MemberAction.Flag in acts) MItem(GIcons.Flag, stringResource(R.string.groupsb_flag_removal), pluralStringResource(R.plurals.groupsb_flag_removal_sub, GroupText.FLAGS_TO_REMOVE, GroupText.FLAGS_TO_REMOVE, fn), danger = true) { view = MView.Flag }
                        else if (m.isOwner && !m.isMe) MItem(GIcons.Flag, stringResource(R.string.groupsb_owner_cant_flag), off = true)
                    }
                    Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 8.dp, bottom = 14.dp), horizontalArrangement = Arrangement.End) {
                        GBtn(stringResource(R.string.common_close), close, GBtnKind.Quiet)
                    }
                }
                MView.Flag -> {
                    val (next, lead) = GroupText.flagCopy(n, m.flags.toInt())
                    Column(Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, top = 20.dp, bottom = 16.dp)) {
                        Text(stringResource(R.string.groupsb_flag_title, fn), Modifier.padding(bottom = 12.dp).semantics { heading() }, style = GType.title, color = c.ink)
                        FlagMeter(m.flags.toInt(), GroupText.FLAGS_TO_REMOVE, yours = true, desc = stringResource(R.string.groupsb_flag_next_of, next.toString(), GroupText.FLAGS_TO_REMOVE.toString()))
                        GPara(lead, top = 12.dp, color = c.ink, style = GType.body.copy(lineHeight = 21.sp))
                        GPara(stringResource(R.string.groupsb_flag_note1))
                        GPara(stringResource(R.string.groupsb_flag_note2, fn))
                        GActs {
                            GBtn(stringResource(R.string.common_back), { view = MView.Actions }, GBtnKind.Quiet)
                            GBtn(stringResource(R.string.groupsb_flag_button, fn), { exec({ app.groups.flag(d.id, m.did) }, done); close() }, if (next >= GroupText.FLAGS_TO_REMOVE) GBtnKind.Danger else GBtnKind.Outline)
                        }
                    }
                }
                MView.Ask -> Column(Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, top = 20.dp, bottom = 16.dp)) {
                    Text(stringResource(R.string.groupsb_ask_chat, fn), Modifier.padding(bottom = 4.dp).semantics { heading() }, style = GType.title, color = c.ink)
                    GPara(stringResource(R.string.groupsb_ask_note1, fn))
                    GPara(stringResource(R.string.groupsb_ask_note2, fn))
                    GActs {
                        GBtn(stringResource(R.string.common_back), { view = MView.Actions }, GBtnKind.Quiet)
                        GBtn(stringResource(R.string.groupsb_send_request), { exec({ app.groups.request(d.id, m.did) }, done); close() }, GBtnKind.Primary)
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
    InfoPage(stringResource(R.string.groupsb_removed_title), onBack) { item { RemovalBody(app, r, onBack) } }
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
        TopBar(stringResource(R.string.groupsb_new_group), onBack)
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(horizontal = 20.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            TinField(name, { name = it.take(100) }, stringResource(R.string.groupsb_group_name), state = if (err != null) FieldState.Error else FieldState.Normal, hint = err)
            InfoCard(stringResource(R.string.groupsb_new_group_host_note),
                icon = PgIcons.Server, kind = BannerKind.Warn)
            Hint(stringResource(R.string.groupsb_new_group_own))
        }
        Box(Modifier.padding(20.dp)) {
            TinButton(if (busy) stringResource(R.string.groupsb_making) else stringResource(R.string.groupsb_create_group), {
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
    val d = open?.takeIf { it.id == id }?.detail ?: run { Page { TopBar(stringResource(R.string.groupsb_add_members), onBack) }; return }
    val picked = remember { mutableStateListOf<String>() }
    var busy by remember { mutableStateOf(false) }
    val exec = rememberRunner()
    val cantInvite = stringResource(R.string.groupsb_cant_invite)
    val rows = remember(contacts, d, sent, blocks) {
        GroupText.pickRows(contacts.map { InviteCandidate(it.did, it.display()) }, d, sent, { blocks.blockedByMe(it) }, { blocks.blockedMeAt(it) != null })
    }
    Page {
        TopBar(stringResource(R.string.groupsb_add_to, d.name), onBack)
        LazyColumn(Modifier.weight(1f), contentPadding = PaddingValues(horizontal = 20.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            item { SectionLabel(pluralStringResource(R.plurals.groupsb_contacts_selected, picked.size, picked.size), Modifier.padding(start = 0.dp)) }
            if (rows.isEmpty()) item { Hint(stringResource(R.string.groupsb_no_contacts)) }
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
            item { Hint(stringResource(R.string.groupsb_add_note), Modifier.padding(top = 8.dp)) }
        }
        Box(Modifier.padding(20.dp)) {
            TinButton(if (busy) stringResource(R.string.groupsb_sending) else if (picked.size > 1) pluralStringResource(R.plurals.groupsb_invite_people, picked.size, picked.size) else stringResource(R.string.groupsb_invite), {
                busy = true
                val dids = picked.toList()
                exec({ app.groups.inviteContacts(id, dids) }) { r -> busy = false; r.onSuccess { say(it); onBack() }.onFailure { say(it.message ?: cantInvite) } }
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
            TopBar(stringResource(R.string.groupsb_scan_invite), { scanning = false })
            QrCameraBox(Modifier.weight(1f).fillMaxWidth(), stringResource(R.string.groupsb_point_invite), false, {}) { t -> token = t.trim(); scanning = false }
        }
        return
    }
    val bad = token.isNotBlank() && !checking && peek == null
    Page {
        TopBar(stringResource(R.string.groupsb_join_group), onBack)
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(horizontal = 20.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
            TinField(token, { token = it.trim(); err = null }, stringResource(R.string.groupsb_invite_field), mono = true, singleLine = false, minHeight = 72.dp, placeholder = "tinline://g/…",
                state = if (bad) FieldState.Error else FieldState.Normal,
                hint = when { checking -> stringResource(R.string.groupsb_checking_host); bad -> stringResource(R.string.groupsb_bad_link); else -> null })
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                TinButton(stringResource(R.string.groupsb_paste), {
                    val cm = ctx.getSystemService(android.content.ClipboardManager::class.java)
                    cm?.primaryClip?.takeIf { it.itemCount > 0 }?.getItemAt(0)?.coerceToText(ctx)?.toString()?.let { token = it.trim() }
                }, Modifier.weight(1f), style = BtnStyle.Outlined, icon = PgIcons.Paste)
                TinButton(stringResource(R.string.groupsb_scan), { scanning = true }, Modifier.weight(1f), style = BtnStyle.Outlined, icon = PgIcons.Scan)
            }
            peek?.let { p ->
                CardBox(Modifier.fillMaxWidth(), radius = 20.dp) {
                    Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                            GroupAvatar(tintOf(p.groupId), 44.dp, radius = 14.dp, gid = p.groupId)
                            Text(p.name, style = TinType.titleM, color = c.ink)
                        }
                        Text(p.issuerDid?.let { did -> personLine(app, did, R.string.groupsb_join_invited_by, R.string.groupsb_join_invited_by_you, R.string.groupsb_join_invited_by_unknown) } ?: stringResource(R.string.groupsb_own_account_link), style = TinType.bodyM, color = c.ink2)
                        Text(personLine(app, p.ownerDid, R.string.groupsb_join_owner, R.string.groupsb_join_owner_you, R.string.groupsb_join_owner_unknown), style = TinType.bodyM, color = c.ink2)
                        p.expires?.let { Text(stringResource(R.string.groupsb_valid_until, whenFull(it.toLong())), style = TinType.bodyM, color = c.ink2) }
                    }
                }
                Hint(stringResource(R.string.groupsb_join_note))
            }
            err?.let { Text(it, style = TinType.bodyM, color = c.er) }
        }
        Box(Modifier.padding(20.dp)) {
            TinButton(if (busy) stringResource(R.string.groupsb_joining) else stringResource(R.string.common_join), {
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
    val name = app.groups.open.collectAsState().value?.takeIf { it.id == id }?.detail?.name
    var token by remember { mutableStateOf<String?>(null) }
    var err by remember { mutableStateOf<String?>(null) }
    var full by remember { mutableStateOf(false) }
    val showFull = stringResource(R.string.groupsb_show_full_screen)
    LaunchedEffect(id) {
        val r = withContext(Dispatchers.IO) { app.groups.invite(id) }
        r.onSuccess { token = it }.onFailure { err = it.message }
    }
    Page {
        TopBar(stringResource(R.string.groupsb_invite_link_title), onBack)
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(horizontal = 20.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(14.dp)) {
            Hint(if (name != null) stringResource(R.string.groupsb_link_note, name) else stringResource(R.string.groupsb_link_note_unnamed))
            val t = token
            // Large, high contrast, the full width: easy to scan from another phone. Tap for full screen.
            Box(Modifier.fillMaxWidth().aspectRatio(1f).clip(SmoothCornerShape(16.dp)).background(Color.White)
                .let { m -> if (t != null) m.clickable(role = Role.Button, onClickLabel = showFull) { full = true } else m },
                contentAlignment = Alignment.Center) {
                when {
                    t != null -> {
                        val bmp = remember(t) { qrBitmap(t, 1024, fg = 0xFF000000.toInt()) }
                        Image(bmp.asImageBitmap(), stringResource(R.string.groupsb_qr_desc), Modifier.fillMaxSize().padding(12.dp))
                    }
                    err != null -> Text(err!!, Modifier.padding(16.dp), style = TinType.bodyM, color = c.er, textAlign = TextAlign.Center)
                    else -> Text(stringResource(R.string.groupsb_making_it), style = TinType.bodyM, color = Color(0xFF4D5853))
                }
            }
            t?.let { Text(it, style = TinType.bodyM.copy(fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace), color = c.ink2, textAlign = TextAlign.Center) }
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                TinButton(stringResource(R.string.common_share), {
                    token?.let { tk -> runCatching { ctx.startActivity(Intent.createChooser(Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, (if (name != null) ctx.getString(R.string.groupsb_share_text, name, tk) else ctx.getString(R.string.groupsb_share_text_unnamed, tk))), ctx.getString(R.string.groupsb_share_title))) } }
                }, Modifier.weight(1f), style = BtnStyle.Tonal, icon = PgIcons.Share, enabled = token != null)
                CopyButton(token, Modifier.weight(1f))
            }
        }
    }
    val t = token
    if (full && t != null) {
        Dialog({ full = false }, DialogProperties(usePlatformDefaultWidth = false)) {
            Box(Modifier.fillMaxSize().background(Color.White).clickable(role = Role.Button, onClickLabel = stringResource(R.string.common_close)) { full = false }.padding(16.dp), contentAlignment = Alignment.Center) {
                val bmp = remember(t) { qrBitmap(t, 1024, fg = 0xFF000000.toInt()) }
                Image(bmp.asImageBitmap(), stringResource(R.string.groupsb_qr_desc_full), Modifier.fillMaxWidth().aspectRatio(1f))
            }
        }
    }
}
