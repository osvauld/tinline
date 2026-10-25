package com.osvauld.p2p

import androidx.compose.ui.res.stringResource
import android.content.Context
import android.content.Intent
import android.widget.Toast
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.compose.LocalLifecycleOwner
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
fun CopyButton(text: String?, modifier: Modifier = Modifier, label: String = stringResource(R.string.card_copy), style: BtnStyle = BtnStyle.Outlined) {
    val ctx = LocalContext.current
    var copied by remember { mutableStateOf(false) }
    LaunchedEffect(copied) { if (copied) { delay(2000); copied = false } }
    TinButton(
        if (copied) stringResource(R.string.common_copied) else label,
        { text?.let { copyToClipboard(ctx, "ticket", it); copied = true } },
        modifier, style = style, icon = if (copied) PgIcons.Check else PgIcons.Copy, enabled = text != null,
    )
}

fun shareCard(ctx: Context, ticket: String): Boolean = try {
    ctx.startActivity(Intent.createChooser(Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, ticket), ctx.getString(R.string.card_share_chooser))); true
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
    val nothingToShare = stringResource(R.string.card_nothing_to_share)
    LaunchedEffect(status?.online, version) {
        withContext(Dispatchers.IO) { runCatching { app.node.myTicket() } }
            .onSuccess { ticket = it; err = null }.onFailure { err = it.message }
    }
    Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(14.dp)) {
        CardBox(Modifier.fillMaxWidth(), radius = 24.dp) {
            Column(Modifier.padding(20.dp).fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(14.dp)) {
                Text(meName, style = TinType.titleL.copy(fontSize = 20.sp), color = c.ink)
                val t = ticket
                Box(Modifier.size(qr).clip(SmoothCornerShape(14.dp)).background(Color.White), contentAlignment = Alignment.Center) {
                    if (t != null) {
                        val bmp = remember(t) { qrBitmap(t, 720, fg = 0xFF17201D.toInt()) }
                        Image(bmp.asImageBitmap(), stringResource(R.string.card_qr_desc), Modifier.fillMaxSize().padding(10.dp))
                        Box(Modifier.size(40.dp).clip(SmoothCornerShape(10.dp)).background(Color.White).padding(3.dp).clip(SmoothCornerShape(8.dp)).background(Color(0xFF0B6B5B)), contentAlignment = Alignment.Center) {
                            TinMark(26.dp, can = Color.White, string = Color(0xFFE8B04A), mouth = Color(0xFF0B6B5B))
                        }
                    } else Text(err ?: stringResource(R.string.card_preparing), Modifier.padding(16.dp), style = TinType.bodyM, color = Color(0xFF4D5853), textAlign = TextAlign.Center)
                }
                Hint(stringResource(R.string.card_works_once), Modifier.widthIn(max = 300.dp), align = TextAlign.Center)
            }
        }
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            TinButton(stringResource(R.string.common_share), { ticket?.let { t -> if (!shareCard(ctx, t)) share = nothingToShare } }, Modifier.weight(1f), style = BtnStyle.Tonal, icon = PgIcons.Share, enabled = ticket != null)
            CopyButton(ticket, Modifier.weight(1f))
        }
        share?.let { Hint(it, color = c.er) }
        if (showNewCode) TinButton(stringResource(R.string.card_new_code), {
            scope.launch { withContext(Dispatchers.IO) { runCatching { app.node.resetTicket() } }; ticket = null; version++ }
        }, style = BtnStyle.Text, icon = PgIcons.Retry)
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
        testLog("clipcheck armed=$armed ok=$okNow focus=${view.hasWindowFocus()}")
        if (!armed || !okNow || !view.hasWindowFocus()) return
        val cm = ctx.getSystemService(Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager
        val d = cm.primaryClipDescription
        armed = false
        // Reading the clipboard shows a system toast on Android 12+, so only read real text, and only if it changed.
        if (d == null || !(d.hasMimeType("text/plain") || d.hasMimeType("text/html"))) return
        val stamp = d.timestamp
        testLog("clipread stamp=$stamp last=${prefs.getLong("clip_stamp", -1L)}")
        if (stamp != 0L && stamp == prefs.getLong("clip_stamp", -1L)) return
        val text = runCatching { clipboardText(ctx) }.getOrNull()
        prefs.edit().putLong("clip_stamp", stamp).apply()
        scope.launch {
            val p = withContext(Dispatchers.IO) { peekOrNull(app, text) }
            testLog("clippeek len=${text?.length} -> ${p?.name} known=${p?.known}")
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
            p == null -> Toast.makeText(ctx, ctx.getString(R.string.card_none_found), Toast.LENGTH_LONG).show()
            else -> prompt = p
        }
    }

    val p = prompt
    if (p != null && showOnScreen) {
        val c = Tin.c
        Box(Modifier.fillMaxSize().zIndex(10f).statusBarsPadding().padding(12.dp), contentAlignment = Alignment.TopCenter) {
            Row(
                Modifier.fillMaxWidth().depth(Depth.Pop, SmoothCornerShape(20.dp)).clip(SmoothCornerShape(20.dp)).background(c.sfRaised).padding(start = 14.dp, end = 8.dp, top = 12.dp, bottom = 12.dp),
                verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Avatar(p.name.ifBlank { "?" }, p.did, 44.dp)
                Column(Modifier.weight(1f)) {
                    Text(if (p.known) stringResource(R.string.card_chat_with, p.name.ifBlank { stringResource(R.string.card_them) }) else stringResource(R.string.card_add_name, p.name.ifBlank { stringResource(R.string.card_this_contact) }), style = TinType.bodyL.copy(fontWeight = FontWeight.Bold), color = c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Text(if (p.known) stringResource(R.string.card_already_contact) else stringResource(R.string.card_found), style = TinType.bodyM, color = c.ink2)
                }
                TinButton(stringResource(R.string.common_not_now), { dismissed += hashOf(p.ticket); prefs.edit().putString("dismissed", hashOf(p.ticket)).apply(); prompt = null }, style = BtnStyle.Text, fill = false, height = 44.dp, textStyle = TinType.label)
                TinButton(if (p.known) stringResource(R.string.card_open) else stringResource(R.string.card_add), { prompt = null; if (p.known) onChat(p) else onAdd(p) }, fill = false, height = 44.dp, textStyle = TinType.label)
            }
        }
    }
}
