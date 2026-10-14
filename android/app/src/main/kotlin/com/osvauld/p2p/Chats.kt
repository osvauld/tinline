package com.osvauld.p2p

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.*
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.material.icons.outlined.Groups as GroupsOutlined
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import uniffi.p2pcore.Chat
import uniffi.p2pcore.Contact

/** The tabs of the bottom bar. Chats opens by default. Groups is group chat on the core (GroupsLive.kt). */
enum class HomeTab { Chats, Groups, Calls, Contacts }

/** Chat list rows from the core's conversations. Only conversations that have a message; a contact's name wins over what they call themselves.
 *  "You" (our DID: saved items for our own devices) is always there, pinned first. */
fun chatRows(chats: List<Chat>, contacts: List<Contact>, links: Map<String, Link>, online: Boolean, nowMs: Long, myDid: String? = null, blocks: Blocks = Blocks()): List<ChatRowUi> {
    val by = contacts.associateBy { it.did }
    val you = chats.firstOrNull { it.peerDid == myDid }?.let { ch ->
        val at = ch.lastActivity.toLong()
        ChatRowUi(ch.peerDid, "You", ch.preview.ifBlank { SELF_SUB }, if (at > 0) listTime(at, nowMs) else "", false, Tick.Two, ch.unread.toInt(), atMs = at)
    }
    return listOfNotNull(you) + chats.filter { it.lastActivity > 0UL && it.peerDid != myDid }.map { ch ->
        val name = by[ch.peerDid]?.display() ?: ch.peerName.ifBlank { "Unknown" }
        val at = ch.lastActivity.toLong()
        val pending = ch.lastOutgoing && ch.lastDelivery == uniffi.p2pcore.DeliveryState.PENDING
        val waiting = pending && (links[ch.peerDid] == Link.Offline || (links[ch.peerDid] == null && !online))
        // Blocked people stay in the list with a red line, so the state is never hidden.
        val status = blocks.status(ch.peerDid)
        ChatRowUi(
            ch.peerDid, name,
            status ?: if (waiting) "Waiting to send — ${firstName(name)} isn’t reachable yet" else ch.preview,
            listTime(at, nowMs), ch.lastOutgoing,
            when { waiting -> Tick.Clock; ch.lastDelivery == uniffi.p2pcore.DeliveryState.DELIVERED -> Tick.Two; else -> Tick.One },
            ch.unread.toInt(),
            alert = status != null, atMs = at,
        )
    }
}

/** Active groups as Chats rows (the playground's Chats tab lists 1:1 and group conversations together, newest first). */
fun groupChatRows(list: List<uniffi.p2pcore.GroupSummary>, me: String, nowMs: Long): List<ChatRowUi> =
    list.filter { GroupText.active(it.myRole) }.map { s ->
        val at = s.last?.at?.toLong() ?: 0L
        ChatRowUi(s.id, s.name, GroupText.preview(s, me), if (at > 0) listTime(at, nowMs) else "", false, Tick.Two, s.unread.toInt(), group = true, atMs = at)
    }

/** "New chat": pick a contact, or add one. */
@Composable
fun NewChatScreen(contacts: List<Contact>, lastMessage: Map<String, String>, onBack: () -> Unit, onPick: (Contact) -> Unit, onAdd: () -> Unit) {
    val c = Tin.c
    var query by remember { mutableStateOf("") }
    var searching by remember { mutableStateOf(false) }
    val sorted = remember(contacts) { contacts.sortedBy { it.display().lowercase() } }
    val shown = sorted.filter { query.isBlank() || it.display().contains(query.trim(), true) || it.name.contains(query.trim(), true) }
    Page {
        if (searching) SearchBar(query, { query = it }, onClose = { searching = false; query = "" }, hint = "Search contacts")
        else TopBar("New chat", onBack)
        LazyColumn(Modifier.weight(1f), contentPadding = PaddingValues(bottom = 24.dp)) {
            if (!searching) item {
                Row(
                    Modifier.padding(start = 16.dp, end = 16.dp, top = 4.dp, bottom = 4.dp).fillMaxWidth().heightIn(min = 52.dp).clip(RoundedCornerShape(50)).background(c.sf2)
                        .clickable(role = Role.Button) { searching = true }.padding(horizontal = 16.dp),
                    verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    Icon(PgIcons.Search, null, tint = c.ink2)
                    Text("Search contacts", style = TinType.bodyL, color = c.ink2)
                }
            }
            if (!searching) item {
                Row(
                    Modifier.fillMaxWidth().heightIn(min = 68.dp).clickable(role = Role.Button, onClick = onAdd).padding(horizontal = 20.dp),
                    verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp),
                ) {
                    Box(Modifier.size(48.dp).clip(CircleShape).background(c.pr), contentAlignment = Alignment.Center) { Icon(PgIcons.Userplus, null, tint = c.onPr) }
                    Column(Modifier.weight(1f)) {
                        Text("Add a contact", style = TinType.bodyL.copy(fontWeight = FontWeight.SemiBold, lineHeight = 22.sp), color = c.ink)
                        Text("Scan their code or paste their card", style = TinType.bodyM, color = c.ink2)
                    }
                }
            }
            if (contacts.isNotEmpty()) item { SectionLabel("CONTACTS · ${contacts.size}") }
            items(shown, key = { it.did }) { ct ->
                Row(
                    Modifier.fillMaxWidth().heightIn(min = 68.dp).clickable(role = Role.Button) { onPick(ct) }.padding(horizontal = 20.dp),
                    verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp),
                ) {
                    Avatar(ct.display(), ct.did, 48.dp)
                    Column(Modifier.weight(1f)) {
                        Text(ct.display(), style = TinType.bodyL.copy(fontWeight = FontWeight.SemiBold, lineHeight = 22.sp), color = c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        Text(lastMessage[ct.did] ?: "No messages yet", style = TinType.bodyM, color = c.ink2)
                    }
                }
            }
        }
    }
}
