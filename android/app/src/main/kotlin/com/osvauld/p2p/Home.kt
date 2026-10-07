package com.osvauld.p2p

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ArrowBack
import androidx.compose.material.icons.rounded.*
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
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
import uniffi.p2pcore.Contact

/** One line of the Recent section / a contact's history. Fed by the call log once the core has one (see [Features.history]). */
data class RecentCall(val did: String, val name: String, val kind: Kind, val whenText: String) {
    enum class Kind { Incoming, Outgoing, Missed, Unreached }
}

private fun String.matchesQuery(q: String) = q.isBlank() || contains(q.trim(), ignoreCase = true)

@Composable
fun HomeScreen(
    app: P2pApp, missing: List<Need>, onFix: (Need) -> Unit,
    onAdd: (scan: Boolean) -> Unit, onSettings: () -> Unit, onContact: (Contact) -> Unit, onCall: (Contact) -> Unit,
    onAvailability: () -> Unit = {}, recents: List<RecentCall> = emptyList(), callError: String? = null,
) {
    val status by app.status.collectAsState()
    val contacts by app.contacts.collectAsState()
    var grace by remember { mutableStateOf(true) }
    LaunchedEffect(Unit) { delay(6000); grace = false }
    HomeContent(
        contacts = contacts, online = status?.online == true, connectingGrace = grace, missing = missing,
        onFix = onFix, onAdd = onAdd, onSettings = onSettings, onContact = onContact, onCall = onCall,
        onAvailability = onAvailability, recents = recents, callError = callError,
    )
}

/** Banner priority: offline, mic, notifications, background (battery, lock-screen), then availability. Only one shows. */
@Composable
private fun TopBanner(online: Boolean, grace: Boolean, missing: List<Need>, onFix: (Need) -> Unit) {
    when {
        !online && !grace -> Banner(BannerKind.Error, Icons.Rounded.WifiOff, "You’re offline", "Calls can’t reach you until you’re back on the internet.")
        Need.Mic in missing -> Banner(BannerKind.Error, Icons.Rounded.MicOff, "Microphone is off", "People won’t hear you on calls.", "Allow") { onFix(Need.Mic) }
        Need.Notifications in missing -> Banner(BannerKind.Warn, Icons.Rounded.NotificationsOff, "Calls won’t ring", "Notifications are off for Tinline.", "Turn on") { onFix(Need.Notifications) }
        Need.Battery in missing -> Banner(BannerKind.Warn, Icons.Rounded.BatteryAlert, "Calls may be missed when idle", "Android is limiting Tinline in the background.", "Fix") { onFix(Need.Battery) }
        Need.FullScreen in missing -> Banner(BannerKind.Warn, Icons.Rounded.PhoneLocked, "Calls won’t show on the lock screen", "Allow full-screen calls so Tinline rings like the phone app.", "Allow") { onFix(Need.FullScreen) }
        // Availability off (Features.availability) and "Set a passphrase" slot in here; the latter is a blocking screen today.
    }
}

@Composable
fun HomeContent(
    contacts: List<Contact>, online: Boolean, connectingGrace: Boolean, missing: List<Need>, onFix: (Need) -> Unit,
    onAdd: (scan: Boolean) -> Unit, onSettings: () -> Unit, onContact: (Contact) -> Unit, onCall: (Contact) -> Unit,
    onAvailability: () -> Unit, recents: List<RecentCall>, callError: String?, startSearching: Boolean = false, startQuery: String = "",
) {
    val c = Tin.c
    var searching by remember { mutableStateOf(startSearching) }
    var query by remember { mutableStateOf(startQuery) }
    val pill = when { online -> PillState.Available; connectingGrace -> PillState.Connecting; else -> PillState.Offline }
    val sorted = remember(contacts) { contacts.sortedBy { it.name.lowercase() } }

    Page {
        Box(Modifier.weight(1f).fillMaxWidth()) {
            Column(Modifier.fillMaxSize()) {
                if (searching) SearchBar(query, { query = it }, onClose = { searching = false; query = "" })
                else {
                    Row(Modifier.fillMaxWidth().heightIn(min = 64.dp).padding(start = 20.dp, end = 8.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        TinMark(28.dp)
                        Text("Tinline", Modifier.weight(1f), style = TinType.titleL.copy(fontSize = 22.sp, lineHeight = 28.sp), color = c.ink)
                        StatusPill(pill, if (Features.availability) onAvailability else null)
                        IconBtn(Icons.Rounded.Settings, "Settings", onSettings)
                    }
                }
                if (contacts.isEmpty() && !searching) {
                    Column(Modifier.weight(1f)) {
                        TopBanner(online, connectingGrace, missing, onFix)
                        EmptyHome(onAdd)
                    }
                } else LazyColumn(Modifier.weight(1f), contentPadding = PaddingValues(bottom = 112.dp)) {
                    if (!searching) {
                        item { TopBanner(online, connectingGrace, missing, onFix) }
                        item {
                            Row(
                                Modifier.padding(start = 16.dp, end = 16.dp, top = 12.dp).fillMaxWidth().heightIn(min = 52.dp).clip(RoundedCornerShape(50)).background(c.sf2)
                                    .clickable(role = Role.Button) { searching = true }.padding(horizontal = 16.dp),
                                verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp),
                            ) {
                                Icon(Icons.Rounded.Search, null, tint = c.ink2)
                                Text("Search contacts", style = TinType.bodyL, color = c.ink2)
                            }
                        }
                        if (callError != null) item { Text(callError, Modifier.padding(horizontal = 20.dp, vertical = 8.dp), style = TinType.bodyM, color = c.er) }
                        if (recents.isNotEmpty()) {
                            item { SectionLabel("RECENT") }
                            items(recents.take(3), key = { "r" + it.did + it.whenText }) { r ->
                                val k = r.kind
                                val red = k == RecentCall.Kind.Missed
                                val ctc = contacts.firstOrNull { it.did == r.did }
                                RowItem(r.name, r.did, subIcon = when (k) {
                                    RecentCall.Kind.Incoming -> Icons.Rounded.CallReceived
                                    RecentCall.Kind.Missed -> Icons.Rounded.CallMissed
                                    else -> Icons.Rounded.CallMade
                                }, sub = r.whenText, nameColor = if (red) c.er else c.ink, subColor = if (red) c.er else c.ink2,
                                    onClick = { ctc?.let(onContact) }, onCall = { ctc?.let(onCall) })
                            }
                        }
                        item { SectionLabel("CONTACTS · ${contacts.size}") }
                    } else item { SectionLabel("CONTACTS") }
                    val shown = sorted.filter { it.name.matchesQuery(query) }
                    items(shown, key = { it.did }) { ct ->
                        RowItem(ct.name.ifBlank { "Unnamed" }, ct.did, highlight = query, onClick = { onContact(ct) }, onCall = { onCall(ct) })
                    }
                    if (searching) item {
                        Row(
                            Modifier.padding(start = 20.dp, end = 20.dp, top = 24.dp).fillMaxWidth().drawBehind {
                                drawRoundRect(c.ln2, cornerRadius = CornerRadius(14.dp.toPx()), style = Stroke(1.dp.toPx(), pathEffect = PathEffect.dashPathEffect(floatArrayOf(10f, 8f))))
                            }.padding(16.dp),
                            verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp),
                        ) {
                            Icon(Icons.Rounded.PersonAdd, null, tint = c.ink2)
                            Text("Not here? Tinline has no directory — add people with their code.", Modifier.weight(1f), style = TinType.bodyM, color = c.ink2)
                            Text("Add", Modifier.clickable(role = Role.Button) { onAdd(false) }.padding(8.dp), style = TinType.label.copy(fontWeight = FontWeight.Bold), color = c.pr)
                        }
                    }
                }
            }
            if (!searching && contacts.isNotEmpty()) Row(
                Modifier.align(Alignment.BottomEnd).padding(end = 20.dp, bottom = 20.dp).shadow(3.dp, RoundedCornerShape(16.dp)).clip(RoundedCornerShape(16.dp)).background(c.pr)
                    .clickable(role = Role.Button) { onAdd(false) }.heightIn(min = 56.dp).padding(start = 16.dp, end = 20.dp),
                verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                Icon(Icons.Rounded.Add, null, tint = c.onPr)
                Text("Add contact", style = TinType.button, color = c.onPr)
            }
        }
    }
}

@Composable
private fun SearchBar(query: String, onChange: (String) -> Unit, onClose: () -> Unit) {
    val c = Tin.c
    val focus = remember { FocusRequester() }
    LaunchedEffect(Unit) { runCatching { focus.requestFocus() } }
    Row(Modifier.fillMaxWidth().heightIn(min = 64.dp).padding(start = 4.dp, end = 8.dp), verticalAlignment = Alignment.CenterVertically) {
        IconBtn(Icons.AutoMirrored.Rounded.ArrowBack, "Close search", onClose)
        Box(Modifier.weight(1f).heightIn(min = 48.dp), contentAlignment = Alignment.CenterStart) {
            if (query.isEmpty()) Text("Search contacts", style = TinType.bodyL.copy(fontSize = 18.sp), color = c.ink2)
            BasicTextField(
                query, onChange, Modifier.fillMaxWidth().focusRequester(focus), singleLine = true,
                textStyle = TinType.bodyL.copy(fontSize = 18.sp, color = c.ink), cursorBrush = SolidColor(c.pr),
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search), keyboardActions = KeyboardActions(),
            )
        }
        if (query.isNotEmpty()) IconBtn(Icons.Rounded.Close, "Clear", { onChange("") })
    }
    Box(Modifier.fillMaxWidth().height(1.dp).background(c.ln))
}

@Composable
private fun RowItem(
    name: String, key: String, sub: String? = null, subIcon: ImageVector? = null, highlight: String = "",
    nameColor: Color = Tin.c.ink, subColor: Color = Tin.c.ink2, onClick: () -> Unit, onCall: () -> Unit,
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
        IconBtn(Icons.Rounded.Call, "Call $name", onCall, tint = c.pr)
    }
}

@Composable
private fun EmptyHome(onAdd: (Boolean) -> Unit) {
    Column(
        Modifier.fillMaxSize().padding(start = 28.dp, end = 28.dp, bottom = 48.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp, Alignment.CenterVertically),
    ) {
        StringIllustration(height = 150.dp)
        H1("Your line is ready")
        Lead("Add someone to call. Meet up or video-chat, open Tinline on both phones, and scan each other’s code.")
        Spacer(Modifier.height(4.dp))
        TinButton("Scan their code", { onAdd(true) }, icon = Icons.Rounded.QrCodeScanner)
        TinButton("Show my code", { onAdd(false) }, style = BtnStyle.Outlined, icon = Icons.Rounded.QrCode2)
    }
}
