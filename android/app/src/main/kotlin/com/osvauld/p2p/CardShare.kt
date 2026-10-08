package com.osvauld.p2p

import android.content.Context
import android.content.Intent
import android.widget.Toast
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.*
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.zIndex
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.p2pcore.CardPeek

/** Text shared into Tinline from another app ("Share -> Tinline"); the UI picks it up once the app is unlocked. */
object CardInbox {
    val shared = MutableStateFlow<String?>(null)

    /** Called from MainActivity for an ACTION_SEND text/plain intent. */
    fun take(intent: Intent?) {
        if (intent?.action != Intent.ACTION_SEND || intent.type?.startsWith("text/") != true) return
        val t = intent.getStringExtra(Intent.EXTRA_TEXT)
        intent.action = null // do not handle the same share again after a recreate
        shared.value = t ?: ""
    }
}

private fun hashOf(ticket: String): String =
    java.security.MessageDigest.getInstance("SHA-256").digest(ticket.toByteArray()).take(8).joinToString("") { "%02x".format(it) }

fun peekOrNull(app: P2pApp, text: String?): CardPeek? =
    if (text.isNullOrBlank()) null else runCatching { app.node.peekCard(text) }.getOrNull()

/** Copy button that says "Copied" with a check for two seconds. */
@Composable
fun CopyButton(text: String?, modifier: Modifier = Modifier, label: String = "Copy", style: BtnStyle = BtnStyle.Outlined) {
    val ctx = LocalContext.current
    var copied by remember { mutableStateOf(false) }
    LaunchedEffect(copied) { if (copied) { delay(2000); copied = false } }
    TinButton(
        if (copied) "Copied" else label,
        { text?.let { copyToClipboard(ctx, "ticket", it); copied = true } },
        modifier, style = style, icon = if (copied) Icons.Rounded.Check else Icons.Rounded.ContentCopy, enabled = text != null,
    )
}

fun shareCard(ctx: Context, ticket: String): Boolean = try {
    ctx.startActivity(Intent.createChooser(Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, ticket), "Share contact card")); true
} catch (_: Exception) { false }

/** My QR code with Copy and Share right under it. Shared by the first-run home and the Add contact screen. */
@Composable
fun MyCodeBlock(app: P2pApp, meName: String, qr: Dp = 248.dp, showNewCode: Boolean = true) {
    val ctx = LocalContext.current
    val c = Tin.c
    val scope = rememberCoroutineScope()
    val status by app.status.collectAsState()
    var ticket by remember { mutableStateOf<String?>(null) }
    var err by remember { mutableStateOf<String?>(null) }
    var version by remember { mutableIntStateOf(0) }
    var share by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(status?.online, version) {
        withContext(Dispatchers.IO) { runCatching { app.node.myTicket() } }
            .onSuccess { ticket = it; err = null }.onFailure { err = it.message }
    }
    Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(14.dp)) {
        CardBox(Modifier.fillMaxWidth(), radius = 24.dp) {
            Column(Modifier.padding(20.dp).fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(14.dp)) {
                Text(meName, style = TinType.titleL.copy(fontSize = 20.sp), color = c.ink)
                val t = ticket
                Box(Modifier.size(qr).clip(RoundedCornerShape(14.dp)).background(Color.White), contentAlignment = Alignment.Center) {
                    if (t != null) {
                        val bmp = remember(t) { qrBitmap(t, 720, fg = 0xFF17201D.toInt()) }
                        Image(bmp.asImageBitmap(), "QR code of your contact card", Modifier.fillMaxSize().padding(10.dp))
                        Box(Modifier.size(40.dp).clip(RoundedCornerShape(10.dp)).background(Color.White).padding(3.dp).clip(RoundedCornerShape(8.dp)).background(Color(0xFF0B6B5B)), contentAlignment = Alignment.Center) {
                            TinMark(26.dp, can = Color.White, string = Color(0xFFE8B04A))
                        }
                    } else Text(err ?: "Preparing your code…", Modifier.padding(16.dp), style = TinType.bodyM, color = Color(0xFF4D5853), textAlign = TextAlign.Center)
                }
                Hint("Works once. Making a new code cancels this one.", Modifier.widthIn(max = 300.dp), align = TextAlign.Center)
            }
        }
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            TinButton("Share", { ticket?.let { t -> if (!shareCard(ctx, t)) share = "Nothing to share with" } }, Modifier.weight(1f), style = BtnStyle.Tonal, icon = Icons.Rounded.Share, enabled = ticket != null)
            CopyButton(ticket, Modifier.weight(1f))
        }
        share?.let { Hint(it, color = c.er) }
        if (showNewCode) TinButton("New code", {
            scope.launch { withContext(Dispatchers.IO) { runCatching { app.node.resetTicket() } }; ticket = null; version++ }
        }, style = BtnStyle.Text, icon = Icons.Rounded.Refresh)
    }
}

/**
 * Looks for a card on the clipboard when the app comes forward, and for text shared into the app, and
 * offers "Add <name>?". Draws nothing when there is nothing to offer.
 * [clipboardOk]: this screen may prompt from the clipboard (Home, Add; not in a call, not locked, not onboarding).
 * [shareOk]: a shared card may be handled now (unlocked, past onboarding).
 */
@Composable
fun CardPrompts(app: P2pApp, clipboardOk: Boolean, shareOk: Boolean, showOnScreen: Boolean, onChat: (CardPeek) -> Unit, onAdd: (CardPeek) -> Unit) {
    val ctx = LocalContext.current
    val view = LocalView.current
    val owner = LocalLifecycleOwner.current
    val scope = rememberCoroutineScope()
    val prefs = remember { ctx.getSharedPreferences("card_prompt", Context.MODE_PRIVATE) }
    val dismissed = remember { mutableSetOf<String>().also { s -> prefs.getString("dismissed", null)?.let(s::add) } }
    var prompt by remember { mutableStateOf<CardPeek?>(null) }
    var armed by remember { mutableStateOf(true) }
    val okNow by rememberUpdatedState(clipboardOk)

    fun check() {
        if (!armed || !okNow || !view.hasWindowFocus()) return
        val cm = ctx.getSystemService(Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager
        val d = cm.primaryClipDescription
        armed = false
        // Reading the clipboard shows a system toast on Android 12+, so only read real text, and only if it changed.
        if (d == null || !(d.hasMimeType("text/plain") || d.hasMimeType("text/html"))) return
        val stamp = d.timestamp
        if (stamp != 0L && stamp == prefs.getLong("clip_stamp", -1L)) return
        val text = runCatching { clipboardText(ctx) }.getOrNull()
        prefs.edit().putLong("clip_stamp", stamp).apply()
        scope.launch {
            val p = withContext(Dispatchers.IO) { peekOrNull(app, text) }
            if (p != null && hashOf(p.ticket) !in dismissed && prompt == null) prompt = p
        }
    }
    DisposableEffect(owner, view) {
        val o = LifecycleEventObserver { _, e -> if (e == Lifecycle.Event.ON_RESUME) { armed = true; check() } }
        val f = android.view.ViewTreeObserver.OnWindowFocusChangeListener { if (it) check() }
        owner.lifecycle.addObserver(o)
        view.viewTreeObserver.addOnWindowFocusChangeListener(f)
        onDispose { owner.lifecycle.removeObserver(o); view.viewTreeObserver.removeOnWindowFocusChangeListener(f) }
    }
    LaunchedEffect(clipboardOk) { if (clipboardOk) check() }

    val shared by CardInbox.shared.collectAsState()
    LaunchedEffect(shared, shareOk) {
        val t = shared ?: return@LaunchedEffect
        if (!shareOk) return@LaunchedEffect
        CardInbox.shared.value = null
        val p = withContext(Dispatchers.IO) { peekOrNull(app, t) }
        when {
            p == null -> Toast.makeText(ctx, "No Tinline contact card in that message", Toast.LENGTH_LONG).show()
            else -> prompt = p
        }
    }

    val p = prompt
    if (p != null && showOnScreen) {
        val c = Tin.c
        Box(Modifier.fillMaxSize().zIndex(10f).statusBarsPadding().padding(12.dp), contentAlignment = Alignment.TopCenter) {
            Row(
                Modifier.fillMaxWidth().shadow(6.dp, RoundedCornerShape(20.dp)).clip(RoundedCornerShape(20.dp)).background(c.sf2).padding(start = 14.dp, end = 8.dp, top = 12.dp, bottom = 12.dp),
                verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Avatar(p.name.ifBlank { "?" }, p.did, 44.dp)
                Column(Modifier.weight(1f)) {
                    Text(if (p.known) "Open chat with ${p.name.ifBlank { "them" }}?" else "Add ${p.name.ifBlank { "this contact" }}?", style = TinType.bodyL.copy(fontWeight = FontWeight.Bold), color = c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Text(if (p.known) "They are already in your contacts." else "Tinline found their card.", style = TinType.bodyM, color = c.ink2)
                }
                TinButton("Not now", { dismissed += hashOf(p.ticket); prefs.edit().putString("dismissed", hashOf(p.ticket)).apply(); prompt = null }, style = BtnStyle.Text, fill = false, height = 44.dp, textStyle = TinType.label)
                TinButton(if (p.known) "Open" else "Add", { prompt = null; if (p.known) onChat(p) else onAdd(p) }, fill = false, height = 44.dp, textStyle = TinType.label)
            }
        }
    }
}
