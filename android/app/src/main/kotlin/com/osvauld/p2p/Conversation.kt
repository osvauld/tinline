package com.osvauld.p2p

import android.widget.Toast
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ArrowBack
import androidx.compose.material.icons.automirrored.rounded.Reply
import androidx.compose.material.icons.automirrored.rounded.Send
import androidx.compose.material.icons.rounded.*
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.p2pcore.Attachment
import uniffi.p2pcore.AttachmentKind
import uniffi.p2pcore.DeliveryState
import uniffi.p2pcore.Message
import uniffi.p2pcore.TransferState
import java.io.File
import java.time.Instant
import java.time.ZoneOffset

/** Debug gallery only: start a conversation with a sheet, banner or viewer already open. */
class ConvPreview(
    val actionsFor: String? = null, val attach: Boolean = false, val editing: String? = null,
    val replying: String? = null, val viewer: String? = null, val draft: String = "",
)

private sealed interface ConvRow {
    data class Day(val label: String) : ConvRow
    data class Msg(val m: Message) : ConvRow
}

private fun List<Message>.sortedChat() = sortedWith(compareBy<Message>({ it.at }, { it.id }))

/** One conversation: header, the day's messages (older days load at the top), and the composer. */
@Composable
fun ConversationScreen(
    source: ChatSource, peerDid: String, name: String, online: Boolean,
    onBack: () -> Unit, onCall: () -> Unit, preview: ConvPreview? = null,
) {
    val c = Tin.c
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    val clipboard = LocalClipboardManager.current
    val msgs = remember { mutableStateListOf<Message>() }
    var loaded by remember { mutableStateOf(false) }
    var olderDay by remember { mutableStateOf<String?>(null) }
    val fetched = remember { mutableSetOf<String>() }
    val progress = remember { mutableStateMapOf<String, Pair<Long, Long>>() }
    val requested = remember { mutableStateListOf<String>() }
    val link = (source.links.collectAsState().value[peerDid] ?: Link.Unknown)
    var nowMs by remember { mutableLongStateOf(System.currentTimeMillis()) }
    var draft by remember { mutableStateOf(preview?.draft ?: "") }
    var actionsFor by remember { mutableStateOf(preview?.actionsFor) }
    var attachOpen by remember { mutableStateOf(preview?.attach == true) }
    var editing by remember { mutableStateOf(preview?.editing) }
    var replying by remember { mutableStateOf(preview?.replying) }
    var viewer by remember { mutableStateOf(preview?.viewer) }
    val first = firstName(name)

    fun toast(t: String) = Toast.makeText(ctx, t, Toast.LENGTH_SHORT).show()
    fun upsert(m: Message) {
        val i = msgs.indexOfFirst { it.id == m.id }
        if (i >= 0) msgs[i] = m else { msgs.add(m); val s = msgs.toList().sortedChat(); msgs.clear(); msgs.addAll(s) }
    }

    LaunchedEffect(Unit) { while (true) { delay(15_000); nowMs = System.currentTimeMillis() } }
    LaunchedEffect(peerDid) {
        withContext(Dispatchers.IO) {
            runCatching { source.day(peerDid, null) }.onSuccess { p -> withContext(Dispatchers.Main) { p.messages.forEach(::upsert); olderDay = p.olderDay } }
        }
        loaded = true
    }
    LaunchedEffect(peerDid) {
        source.events.collect { e ->
            when (e) {
                is ChatEvent.Added -> if (e.message.peerDid == peerDid) upsert(e.message)
                is ChatEvent.Changed -> if (e.message.peerDid == peerDid) upsert(e.message)
                is ChatEvent.Delivery -> if (e.peerDid == peerDid) msgs.indexOfFirst { it.id == e.messageId }.takeIf { it >= 0 }?.let { msgs[it] = msgs[it].copy(delivery = e.delivery) }
                is ChatEvent.Progress -> if (e.peerDid == peerDid) {
                    if (e.done >= e.total) progress.remove(e.hash) else progress[e.hash] = e.done to e.total
                }
            }
        }
    }
    // Reading: the open conversation never raises a notification, and its unread count is cleared.
    val owner = LocalLifecycleOwner.current
    DisposableEffect(owner, peerDid) {
        val o = LifecycleEventObserver { _, ev ->
            if (ev == Lifecycle.Event.ON_RESUME) { OpenChat.peer = peerDid; ChatNotifier.cancel(ctx, peerDid) }
            if (ev == Lifecycle.Event.ON_PAUSE && OpenChat.peer == peerDid) OpenChat.peer = null
        }
        owner.lifecycle.addObserver(o)
        onDispose { owner.lifecycle.removeObserver(o); if (OpenChat.peer == peerDid) OpenChat.peer = null }
    }
    LaunchedEffect(peerDid, msgs.size) {
        withContext(Dispatchers.IO) { runCatching { source.markRead(peerDid); source.refresh() } }
    }

    val listState = rememberLazyListState()
    val rows = remember(msgs.toList(), nowMs / 3_600_000) {
        buildList<ConvRow> {
            var last: Long? = null
            msgs.forEach { m ->
                val at = m.at.toLong()
                if (last == null || !sameDay(last!!, at)) add(ConvRow.Day(dayLabel(at, System.currentTimeMillis())))
                add(ConvRow.Msg(m)); last = at
            }
        }.reversed()
    }
    // Reached the oldest loaded day: read the next older one from this phone, else ask their phone once.
    val atTop = !listState.canScrollForward
    LaunchedEffect(atTop, loaded, olderDay, msgs.size, link) {
        if (!atTop || !loaded) return@LaunchedEffect
        withContext(Dispatchers.IO) {
            val od = olderDay
            if (od != null) {
                runCatching { source.day(peerDid, od) }.onSuccess { p -> withContext(Dispatchers.Main) { p.messages.forEach(::upsert); olderDay = p.olderDay } }
            } else if (msgs.isNotEmpty() && link != Link.Offline) {
                val oldest = Instant.ofEpochMilli(msgs.first().at.toLong()).atZone(ZoneOffset.UTC).toLocalDate().toString()
                if (fetched.add(oldest)) runCatching { source.fetchOlder(peerDid, oldest) }.onSuccess { n ->
                    if (n > 0) runCatching { source.day(peerDid, oldest) }.onSuccess { p -> withContext(Dispatchers.Main) { olderDay = p.olderDay; p.messages.forEach(::upsert) } }
                }
            }
        }
    }
    LaunchedEffect(msgs.size) { if (listState.firstVisibleItemIndex <= 2) listState.animateScrollToItem(0) }

    fun send() {
        val text = draft.trim()
        if (text.isEmpty()) return
        val edit = editing
        val reply = replying
        draft = ""; editing = null; replying = null
        scope.launch(Dispatchers.IO) {
            runCatching { if (edit != null) source.edit(peerDid, edit, text) else source.sendText(peerDid, text, reply) }
                .onSuccess { m -> withContext(Dispatchers.Main) { upsert(m); listState.animateScrollToItem(0) } }
                .onFailure { withContext(Dispatchers.Main) { toast("Couldn’t send that message."); if (edit == null) draft = text } }
        }
    }
    fun sendFile(file: File, mime: String) {
        scope.launch(Dispatchers.IO) {
            runCatching { source.sendFile(peerDid, file.path, mime, null) }
                .onSuccess { m -> withContext(Dispatchers.Main) { upsert(m); listState.animateScrollToItem(0) } }
                .onFailure { withContext(Dispatchers.Main) { toast("Couldn’t send that file.") } }
            file.parentFile?.takeIf { it.parentFile?.name == "chat_out" }?.deleteRecursively()
        }
    }
    fun sendUri(uri: android.net.Uri?) {
        if (uri == null) return
        scope.launch(Dispatchers.IO) {
            runCatching { ChatMedia.copyIn(ctx, uri) }
                .onSuccess { (f, mime) -> sendFile(f, mime) }
                .onFailure { withContext(Dispatchers.Main) { toast("Couldn’t read that file.") } }
        }
    }
    fun download(m: Message) {
        requested.add(m.id)
        scope.launch(Dispatchers.IO) { runCatching { source.download(peerDid, m.id) }.onFailure { withContext(Dispatchers.Main) { requested.remove(m.id); toast("Couldn’t start the download.") } } }
    }
    fun saveToPhone(m: Message) {
        val a = m.attachment ?: return
        scope.launch(Dispatchers.IO) {
            val ok = runCatching { ChatMedia.saveToPhone(ctx, ChatMedia.decrypt(ctx, source, m), a.name, a.mime) }.getOrDefault(false)
            withContext(Dispatchers.Main) { toast(if (ok) "Saved to your phone" else "Couldn’t save that file.") }
        }
    }

    val photoPicker = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { sendUri(it) }
    val filePicker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { sendUri(it) }
    var cameraFile by remember { mutableStateOf<File?>(null) }
    val camera = rememberLauncherForActivityResult(ActivityResultContracts.TakePicture()) { ok ->
        cameraFile?.let { f -> if (ok && f.length() > 0) sendFile(f, "image/jpeg") }
        cameraFile = null
    }
    fun launchCamera() {
        val (f, uri) = ChatMedia.cameraTarget(ctx)
        cameraFile = f
        try { camera.launch(uri) } catch (e: Exception) { toast("No camera available.") }
    }
    val cameraPerm = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { if (it) launchCamera() else toast("Allow the camera to take photos.") }

    val byId = remember(msgs.toList()) { msgs.associateBy { it.id } }
    Box(Modifier.fillMaxSize()) {
        Page {
            ConvHeader(name, peerDid, link, onBack, onCall, first)
            if (link == Link.Offline) InfoCard(
                "$first isn’t reachable right now. Your messages stay on this phone and send by themselves when you’re both online.",
                Modifier.padding(start = 12.dp, end = 12.dp, top = 8.dp), icon = Icons.Rounded.Schedule, kind = BannerKind.Warn,
            )
            LazyColumn(
                Modifier.weight(1f).fillMaxWidth(), state = listState, reverseLayout = true,
                contentPadding = PaddingValues(horizontal = 12.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                items(rows, key = { r -> when (r) { is ConvRow.Day -> "d" + r.label + rows.indexOf(r); is ConvRow.Msg -> r.m.id } }) { r ->
                    when (r) {
                        is ConvRow.Day -> Box(Modifier.fillMaxWidth().padding(top = 10.dp, bottom = 6.dp), contentAlignment = Alignment.Center) {
                            Text(r.label, Modifier.clip(RoundedCornerShape(50)).background(c.sf2).padding(horizontal = 12.dp, vertical = 4.dp),
                                style = TinType.caption.copy(fontWeight = FontWeight.SemiBold), color = c.ink2)
                        }
                        is ConvRow.Msg -> Bubble(
                            r.m, name, link, online, nowMs, progress, requested, byId, source,
                            selected = actionsFor == r.m.id,
                            onLong = { if (!r.m.deleted) actionsFor = r.m.id },
                            onDownload = { download(r.m) }, onOpenPhoto = { viewer = r.m.id }, onSaveFile = { saveToPhone(r.m) },
                        )
                    }
                }
                if (loaded && olderDay == null) item(key = "older") {
                    Row(
                        Modifier.fillMaxWidth().padding(start = 0.dp, end = 0.dp, top = 8.dp, bottom = 8.dp).border(1.dp, c.ln2, RoundedCornerShape(14.dp)).padding(16.dp),
                        horizontalArrangement = Arrangement.spacedBy(12.dp),
                    ) {
                        Icon(Icons.Rounded.History, null, tint = c.ink2)
                        Column(Modifier.weight(1f)) {
                            Text("Older messages are on $first’s phone", style = TinType.bodyM.copy(fontSize = 15.sp, fontWeight = FontWeight.SemiBold), color = c.ink)
                            Text("This phone keeps them from when you started using it. Earlier days load when you’re both online.", Modifier.padding(top = 4.dp), style = TinType.bodyM, color = c.ink2)
                        }
                    }
                }
            }
            Composer(
                draft, { draft = it }, editing != null, replying?.let { byId[it] }?.let { ComposerBanner("Replying to ${if (it.outgoing) "yourself" else first}", ChatNotifier.previewOf(it)) },
                editing?.let { byId[it] }?.let { ComposerBanner("Editing", it.text) },
                onCancelBanner = { if (editing != null) draft = ""; editing = null; replying = null },
                onAttach = { attachOpen = true }, onSend = ::send,
                onVoice = { path, ms, wave -> scope.launch(Dispatchers.IO) { runCatching { source.sendVoice(peerDid, path, ms, wave) }.onSuccess { m -> withContext(Dispatchers.Main) { upsert(m) } }.onFailure { withContext(Dispatchers.Main) { toast("Couldn’t send that voice message.") } } } },
            )
        }
        val vm = viewer?.let { byId[it] }
        if (vm != null) PhotoViewer(vm, name, source, onClose = { viewer = null }, onSave = { saveToPhone(vm) }, onShare = {
            scope.launch(Dispatchers.IO) { runCatching { ChatMedia.share(ctx, ChatMedia.decrypt(ctx, source, vm), vm.attachment?.mime ?: "image/*") } }
        })
    }
    val am = actionsFor?.let { byId[it] }
    if (am != null) ActionsSheet(
        am, first, onDismiss = { actionsFor = null },
        onReply = { actionsFor = null; editing = null; replying = am.id },
        onCopy = { actionsFor = null; clipboard.setText(AnnotatedString(am.text)); toast("Copied") },
        onEdit = { actionsFor = null; replying = null; editing = am.id; draft = am.text },
        onDelete = {
            actionsFor = null
            scope.launch(Dispatchers.IO) { runCatching { source.delete(peerDid, am.id) }.onSuccess { m -> withContext(Dispatchers.Main) { upsert(m) } }.onFailure { withContext(Dispatchers.Main) { toast("Couldn’t delete that message.") } } }
        },
        onSave = { actionsFor = null; saveToPhone(am) },
    )
    if (attachOpen) AttachSheet(
        first, onDismiss = { attachOpen = false },
        onPhotos = { attachOpen = false; photoPicker.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageAndVideo)) },
        onCamera = {
            attachOpen = false
            if (Perms.granted(ctx, android.Manifest.permission.CAMERA)) launchCamera() else cameraPerm.launch(android.Manifest.permission.CAMERA)
        },
        onFile = { attachOpen = false; filePicker.launch(arrayOf("*/*")) },
    )
}

@Composable
private fun ConvHeader(name: String, peerDid: String, link: Link, onBack: () -> Unit, onCall: () -> Unit, first: String) {
    val c = Tin.c
    Row(Modifier.fillMaxWidth().heightIn(min = 64.dp).padding(horizontal = 4.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        IconBtn(Icons.AutoMirrored.Rounded.ArrowBack, "Back", onBack)
        Avatar(name, peerDid, 40.dp)
        Column(Modifier.weight(1f)) {
            Text(name, style = TinType.bodyL.copy(fontSize = 17.sp, lineHeight = 22.sp, fontWeight = FontWeight.Bold), color = c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (link != Link.Unknown) Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Dot(if (link == Link.Offline) c.ln2 else c.pr, 8.dp, hollow = link == Link.Offline)
                Text(when (link) { Link.Connected -> "Connected · direct"; Link.Relayed -> "Connected · relayed"; else -> "Not connected" }, style = TinType.caption.copy(fontWeight = FontWeight.Normal), color = c.ink2)
            }
        }
        IconBtn(Icons.Rounded.Call, "Call $first", onCall, tint = c.pr)
    }
    Box(Modifier.fillMaxWidth().height(1.dp).background(c.ln))
}

private class ComposerBanner(val title: String, val text: String)

@Composable
private fun Composer(
    draft: String, onDraft: (String) -> Unit, editing: Boolean, replyBanner: ComposerBanner?, editBanner: ComposerBanner?,
    onCancelBanner: () -> Unit, onAttach: () -> Unit, onSend: () -> Unit, onVoice: (String, Int, ByteArray) -> Unit,
) {
    val c = Tin.c
    val voice = rememberVoiceRecState { r -> onVoice(r.path, r.durationMs, r.waveform) }
    val banner = editBanner ?: replyBanner
    if (banner != null) Row(
        Modifier.padding(horizontal = 8.dp).fillMaxWidth().clip(RoundedCornerShape(topStart = 16.dp, topEnd = 16.dp)).background(c.sf)
            .border(1.dp, c.ln, RoundedCornerShape(topStart = 16.dp, topEnd = 16.dp)).padding(start = 14.dp, top = 8.dp, bottom = 8.dp, end = 8.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Icon(if (editBanner != null) Icons.Rounded.Edit else Icons.AutoMirrored.Rounded.Reply, null, tint = c.pr, modifier = Modifier.size(20.dp))
        Column(Modifier.weight(1f)) {
            Text(banner.title, style = TinType.bodyM.copy(fontSize = 13.sp, fontWeight = FontWeight.SemiBold), color = c.pr)
            BannerLine(banner.text)
        }
        IconBtn(Icons.Rounded.Close, if (editBanner != null) "Cancel editing" else "Cancel reply", onCancelBanner, tint = c.ink2)
    }
    Row(Modifier.fillMaxWidth().padding(start = 8.dp, end = 8.dp, top = if (banner != null) 0.dp else 8.dp, bottom = 12.dp), verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        val shape = if (banner != null) RoundedCornerShape(bottomStart = 24.dp, bottomEnd = 24.dp) else RoundedCornerShape(24.dp)
        if (voice.recording) VoiceRecordingBar(voice, Modifier.weight(1f)) else Row(
            Modifier.weight(1f).heightIn(min = 48.dp).clip(shape).background(if (editing) c.sf else c.sf2).let { if (editing) it.border(1.dp, c.pr, shape) else it },
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (!editing) Box(Modifier.padding(start = 4.dp)) { IconBtn(Icons.Rounded.Add, "Attach", onAttach, tint = c.ink2) } else Spacer(Modifier.width(14.dp))
            Box(Modifier.weight(1f).padding(vertical = 10.dp, horizontal = 4.dp)) {
                if (draft.isEmpty()) Text("Message", style = TinType.bodyL, color = c.ink2)
                BasicTextField(
                    draft, onDraft, Modifier.fillMaxWidth(), maxLines = 6, textStyle = TinType.bodyL.copy(color = c.ink), cursorBrush = SolidColor(c.pr),
                    keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
                )
            }
            Spacer(Modifier.width(8.dp))
        }
        if (draft.isBlank() && !editing) VoiceMicButton(voice)
        else Box(
            Modifier.size(48.dp).clip(CircleShape).background(if (draft.isBlank()) Tin.c.sf3 else c.pr).clickable(enabled = draft.isNotBlank(), role = Role.Button, onClick = onSend),
            contentAlignment = Alignment.Center,
        ) { Icon(if (editing) Icons.Rounded.Check else Icons.AutoMirrored.Rounded.Send, if (editing) "Save edit" else "Send", tint = if (draft.isBlank()) c.ink2 else c.onPr) }
    }
}

@Composable
private fun BannerLine(text: String) =
    Text(text, style = TinType.bodyM.copy(fontSize = 13.sp, lineHeight = 18.sp), color = Tin.c.ink2, maxLines = 1, overflow = TextOverflow.Ellipsis)

// ------------------------------------------------------------------ bubbles

@OptIn(androidx.compose.foundation.ExperimentalFoundationApi::class)
@Composable
private fun Bubble(
    m: Message, name: String, link: Link, online: Boolean, nowMs: Long, progress: Map<String, Pair<Long, Long>>, requested: List<String>,
    byId: Map<String, Message>, source: ChatSource, selected: Boolean,
    onLong: () -> Unit, onDownload: () -> Unit, onOpenPhoto: () -> Unit, onSaveFile: () -> Unit,
) {
    val c = Tin.c
    val out = m.outgoing
    val shape = RoundedCornerShape(18.dp, 18.dp, if (out) 6.dp else 18.dp, if (out) 18.dp else 6.dp)
    val first = firstName(name)
    Box(Modifier.fillMaxWidth(), contentAlignment = if (out) Alignment.CenterEnd else Alignment.CenterStart) {
        if (m.deleted) {
            Column(
                Modifier.widthIn(max = 290.dp).clip(shape).let { if (out) it.border(1.dp, c.ln2, shape) else it }.padding(start = 12.dp, end = 12.dp, top = 8.dp, bottom = 6.dp),
            ) {
                Text(if (out) "You deleted this message" else "$first deleted this message", style = TinType.bodyL.copy(fontSize = 15.sp, lineHeight = 21.sp, fontStyle = FontStyle.Italic), color = c.ink2)
                Meta(msClock(m.at.toLong()), null, c.ink2, null)
            }
            return@Box
        }
        val a = m.attachment
        val photo = a != null && a.isImage()
        val prog = a?.let { progress[it.hash] }
        val tick = if (out) tickOf(m, link, online, nowMs) else null
        val bg = if (out) c.prc else c.sf
        val fg = if (out) c.onPrc else c.ink
        Column(
            Modifier.widthIn(max = 290.dp).clip(shape).background(bg).let { if (!out) it.border(1.dp, c.ln, shape) else it }
                .let { if (selected) it.border(3.dp, c.pr, shape) else it }
                .combinedClickable(onClick = {}, onLongClick = onLong)
                .padding(if (photo) PaddingValues(4.dp) else PaddingValues(start = 12.dp, end = 12.dp, top = 8.dp, bottom = 6.dp)),
        ) {
            m.replyTo?.let { rid ->
                val q = byId[rid]
                Column(
                    Modifier.padding(bottom = 6.dp).fillMaxWidth().clip(RoundedCornerShape(8.dp)).background(if (out) Color.White.copy(alpha = .45f) else c.bg)
                        .padding(start = 11.dp, top = 4.dp, end = 8.dp, bottom = 4.dp).drawLeftBar(c.pr),
                ) {
                    Text(if (q == null) "Earlier message" else if (q.outgoing) "You" else first, style = TinType.caption.copy(fontWeight = FontWeight.Bold), color = c.pr)
                    if (q != null) Text(ChatNotifier.previewOf(q), style = TinType.bodyM.copy(fontSize = 13.sp, lineHeight = 18.sp), color = fg, maxLines = 2, overflow = TextOverflow.Ellipsis)
                }
            }
            if (a != null) when {
                a.kind == AttachmentKind.VOICE -> VoiceBubble(m, a, a.state == TransferState.READY || out, out, source, onDownload)
                photo -> PhotoBox(m, a, out, prog, link, source, onDownload, onOpenPhoto)
                else -> FileRow(m, a, out, prog, link, first, m.id in requested, onDownload, onSaveFile)
            }
            if (m.text.isNotBlank()) Text(m.text, style = TinType.bodyL.copy(fontSize = 15.sp, lineHeight = 21.sp), color = fg)
            val at = msClock(m.at.toLong())
            val sendingPct = if (out && prog != null && prog.second > 0) (prog.first * 100 / prog.second).toInt() else null
            val label = when {
                sendingPct != null -> "Sending $sendingPct% · $at"
                tick == Tick.Clock -> "Waiting · $at"
                m.editedAt != null -> "edited · $at"
                else -> at
            }
            Meta(label, tick, if (tick == Tick.Clock) c.threadText else if (out) c.onPrc else c.ink2, if (photo) 8.dp else 0.dp, if (photo) 4.dp else 0.dp)
        }
    }
}

private fun Modifier.drawLeftBar(color: Color) = drawBehind { drawRect(color, size = androidx.compose.ui.geometry.Size(3.dp.toPx(), size.height)) }

@Composable
private fun Meta(text: String, tick: Tick?, color: Color, hPad: androidx.compose.ui.unit.Dp?, bPad: androidx.compose.ui.unit.Dp = 0.dp) {
    Row(
        Modifier.fillMaxWidth().padding(top = 4.dp, start = hPad ?: 0.dp, end = hPad ?: 0.dp, bottom = bPad),
        horizontalArrangement = Arrangement.spacedBy(4.dp, Alignment.End), verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(text, style = TinType.caption.copy(fontSize = 11.sp, lineHeight = 14.sp, fontWeight = FontWeight.Normal), color = color)
        if (tick != null) TickIcon(tick, if (tick == Tick.Two) Tin.c.pr else color)
    }
}

@Composable
private fun PhotoBox(m: Message, a: Attachment, out: Boolean, prog: Pair<Long, Long>?, link: Link, source: ChatSource, onDownload: () -> Unit, onOpen: () -> Unit) {
    val c = Tin.c
    val ctx = LocalContext.current
    val ready = a.state == TransferState.READY || out
    val bmp by produceState<androidx.compose.ui.graphics.ImageBitmap?>(null, m.id, ready) {
        value = if (!ready) null else withContext(Dispatchers.IO) { runCatching { ChatMedia.bitmap(ChatMedia.decrypt(ctx, source, m), 720) }.getOrNull() }
    }
    Box(Modifier.size(240.dp, 180.dp).clip(RoundedCornerShape(12.dp)).background(c.sf3).clickable(role = Role.Button) { if (ready && bmp != null) onOpen() else if (!ready) onDownload() }, contentAlignment = Alignment.Center) {
        bmp?.let { Image(it, a.name, Modifier.fillMaxSize(), contentScale = ContentScale.Crop) }
        val frac = prog?.let { if (it.second > 0) it.first.toFloat() / it.second else 0f }
            ?: if (a.state == TransferState.DOWNLOADING && a.size > 0UL) a.transferred.toFloat() / a.size.toFloat() else null
        when {
            frac != null && !(ready && prog == null) -> Ring { CircularProgressIndicator({ frac }, Modifier.size(56.dp), color = Color.White, trackColor = Color.White.copy(alpha = .25f), strokeWidth = 3.dp) }
            a.state == TransferState.REMOTE && !out -> Ring { Icon(Icons.Rounded.Download, "Download photo", tint = Color.White) }
            a.state == TransferState.FAILED -> Ring { Icon(Icons.Rounded.Refresh, "Retry download", tint = Color.White) }
            bmp == null && ready -> Icon(Icons.Rounded.Image, null, tint = c.ink2)
        }
    }
}

@Composable
private fun Ring(content: @Composable BoxScope.() -> Unit) =
    Box(Modifier.size(56.dp).clip(CircleShape).background(Color(0x8C0F1513)), contentAlignment = Alignment.Center, content = content)

@Composable
private fun FileRow(m: Message, a: Attachment, out: Boolean, prog: Pair<Long, Long>?, link: Link, first: String, asked: Boolean, onDownload: () -> Unit, onSave: () -> Unit) {
    val c = Tin.c
    val ready = a.state == TransferState.READY || out
    val size = sizeText(a.size.toLong())
    val frac = prog?.let { if (it.second > 0) it.first.toFloat() / it.second else 0f }
        ?: if (a.state == TransferState.DOWNLOADING && a.size > 0UL) a.transferred.toFloat() / a.size.toFloat() else null
    val paused = link == Link.Offline
    val sub = when {
        ready -> "$size · ${typeLabel(a)}"
        a.state == TransferState.DOWNLOADING && !paused -> "Downloading · ${sizeText((frac ?: 0f).times(a.size.toLong()).toLong())} of $size"
        (a.state == TransferState.DOWNLOADING || asked) && paused -> "$size · paused — waiting for $first to come online"
        asked -> "$size · starting…"
        a.state == TransferState.FAILED -> "$size · couldn’t download — tap to retry"
        else -> "$size · not downloaded"
    }
    val canDownload = !ready && (a.state == TransferState.REMOTE || a.state == TransferState.FAILED) && !asked
    Row(
        Modifier.widthIn(min = 240.dp).clip(RoundedCornerShape(8.dp)).clickable(role = Role.Button) { if (ready) onSave() else if (canDownload) onDownload() },
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Box(Modifier.size(44.dp).clip(RoundedCornerShape(12.dp)).background(if (out) Color.White.copy(alpha = .55f) else c.sf2), contentAlignment = Alignment.Center) {
            Icon(if (a.isVideo()) Icons.Rounded.Videocam else Icons.Rounded.Description, null, tint = c.pr)
        }
        Column(Modifier.weight(1f, fill = false)) {
            Text(a.name, style = TinType.bodyL.copy(fontSize = 15.sp, lineHeight = 20.sp, fontWeight = FontWeight.SemiBold), color = if (out) c.onPrc else c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(sub, style = TinType.caption.copy(fontWeight = FontWeight.Normal), color = if (out) c.onPrc else c.ink2)
            if (frac != null && !ready && !paused) Box(Modifier.padding(top = 6.dp).fillMaxWidth().height(4.dp).clip(RoundedCornerShape(2.dp)).background(Color.Black.copy(alpha = .1f))) {
                Box(Modifier.fillMaxWidth(frac.coerceIn(0f, 1f)).fillMaxHeight().background(c.pr))
            }
        }
        if (canDownload && a.state == TransferState.REMOTE) IconBtn(Icons.Rounded.Download, "Download ${a.name}", onDownload, tint = c.pr)
    }
}

// ------------------------------------------------------------------ sheets

@Composable
private fun ActionsSheet(
    m: Message, first: String, onDismiss: () -> Unit, onReply: () -> Unit, onCopy: () -> Unit, onEdit: () -> Unit, onDelete: () -> Unit, onSave: () -> Unit,
) {
    val c = Tin.c
    val at = msClock(m.at.toLong())
    TinSheet(onDismiss) {
        Hint(
            if (!m.outgoing) "Received $at" else if (m.delivery == DeliveryState.DELIVERED) "Sent $at · on $first’s phone" else "Sent $at · only on this phone so far",
            Modifier.padding(start = 8.dp, bottom = 8.dp),
        )
        SheetAction(Icons.AutoMirrored.Rounded.Reply, "Reply", onClick = onReply)
        if (m.text.isNotBlank()) SheetAction(Icons.Rounded.ContentCopy, "Copy text", onClick = onCopy)
        if (m.attachment?.let { it.state == TransferState.READY || m.outgoing } == true) SheetAction(Icons.Rounded.SaveAlt, "Save to phone", onClick = onSave)
        if (m.outgoing && m.attachment == null) SheetAction(Icons.Rounded.Edit, "Edit", onClick = onEdit)
        if (m.outgoing) SheetAction(Icons.Rounded.Delete, "Delete for both of you", color = c.er, onClick = onDelete)
        if (m.outgoing) Hint("Edits and deletes reach $first’s phone the next time you’re both online.", Modifier.padding(start = 8.dp, top = 8.dp))
    }
}

@Composable
private fun SheetAction(icon: ImageVector, label: String, color: Color = Tin.c.ink, onClick: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().heightIn(min = 56.dp).clip(RoundedCornerShape(12.dp)).clickable(role = Role.Button, onClick = onClick).padding(horizontal = 8.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Icon(icon, null, tint = if (color == Tin.c.ink) Tin.c.ink2 else color)
        Text(label, style = TinType.bodyL, color = color)
    }
}

@Composable
private fun AttachSheet(first: String, onDismiss: () -> Unit, onPhotos: () -> Unit, onCamera: () -> Unit, onFile: () -> Unit) {
    val c = Tin.c
    TinSheet(onDismiss) {
        Text("Send to $first", Modifier.padding(start = 4.dp, bottom = 8.dp), style = TinType.titleM, color = c.ink)
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            listOf(Triple(Icons.Rounded.PhotoLibrary, "Photos & videos", onPhotos), Triple(Icons.Rounded.PhotoCamera, "Camera", onCamera), Triple(Icons.Rounded.InsertDriveFile, "File", onFile)).forEach { (icon, label, go) ->
                Column(Modifier.weight(1f).clip(RoundedCornerShape(12.dp)).clickable(role = Role.Button, onClick = go).padding(vertical = 8.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Box(Modifier.size(60.dp).clip(RoundedCornerShape(18.dp)).background(c.prc), contentAlignment = Alignment.Center) { Icon(icon, null, tint = c.onPrc) }
                    Text(label, style = TinType.bodyM.copy(fontWeight = FontWeight.Medium), color = c.ink)
                }
            }
        }
        Row(Modifier.padding(start = 4.dp, top = 12.dp), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.Top) {
            Icon(Icons.Rounded.Lock, null, tint = c.ink2, modifier = Modifier.size(18.dp))
            Hint("Files go straight to $first’s phone, encrypted. Up to 2 GB each. Both of you need to be online while it transfers.")
        }
    }
}

// ------------------------------------------------------------------ photo viewer

/** Full-screen photo, always dark. Share and Save decrypt a copy; the original stays encrypted in the core. */
@Composable
private fun PhotoViewer(m: Message, name: String, source: ChatSource, onClose: () -> Unit, onSave: () -> Unit, onShare: () -> Unit) {
    val ctx = LocalContext.current
    BackHandler(onBack = onClose)
    val bmp by produceState<androidx.compose.ui.graphics.ImageBitmap?>(null, m.id) {
        value = withContext(Dispatchers.IO) { runCatching { ChatMedia.bitmap(ChatMedia.decrypt(ctx, source, m), 2048) }.getOrNull() }
    }
    Column(Modifier.fillMaxSize().background(Color.Black).systemBarsPadding()) {
        Row(Modifier.fillMaxWidth().height(64.dp).padding(horizontal = 4.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            IconBtn(Icons.Rounded.Close, "Close", onClose, tint = Color.White)
            Column(Modifier.weight(1f)) {
                Text(if (m.outgoing) "You" else name, style = TinType.bodyL.copy(fontWeight = FontWeight.SemiBold), color = Color.White)
                Text(viewerWhen(m.at.toLong(), System.currentTimeMillis()), style = TinType.caption, color = Color(0xFFA6B2AD))
            }
            IconBtn(Icons.Rounded.Share, "Share", onShare, tint = Color.White)
            IconBtn(Icons.Rounded.SaveAlt, "Save to phone", onSave, tint = Color.White)
        }
        Box(Modifier.weight(1f).fillMaxWidth(), contentAlignment = Alignment.Center) {
            bmp?.let { Image(it, m.attachment?.name, Modifier.fillMaxSize(), contentScale = ContentScale.Fit) }
        }
        Row(Modifier.padding(start = 20.dp, end = 20.dp, top = 16.dp, bottom = 24.dp), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.Top) {
            Icon(Icons.Rounded.Lock, null, tint = Color(0xFFA6B2AD), modifier = Modifier.size(18.dp))
            Text("Kept encrypted inside Tinline. “Save to phone” copies it to your gallery, where other apps can see it.", style = TinType.bodyM.copy(fontSize = 13.sp, lineHeight = 18.sp), color = Color(0xFFA6B2AD))
        }
    }
}

/** Voice message body: decrypts the attachment to the cache once, then hands the file to the player. */
@Composable
private fun VoiceBubble(m: Message, a: Attachment, ready: Boolean, out: Boolean, source: ChatSource, onDownload: () -> Unit) {
    val c = Tin.c
    val ctx = LocalContext.current
    var path by remember(m.id) { mutableStateOf<String?>(null) }
    LaunchedEffect(m.id, ready) {
        if (!ready) return@LaunchedEffect
        path = withContext(Dispatchers.IO) {
            val f = java.io.File(ctx.cacheDir, "voice/${m.id}.ogg")
            if (f.exists() && f.length() > 0) f.absolutePath else runCatching {
                f.parentFile?.mkdirs(); source.save(m.peerDid, m.id, f.absolutePath); f.absolutePath
            }.getOrNull()
        }
    }
    val p = path
    if (p != null) VoiceBubbleContent(m.id, p, a.durationMs.toInt(), a.waveform, out)
    else Row(Modifier.widthIn(min = 230.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        val remote = !ready && a.state != TransferState.DOWNLOADING
        Box(
            Modifier.size(40.dp).clip(CircleShape).background(c.pr).clickable(enabled = remote, role = Role.Button) { onDownload() },
            contentAlignment = Alignment.Center,
        ) { Icon(if (remote) Icons.Rounded.Download else Icons.Rounded.PlayArrow, if (remote) "Download voice message" else "Voice message loading", tint = c.onPr) }
        Text(formatMs(a.durationMs.toInt()), style = TinType.caption.copy(fontFamily = PlexMono, fontSize = 12.sp), color = if (out) c.onPrc else c.ink2)
    }
}
