package com.osvauld.p2p

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import uniffi.p2pcore.CallRecord
import uniffi.p2pcore.Contact
import uniffi.p2pcore.ContactRequest

/*
 * The home tabs' lists, from the playground: screens/home/ChatsList.svelte (72 dp rows), screens/calls/CallsList.svelte,
 * screens/people/ContactsList.svelte + PersonRow.svelte (64 dp, fixed). List padding 0 8; rows radius 14 (--r-ctl).
 */

private val RowShape = RoundedCornerShape(14.dp)

// ---------------------------------------------------------------- Chats

/** Chats tab: All / Unread chips, then 1:1 and group conversations, newest first. [top] goes above the chips (banners). */
@Composable
fun ChatsTabBody(
    rows: List<ChatRowUi>, state: LazyListState, onOpen: (ChatRowUi) -> Unit,
    top: @Composable () -> Unit = {}, header: @Composable () -> Unit = {},
    /** The first screen's hero, shown while there are no chats at all. */
    empty: (@Composable () -> Unit)? = null,
) {
    var unreadOnly by remember { mutableStateOf(false) }
    val drafts by ConversationDrafts.previews.collectAsState()
    val shown = rows.filter { !unreadOnly || it.unread > 0 }
    LazyColumn(Modifier.fillMaxSize(), state = state, contentPadding = PaddingValues(bottom = 8.dp)) {
        item(key = "_title") { header() }
        item(key = "_top") { top() }
        // With no chats at all the filters mean nothing yet: the whole row goes, so the hero sits at the same height as on Calls.
        if (rows.isNotEmpty()) item(key = "_chips") {
            Row(Modifier.padding(start = 16.dp, end = 16.dp, top = 4.dp, bottom = 8.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                FilterChip("All", !unreadOnly) { unreadOnly = false }
                FilterChip("Unread", unreadOnly) { unreadOnly = true }
            }
        }
        if (rows.isEmpty()) item(key = "_empty") { empty?.invoke() ?: EmptyQuietLine(EmptyQuiet.CHATS) }
        else if (shown.isEmpty()) item(key = "_caughtup") { EmptyQuietLine(EmptyQuiet.UNREAD) }
        items(shown, key = { (if (it.group) "g" else "d") + it.did }) { r ->
            Box(Modifier.padding(horizontal = 8.dp)) { ChatListRow(r, drafts[r.did], onClick = { onOpen(r) }) }
        }
    }
}

/** One conversation: avatar 48, name 15/600 + time 11 (accent-ink 700 when unread), preview 13 muted with the draft, Waiting, or block state. */
@Composable
fun ChatListRow(r: ChatRowUi, draft: String?, selected: Boolean = false, onClick: () -> Unit) {
    val c = Tin.c
    val hot = r.unread > 0
    Row(
        Modifier.fillMaxWidth().heightIn(min = 72.dp).pressable(onClick, RowShape, rest = if (selected) c.accentSoft else androidx.compose.ui.graphics.Color.Transparent).padding(8.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        if (r.group) ListGroupAvatar(48.dp, gid = r.did) else Avatar(r.name, r.did, 48.dp)
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(r.name, Modifier.weight(1f), style = TinFs.body.copy(fontWeight = FontWeight.SemiBold), color = c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(r.time, Modifier.width(56.dp), textAlign = TextAlign.End, maxLines = 1, overflow = TextOverflow.Ellipsis, style = TinFs.meta.copy(lineHeight = 21.sp, fontWeight = if (hot) FontWeight.Bold else FontWeight.Normal), color = if (hot) c.accentInk else c.ink2)
            }
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                val text = buildAnnotatedString {
                    when {
                        r.alert -> withStyle(SpanStyle(color = c.danger, fontWeight = FontWeight.SemiBold)) { append(r.preview) }
                        !draft.isNullOrBlank() -> { withStyle(SpanStyle(color = c.accentInk, fontWeight = FontWeight.SemiBold)) { append("Draft:") }; append(" " + draft.replace('\n', ' ')) }
                        r.tick == Tick.Clock -> { withStyle(SpanStyle(color = c.threadText, fontWeight = FontWeight.SemiBold)) { append("Waiting") }; append(" · " + r.preview) }
                        else -> append(r.preview)
                    }
                }
                Row(Modifier.weight(1f), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(3.dp)) {
                    if (r.tick == Tick.Clock && draft.isNullOrBlank() && !r.alert) Icon(PgIcons.Clock, null, tint = c.threadText, modifier = Modifier.size(13.dp))
                    // My last message's tick: the conversation's glyph at 12 dp in a slot kept from the start (no shift).
                    else if (r.markSlot && draft.isNullOrBlank() && !r.alert) Box(Modifier.size(tickSlotWidth(12.dp), 12.dp)) {
                        r.mark?.let { v ->
                            ReceiptTickGlyph(r.markKey, v, c.ink2, c.tickRead, c.tickBad, if (selected) c.accentSoft else c.bg,
                                if (r.group) listTickLabel(v) else tickLabel(v, firstName(r.name)))
                        }
                    }
                    Text(text, style = TinFs.sub.copy(lineHeight = 21.sp), color = c.ink2, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
                Box(Modifier.width(32.dp).height(18.dp), contentAlignment = Alignment.CenterEnd) { if (hot) CountBadge(r.unread) }
            }
        }
    }
}

// ---------------------------------------------------------------- Calls

/** "Outgoing", "Missed", "No answer"...: CallsList.svelte `what()`, over the core's reasons. */
fun callWhat(r: CallRecord): String = when {
    r.durationSecs > 0u -> if (r.incoming) "Incoming" else "Outgoing"
    r.incoming && r.reason == "unavailable" -> "While not available"
    r.incoming -> if (r.missed) "Missed" else "Declined"
    else -> when (r.reason) {
        "no_answer" -> "No answer"
        "declined", "busy" -> "Declined"
        "cancelled" -> "Cancelled"
        "unreachable" -> "Couldn’t connect"
        else -> "Call"
    }
}

/**
 * Group calls on the Calls tab (CallsList.svelte, 2026-10-14): [log] is the core's group call log, [names] the groups I am
 * still in (a group I left has no row). The avatar opens the group's info, the row starts or joins a call there.
 */
class GroupCallsUi(
    val log: List<uniffi.p2pcore.GroupCallRecord>, val names: Map<String, String>,
    val onInfo: (String) -> Unit, val onCall: (String) -> Unit,
)

private sealed interface CallEntry {
    val at: Long
    data class One(val r: CallRecord) : CallEntry { override val at get() = r.startedAt.toLong() }
    data class Group(val r: uniffi.p2pcore.GroupCallRecord, val name: String) : CallEntry { override val at get() = r.startedAt.toLong() }
}

/** Calls tab: every call record, newest first. The avatar opens their profile, the row opens the chat, the phone calls back. */
@Composable
fun CallsTabBody(
    history: List<CallRecord>, nameOf: (CallRecord) -> String, canCall: (String) -> Boolean, state: LazyListState,
    groupCalls: GroupCallsUi? = null,
    onProfile: (String) -> Unit, onCall: (String) -> Unit,
    top: @Composable () -> Unit = {}, header: @Composable () -> Unit = {},
    /** The first screen's hero while nobody is added yet; otherwise the quiet line. */
    empty: (@Composable () -> Unit)? = null,
) {
    val c = Tin.c
    val now = System.currentTimeMillis()
    val entries = remember(history, groupCalls) {
        (history.map { CallEntry.One(it) } + (groupCalls?.let { g -> g.log.mapNotNull { r -> g.names[r.groupId]?.let { CallEntry.Group(r, it) } } } ?: emptyList()))
            .sortedByDescending { it.at }
    }
    LazyColumn(Modifier.fillMaxSize(), state = state, contentPadding = PaddingValues(bottom = 8.dp)) {
        item(key = "_title") { header() }
        item(key = "_top") { top() }
        if (entries.isEmpty()) item(key = "_empty") { empty?.invoke() ?: EmptyQuietLine(EmptyQuiet.CALLS) }
        items(entries, key = { e -> when (e) { is CallEntry.One -> e.r.callId + ":" + e.r.startedAt; is CallEntry.Group -> "g:" + e.r.groupId + ":" + e.r.callId } }) { e ->
            if (e is CallEntry.Group) { GroupCallRow(e.r, e.name, now, groupCalls!!); return@items }
            val r = (e as CallEntry.One).r
            val name = nameOf(r)
            val bad = r.missed
            val callable = canCall(r.peerDid)
            val what = callWhat(r)
            val stamp = listTime(r.startedAt.toLong() * 1000, now)
            Row(Modifier.padding(horizontal = 8.dp).fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(2.dp)) {
                Box(Modifier.pressable({ onProfile(r.peerDid) }, RowShape).semantics { contentDescription = "$name’s profile" }.padding(start = 8.dp, top = 8.dp, bottom = 8.dp)) {
                    Avatar(name, r.peerDid, 48.dp)
                }
                // The row calls them (decided 2026-10-14: this is the Calls tab; the chat lives in Chats). Someone I can't
                // call: their profile, which says why.
                Row(
                    Modifier.weight(1f).heightIn(min = 68.dp).pressable({ if (callable) onCall(r.peerDid) else onProfile(r.peerDid) }, RowShape)
                        .semantics(mergeDescendants = true) {
                            contentDescription = "$name, $what call" + (if (r.durationSecs > 0u) ", " + uniffi.present.clock(r.durationSecs.toLong()) else "") + ", $stamp. " + if (callable) "Call" else "Can’t call"
                        }
                        .padding(start = 12.dp, top = 8.dp, bottom = 8.dp),
                    verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                        Text(name, style = TinFs.body.copy(fontWeight = FontWeight.SemiBold), color = c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        val fg = if (bad) c.danger else c.ink2
                        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(5.dp)) {
                            Icon(if (r.incoming) PgIcons.CallIn else PgIcons.CallOut, null, tint = fg, modifier = Modifier.size(15.dp))
                            Text(if (r.durationSecs > 0u) "$what · ${uniffi.present.clock(r.durationSecs.toLong())}" else what, style = TinFs.sub.copy(lineHeight = 21.sp, fontFeatureSettings = "tnum"), color = fg, maxLines = 1)
                        }
                    }
                    Text(stamp, style = TinFs.meta, color = c.ink2)
                    Box(Modifier.size(44.dp), contentAlignment = Alignment.Center) {
                        Icon(PgIcons.Phone, null, Modifier.size(22.dp).alpha(if (callable) 1f else .45f), tint = if (callable) c.accentInk else c.ink2)
                    }
                }
            }
        }
    }
}

/** A group call in the Calls tab: group avatar (opens its info), name, people icon + "Group call · 12 min · 3 people" (calm), time, phone. */
@Composable
private fun GroupCallRow(r: uniffi.p2pcore.GroupCallRecord, name: String, now: Long, g: GroupCallsUi) {
    val c = Tin.c
    val what = uniffi.p2pcore.groupCallLabel(r)
    val stamp = listTime(r.startedAt.toLong() * 1000, now)
    Row(Modifier.padding(horizontal = 8.dp).fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(2.dp)) {
        Box(Modifier.pressable({ g.onInfo(r.groupId) }, RowShape).semantics { contentDescription = "$name, group info" }.padding(start = 8.dp, top = 8.dp, bottom = 8.dp)) {
            GroupAvatar(0, 48.dp, gid = r.groupId)
        }
        Row(
            Modifier.weight(1f).heightIn(min = 68.dp).pressable({ g.onCall(r.groupId) }, RowShape)
                .semantics(mergeDescendants = true) { contentDescription = "$name, $what, $stamp. Start or join a call" }
                .padding(start = 12.dp, top = 8.dp, bottom = 8.dp),
            verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(name, style = TinFs.body.copy(fontWeight = FontWeight.SemiBold), color = c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(5.dp)) {
                    Icon(PgIcons.Users, null, tint = c.ink2, modifier = Modifier.size(15.dp))
                    Text(what, style = TinFs.sub.copy(lineHeight = 21.sp, fontFeatureSettings = "tnum"), color = c.ink2, maxLines = 1)
                }
            }
            Text(stamp, style = TinFs.meta, color = c.ink2)
            Box(Modifier.size(44.dp), contentAlignment = Alignment.Center) {
                Icon(PgIcons.Phone, null, Modifier.size(22.dp), tint = c.accentInk)
            }
        }
    }
}

// ---------------------------------------------------------------- Contacts

/** PersonRow: 64 dp fixed; avatar 44, name 600 (21), sub 13/18 kept even when empty, so a changing subtitle never moves the list. */
@Composable
fun PersonRow(name: String, key: String, sub: String, onClick: (() -> Unit)?, selected: Boolean = false, dim: Boolean = false, end: (@Composable () -> Unit)? = null) {
    val c = Tin.c
    Row(
        Modifier.fillMaxWidth().heightIn(min = 64.dp)
            .then(if (onClick != null) Modifier.pressable(onClick, RowShape, rest = if (selected) c.accentSoft else androidx.compose.ui.graphics.Color.Transparent) else Modifier)
            .padding(horizontal = 8.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Row(Modifier.weight(1f), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Avatar(name, key, 44.dp, alpha = if (dim) 0.6f else 1f)
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(1.dp)) {
                Text(name, Modifier.alpha(if (dim) 0.6f else 1f), style = TinFs.body.copy(fontWeight = FontWeight.SemiBold), color = c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(sub, Modifier.heightIn(min = 18.dp), style = TinFs.sub, color = c.ink2, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
        end?.invoke()
    }
}

/** Contacts tab: a find field, "Add contact", my contacts A to Z under letters, then people I blocked, then the count. */
@Composable
fun ContactsTabBody(
    contacts: List<Contact>, blocked: (String) -> Boolean, sub: (String) -> String, state: LazyListState,
    onOpen: (Contact) -> Unit, onAdd: () -> Unit, top: @Composable () -> Unit = {}, header: @Composable () -> Unit = {},
    waitingIn: List<ContactRequest> = emptyList(), waitingOut: List<ContactRequest> = emptyList(),
    onAccept: (String) -> Unit = {}, onNotNow: (String) -> Unit = {},
) {
    val c = Tin.c
    var q by remember { mutableStateOf("") }
    fun match(ct: Contact) = q.isBlank() || ct.display().contains(q.trim(), true) || ct.name.contains(q.trim(), true)
    val sorted = contacts.sortedBy { it.display().lowercase() }
    val mine = sorted.filter { !blocked(it.did) && match(it) }
    val gone = sorted.filter { blocked(it.did) && match(it) }
    val total = contacts.count { !blocked(it.did) }
    val groups = mine.groupBy { (it.display().firstOrNull() ?: '#').uppercaseChar() }
    LazyColumn(Modifier.fillMaxSize(), state = state, contentPadding = PaddingValues(bottom = 24.dp)) {
        item(key = "_title") { header() }
        item(key = "_top") { top() }
        item(key = "_find") {
            var focused by remember { mutableStateOf(false) }
            Row(
                Modifier.padding(start = 16.dp, end = 16.dp, top = 4.dp, bottom = 8.dp).fillMaxWidth().height(44.dp)
                    .then(if (focused) Modifier.border(2.dp, c.accent, RoundedCornerShape(22.dp)) else Modifier)
                    .clip(RoundedCornerShape(22.dp)).background(c.sf2).padding(horizontal = 14.dp),
                verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Icon(PgIcons.Search, null, tint = c.ink2, modifier = Modifier.size(18.dp))
                Box(Modifier.weight(1f), contentAlignment = Alignment.CenterStart) {
                    if (q.isEmpty()) Text("Find a contact", style = TinFs.body, color = c.ink2)
                    BasicTextField(q, { q = it }, Modifier.fillMaxWidth().onFocusChanged { focused = it.isFocused }.semantics { contentDescription = "Find a contact" },
                        singleLine = true, textStyle = TinFs.body.copy(color = c.ink), cursorBrush = SolidColor(c.accentInk))
                }
            }
        }
        item(key = "_add") {
            Row(
                Modifier.padding(horizontal = 8.dp).fillMaxWidth().heightIn(min = 64.dp).pressable(onAdd, RowShape).padding(horizontal = 8.dp),
                verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Box(Modifier.size(44.dp).clip(CircleShape).background(c.accentSoft), contentAlignment = Alignment.Center) {
                    Icon(PgIcons.UserPlus, null, tint = c.accentInk, modifier = Modifier.size(22.dp))
                }
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(1.dp)) {
                    Text("Add contact", style = TinFs.body.copy(fontWeight = FontWeight.SemiBold), color = c.ink)
                    Text("Share your code or paste theirs", style = TinFs.sub, color = c.ink2)
                }
            }
        }
        // People waiting for my answer: a card each (Accept / Not now). No "declined" anywhere: an unanswered request waits.
        if (waitingIn.isNotEmpty()) {
            item(key = "_hrq") { ListSection("Waiting for you", Modifier.padding(start = 16.dp, end = 16.dp, top = 18.dp, bottom = 6.dp)) }
            items(waitingIn, key = { "rq" + it.did }) { r ->
                Column(Modifier.padding(horizontal = 8.dp, vertical = 6.dp).fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(c.sf)
                    .border(1.dp, c.hairline, RoundedCornerShape(14.dp)).padding(14.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                        Avatar(r.name.ifBlank { "?" }, r.did, 44.dp)
                        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(1.dp)) {
                            Text(r.name, style = TinFs.body.copy(fontWeight = FontWeight.SemiBold), color = c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            Text("Wants to add you as a contact", style = TinFs.sub, color = c.ink2)
                        }
                    }
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        PBtn("Accept", { onAccept(r.did) }, variant = PVariant.Primary)
                        PBtn("Not now", { onNotNow(r.did) }, variant = PVariant.Quiet)
                    }
                    Text("Accepting lets you message and call each other.", style = TinFs.sub.copy(lineHeight = 19.sp), color = c.ink2)
                }
            }
        }
        if (waitingOut.isNotEmpty()) {
            item(key = "_hsent") { ListSection("Waiting for them", Modifier.padding(start = 16.dp, end = 16.dp, top = 18.dp, bottom = 6.dp)) }
            items(waitingOut, key = { "sent" + it.did }) { x ->
                Row(Modifier.padding(horizontal = 8.dp).fillMaxWidth().heightIn(min = 64.dp).padding(8.dp),
                    verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    Avatar(x.name.ifBlank { "?" }, x.did, 40.dp)
                    Column(Modifier.weight(1f)) {
                        Text(x.name, style = TinFs.body.copy(fontWeight = FontWeight.SemiBold), color = c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        Text("Request sent. Your line opens when ${firstName(x.name)} accepts.", style = TinFs.sub, color = c.ink2)
                    }
                }
            }
        }
        groups.forEach { (letter, list) ->
            item(key = "_h$letter") { ListSection("$letter", Modifier.padding(start = 16.dp, end = 16.dp, top = 18.dp, bottom = 6.dp)) }
            items(list, key = { it.did }) { ct ->
                Box(Modifier.padding(horizontal = 8.dp)) { PersonRow(ct.display(), ct.did, sub(ct.did), onClick = { onOpen(ct) }) }
            }
        }
        if (gone.isNotEmpty()) {
            item(key = "_hb") { ListSection("Blocked", Modifier.padding(start = 16.dp, end = 16.dp, top = 18.dp, bottom = 6.dp)) }
            items(gone, key = { "b" + it.did }) { ct ->
                Box(Modifier.padding(horizontal = 8.dp)) { PersonRow(ct.display(), ct.did, "Blocked", onClick = { onOpen(ct) }, dim = true) }
            }
        }
        if (total == 0 && gone.isEmpty() && waitingOut.isEmpty() && q.isBlank()) item(key = "_empty") { EmptyQuietLine(EmptyQuiet.CONTACTS) }
        else if (total == 0 && q.isBlank()) Unit
        else item(key = "_foot") {
            val t = if (q.isNotBlank() && mine.isEmpty() && gone.isEmpty()) "No one called “${q.trim()}” in your contacts." else "$total ${if (total == 1) "contact" else "contacts"}"
            Text(t, Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 14.dp).heightIn(min = 19.dp), style = TinFs.sub.copy(lineHeight = 19.sp), color = c.ink2, textAlign = TextAlign.Center)
        }
    }
}

/** A group row's mark in words (screen reader). */
private fun listTickLabel(v: TickVis): String = when (v) {
    TickVis.SLACK -> "Waiting to send"
    TickVis.ONE -> "Sent"
    TickVis.TWO -> "Delivered to everyone"
    TickVis.READ -> "Read by everyone"
    TickVis.BAD -> "Not delivered"
}
