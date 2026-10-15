package com.osvauld.p2p

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/*
 * PLACEHOLDER screens for chat requests, blocking, per-contact permissions and address changes
 * (boards AndroidChatRequest, AndroidBlocking, AndroidPrivacy). Backed by [FakeGroupsPrivacy].
 */

private fun first(name: String) = name.trim().substringBefore(' ')
private fun Outcome.reason() = (this as? Outcome.Refused)?.reason

// ------------------------------------------------------------------ chat requests

/** "Ask Leo for a private chat", from a member's sheet. */
@Composable
fun AskPrivateChatSheet(myName: String, name: String, group: String, onDismiss: () -> Unit, onSend: (String) -> Unit) {
    val c = Tin.c
    val fn = first(name)
    var note by rememberSaveable { mutableStateOf("") }
    TinSheet(onDismiss) {
        Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
            Row(Modifier.padding(top = 18.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                PersonAvatar(name, FakeGroupsPrivacy.did(name), 48.dp, contact = false)
                Column {
                    Text("Ask $fn for a private chat", style = TinType.titleL.copy(fontSize = 20.sp), color = c.ink, modifier = Modifier.semantics { heading() })
                    Hint("Member of $group")
                }
            }
            TinField(note, { note = it }, "Add a note (optional)", placeholder = "Hi $fn, it’s ${first(myName)}.")
            Text("$fn sees your name, the group and your note. The request goes through the group’s host sealed, so the host can’t read it.", style = TinType.bodyM, color = c.ink2)
            Text("If $fn accepts, you can message each other directly. Calls stay off until one of you allows them.", style = TinType.bodyM, color = c.ink2)
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End)) {
                TinButton("Cancel", onDismiss, style = BtnStyle.Text, fill = false)
                TinButton("Send request", { onSend(note) }, fill = false)
            }
        }
    }
}

/** Incoming request card, in its own block above chats. Never rings. */
@Composable
fun IncomingRequestCard(src: FakeGroupsPrivacy, nav: PNav, r: IncomingRequest) {
    val c = Tin.c
    val fn = first(r.fromName)
    Column(
        Modifier.padding(horizontal = 16.dp).fillMaxWidth().clip(SmoothCornerShape(20.dp)).background(c.sf).border(1.dp, c.ln, SmoothCornerShape(20.dp)).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            PersonAvatar(r.fromName, r.fromDid, 48.dp)
            Column(Modifier.weight(1f)) {
                Text(r.fromName, style = TinType.bodyL.copy(fontSize = 17.sp, fontWeight = FontWeight.Bold), color = c.ink)
                Hint("Member of ${r.viaGroup} · you share ${if (r.sharedGroups == 1) "1 group" else "${r.sharedGroups} groups"}")
            }
        }
        if (r.note.isNotBlank()) Text(r.note, Modifier.fillMaxWidth().clip(SmoothCornerShape(14.dp)).background(c.sf2).padding(horizontal = 12.dp, vertical = 10.dp),
            style = TinType.bodyL.copy(fontSize = 15.sp, lineHeight = 21.sp), color = c.ink)
        Hint("Accepting adds $fn as a contact for messages only. You can allow calls later.")
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            TinButton("Accept", { src.acceptRequest(r.id)?.let { nav.go(PRoute.Dm(it)) } }, Modifier.weight(1f), height = 48.dp)
            TinButton("Decline", { src.declineRequest(r.id); nav.say("$fn is told you declined") }, Modifier.weight(1f), style = BtnStyle.Outlined, height = 48.dp)
        }
        Box(Modifier.fillMaxWidth().heightIn(min = 48.dp).clip(RoundedCornerShape(50)).clickable(role = Role.Button) { src.blockRequester(r.id); nav.say("$fn is blocked and told so") },
            contentAlignment = Alignment.Center) {
            Text("Block $fn", style = TinType.label, color = c.er)
        }
    }
}

/** Sent request row: waiting, declined or expired, each worded once. */
@Composable
fun SentRequestRow(src: FakeGroupsPrivacy, r: SentRequest, now: Long) {
    val c = Tin.c
    val fn = first(r.toName)
    val (sub, red) = when (r.state) {
        SentRequestState.Waiting -> "Waiting for $fn · via ${r.viaGroup}" to false
        SentRequestState.Declined -> "$fn declined" to true
        SentRequestState.Expired -> "No answer after 7 days · request expired" to false
        SentRequestState.Accepted -> "$fn accepted" to false
    }
    Row(Modifier.fillMaxWidth().heightIn(min = 72.dp).padding(start = 20.dp, end = 8.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp)) {
        PersonAvatar(r.toName, r.toDid, 48.dp, contact = false)
        Column(Modifier.weight(1f)) {
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(r.toName, Modifier.weight(1f), style = TinType.bodyL.copy(fontWeight = FontWeight.SemiBold), color = c.ink)
                Text(agoOrDate(r.endedAtMs ?: r.sentAtMs, now), style = TinType.caption.copy(fontWeight = FontWeight.Normal), color = c.ink2)
            }
            Text(sub, style = TinType.bodyM, color = if (red) c.er else c.ink2, maxLines = 1)
        }
        if (r.state == SentRequestState.Waiting) IconBtn(PgIcons.X, "Cancel request to $fn", { src.cancelRequest(r.id) }, tint = c.ink2)
        else IconBtn(PgIcons.X, "Dismiss", { src.dismissRequest(r.id) }, tint = c.ink2)
    }
}

// ------------------------------------------------------------------ 1:1 chat

private sealed interface DmRow {
    data class Item(val item: DmItem) : DmRow
    data class BlockedNotice(val notice: BlockNotice) : DmRow
    data object CallsChip : DmRow
    data class Attempts(val n: Int) : DmRow
    data object Lost : DmRow
}

/**
 * 1:1 chat for preview contacts: accepted messages-only chat with the Allow calls chip, blocker's view,
 * blocked person's view, lost contact, group invite cards (boards Main, AndroidChatRequest, AndroidBlocking, AndroidPrivacy).
 */
@Composable
fun PDmScreen(src: FakeGroupsPrivacy, nav: PNav, did: String) {
    val c = Tin.c
    val people by src.people.collectAsState()
    val blocked by src.blocked.collectAsState()
    val invites by src.invites.collectAsState()
    val p = people.firstOrNull { it.did == did } ?: run { LaunchedEffect(Unit) { nav.back() }; return }
    val fn = first(p.name)
    val iBlocked = blocked.firstOrNull { it.did == did }
    var draft by rememberSaveable { mutableStateOf("") }
    var callSheet by remember { mutableStateOf(false) }
    val list = rememberLazyListState()
    LaunchedEffect(did) { src.markPersonRead(did) }

    val rows = remember(p, iBlocked) {
        val out = mutableListOf<DmRow>()
        val notice = p.blockedMe
        var noticeShown = notice == null
        p.items.sortedBy { it.atMs }.forEachIndexed { i, it ->
            if (!noticeShown && notice != null && it.atMs > notice.atMs) { out += DmRow.BlockedNotice(notice); noticeShown = true }
            out += DmRow.Item(it)
            if (i == 1 && p.origin == ContactOrigin.GroupRequest && !p.callsChipDone && !p.canCall) out += DmRow.CallsChip
        }
        if (!noticeShown && notice != null) out += DmRow.BlockedNotice(notice)
        if (p.origin == ContactOrigin.GroupRequest && !p.callsChipDone && !p.canCall && out.none { it is DmRow.CallsChip }) out += DmRow.CallsChip
        if (iBlocked != null && p.attemptsSinceBlock > 0) out += DmRow.Attempts(p.attemptsSinceBlock)
        if (p.lost) out += DmRow.Lost
        out
    }
    LaunchedEffect(rows.size) { if (rows.isNotEmpty()) list.scrollToItem(rows.lastIndex) }
    val check = src.callCheck(did)
    PreviewPage {
        ChatHeader(
            p.name, nav.back,
            sub = when {
                iBlocked != null -> "Blocked"
                p.blockedMe != null -> "Blocked you"
                p.lost -> "Can’t reach $fn"
                !p.canCall && !p.theyAllowMyCalls -> "Messages only"
                else -> null
            },
            subColor = when { iBlocked != null || p.blockedMe != null -> c.er; p.lost -> c.threadText; else -> c.ink2 },
            avatar = { PersonAvatar(p.name, p.did, 40.dp) },
            onTitle = { nav.go(PRoute.Contact(did)) },
        ) {
            val allowed = check == CallCheck.Allowed
            IconBtn(if (allowed) PgIcons.Phone else PgIcons.Callend, if (allowed) "Call $fn" else "Call $fn, not available", {
                when (check) {
                    CallCheck.Allowed -> nav.say("Calls aren’t connected in the preview")
                    CallCheck.TheyHaveNotAllowed -> callSheet = true
                    CallCheck.IBlocked -> nav.say("You blocked $fn. Unblock to call.")
                    CallCheck.BlockedMe -> nav.say("Call ended: $fn blocked you")
                    CallCheck.Lost -> nav.say("Can’t reach $fn until you reconnect")
                }
            }, tint = if (allowed) c.ink else c.ink2)
        }
        LazyColumn(Modifier.weight(1f).fillMaxWidth(), state = list, contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            items(rows) { row ->
                Column(Modifier.fillMaxWidth()) {
                    when (row) {
                        is DmRow.Item -> DmItemView(src, nav, p, row.item, invites, iBlocked)
                        is DmRow.BlockedNotice -> NoticeCard(c.erc, c.onErc, "$fn blocked you",
                            "You can’t message or call $fn. Your earlier messages stay here." + sharedGroupLine(src, did)) {
                            Text("Signed by $fn · ${whenFull(row.notice.atMs)}", style = TinType.caption.copy(fontWeight = FontWeight.Normal), color = c.onErc)
                        }
                        DmRow.CallsChip -> Row(
                            Modifier.align(Alignment.CenterHorizontally).clip(SmoothCornerShape(16.dp)).background(c.sf).border(1.dp, c.ln, SmoothCornerShape(16.dp))
                                .padding(start = 14.dp, end = 6.dp, top = 6.dp, bottom = 6.dp),
                            verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp),
                        ) {
                            Text("Calls are off for $fn.", style = TinType.bodyM, color = c.ink)
                            TinButton("Allow calls", { src.setCanCall(did, true); nav.say("$fn can call you now") }, style = BtnStyle.Outlined, fill = false, height = 40.dp, textStyle = TinType.label)
                            IconBtn(PgIcons.X, "Dismiss", { src.dismissCallsChip(did) }, tint = c.ink2)
                        }
                        is DmRow.Attempts -> SysLine("$fn tried to message you ${if (row.n == 1) "once" else "${row.n} times"} since")
                        DmRow.Lost -> NoticeCard(c.warn, c.onWarn, "You and $fn lost each other’s address",
                            "You were both offline while your addresses changed, and you share no group that could pass the new ones on. Meet or send your QR another way to reconnect.") {
                            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                TinButton("Show my QR", nav.showMyQr, Modifier.weight(1f), height = 48.dp)
                                TinButton("Scan $fn’s", { src.reconnect(did); nav.say("Placeholder: reconnected as if $fn’s QR was scanned") }, Modifier.weight(1f), style = BtnStyle.Outlined, height = 48.dp)
                            }
                            Text("Your chat history and verification are kept.", style = TinType.caption.copy(fontWeight = FontWeight.Normal), color = c.onWarn)
                        }
                    }
                }
            }
        }
        when {
            iBlocked != null -> BlockedFooter("You blocked $fn. $fn can’t message or call you.") {
                TinButton("Unblock", { src.unblock(did) }, Modifier.weight(1f), style = BtnStyle.Outlined, height = 48.dp)
            }
            p.blockedMe != null -> BlockedFooter("You can’t send messages to $fn.")
            else -> Composer(draft, { draft = it }, "Message", onSend = { src.sendText(did, draft).reason()?.let(nav.say); draft = "" })
        }
    }
    if (callSheet) CallsNotAllowedSheet(p.name, onDismiss = { callSheet = false }) {
        src.askToAllowCalls(did).reason()?.let(nav.say) ?: nav.say("Asked $fn to allow your calls"); callSheet = false
    }
}

private fun sharedGroupLine(src: FakeGroupsPrivacy, did: String): String {
    val shared = src.groups.value.filter { g -> g.removal == null && g.activeMembers.any { it.did == did } }.map { it.name }
    return if (shared.isEmpty()) "" else " You’re both still in ${shared.joinToString()}."
}

@Composable
private fun ColumnScope.DmItemView(src: FakeGroupsPrivacy, nav: PNav, p: Person, item: DmItem, invites: List<GroupInvite>, iBlocked: BlockedPerson?) {
    val c = Tin.c
    val fn = first(p.name)
    when (item) {
        is DmItem.Text -> if (item.outgoing) {
            val (label, color) = when (item.tick) {
                DmTick.Sent -> "Sent" to null
                DmTick.Delivered -> "Delivered" to null
                DmTick.Read -> "Read" to null
                DmTick.WaitingToReach -> "Waiting to reach $fn" to c.threadText
                DmTick.NotDeliveredBlocked -> "Not delivered: $fn blocked you" to c.er
            }
            Bubble(item.text, true, tick = label, tickColor = color, greyed = item.tick == DmTick.NotDeliveredBlocked,
                tickIcon = if (item.tick == DmTick.NotDeliveredBlocked) ({ Icon(PgIcons.Warning, null, tint = c.er, modifier = Modifier.size(13.dp)) }) else null)
        } else Bubble(item.text, false)
        is DmItem.Sys -> SysLine(if (iBlocked != null && item.text.startsWith("You blocked")) "${item.text} · ${whenFull(item.atMs)}" else item.text)
        is DmItem.Invite -> invites.firstOrNull { it.id == item.inviteId }?.let { InviteCard(src, nav, it) }
        is DmItem.CallAsk -> if (item.outgoing) Bubble("${first(src.myName)} would like to be able to call you", true, tick = "Asked $fn to allow calls")
        else Column(
            Modifier.align(Alignment.Start).widthIn(max = 300.dp).clip(SmoothCornerShape(18.dp)).background(c.sf).border(1.dp, c.ln, SmoothCornerShape(18.dp)).padding(12.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text("$fn would like to be able to call you", style = TinType.bodyL.copy(fontSize = 15.sp), color = c.ink)
            if (item.handled) Hint(if (p.canCall) "You allowed calls" else "You kept calls off")
            else Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                TinButton("Allow", { src.answerCallAsk(p.did, item.id, true) }, fill = false, height = 40.dp, textStyle = TinType.label)
                TinButton("Not now", { src.answerCallAsk(p.did, item.id, false) }, style = BtnStyle.Text, fill = false, height = 40.dp, textStyle = TinType.label)
            }
        }
    }
}

/** Group invite card in the inviter's 1:1 chat (board Main). */
@Composable
internal fun ColumnScope.InviteCard(src: FakeGroupsPrivacy, nav: PNav, inv: GroupInvite) {
    val c = Tin.c
    val mine = inv.inviterDid == src.myDid
    val now = System.currentTimeMillis()
    val expired = inv.state == InviteState.Expired || (inv.state == InviteState.Pending && now > inv.expiresAtMs)
    Column(Modifier.align(if (mine) Alignment.End else Alignment.Start).width(300.dp)) {
        Column(
            Modifier.fillMaxWidth().clip(SmoothCornerShape(20.dp)).background(c.sf).border(1.dp, c.ln, SmoothCornerShape(20.dp)).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                GroupAvatar(0, 48.dp, radius = 16.dp, gid = inv.id)
                Column {
                    Text(inv.groupName, style = TinType.bodyL.copy(fontSize = 17.sp, fontWeight = FontWeight.Bold), color = c.ink)
                    Hint(if (mine) "Group invite you sent" else "Group invite from ${first(inv.inviterName)} · ${inv.memberCount} members")
                }
            }
            if (!mine) {
                Hint("Owner: ${inv.ownerName} · Hosts: ${inv.hostLabels.ifEmpty { listOf("none yet") }.joinToString()}")
                Text("Members will see your name and your messages. They won’t get your address: anyone who wants a private chat has to ask first.", style = TinType.bodyM, color = c.ink2)
            }
            when {
                mine -> Hint("Waiting for them to join")
                inv.state == InviteState.Joined -> Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Icon(PgIcons.Check, null, tint = c.pr, modifier = Modifier.size(20.dp))
                    Text("Joined", Modifier.weight(1f), style = TinType.label, color = c.pr)
                    inv.groupId?.let { gid -> TinButton("Open", { nav.go(PRoute.GroupChat(gid)) }, style = BtnStyle.Text, fill = false, height = 40.dp) }
                }
                expired -> Text("Invite expired", style = TinType.label, color = c.ink2)
                inv.state == InviteState.Declined -> Text("You said not now", style = TinType.label, color = c.ink2)
                else -> Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    TinButton("Join", { src.joinInvite(inv.id).reason()?.let(nav.say) }, Modifier.weight(1f), height = 44.dp)
                    TinButton("Not now", { src.declineInvite(inv.id) }, style = BtnStyle.Text, fill = false, height = 44.dp)
                }
            }
        }
        if (!mine && inv.state == InviteState.Pending && !expired)
            Text("Invite signed by ${first(inv.inviterName)} · expires ${inWords(inv.expiresAtMs, now).replaceFirstChar { it.lowercase() }}",
                Modifier.padding(start = 4.dp, top = 4.dp), style = TinType.caption.copy(fontWeight = FontWeight.Normal), color = c.ink2)
    }
}

/** "Leo hasn’t allowed calls from you": never dials (board AndroidPrivacy). */
@Composable
fun CallsNotAllowedSheet(name: String, onDismiss: () -> Unit, onAsk: () -> Unit) {
    val c = Tin.c
    val fn = first(name)
    TinSheet(onDismiss) {
        Column(Modifier.padding(top = 12.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text("$fn hasn’t allowed calls from you", style = TinType.titleL.copy(fontSize = 20.sp), color = c.ink, modifier = Modifier.semantics { heading() })
            DialogText("You can still message $fn. Ask $fn to turn on “Call you” for you in your contact page.")
            TinButton("Ask $fn to allow calls", onAsk)
            TinButton("OK", onDismiss, style = BtnStyle.Text, height = 48.dp)
        }
    }
}

// ------------------------------------------------------------------ block

/** Every consequence listed before the tap (board AndroidBlocking). Personal; groups unaffected. */
@Composable
fun BlockDialog(name: String, sharedGroups: List<String>, onDismiss: () -> Unit, onBlock: () -> Unit) {
    val c = Tin.c
    val fn = first(name)
    TinDialog("Block $fn?", onDismiss, "Block", onBlock, destructive = true, icon = PgIcons.Off) {
        BlockLine(PgIcons.Callend, "$fn can’t message or call you, on any of your devices.")
        BlockLine(PgIcons.Announce, "$fn is told you blocked them.")
        BlockLine(PgIcons.Off, "$fn won’t get your new address next time it changes, so $fn loses any way to reach this phone.")
        if (sharedGroups.isNotEmpty()) BlockLine(PgIcons.Users, null, rich = {
            Text(androidx.compose.ui.text.buildAnnotatedString {
                append("You both stay in ")
                pushStyle(androidx.compose.ui.text.SpanStyle(fontWeight = FontWeight.Bold, color = c.ink)); append(sharedGroups.joinToString()); pop()
                append(". $fn’s group messages show collapsed for you.")
            }, style = TinType.bodyM.copy(fontSize = 15.sp, lineHeight = 22.sp), color = c.ink2)
        })
        BlockLine(PgIcons.Clock, "Your chat history stays on this phone.")
    }
}

@Composable
private fun BlockLine(icon: ImageVector, text: String?, rich: (@Composable () -> Unit)? = null) {
    val c = Tin.c
    Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Icon(icon, null, tint = c.ink2, modifier = Modifier.size(22.dp))
        if (rich != null) rich() else Text(text.orEmpty(), style = TinType.bodyM.copy(fontSize = 15.sp, lineHeight = 22.sp), color = c.ink2)
    }
}

// ------------------------------------------------------------------ contact page

/** Contact page: Message / Call switches, connection, Block (no separate Remove: decision U19). */
@Composable
fun PContactScreen(src: FakeGroupsPrivacy, nav: PNav, did: String) {
    val c = Tin.c
    val people by src.people.collectAsState()
    val blocked by src.blocked.collectAsState()
    val groups by src.groups.collectAsState()
    val p = people.firstOrNull { it.did == did } ?: run { LaunchedEffect(Unit) { nav.back() }; return }
    val fn = first(p.name)
    val isBlocked = blocked.any { it.did == did }
    var blocking by remember { mutableStateOf(false) }
    val now = System.currentTimeMillis()
    PreviewPage {
        TopBar(null, nav.back)
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState())) {
            Column(Modifier.fillMaxWidth().padding(horizontal = 24.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(6.dp)) {
                PersonAvatar(p.name, p.did, 88.dp)
                Text(p.name, style = TinType.h1.copy(fontSize = 26.sp), color = c.ink, modifier = Modifier.semantics { heading() })
                Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    if (p.originGroup != null) SmallPill("From ${p.originGroup}", c.sf2, c.ink2)
                    if (p.verifiedAtMs != null) SmallPill("Verified", c.prc, c.onPrc)
                    if (isBlocked) SmallPill("Blocked", c.erc, c.onErc)
                }
            }
            SectionLabel("WHAT ${fn.uppercase()} CAN DO")
            SwitchRow(PgIcons.Chat, "Message you",
                when {
                    isBlocked -> "Off while blocked"
                    p.canMessage && p.origin == ContactOrigin.GroupRequest -> "On since you accepted $fn’s request"
                    p.canMessage -> "On. $fn can message you"
                    else -> "Off. $fn’s messages are refused"
                }, p.canMessage && !isBlocked, enabled = !isBlocked) { src.setCanMessage(did, it) }
            SwitchRow(PgIcons.Phone, "Call you",
                when {
                    isBlocked -> if (p.canCall) "Off while blocked. Comes back on if you unblock" else "Off while blocked"
                    p.canCall -> "On. $fn can ring your phone"
                    else -> "Off. Turn on to let $fn ring your phone"
                }, p.canCall && !isBlocked, enabled = !isBlocked) { src.setCanCall(did, it) }
            SectionLabel("CONNECTION")
            val theirs = p.theirAddressChangedAtMs?.let { " Theirs changed ${agoOrDate(it, now).let { s -> if (s.first().isDigit() || s.startsWith("Just")) s.lowercase() else "on $s" }}." } ?: ""
            InfoRow(PgIcons.Retry,
                when { p.lost -> "Can’t reach $fn"; isBlocked -> "Not getting your new address"; p.hasMyCurrentAddress -> "Address up to date"; else -> "Waiting to pass on your address" },
                when {
                    p.lost -> "You both changed address while apart. Re-scan a QR to reconnect."
                    isBlocked -> "Blocked people skip your address changes."
                    p.hasMyCurrentAddress -> "$fn has your current address.$theirs"
                    else -> "$fn gets it when either of you next connects.$theirs"
                })
            InfoRow(PgIcons.Shield, "Safety number", p.verifiedAtMs?.let { "Verified ${whenDate(it)}" } ?: "Not verified yet")
            val shared = groups.filter { g -> g.removal == null && g.activeMembers.any { it.did == did } }
            if (shared.isNotEmpty()) InfoRow(PgIcons.Users, "Groups in common", shared.joinToString { it.name })
        }
        Box(Modifier.padding(start = 20.dp, end = 20.dp, bottom = 16.dp)) {
            if (isBlocked) TinButton("Unblock", { src.unblock(did) }, style = BtnStyle.Outlined)
            else TinButton("Block", { blocking = true }, style = BtnStyle.OutlinedDanger, icon = PgIcons.Off)
        }
    }
    if (blocking) BlockDialog(p.name, groups.filter { g -> g.removal == null && g.activeMembers.any { it.did == did } }.map { it.name }, onDismiss = { blocking = false }) {
        src.block(did, p.name).reason()?.let(nav.say); blocking = false
    }
}

@Composable
private fun SwitchRow(icon: ImageVector, title: String, sub: String, on: Boolean, enabled: Boolean, onChange: (Boolean) -> Unit) {
    val c = Tin.c
    Row(
        Modifier.fillMaxWidth().heightIn(min = 64.dp).clickable(enabled = enabled, role = Role.Switch) { onChange(!on) }.padding(horizontal = 20.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Icon(icon, null, tint = c.ink2)
        Column(Modifier.weight(1f)) {
            Text(title, style = TinType.bodyL, color = if (enabled) c.ink else c.ink2)
            Text(sub, style = TinType.bodyM.copy(fontSize = 13.sp, lineHeight = 18.sp), color = c.ink2)
        }
        androidx.compose.material3.Switch(on, if (enabled) onChange else null, enabled = enabled,
            colors = androidx.compose.material3.SwitchDefaults.colors(
                checkedThumbColor = c.onPr, checkedTrackColor = c.pr, checkedBorderColor = c.pr,
                uncheckedThumbColor = c.ln2, uncheckedTrackColor = c.sf3, uncheckedBorderColor = c.ln2,
            ),
            modifier = Modifier.semantics { this.contentDescription = title })
    }
}

@Composable
private fun InfoRow(icon: ImageVector, title: String, sub: String) {
    val c = Tin.c
    Row(Modifier.fillMaxWidth().heightIn(min = 64.dp).padding(horizontal = 20.dp, vertical = 6.dp).semantics(mergeDescendants = true) {},
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp)) {
        Icon(icon, null, tint = c.ink2)
        Column(Modifier.weight(1f)) {
            Text(title, style = TinType.bodyL, color = c.ink)
            Text(sub, style = TinType.bodyM.copy(fontSize = 13.sp, lineHeight = 18.sp), color = c.ink2)
        }
    }
}

// ------------------------------------------------------------------ blocked people

/** Settings › Privacy › Blocked people: one list across all devices. */
@Composable
fun PBlockedScreen(src: FakeGroupsPrivacy, nav: PNav) {
    val c = Tin.c
    val blocked by src.blocked.collectAsState()
    val people by src.people.collectAsState()
    PreviewPage {
        TopBar("Blocked people", nav.back)
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState())) {
            if (blocked.isEmpty()) Hint("Nobody is blocked.", Modifier.padding(horizontal = 20.dp, vertical = 12.dp))
            blocked.forEach { b ->
                Row(Modifier.fillMaxWidth().heightIn(min = 72.dp).padding(start = 20.dp, end = 8.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp)) {
                    PersonAvatar(b.name, b.did, 44.dp, contact = people.any { it.did == b.did })
                    Column(Modifier.weight(1f)) {
                        Text(b.name, style = TinType.bodyL.copy(fontWeight = FontWeight.SemiBold), color = c.ink)
                        Hint("Blocked ${whenDate(b.atMs)} · ${if (b.via == BlockVia.ChatRequest) "from a chat request" else "contact"}")
                    }
                    Box(Modifier.heightIn(min = 48.dp).clip(RoundedCornerShape(50)).clickable(role = Role.Button, onClickLabel = "Unblock ${b.name}") { src.unblock(b.did); nav.say("${first(b.name)} is told they’re unblocked") }
                        .padding(horizontal = 12.dp), contentAlignment = Alignment.Center) {
                        Text("Unblock", style = TinType.button.copy(fontSize = 15.sp), color = c.pr)
                    }
                }
            }
            CardBox(Modifier.padding(20.dp).fillMaxWidth(), radius = 20.dp) {
                Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text("Unblocking", style = TinType.bodyL.copy(fontSize = 15.sp, fontWeight = FontWeight.Bold), color = c.ink)
                    Text("They can message you again and get your current address. Calls come back only if they were allowed before. They’re told they’re unblocked.",
                        style = TinType.bodyM, color = c.ink2)
                }
            }
        }
    }
}

// ------------------------------------------------------------------ address changes

/** Settings › Privacy › Address changes: one setting, how often (board AndroidPrivacy). */
@Composable
fun PAddressScreen(src: FakeGroupsPrivacy, nav: PNav) {
    val c = Tin.c
    val a by src.address.collectAsState()
    val now = System.currentTimeMillis()
    PreviewPage {
        TopBar("Address changes", nav.back)
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(horizontal = 20.dp, vertical = 4.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            Text("Your phone gets a new network address on a schedule. Your contacts and group hosts are told automatically; blocked people and anyone else holding an old address lose the way to reach you.",
                style = TinType.bodyM.copy(fontSize = 15.sp, lineHeight = 22.sp), color = c.ink2)
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("Change my address every", style = TinType.caption.copy(fontSize = 13.sp, fontWeight = FontWeight.SemiBold), color = c.ink2)
                Segmented(AddressInterval.entries, a.interval, { it.label }, { src.setInterval(it) }, "Interval")
            }
            CardBox(Modifier.fillMaxWidth(), radius = 20.dp) {
                Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    KeyValue("Last change", dayAndTime(a.lastChangeMs, now))
                    KeyValue("Next change", inWords(a.nextChangeMs, now))
                    KeyValue("Contacts told", "${a.contactsTold} of ${a.contactsTotal}")
                    val missing = a.contactsTotal - a.contactsTold
                    if (missing > 0) Hint("${if (missing == 1) "1 contact hasn’t" else "$missing contacts haven’t"} been online since. They’ll get it when either of you next connects.")
                }
            }
            InfoCard("Shorter intervals cut off old addresses sooner but use a little more battery, and a contact who is offline for longer than two of your intervals may need to re-share a QR.",
                icon = PgIcons.Warning, kind = BannerKind.Warn)
            Hint("Linked devices each change their own address; the setting syncs.")
        }
    }
}

@Composable
private fun KeyValue(k: String, v: String) {
    val c = Tin.c
    Row(Modifier.fillMaxWidth().semantics(mergeDescendants = true) {}) {
        Text(k, Modifier.weight(1f), style = TinType.bodyL.copy(fontSize = 15.sp), color = c.ink)
        Text(v, style = TinType.bodyL.copy(fontSize = 15.sp, fontWeight = FontWeight.Bold), color = c.ink, textAlign = TextAlign.End)
    }
}

// ------------------------------------------------------------------ requests screen

/** All chat requests: incoming cards, then sent ones (board AndroidChatRequest), behind the compact row. */
@Composable
fun PRequestsScreen(src: FakeGroupsPrivacy, nav: PNav) {
    val reqs by src.requests.collectAsState()
    val now = System.currentTimeMillis()
    LaunchedEffect(Unit) { src.expireDue() }
    val incoming = reqs.incoming.filter { it.state == IncomingRequestState.Pending }
    PreviewPage {
        TopBar("Chat requests", nav.back)
        LazyColumn(Modifier.weight(1f).fillMaxWidth(), contentPadding = PaddingValues(bottom = 24.dp)) {
            item { SectionLabel("REQUESTS · ${incoming.size}") }
            if (incoming.isEmpty()) item { Hint("No new requests. Requests never ring.", Modifier.padding(horizontal = 20.dp)) }
            items(incoming, key = { it.id }) { r -> Column(Modifier.padding(bottom = 8.dp)) { IncomingRequestCard(src, nav, r) } }
            item { SectionLabel("SENT REQUESTS") }
            if (reqs.sent.isEmpty()) item { Hint("Ask a group member for a private chat from their member sheet.", Modifier.padding(horizontal = 20.dp)) }
            items(reqs.sent, key = { it.id }) { r -> SentRequestRow(src, r, now) }
            item { Hint("You can send one request per person per day. A request expires after 7 days.", Modifier.padding(start = 20.dp, end = 20.dp, top = 16.dp)) }
        }
    }
}
