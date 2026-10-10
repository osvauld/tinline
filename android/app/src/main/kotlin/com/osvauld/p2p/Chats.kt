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
import uniffi.p2pcore.Chat
import uniffi.p2pcore.Contact

/** The three tabs of the bottom bar. Chats opens by default. */
enum class HomeTab { Chats, Calls, Contacts }

/** Bottom bar: Chats · Calls · Contacts, with a red count on Chats (unread) and Calls (missed since you looked). */
@Composable
fun TinNavBar(tab: HomeTab, onTab: (HomeTab) -> Unit, chatBadge: Int = 0, callBadge: Int = 0) {
    val c = Tin.c
    Row(Modifier.fillMaxWidth().background(c.sf2).navigationBarsPadding().height(80.dp)) {
        listOf(
            Triple(HomeTab.Chats, Icons.Rounded.ChatBubbleOutline to Icons.Rounded.Chat, "Chats") to chatBadge,
            Triple(HomeTab.Calls, Icons.Rounded.Call to Icons.Rounded.Call, "Calls") to callBadge,
            Triple(HomeTab.Contacts, Icons.Rounded.PeopleOutline to Icons.Rounded.People, "Contacts") to 0,
        ).forEach { (t, badge) ->
            val (id, icons, label) = t
            val on = id == tab
            Column(
                Modifier.weight(1f).fillMaxHeight().clickable(role = Role.Tab) { onTab(id) },
                horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(4.dp, Alignment.CenterVertically),
            ) {
                Box(Modifier.size(64.dp, 32.dp).clip(RoundedCornerShape(50)).background(if (on) c.prc else androidx.compose.ui.graphics.Color.Transparent), contentAlignment = Alignment.Center) {
                    Icon(if (on) icons.second else icons.first, null, tint = if (on) c.onPrc else c.ink2)
                    if (badge > 0) Box(
                        Modifier.align(Alignment.TopEnd).offset(x = (-6).dp, y = (-2).dp).defaultMinSize(18.dp, 18.dp).clip(RoundedCornerShape(50)).background(c.callEnd).padding(horizontal = 4.dp),
                        contentAlignment = Alignment.Center,
                    ) { Text(if (badge > 99) "99+" else "$badge", style = TinType.caption.copy(fontSize = 11.sp, fontWeight = FontWeight.Bold), color = androidx.compose.ui.graphics.Color.White) }
                }
                Text(label, style = TinType.caption.copy(fontWeight = if (on) FontWeight.Bold else FontWeight.Medium), color = if (on) c.ink else c.ink2)
            }
        }
    }
}

/** Chat list rows from the core's conversations. Only conversations that have a message; a contact's name wins over what they call themselves.
 *  "You" (our DID: saved items for our own devices) is always there, pinned first. */
fun chatRows(chats: List<Chat>, contacts: List<Contact>, links: Map<String, Link>, online: Boolean, nowMs: Long, myDid: String? = null): List<ChatRowUi> {
    val by = contacts.associateBy { it.did }
    val you = chats.firstOrNull { it.peerDid == myDid }?.let { ch ->
        val at = ch.lastActivity.toLong()
        ChatRowUi(ch.peerDid, "You", ch.preview.ifBlank { SELF_SUB }, if (at > 0) listTime(at, nowMs) else "", false, Tick.Two, ch.unread.toInt())
    }
    return listOfNotNull(you) + chats.filter { it.lastActivity > 0UL && it.peerDid != myDid }.map { ch ->
        val name = by[ch.peerDid]?.display() ?: ch.peerName.ifBlank { "Unknown" }
        val at = ch.lastActivity.toLong()
        val pending = ch.lastOutgoing && ch.lastDelivery == uniffi.p2pcore.DeliveryState.PENDING
        val waiting = pending && (links[ch.peerDid] == Link.Offline || (links[ch.peerDid] == null && !online))
        ChatRowUi(
            ch.peerDid, name,
            if (waiting) "Waiting to send — ${firstName(name)} isn’t reachable yet" else ch.preview,
            listTime(at, nowMs), ch.lastOutgoing,
            when { waiting -> Tick.Clock; ch.lastDelivery == uniffi.p2pcore.DeliveryState.DELIVERED -> Tick.Two; else -> Tick.One },
            ch.unread.toInt(),
        )
    }
}

/** Chats tab body: the list, or the empty state. The FAB sits above the bar. */
@Composable
fun ChatsBody(rows: List<ChatRowUi>, query: String, banner: @Composable () -> Unit, onSearch: () -> Unit, onChat: (String) -> Unit, onNewChat: () -> Unit, searching: Boolean, myDid: String? = null) {
    val c = Tin.c
    // Only an unused "You" row: still the empty state, with a way into "You".
    if (rows.all { it.did == myDid && it.time.isEmpty() } && !searching) {
        Column(Modifier.fillMaxSize()) {
            banner()
            Column(
                Modifier.weight(1f).fillMaxWidth().padding(start = 32.dp, end = 32.dp, bottom = 40.dp),
                horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(16.dp, Alignment.CenterVertically),
            ) {
                StringIllustration(height = 150.dp)
                Text("No messages yet", style = TinType.titleL, color = c.ink, textAlign = TextAlign.Center)
                Text("Messages go straight from your phone to theirs, end-to-end encrypted. Nothing is stored on a server.", style = TinType.bodyL, color = c.ink2, textAlign = TextAlign.Center)
                Spacer(Modifier.height(2.dp))
                TinButton("Start a chat", onNewChat)
                Hint("You can message anyone in your contacts.", align = TextAlign.Center)
                if (myDid != null && rows.any { it.did == myDid }) TinButton("Notes to yourself", { onChat(myDid) }, style = BtnStyle.Outlined)
            }
        }
        return
    }
    val shown = rows.filter { query.isBlank() || it.name.contains(query.trim(), true) || it.preview.contains(query.trim(), true) }
    Box(Modifier.fillMaxSize()) {
        LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 112.dp)) {
            if (!searching) {
                item { banner() }
                item {
                    Row(
                        Modifier.padding(start = 16.dp, end = 16.dp, top = 4.dp, bottom = 4.dp).fillMaxWidth().heightIn(min = 52.dp).clip(RoundedCornerShape(50)).background(c.sf2)
                            .clickable(role = Role.Button, onClick = onSearch).padding(horizontal = 16.dp),
                        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp),
                    ) {
                        Icon(Icons.Rounded.Search, null, tint = c.ink2)
                        Text("Search chats", style = TinType.bodyL, color = c.ink2)
                    }
                }
            }
            items(shown, key = { it.did }) { ChatRow(it, onClick = { onChat(it.did) }) }
            if (searching && shown.isEmpty()) item { Hint("No chats match.", Modifier.padding(24.dp)) }
        }
        if (!searching) NewChatFab(onNewChat, Modifier.align(Alignment.BottomEnd))
    }
}

@Composable
private fun NewChatFab(onClick: () -> Unit, modifier: Modifier) {
    val c = Tin.c
    Row(
        modifier.padding(end = 20.dp, bottom = 20.dp).shadow(3.dp, RoundedCornerShape(16.dp)).clip(RoundedCornerShape(16.dp)).background(c.pr)
            .clickable(role = Role.Button, onClick = onClick).heightIn(min = 56.dp).padding(start = 16.dp, end = 20.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Icon(Icons.Rounded.Edit, null, tint = c.onPr)
        Text("New chat", style = TinType.button, color = c.onPr)
    }
}

@Composable
private fun ChatRow(r: ChatRowUi, onClick: () -> Unit) {
    val c = Tin.c
    val unread = r.unread > 0
    Row(
        Modifier.fillMaxWidth().heightIn(min = 76.dp).clickable(role = Role.Button, onClick = onClick).padding(horizontal = 20.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        Avatar(r.name, r.did, 48.dp)
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Row(verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(r.name, Modifier.weight(1f), style = TinType.bodyL.copy(fontWeight = FontWeight.SemiBold, lineHeight = 22.sp), color = c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(r.time, style = TinType.caption.copy(fontWeight = if (unread) FontWeight.Bold else FontWeight.Medium), color = if (unread) c.pr else c.ink2)
            }
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                if (r.mine) TickIcon(r.tick, when (r.tick) { Tick.Two -> c.pr; Tick.Clock -> c.thread; Tick.One -> c.ink2 }, 16.dp)
                Text(
                    r.preview, Modifier.weight(1f), style = TinType.bodyM, maxLines = 1, overflow = TextOverflow.Ellipsis,
                    color = if (unread) c.ink else c.ink2, fontWeight = if (unread) FontWeight.SemiBold else FontWeight.Normal,
                )
                if (unread) Box(Modifier.defaultMinSize(22.dp, 22.dp).clip(RoundedCornerShape(50)).background(c.pr).padding(horizontal = 6.dp), contentAlignment = Alignment.Center) {
                    Text("${r.unread}", style = TinType.caption.copy(fontWeight = FontWeight.Bold), color = c.onPr)
                }
            }
        }
    }
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
                    Icon(Icons.Rounded.Search, null, tint = c.ink2)
                    Text("Search contacts", style = TinType.bodyL, color = c.ink2)
                }
            }
            if (!searching) item {
                Row(
                    Modifier.fillMaxWidth().heightIn(min = 68.dp).clickable(role = Role.Button, onClick = onAdd).padding(horizontal = 20.dp),
                    verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp),
                ) {
                    Box(Modifier.size(48.dp).clip(CircleShape).background(c.pr), contentAlignment = Alignment.Center) { Icon(Icons.Rounded.PersonAdd, null, tint = c.onPr) }
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
