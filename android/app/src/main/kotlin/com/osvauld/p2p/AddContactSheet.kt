package com.osvauld.p2p

import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.CameraAlt
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.p2pcore.AddException
import uniffi.p2pcore.AddOutcome
import uniffi.p2pcore.CardPeek
import uniffi.p2pcore.CodePeek

/*
 * Add contact (people/AddContact.svelte, decided 2026-10-13; SIGNUP-HANDOFF.md § 6b and § 6f), as a bottom sheet over Chats.
 * Two ways only, By code (first, the default) and QR. Copies the bare code. "Paste their code" reads the clipboard and looks
 * it up at once (core find_card inside peek_card). Hints are muted and polite, never red. The body is a fixed 328 high, so the
 * sheet never resizes; the found state (me · the mark · them) replaces it in place. Adding dials them (core add_contact_by_code):
 * their device holds it as a request ("Request sent…") until they accept. The same sheet is the owner's side too: "Contact
 * request" (Not now / Accept) when someone used my code, "New contact" once the line is open (P2pApp.incoming).
 */

/** Why an add didn't work, by the core's AddError kind (never its text). */
private enum class Reach { Timeout, Refused, Blocked, Offline }

/** "Mon 19 Oct" (codeUntil in people.svelte.ts), local time. */
private fun codeUntil(exp: ULong): String = java.text.SimpleDateFormat("EEE d MMM", java.util.Locale.getDefault()).format(java.util.Date(exp.toLong() * 1000))

@Composable
fun AddContactSheet(
    app: P2pApp, startOnScan: Boolean = false, ticket: String? = null, pasteOnOpen: Boolean = false,
    onClose: () -> Unit, onMessage: (String) -> Unit, onProfile: (String) -> Unit,
) {
    val ctx = LocalContext.current
    val c = Tin.c
    val scope = rememberCoroutineScope()
    val contacts by app.contacts.collectAsState()
    val status by app.status.collectAsState()
    val blocks by app.blocks.collectAsState()
    val meName = remember { app.node.profile()?.name ?: "" }
    var qrTab by rememberSaveable { mutableStateOf(startOnScan) }
    var scanning by remember { mutableStateOf(startOnScan) }
    var hint by remember { mutableStateOf("") }
    var found by remember { mutableStateOf<CardPeek?>(null) }
    var scanned by remember { mutableStateOf(false) }
    var adding by remember { mutableStateOf(false) }
    var slow by remember { mutableStateOf(false) }
    var fail by remember { mutableStateOf<Reach?>(null) }
    var added by remember { mutableStateOf(false) }
    var requested by remember { mutableStateOf(false) }
    var copied by remember { mutableStateOf(false) }
    var myCode by remember { mutableStateOf<String?>(null) }
    var myCodeExp by remember { mutableStateOf<ULong?>(null) }
    var codeVersion by remember { mutableIntStateOf(0) }
    val inc by app.incoming.collectAsState()
    var answering by remember { mutableStateOf(false) }
    // A code can be replaced (a rename, a block): read it again whenever contacts change (don't cache it).
    LaunchedEffect(status?.online, codeVersion, contacts.size) {
        withContext(Dispatchers.IO) { runCatching { app.node.myCode() } }.onSuccess { myCode = it.code; myCodeExp = it.expiresAt }
    }
    // They accepted while the sheet is still on them: it turns taut by itself.
    val acc by app.accepted.collectAsState()
    LaunchedEffect(acc) { val a = acc ?: return@LaunchedEffect; if (found?.did == a.did && !added) { added = true; requested = false } }
    DisposableEffect(Unit) { app.addSheetOpen = true; onDispose { app.addSheetOpen = false } }
    LaunchedEffect(copied) { if (copied) { delay(1600); copied = false } }

    fun reset() { found = null; hint = ""; fail = null; added = false; requested = false; scanned = false }
    fun look(text: String?, how: Boolean) {
        reset()
        if (text.isNullOrBlank()) { hint = "Your clipboard is empty right now. When their code arrives, copy it and paste it here."; return }
        scope.launch {
            // core peek_code: says why a code can't be used, without dialling.
            when (val p = withContext(Dispatchers.IO) { runCatching { app.node.peekCode(text) }.getOrNull() }) {
                is CodePeek.Found -> { found = p.card; scanned = how }
                is CodePeek.Own -> hint = "That’s your own code. Send it to them, and paste theirs here."
                is CodePeek.Expired -> hint = "That code from ${firstName(p.name)} has expired. Codes last 7 days, so ask them for a fresh one."
                else -> hint = "That doesn’t look like a Tinline code. Could they copy it again for you?"
            }
        }
    }
    fun add() {
        val p = found ?: return
        if (adding) return
        fail = null
        if (status?.started == true && status?.online == false) { fail = Reach.Offline; return }
        adding = true; slow = false
        scope.launch {
            val timer = launch { delay(2500); slow = true }
            val r = withContext(Dispatchers.IO) { runCatching { app.node.addContactByCode(p.ticket) } }
            timer.cancel(); adding = false; slow = false
            r.onSuccess {
                app.refresh()
                when (it) {
                    is AddOutcome.Added -> { added = true; Cues.line() }
                    // Delivered: the line keeps its connecting sway while they decide.
                    is AddOutcome.Requested -> if (!added) requested = true
                }
            }
            // By the core's AddError kind, never its display text.
            r.onFailure {
                fail = when (it) {
                    is AddException.Refused -> Reach.Refused
                    is AddException.Blocked -> Reach.Blocked
                    is AddException.Offline -> Reach.Offline
                    else -> Reach.Timeout
                }
            }
        }
    }
    fun accept(did: String) {
        if (answering) return
        answering = true
        scope.launch {
            val ok = app.acceptRequest(did)
            answering = false
            if (ok) app.incoming.value = app.incoming.value?.takeIf { it.did == did }?.copy(joined = true)
        }
    }
    LaunchedEffect(Unit) {
        if (ticket != null) { val p = withContext(Dispatchers.IO) { peekOrNull(app, ticket) }; if (p != null) { found = p; add() } }
        else if (pasteOnOpen) look(clipboardText(ctx), false)
    }

    val i = inc
    PgSheet(onDismiss = onClose) {
        PgDialogBody(when { i == null -> "Add contact"; i.joined -> "New contact"; else -> "Contact request" }) {
            // `.tabs.gone`: hidden but keeping its height, so the sheet doesn't jump when it becomes a request.
            Row(Modifier.padding(top = 8.dp).alpha(if (i != null) 0f else 1f), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                PgChip("Code", qrTab.not(), { if (i == null) { qrTab = false; scanning = false; reset() } })
                PgChip("QR", qrTab, { if (i == null) { qrTab = true; reset() } })
            }
            Box(Modifier.fillMaxWidth().padding(top = 12.dp).height(328.dp), contentAlignment = Alignment.Center) {
                val p = found
                when {
                    i != null -> Incoming(meName, i)
                    p != null -> Found(meName, p, scanned, added, requested, adding, slow, fail, p.known, p.known && blocks.of(p.did).blockedByMe)
                    qrTab -> Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(10.dp)) {
                        Box(Modifier.size(196.dp), contentAlignment = Alignment.Center) {
                            if (scanning) QrCameraBox(Modifier.size(196.dp).clip(SmoothCornerShape(20.dp)), "Point the camera at the code on their screen.", false, {}) { t -> scanning = false; look(t, true) }
                            else Box(Modifier.size(176.dp).clip(SmoothCornerShape(12.dp)).background(Color.White), contentAlignment = Alignment.Center) {
                                val t = myCode
                                if (t != null) Image(remember(t) { qrBitmap(t, 528, fg = 0xFF17201D.toInt()) }.asImageBitmap(), "Your QR code", Modifier.fillMaxSize().padding(8.dp))
                                else Text("Preparing your code…", style = PgType.sub, color = Color(0xFF4D5853))
                            }
                        }
                        Text(if (scanning) "Point the camera at the code on their screen." else "Let them scan this with Tinline.", style = PgType.sub19, color = c.ink2, textAlign = TextAlign.Center)
                        PBtn(if (scanning) "Show my code" else "Scan their code", { scanning = !scanning }, Modifier.fillMaxWidth())
                    }
                    else -> Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                            // "New code" sits on the label's line (a text button, same height), so it never moves anything.
                            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                                Text("Your code", Modifier.weight(1f), style = PgType.sub.copy(fontWeight = FontWeight.Bold, lineHeight = 21.sp), color = c.ink)
                                Text("New code", Modifier.clickable(role = Role.Button) {
                                    scope.launch { withContext(Dispatchers.IO) { runCatching { app.node.resetTicket() } }; myCode = null; codeVersion++ }
                                }, style = PgType.sub.copy(fontWeight = FontWeight.SemiBold, lineHeight = 21.sp), color = c.accentInk)
                            }
                            // Reusable for 7 days (core my_code); the date is the code's own exp.
                            Text(myCodeExp?.let { "Send it to anyone you’d like to talk to, in any app. It works until ${codeUntil(it)}." }
                                ?: "Send it to anyone you’d like to talk to, in any app.", style = PgType.sub19, color = c.ink2)
                            Text(myCode ?: "Preparing your code…", Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).background(c.sf2).padding(horizontal = 12.dp, vertical = 8.dp),
                                style = PgType.mono.copy(fontSize = 12.sp, lineHeight = 18.sp), color = c.ink2, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            // the bare code only (OSVC2:…)
                            PBtn(if (copied) "Copied" else "Copy my code", { myCode?.let { copyToClipboard(ctx, "Tinline code", it); copied = true } }, Modifier.fillMaxWidth(),
                                variant = PVariant.Primary, enabled = myCode != null)
                        }
                        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                            Box(Modifier.weight(1f).height(1.dp).background(c.ln)); Text("or", style = PgType.sub, color = c.ink2); Box(Modifier.weight(1f).height(1.dp).background(c.ln))
                        }
                        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                            Text("Their code", style = PgType.sub.copy(fontWeight = FontWeight.Bold, lineHeight = 21.sp), color = c.ink)
                            PBtn("Paste their code", { look(clipboardText(ctx), false) }, Modifier.fillMaxWidth())
                            // Hints are not errors: muted, polite (never red, never an alert).
                            Text(hint, Modifier.fillMaxWidth().heightIn(min = 38.dp).semantics { liveRegion = LiveRegionMode.Polite }, style = PgType.sub19, color = c.ink2)
                        }
                    }
                }
            }
            val p = found
            val first = p?.let { firstName(it.name) }.orEmpty()
            PfActs(top = 2.dp) {
                when {
                    i != null && !i.joined -> {
                        PBtn("Not now", onClose, variant = PVariant.Quiet, enabled = !answering)
                        PBtn("Accept", { accept(i.did) }, variant = PVariant.Primary, enabled = !answering, minWidth = 132.dp)
                    }
                    i != null -> { PBtn("Done", onClose, variant = PVariant.Quiet); PBtn("Message ${firstName(i.name)}", { onMessage(i.did) }, variant = PVariant.Primary, minWidth = 132.dp) }
                    p == null -> PBtn("Close", onClose, variant = PVariant.Quiet)
                    requested && !added -> PBtn("Done", onClose, variant = PVariant.Primary, minWidth = 132.dp)
                    added -> { PBtn("Done", onClose, variant = PVariant.Quiet); PBtn("Message $first", { onMessage(p.did) }, variant = PVariant.Primary) }
                    p.known -> { PBtn("Close", onClose, variant = PVariant.Quiet); PBtn("Open profile", { onProfile(p.did) }, variant = PVariant.Primary) }
                    fail == Reach.Refused -> { PBtn("Back", { reset() }, variant = PVariant.Quiet); PBtn("Paste a new code", { reset() }, variant = PVariant.Primary) }
                    fail == Reach.Blocked -> { PBtn("Back", { reset() }, variant = PVariant.Quiet); PBtn("Close", onClose, variant = PVariant.Primary, minWidth = 132.dp) }
                    else -> {
                        PBtn("Back", { reset() }, variant = PVariant.Quiet, enabled = !adding)
                        // a fixed box ("Add Tara" / "Adding…" / "Try again"): min 96 + 2 x 18
                        PBtn(when { adding -> "Adding…"; fail != null -> "Try again"; else -> "Add $first" }, ::add, variant = PVariant.Primary, enabled = !adding, minWidth = 132.dp)
                    }
                }
            }
        }
    }
}

/** LineJoin.svelte: me · the mark · them, fixed 64 high. One TinMark instance per face (same composition slot across states),
 *  so slack → ringing (the call "connecting" sway, 1.2 s, 6°) → taut (LINE_SPRING) animates. */
@Composable
private fun LineJoin(meName: String, name: String, did: String, state: MarkState) {
    Row(Modifier.height(64.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        SelfAvatar(meName.ifBlank { "?" }, 56.dp)
        TinMark(64.dp, state = state, live = true, lineColor = true, sway = Sway(1.2, 6.0), spring = MarkSpring.Line,
            description = when (state) { MarkState.Taut -> "Connected"; MarkState.Ringing -> "Connecting"; else -> "Not connected yet" })
        Avatar(name.ifBlank { "?" }, did, 56.dp)
    }
}

/** The accent chip under the name ("Scanned in person", "Has your code", "Accepted your request"). */
@Composable
private fun StateChip(icon: androidx.compose.ui.graphics.vector.ImageVector, text: String) {
    val c = Tin.c
    Row(Modifier.clip(RoundedCornerShape(13.dp)).background(c.accentSoft).padding(horizontal = 10.dp, vertical = 3.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        Icon(icon, null, Modifier.size(14.dp), tint = c.accentInk)
        Text(text, style = PgType.sub.copy(fontWeight = FontWeight.SemiBold, lineHeight = 20.sp), color = c.accentInk)
    }
}

/** The reason line, in a fixed three-line slot (the face never moves); muted, announced politely. */
@Composable
private fun Why(text: String) = Text(text, Modifier.widthIn(max = 300.dp).heightIn(min = 57.dp).semantics { liveRegion = LiveRegionMode.Polite },
    style = PgType.sub19, color = Tin.c.ink2, textAlign = TextAlign.Center)

/** The found card: LineJoin, the name, and the reason line. */
@Composable
private fun Found(meName: String, p: CardPeek, scanned: Boolean, added: Boolean, requested: Boolean, adding: Boolean, slow: Boolean, fail: Reach?, known: Boolean, blocked: Boolean) {
    val c = Tin.c
    val first = firstName(p.name)
    // slack (recognised) → ringing while the dial runs and while the request waits → taut.
    val state = when { added || known -> MarkState.Taut; adding || requested -> MarkState.Ringing; else -> MarkState.Slack }
    val why = when {
        blocked -> "You blocked $first. Unblock them in Privacy before adding them again."
        known -> "$first is already in your contacts."
        added -> "Your line to $first is open. Say hello."
        requested -> "Request sent. Your line opens when $first accepts."
        adding -> if (slow) "Still trying. $first’s Tinline needs to be open and online for this." else "Reaching $first’s Tinline…"
        fail == Reach.Timeout -> "Couldn’t reach $first’s Tinline. It needs to be open and online, so try again when $first’s around."
        fail == Reach.Refused -> "$first’s code isn’t valid any more. They may have made a new one, so ask $first for their newest code."
        fail == Reach.Blocked -> "You can’t add $first right now. They’ve chosen not to be in touch here."
        fail == Reach.Offline -> "You’re offline. Once you’re connected, try again."
        scanned -> "You scanned it from their screen, so you know whose code this is."
        else -> "Is this the $first you expected? If you’re not sure, check with them first."
    }
    Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)) {
        LineJoin(meName, p.name, p.did, state)
        Text(p.name, style = PgType.title.copy(fontSize = 20.sp, lineHeight = 26.sp), color = c.ink)
        if (scanned) StateChip(PgIcons.Check, "Scanned in person")
        Why(why)
    }
}

/** The owner's side (or mine, when they accepted while the sheet was closed): AddContact.svelte `inc`. */
@Composable
private fun Incoming(meName: String, i: IncomingSheet) {
    val f = firstName(i.name)
    Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)) {
        LineJoin(meName, i.name, i.did, if (i.joined) MarkState.Taut else MarkState.Ringing)
        Text(i.name, style = PgType.title.copy(fontSize = 20.sp, lineHeight = 26.sp), color = Tin.c.ink)
        StateChip(PgIcons.UserPlus, if (i.theirs) "Accepted your request" else "Has your code")
        Why(when {
            !i.joined -> "$f wants to add you as a contact. Accepting lets you message and call each other."
            i.theirs -> "$f accepted. Your line to $f is open. Say hello."
            else -> "Your line to $f is open. Say hello."
        })
    }
}
