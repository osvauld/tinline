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
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.IntOffset
import kotlin.math.roundToInt
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.draw.blur
import androidx.compose.ui.layout.SubcomposeLayout
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.constrainWidth
import androidx.compose.ui.unit.constrainHeight
import androidx.compose.ui.unit.LayoutDirection
import kotlin.math.ceil
import kotlin.math.max
import androidx.compose.ui.platform.LocalClipboard
import androidx.compose.ui.platform.ClipEntry
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextStyle
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
import kotlinx.coroutines.withTimeout
import kotlinx.coroutines.flow.first
import uniffi.p2pcore.Attachment
import uniffi.p2pcore.AttachmentKind
import uniffi.p2pcore.DeliveryState
import uniffi.p2pcore.MessageReceiptStatus
import uniffi.p2pcore.ReceiptProofKind
import uniffi.p2pcore.ReceiptTick
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
    data class Msg(val m: Message, val pos: BubblePos = BubblePos.Solo) : ConvRow
    /** "n new messages": before the oldest of the messages that were unread when the chat opened. */
    data class Unread(val n: Int) : ConvRow
    /** A call with this person (MessageRow `.callrec`), on the caller's side. */
    data class Call(val r: uniffi.p2pcore.CallRecord) : ConvRow
}

private fun List<Message>.sortedChat() = sortedWith(compareBy<Message>({ it.at }, { it.id }))

/** One conversation: header, the day's messages (older days load at the top), and the composer. */
@Composable
fun ConversationScreen(
    source: ChatSource, peerDid: String, name: String, online: Boolean,
    onBack: () -> Unit, onCall: () -> Unit, preview: ConvPreview? = null, addedAtSecs: Long? = null,
    /** The "You" conversation: notes and files for our own devices, no contact on the other side. */
    isSelf: Boolean = false,
    /** Blocking on either side (Blocking.kt); null for "You" and previews. Chats are never deleted (U24): a block only closes the composer. */
    block: ConvBlock? = null,
    onUnblock: () -> Unit = {},
    onAddContact: () -> Unit = {},
    /** Why our message is not delivered ("blocked"), from the core. Called off the main thread. */
    notDelivered: (String) -> String? = { null },
    onInfo: (() -> Unit)? = null,
    /** Global-search integration: route passes the real hit, including its UTC day. */
    searchHit: uniffi.p2pcore.MessageHit? = null,
    /** Calls with this person, shown in the conversation as pills. */
    calls: List<uniffi.p2pcore.CallRecord> = emptyList(),
    /** The header's video button: place the call and turn the camera on. */
    onVideo: (() -> Unit)? = null,
) {
    val c = Tin.c
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    val clipboard = LocalClipboard.current
    val msgs = remember { mutableStateListOf<Message>() }
    var loaded by remember { mutableStateOf(false) }
    var olderDay by remember { mutableStateOf<String?>(null) }
    val fetched = remember { mutableSetOf<String>() }
    // Their phone answered "nothing older": the banner would be false.
    var noOlder by remember { mutableStateOf(false) }
    val progress = remember { mutableStateMapOf<String, Pair<Long, Long>>() }
    val requested = remember { mutableStateListOf<String>() }
    val link = (source.links.collectAsState().value[peerDid] ?: Link.Unknown)
    var nowMs by remember { mutableLongStateOf(System.currentTimeMillis()) }
    val savedDraft = remember(peerDid) { ConversationDrafts.forPeer(peerDid) }
    var draft by savedDraft::text
    var actionsFor by remember { mutableStateOf(preview?.actionsFor) }
    var attachOpen by remember { mutableStateOf(preview?.attach == true) }
    var editing by savedDraft::edit
    var replying by savedDraft::reply
    var sending by remember { mutableStateOf(false) }
    val find = remember(peerDid) { FindState() }
    LaunchedEffect(Unit) {
        if (preview != null) { draft = preview.draft; replying = preview.replying; editing = preview.editing }
    }
    LaunchedEffect(draft, editing) { ConversationDrafts.publish(peerDid, savedDraft) }
    DisposableEffect(peerDid) { onDispose { savedDraft.cancelEdit(); ConversationDrafts.publish(peerDid, savedDraft) } }
    BackHandler(find.open) { find.close() }
    var viewer by remember { mutableStateOf(preview?.viewer) }
    var docViewer by remember { mutableStateOf<String?>(null) }
    val brokenImages = remember { mutableStateListOf<String>() }
    val first = firstName(name)
    val composerState = block?.composer ?: ComposerState.Open
    val blockedMeAt = block?.view?.blockedMeAtSecs
    // Ids of our pending messages that won't arrive because they blocked us.
    val undelivered = remember { mutableStateListOf<String>() }

    // The core's recipient-signed status per message id (mark + provenance). Missing = not read yet: the mark's
    // slot stays reserved and empty, never "delivered".
    val receipts = remember(peerDid) { mutableStateMapOf<String, MessageReceiptStatus>() }
    fun loadReceipt(id: String) {
        scope.launch(Dispatchers.IO) {
            runCatching { source.receipts(peerDid, id) }.getOrNull()?.let { st -> withContext(Dispatchers.Main) { receipts[st.messageId] = st } }
        }
    }
    suspend fun loadDayReceipts(day: String) {
        val list = withContext(Dispatchers.IO) { runCatching { source.dayReceipts(peerDid, day) }.getOrDefault(emptyList()) }
        list.forEach { receipts[it.messageId] = it }
    }

    fun toast(t: String) = Toast.makeText(ctx, t, Toast.LENGTH_SHORT).show()
    fun upsert(m: Message) {
        val i = msgs.indexOfFirst { it.id == m.id }
        if (i >= 0) msgs[i] = m else { msgs.add(m); val s = msgs.toList().sortedChat(); msgs.clear(); msgs.addAll(s) }
    }

    LaunchedEffect(Unit) { while (true) { delay(15_000); nowMs = System.currentTimeMillis() } }
    LaunchedEffect(peerDid) {
        withContext(Dispatchers.IO) {
            runCatching { source.day(peerDid, null) }.onSuccess { p -> withContext(Dispatchers.Main) { p.messages.forEach(::upsert); olderDay = p.olderDay; loadDayReceipts(p.day) } }
        }
        loaded = true
    }
    LaunchedEffect(peerDid) {
        source.events.collect { e ->
            when (e) {
                is ChatEvent.Added -> if (e.message.peerDid == peerDid) { upsert(e.message); if (e.message.outgoing) loadReceipt(e.message.id) }
                // Also raised when the contact's signed receipts for the shown version were committed.
                is ChatEvent.Changed -> if (e.message.peerDid == peerDid) { upsert(e.message); if (e.message.outgoing) loadReceipt(e.message.id) }
                is ChatEvent.Delivery -> if (e.peerDid == peerDid) msgs.indexOfFirst { it.id == e.messageId }.takeIf { it >= 0 }?.let { msgs[it] = msgs[it].copy(delivery = e.delivery); loadReceipt(e.messageId) }
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
    LaunchedEffect(peerDid) {
        if (!isSelf) withContext(Dispatchers.IO) { runCatching { source.watchPresence(peerDid) } }
    }
    LaunchedEffect(peerDid, msgs.size) {
        withContext(Dispatchers.IO) { runCatching { source.markRead(peerDid); source.refresh() } }
    }

    val listState = rememberLazyListState()
    // The unread count when the chat opened (it is cleared right away): where "n new messages" goes.
    val unreadAtOpen = remember(peerDid) { source.chats.value.firstOrNull { it.peerDid == peerDid }?.unread?.toInt() ?: 0 }
    val dividerId = remember(peerDid, loaded) { if (unreadAtOpen > 0) msgs.filter { !it.outgoing }.takeLast(unreadAtOpen).firstOrNull()?.id else null }
    val rows = remember(msgs.toList(), calls, nowMs / 3_600_000, dividerId) {
        buildList<ConvRow> {
            var last: Long? = null
            val pending = calls.sortedBy { it.startedAt }.toMutableList()
            fun flushCalls(upTo: Long) {
                while (pending.isNotEmpty() && pending.first().startedAt.toLong() * 1000 <= upTo) {
                    val r = pending.removeAt(0); val at = r.startedAt.toLong() * 1000
                    if (last == null || !sameDay(last!!, at)) add(ConvRow.Day(dayLabel(at, System.currentTimeMillis())))
                    add(ConvRow.Call(r)); last = at
                }
            }
            // Same sender, same day, within 5 minutes: one run (playground Conversation.svelte), so stack-facing corners go tight.
            fun joined(a: Message?, b: Message?) = a != null && b != null && a.outgoing == b.outgoing &&
                sameDay(a.at.toLong(), b.at.toLong()) && kotlin.math.abs(b.at.toLong() - a.at.toLong()) <= 5 * 60_000
            msgs.forEachIndexed { i, m ->
                val at = m.at.toLong()
                val callBefore = pending.isNotEmpty() && pending.first().startedAt.toLong() * 1000 <= at
                flushCalls(at)
                if (last == null || !sameDay(last!!, at)) add(ConvRow.Day(dayLabel(at, System.currentTimeMillis())))
                if (m.id == dividerId) add(ConvRow.Unread(unreadAtOpen))
                val callAfter = pending.isNotEmpty() && msgs.getOrNull(i + 1)?.let { pending.first().startedAt.toLong() * 1000 <= it.at.toLong() } == true
                val a = !callBefore && m.id != dividerId && joined(msgs.getOrNull(i - 1), m); val b = !callAfter && joined(m, msgs.getOrNull(i + 1))
                add(ConvRow.Msg(m, if (a && b) BubblePos.Mid else if (a) BubblePos.Last else if (b) BubblePos.First else BubblePos.Solo)); last = at
            }
            flushCalls(Long.MAX_VALUE)
        }.reversed()
    }
    val currentRows = rememberUpdatedState(rows)
    // Reached the oldest loaded day: read the next older one from this phone, else ask their phone once.
    val atTop = !listState.canScrollForward
    LaunchedEffect(atTop, loaded, olderDay, msgs.size, link) {
        if (!atTop || !loaded) return@LaunchedEffect
        withContext(Dispatchers.IO) {
            val od = olderDay
            if (od != null) {
                runCatching { source.day(peerDid, od) }.onSuccess { p -> withContext(Dispatchers.Main) { p.messages.forEach(::upsert); olderDay = p.olderDay; loadDayReceipts(p.day) } }
            } else if (msgs.isNotEmpty() && link != Link.Offline) {
                val oldest = Instant.ofEpochMilli(msgs.first().at.toLong()).atZone(ZoneOffset.UTC).toLocalDate().toString()
                if (fetched.add(oldest)) runCatching { source.fetchOlder(peerDid, oldest) }.onSuccess { n ->
                    if (n == 0) noOlder = true
                    if (n > 0) runCatching { source.day(peerDid, oldest) }.onSuccess { p -> withContext(Dispatchers.Main) { olderDay = p.olderDay; p.messages.forEach(::upsert); loadDayReceipts(p.day) } }
                }
            }
        }
    }
    LaunchedEffect(msgs.size) { if (!find.open && searchHit == null && listState.firstVisibleItemIndex <= 2) listState.scrollToItem(0) }
    suspend fun jumpTo(hit: FindHit) {
        if (hit.day.isNotBlank()) {
            val page = withContext(Dispatchers.IO) { source.day(peerDid, hit.day) }
            page.messages.forEach(::upsert)
            loadDayReceipts(page.day)
        }
        val index = withTimeout(2000) {
            snapshotFlow { currentRows.value.indexOfFirst { it is ConvRow.Msg && it.m.id == hit.id } }
                .first { it >= 0 }
        }
        listState.scrollToItem(index + if (block != null && (block.view.blockedByMe || blockedMeAt != null)) 1 else 0)
    }
    var foundId by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(searchHit, loaded) {
        val hit = searchHit ?: return@LaunchedEffect
        if (!loaded || hit.conv != peerDid) return@LaunchedEffect
        runCatching { jumpTo(FindHit(hit.id, hit.day)) }.onSuccess { foundId = hit.id; delay(2600); foundId = null }
            .onFailure { toast(friendly(it)) }
    }
    val app = remember(ctx) { ctx.applicationContext as? P2pApp }
    val coreFindSource = app?.let { runCatching { source === it.realChat }.getOrDefault(false) } == true
    // Loading a hit's day must not restart indexed search or reset its cursor/current selection.
    val localFindMessages = if (coreFindSource) null else msgs.toList()
    LaunchedEffect(find.open, find.q, localFindMessages) {
        if (!find.open) return@LaunchedEffect
        val query = find.q.trim()
        find.error = null; find.cur = 0; find.next = null
        if (query.codePointCount(0, query.length) < 2) { find.hits = emptyList(); return@LaunchedEffect }
        delay(150)
        val result = withContext(Dispatchers.IO) { runCatching { ChatFind.core(app, source, peerDid, query, null) } }
        val page = result.getOrNull()
        find.hits = page?.first ?: if (result.isSuccess && source !is CoreChatSource) ChatFind.local(msgs, query) else emptyList()
        find.next = page?.second
        find.error = result.exceptionOrNull()?.let(::friendly)
    }
    LaunchedEffect(find.current, find.open) {
        if (find.open) find.current?.let { runCatching { jumpTo(it) }.onFailure { toast(friendly(it)) } }
    }
    fun olderMatch() {
        if (find.cur + 1 < find.hits.size) { find.cur++; return }
        val cursor = find.next
        if (cursor == null) { find.cur = 0; return }
        scope.launch {
            val query = find.q
            val result = withContext(Dispatchers.IO) { runCatching { ChatFind.core(app, source, peerDid, query.trim(), cursor) } }
            if (!find.open || query != find.q) return@launch
            result.onSuccess { page ->
                if (page != null) {
                    val oldSize = find.hits.size
                    find.hits = (find.hits + page.first).distinctBy { it.id }; find.next = page.second
                    if (find.hits.size > oldSize) find.cur = oldSize
                }
            }.onFailure { find.error = friendly(it) }
        }
    }
    val pendingIds = msgs.filter { it.outgoing && it.delivery == DeliveryState.PENDING }.map { it.id }
    LaunchedEffect(blockedMeAt, pendingIds) {
        val ids = if (blockedMeAt == null) emptyList() else withContext(Dispatchers.IO) { pendingIds.filter { notDelivered(it) != null } }
        undelivered.clear(); undelivered.addAll(ids)
    }

    fun send() {
        val text = draft.trim()
        if (text.isEmpty() || sending) return
        val edit = editing
        val reply = replying
        val originalDraft = draft
        sending = true
        scope.launch {
            try {
                val result = withContext(Dispatchers.IO) { runCatching { if (edit != null) source.edit(peerDid, edit, text) else source.sendText(peerDid, text, reply) } }
                result.onSuccess { m ->
                    upsert(m)
                    loadReceipt(m.id)
                    if (draft == originalDraft && editing == edit && replying == reply) savedDraft.sent()
                    if (edit == null) { TinFx.messageSent(); Cues.sent() }
                    listState.scrollToItem(0)
                }.onFailure { toast(friendly(it)) }
            } finally { sending = false }
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
    fun cancel(m: Message) {
        requested.remove(m.id)
        scope.launch(Dispatchers.IO) { runCatching { source.cancel(peerDid, m.id) } }
    }
    fun saveToPhone(m: Message) {
        val a = m.attachment ?: return
        scope.launch(Dispatchers.IO) {
            val ok = runCatching { ChatMedia.saveToPhone(ctx, ChatMedia.decrypt(ctx, source, m), a.name, a.mime) }.getOrDefault(false)
            withContext(Dispatchers.Main) { toast(if (ok) "Saved to your phone" else "Couldn’t save that file.") }
        }
    }

    fun openFile(m: Message) {
        val a = m.attachment ?: return
        val plan = FileOpen.plan(a.name, a.mime)
        when (plan.kind) {
            OpenKind.NONE -> saveToPhone(m)
            OpenKind.PDF, OpenKind.TEXT -> docViewer = m.id
            OpenKind.EXTERNAL -> scope.launch(Dispatchers.IO) {
                val ok = runCatching { FileOpen.openExternally(ctx, ChatMedia.decrypt(ctx, source, m), plan.mime) }.getOrDefault(false)
                if (!ok) withContext(Dispatchers.Main) { toast("No app on this phone opens .${FileOpen.extOf(a.name)} files — Save it instead") }
            }
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
            Box(Modifier.fillMaxWidth().height(56.dp)) {
                if (find.open) FindBar(find, {}, ::olderMatch, { if (find.hits.isNotEmpty()) find.cur = (find.cur - 1 + find.hits.size) % find.hits.size }, { find.close() })
                else ConvHeader(name, peerDid, link, onBack, onCall, first, isSelf, composerState, block?.callGate ?: CallGate.Ok, { find.show() }, onInfo, online, onVideo)
            }
            Box(Modifier.weight(1f).fillMaxWidth().background(c.canvas)) {
            LazyColumn(
                Modifier.fillMaxSize(), state = listState, reverseLayout = true,
                contentPadding = PaddingValues(horizontal = 10.dp, vertical = 8.dp),
            ) {
                // Newest at the bottom (reversed list): the block note sits under the last message.
                if (block != null && (block.view.blockedByMe || blockedMeAt != null)) item(key = "block") { BlockNote(name, block.view) }
                items(rows, key = { r -> when (r) { is ConvRow.Day -> "d" + r.label + rows.indexOf(r); is ConvRow.Msg -> r.m.id; is ConvRow.Unread -> "unread"; is ConvRow.Call -> "call" + r.r.callId } }) { r ->
                    when (r) {
                        is ConvRow.Day -> PgDayPill(r.label, Modifier.padding(vertical = 10.dp))
                        is ConvRow.Unread -> PgUnreadDivider(r.n)
                        is ConvRow.Call -> CallPill(r.r, enabled = block?.callGate ?: CallGate.Ok == CallGate.Ok, onCall)
                        is ConvRow.Msg -> Bubble(
                            r.m, r.pos, name, link, online, nowMs, progress, requested, byId, source, receipts[r.m.id],
                            notDelivered = if (r.m.id in undelivered) BlockCopy.notDelivered(name) else null,
                            selected = actionsFor == r.m.id || foundId == r.m.id,
                            findQuery = if (find.open && find.hits.any { it.id == r.m.id }) find.q else "", findCurrent = find.current?.id == r.m.id,
                            onLong = { if (!r.m.deleted) actionsFor = r.m.id },
                            onReply = { savedDraft.beginReply(r.m.id) },
                            onDownload = { download(r.m) }, onCancel = { cancel(r.m) }, onOpenPhoto = { viewer = r.m.id }, onSaveFile = { saveToPhone(r.m) },
                            onOpenFile = { openFile(r.m) }, broken = r.m.id in brokenImages, onBroken = { if (r.m.id !in brokenImages) brokenImages.add(r.m.id) },
                        )
                    }
                }
                // Only when this phone may lack earlier days: the contact was added before our oldest day and their phone has not said "nothing older".
                val oldestDay = msgs.firstOrNull()?.let { Instant.ofEpochMilli(it.at.toLong()).atZone(ZoneOffset.UTC).toLocalDate() }
                val addedDay = addedAtSecs?.let { Instant.ofEpochSecond(it).atZone(ZoneOffset.UTC).toLocalDate() }
                val mayHaveOlder = oldestDay != null && addedDay != null && addedDay.isBefore(oldestDay) && !noOlder
                if (loaded && olderDay == null && mayHaveOlder) item(key = "older") {
                    Row(
                        Modifier.fillMaxWidth().padding(start = 0.dp, end = 0.dp, top = 8.dp, bottom = 8.dp).border(1.dp, c.ln2, SmoothCornerShape(14.dp)).padding(16.dp),
                        horizontalArrangement = Arrangement.spacedBy(12.dp),
                    ) {
                        Icon(PgIcons.Clock, null, tint = c.ink2)
                        Column(Modifier.weight(1f)) {
                            Text("Older messages are on $first’s phone", style = TinType.bodyM.copy(fontSize = 15.sp, fontWeight = FontWeight.SemiBold), color = c.ink)
                            Text("This phone keeps them from when you started using it. Earlier days load when you’re both online.", Modifier.padding(top = 4.dp), style = TinType.bodyM, color = c.ink2)
                        }
                    }
                }
            }
            if ((!online || link == Link.Offline) && composerState == ComposerState.Open) PgStrip(
                if (!online) "You’re offline. Messages send when you’re back online." else "$first is offline. Messages send when you’re both online.", Pg.Off, modifier = Modifier.align(Alignment.TopCenter),
            )
            }
            if (composerState != ComposerState.Open) BlockedFooter(name, composerState, onUnblock, onAddContact)
            else Composer(
                draft, { draft = it }, editing != null, replying?.let { byId[it] }?.let { ComposerBanner("Replying to ${if (it.outgoing) "yourself" else first}", ChatNotifier.previewOf(it)) },
                editing?.let { byId[it] }?.let { ComposerBanner("Editing message", it.text) },
                onCancelBanner = { savedDraft.cancel() },
                onAttach = { attachOpen = true }, onSend = ::send,
                onVoice = { path, ms, wave -> scope.launch(Dispatchers.IO) { runCatching { source.sendVoice(peerDid, path, ms, wave) }.onSuccess { m -> withContext(Dispatchers.Main) { upsert(m) } }.onFailure { withContext(Dispatchers.Main) { toast("Couldn’t send that voice message.") } } } },
            )
        }
        val dm = docViewer?.let { byId[it] }
        if (dm != null) FileViewer(dm, FileOpen.plan(dm.attachment?.name ?: "", dm.attachment?.mime ?: ""), source, onClose = { docViewer = null }, onSave = { saveToPhone(dm) }, onShare = {
            scope.launch(Dispatchers.IO) { runCatching { ChatMedia.share(ctx, ChatMedia.decrypt(ctx, source, dm), dm.attachment?.mime ?: "*/*") } }
        })
        val vm = viewer?.let { byId[it] }
        if (vm != null) PhotoViewer(vm, name, source, onClose = { viewer = null }, onSave = { saveToPhone(vm) }, onShare = {
            scope.launch(Dispatchers.IO) { runCatching { ChatMedia.share(ctx, ChatMedia.decrypt(ctx, source, vm), vm.attachment?.mime ?: "image/*") } }
        })
    }
    val am = actionsFor?.let { byId[it] }
    if (am != null) ActionsSheet(
        am, first, isSelf, receipts[am.id], name, onDismiss = { actionsFor = null },
        onReply = { actionsFor = null; savedDraft.beginReply(am.id) },
        onCopy = { actionsFor = null; scope.launch { clipboard.setClipEntry(ClipEntry(android.content.ClipData.newPlainText("message", am.text))) }; toast("Copied") },
        onEdit = { actionsFor = null; savedDraft.beginEdit(am.id, am.text) },
        onDelete = {
            actionsFor = null
            scope.launch(Dispatchers.IO) { runCatching { source.delete(peerDid, am.id) }.onSuccess { m -> withContext(Dispatchers.Main) { upsert(m); loadReceipt(m.id) } }.onFailure { withContext(Dispatchers.Main) { toast("Couldn’t delete that message.") } } }
        },
        onSave = { actionsFor = null; saveToPhone(am) },
    )
    if (attachOpen) AttachSheet(
        first, isSelf, onDismiss = { attachOpen = false },
        onPhotos = { attachOpen = false; photoPicker.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageAndVideo)) },
        onCamera = {
            attachOpen = false
            if (Perms.granted(ctx, android.Manifest.permission.CAMERA)) launchCamera() else cameraPerm.launch(android.Manifest.permission.CAMERA)
        },
        onFile = { attachOpen = false; filePicker.launch(arrayOf("*/*")) },
    )
}

/** Conversation.svelte `.bar` (Android): back · the head (avatar 36, name 17/22, status 13/18 muted; tap = details)
 *  · call · video · find · details, each a 44 round button; call and video dim (with the reason) when calling isn't possible. */
@Composable
private fun ConvHeader(name: String, peerDid: String, link: Link, onBack: () -> Unit, onCall: () -> Unit, first: String, isSelf: Boolean, blockState: ComposerState = ComposerState.Open, callGate: CallGate = CallGate.Ok, onFind: () -> Unit = {}, onInfo: (() -> Unit)? = null, online: Boolean = true, onVideo: (() -> Unit)? = null) {
    val c = Tin.c
    Row(Modifier.fillMaxWidth().height(56.dp).background(c.sf).padding(horizontal = 6.dp, vertical = 4.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(2.dp)) {
        PgIconBtn(Pg.Back, "Back to chats", onBack)
        val status = when {
            isSelf -> SELF_SUB
            blockState == ComposerState.BlockedByMe -> "Blocked"
            blockState == ComposerState.BlockedMe -> "Blocked you"
            blockState == ComposerState.NotContact -> "Not in your contacts"
            !online -> "Waiting for connection"
            else -> (if (link == Link.Connected || link == Link.Relayed) "Online" else "Offline") + if (callGate == CallGate.NotAllowed) " · messages only" else ""
        }
        val statusColor = if (blockState == ComposerState.BlockedByMe || blockState == ComposerState.BlockedMe) c.er else c.ink2
        Row(Modifier.weight(1f).heightIn(min = 44.dp).clip(SmoothCornerShape(12.dp)).then(if (onInfo != null) Modifier.clickable(onClick = onInfo) else Modifier)
            .padding(start = 6.dp, end = 10.dp, top = 4.dp, bottom = 4.dp)
            .semantics(mergeDescendants = true) { contentDescription = "$name. $status. Chat details" },
            verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            Avatar(name, peerDid, 36.dp)
            Column(Modifier.weight(1f)) {
                Text(name, style = PgBodyText.copy(fontSize = 17.sp, lineHeight = 22.sp, fontWeight = FontWeight.SemiBold), color = c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(status, style = PgBodyText.copy(fontSize = 13.sp, lineHeight = 18.sp), color = statusColor, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
        if (!isSelf) {
            val ok = blockState == ComposerState.Open && callGate == CallGate.Ok
            // Dimmed buttons still answer a tap: the call path says why it can't call.
            // My call with them is running (decided 2026-10-14): the phone turns green and returns to it, as a group's does.
            val live = P2pApp.instanceOrNull()?.calls?.ui?.collectAsState()?.value?.takeIf {
                it.info.peerDid == peerDid && !(it.info.incoming && it.state is uniffi.p2pcore.CallState.Ringing) && it.state !is uniffi.p2pcore.CallState.Ended }
            val ctx = androidx.compose.ui.platform.LocalContext.current
            if (live != null) Box(Modifier.size(44.dp).clip(CircleShape).background(c.accentSoft)) {
                PgIconBtn(Pg.Phone, "Return to call", { ctx.startActivity(android.content.Intent(ctx, CallActivity::class.java)) }, tint = c.accentInk)
            }
            else Box(Modifier.alpha(if (ok) 1f else .45f)) { PgIconBtn(Pg.Phone, if (ok) "Call" else "Can’t call $first", onCall) }
            if (onVideo != null) Box(Modifier.alpha(if (ok) 1f else .45f)) { PgIconBtn(Pg.Video, if (ok) "Video call" else "Can’t call $first", if (ok) onVideo else onCall) }
        }
        PgIconBtn(Pg.Search, "Find in this chat", onFind)
        if (onInfo != null) PgIconBtn(Pg.Info, "Chat details", onInfo)
    }
}

/** MessageRow `.callrec`: a one-line pill (32 tall, radius 16, surface, hairline) on the caller's side; tap = call back. */
@Composable
private fun CallPill(r: uniffi.p2pcore.CallRecord, enabled: Boolean, onCall: () -> Unit) {
    val c = Tin.c
    val answered = r.durationSecs > 0u
    val d = r.durationSecs.toLong()
    val text = when {
        answered -> "Voice call · ${d / 60}:${"%02d".format(d % 60)}"
        r.incoming -> if (r.reason == "declined_local") "Declined voice call" else "Missed voice call"
        else -> when (r.reason) { "no_answer" -> "No answer"; "declined" -> "Declined"; "cancelled", "hangup_local" -> "Cancelled call"; else -> "Call failed" }
    }
    val bad = !answered && r.incoming && r.reason != "declined_local"
    val time = java.time.Instant.ofEpochSecond(r.startedAt.toLong()).atZone(java.time.ZoneId.systemDefault()).toLocalTime().let { "%02d:%02d".format(it.hour, it.minute) }
    Row(Modifier.fillMaxWidth().padding(top = 10.dp), horizontalArrangement = if (r.incoming) Arrangement.Start else Arrangement.End) {
        Row(Modifier.heightIn(min = 32.dp).clip(RoundedCornerShape(16.dp)).background(c.sf).border(1.dp, c.ln, RoundedCornerShape(16.dp))
            .clickable(enabled = enabled, onClick = onCall).padding(horizontal = 12.dp)
            .semantics(mergeDescendants = true) { contentDescription = "$text, $time. Call back" },
            verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            Icon(Pg.Phone, null, Modifier.size(16.dp), tint = if (bad) c.er else c.ink2)
            Text(text, style = PgBodyText.copy(fontSize = 13.sp, lineHeight = 18.sp, fontWeight = FontWeight.SemiBold), color = if (bad) c.er else c.ink, maxLines = 1)
            Text(time, style = PgBodyText.copy(fontSize = 11.sp, lineHeight = 14.sp, fontFeatureSettings = "tnum"), color = c.ink2)
        }
    }
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
    PgComposer(draft, onDraft, "Message", banner?.let { ComposerCtx(it.title, it.text, if (editing) "Cancel edit" else "Cancel reply") },
        editing, onCancelBanner, onSend, onAttach,
        field = if (voice.recording) { modifier -> VoiceRecordingBar(voice, modifier) } else null,
        trailing = { VoiceMicButton(voice) },
    )
}

@Composable
private fun BannerLine(text: String) =
    Text(text, style = TinType.bodyM.copy(fontSize = 13.sp, lineHeight = 18.sp), color = Tin.c.ink2, maxLines = 1, overflow = TextOverflow.Ellipsis)

// ------------------------------------------------------------------ bubbles

@OptIn(androidx.compose.foundation.ExperimentalFoundationApi::class)
@Composable
private fun Bubble(
    m: Message, pos: BubblePos, name: String, link: Link, online: Boolean, nowMs: Long, progress: Map<String, Pair<Long, Long>>, requested: List<String>,
    byId: Map<String, Message>, source: ChatSource, receipt: MessageReceiptStatus?, notDelivered: String? = null, selected: Boolean,
    onLong: () -> Unit, onReply: () -> Unit, onDownload: () -> Unit, onCancel: () -> Unit, onOpenPhoto: () -> Unit, onSaveFile: () -> Unit,
    onOpenFile: () -> Unit, broken: Boolean, onBroken: () -> Unit,
    findQuery: String = "", findCurrent: Boolean = false,
) {
    val c = Tin.c
    val out = m.outgoing
    val shape = bubbleShape(pos, out)
    val first = firstName(name)
    var drag by remember(m.id) { mutableFloatStateOf(0f) }
    val threshold = with(LocalDensity.current) { 64.dp.toPx() }
    val replyAction by rememberUpdatedState(onReply)
    val swipe = if (m.deleted) Modifier else Modifier.pointerInput(m.id, threshold) {
        detectHorizontalDragGestures(
            onDragEnd = {
                if (drag >= threshold) replyAction()
                drag = 0f
            },
            onDragCancel = { drag = 0f },
        ) { change, amount ->
            if (drag > 0f || amount > 0f) {
                change.consume()
                drag = (drag + amount).coerceIn(0f, threshold * 1.4f)
            }
        }
    }
    BoxWithConstraints(Modifier.fillMaxWidth().padding(top = gapAbove(pos)).then(swipe), contentAlignment = if (out) Alignment.CenterEnd else Alignment.CenterStart) {
        // Up to 82% of the screen (capped for tablets), so text is not wrapped into a narrow column.
        val bubbleMaxWidth = minOf(maxWidth * .78f, 560.dp)
        if (drag > 0f) Icon(PgIcons.Reply, "Reply", tint = c.pr,
            modifier = Modifier.align(Alignment.CenterStart).size(24.dp))
        if (m.deleted) {
            Column(
                Modifier.widthIn(max = bubbleMaxWidth).clip(shape).let { if (out) it.border(1.dp, c.ln2, shape) else it }.padding(start = 12.dp, end = 12.dp, top = 8.dp, bottom = 6.dp),
            ) {
                Text(if (out) "You deleted this message" else "$first deleted this message", style = TinType.bodyL.copy(fontSize = 15.sp, lineHeight = 21.sp, fontStyle = FontStyle.Italic), color = c.ink2)
                Meta(msClock(m.at.toLong()), null, c.ink2, null)
            }
            return@BoxWithConstraints
        }
        val a = m.attachment
        if (a == null && notDelivered == null) {
            val meta = BubbleMeta(msClock(m.at.toLong()), m.editedAt != null, receiptMark(m, receipt, false, first))
            PgTextBubble(
                ChatFind.marked(Emoji.styled(m.text), findQuery, findCurrent, c.thread), pos, out, meta,
                Modifier.widthIn(max = bubbleMaxWidth).combinedClickable(onClick = {}, onLongClick = onLong)
                    .let { if (selected) it.border(2.dp, c.accentInk, shape) else it },
                emoji = m.replyTo == null && Emoji.isBig(m.text), held = selected,
                quote = m.replyTo?.let { rid -> { val q = byId[rid]; PgQuote(if (q == null) null else if (q.outgoing) "You" else first, c.accentInk, q?.let(ChatNotifier::previewOf) ?: "Original message unavailable", null) } },
            )
            return@BoxWithConstraints
        }
        val photo = a != null && a.isImage() && !broken
        val prog = a?.let { progress[it.hash] }
        val tick = if (out && receipt != null && receipt.tick != ReceiptTick.NONE) tickOf(m, receipt.tick, link, online, nowMs) else null
        val mark = receiptMark(m, receipt, notDelivered != null, first)
        val bg = if (out) c.prc else c.bubIn
        val fg = if (out) c.onPrc else c.ink
        Column(
            Modifier.offset { IntOffset(drag.roundToInt(), 0) }.widthIn(max = bubbleMaxWidth)
                .clip(shape).background(bg).let { if (!out) it.border(1.dp, c.bubInLine, shape) else it }
                .let { if (selected) it.border(3.dp, c.pr, shape) else it }
                .combinedClickable(onClick = {}, onLongClick = onLong)
                .padding(if (photo) PaddingValues(start = 3.dp, end = 3.dp, top = 3.dp, bottom = 6.dp) else PaddingValues(start = 12.dp, end = 12.dp, top = 8.dp, bottom = 6.dp)),
        ) {
            m.replyTo?.let { rid ->
                val q = byId[rid]
                Column(
                    Modifier.padding(bottom = 8.dp).fillMaxWidth().clip(SmoothCornerShape(8.dp))
                        .background(if (out) Color.White.copy(alpha = .22f) else c.bg)
                        .drawLeftBar(c.pr).padding(start = 12.dp, top = 7.dp, end = 10.dp, bottom = 7.dp),
                ) {
                    Text(if (q == null) "Earlier message" else if (q.outgoing) "You" else first, style = TinType.caption.copy(fontWeight = FontWeight.Bold), color = c.pr)
                    Text(q?.let { ChatNotifier.previewOf(it) } ?: "Message unavailable",
                        style = TinType.bodyM.copy(fontSize = 13.sp, lineHeight = 18.sp),
                        color = fg.copy(alpha = .8f), maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
            if (a != null) when {
                a.kind == AttachmentKind.VOICE -> VoiceBubble(m, a, a.state == TransferState.READY || out, out, source, onDownload)
                photo -> PhotoBox(m, a, out, pos, first, out && tick == Tick.Clock, prog, source, onDownload, onCancel, onOpenPhoto, onBroken)
                else -> FileRow(a, out, prog, link, first, m.id in requested, onDownload, onCancel, onSaveFile, onOpenFile, onLong)
            }
            val at = msClock(m.at.toLong())
            val sendingPct = if (out && prog != null && prog.second > 0) (prog.first * 100 / prog.second).toInt() else null
            val label = when {
                notDelivered != null -> "$notDelivered · $at"
                sendingPct != null && !photo -> "Sending $sendingPct% · $at"
                tick == Tick.Clock -> "Waiting · $at"
                m.editedAt != null -> "edited · $at"
                else -> at
            }
            val metaColor = if (notDelivered != null) c.er else if (tick == Tick.Clock) c.threadText else if (out) c.onPrc else c.ink2
            if (a == null && m.text.isNotBlank()) {
                // Only a few emojis: big, like the other chat apps; emojis in text: a bit larger.
                val big = Emoji.isBig(m.text)
                val style = if (big) TinType.bodyL.copy(fontSize = 38.sp, lineHeight = 46.sp) else TinType.bodyL.copy(fontSize = 15.sp, lineHeight = 21.sp)
                CompactTextBubble(if (big) AnnotatedString(m.text.trim()) else Emoji.styled(m.text), style, fg, label, mark, metaColor)
            } else {
                // A photo's caption and time keep to the photo's width (the bubble is never wider than the photo).
                val w = if (photo) Modifier.width(photoBox(a.width, a.height).first) else Modifier
                if (m.text.isNotBlank()) Text(Emoji.styled(m.text), w.padding(horizontal = if (photo) 9.dp else 0.dp).padding(top = if (photo) 5.dp else 0.dp),
                    style = TinType.bodyL.copy(fontSize = 15.sp, lineHeight = 21.sp), color = fg)
                Box(w) { Meta(label, mark, metaColor, if (photo) 7.dp else 0.dp, 0.dp) }
            }
        }
    }
}

/** Reserve the last line's trailing space for time/ticks, without squeezing the message.
 * RTL uses a separate metadata row until bidi-aware trailing placement is supported. */
@Composable
private fun CompactTextBubble(text: AnnotatedString, style: TextStyle, color: Color, label: String, mark: MetaReceipt?, metaColor: Color) {
    val measurer = rememberTextMeasurer()
    val metaStyle = TinType.caption.copy(fontSize = 11.sp, lineHeight = 14.sp, fontWeight = FontWeight.Normal)
    SubcomposeLayout { constraints ->
        val loose = constraints.copy(minWidth = 0, minHeight = 0)
        val meta = subcompose("meta") {
            Row(horizontalArrangement = Arrangement.spacedBy(4.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(label, style = metaStyle, color = metaColor)
                ReceiptMark(mark, metaColor)
            }
        }.single().measure(loose)
        val result = measurer.measure(text, style = style, constraints = loose)
        val last = result.lineCount - 1
        val trailing = ceil(result.getLineRight(last)).toInt()
        val gap = 8.dp.roundToPx()
        val needed = trailing + gap + meta.width
        val inline = layoutDirection == LayoutDirection.Ltr && needed <= constraints.maxWidth
        val width = constraints.constrainWidth(max(result.size.width, if (inline) needed else meta.width))
        val body = subcompose("body") {
            Text(text, style = style, color = color)
        }.single().measure(Constraints.fixedWidth(result.size.width))
        val metaY = if (inline) max(0, body.height - meta.height) else body.height + 3.dp.roundToPx()
        val height = constraints.constrainHeight(max(body.height, metaY + meta.height))
        layout(width, height) {
            body.placeRelative(0, 0)
            meta.placeRelative(width - meta.width, metaY)
        }
    }
}

/** My message's receipt mark for the bubble meta (ReceiptTicks.kt): the core's `ReceiptTick`, or not delivered. */
private fun receiptMark(m: Message, receipt: MessageReceiptStatus?, notDelivered: Boolean, first: String): MetaReceipt? {
    if (!m.outgoing || receipt?.tick == ReceiptTick.NONE) return null
    val vis = tickVisOf(receipt?.tick, notDelivered)
    return MetaReceipt(vis, m.id, vis?.let { tickLabel(it, first) } ?: "")
}

@Composable
private fun ReceiptMark(mark: MetaReceipt?, metaColor: Color) {
    if (mark == null || (mark.vis == null && !mark.reserve)) return
    Box(Modifier.size(tickSlotWidth(12.dp), 12.dp), contentAlignment = Alignment.Center) {
        mark.vis?.let { ReceiptTickGlyph(mark.key, it, metaColor, Tin.c.tickRead, Tin.c.tickBad, Tin.c.prc, mark.label) }
    }
}

private fun Modifier.drawLeftBar(color: Color) = drawBehind { drawRect(color, size = androidx.compose.ui.geometry.Size(3.dp.toPx(), size.height)) }

@Composable
private fun Meta(text: String, mark: MetaReceipt?, color: Color, hPad: androidx.compose.ui.unit.Dp?, bPad: androidx.compose.ui.unit.Dp = 0.dp) {
    Row(
        Modifier.fillMaxWidth().padding(top = 4.dp, start = hPad ?: 0.dp, end = hPad ?: 0.dp, bottom = bPad),
        horizontalArrangement = Arrangement.spacedBy(4.dp, Alignment.End), verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(text, style = TinType.caption.copy(fontSize = 11.sp, lineHeight = 14.sp, fontWeight = FontWeight.Normal), color = color)
        ReceiptMark(mark, color)
    }
}

/** A photo's box in its bubble: the true shape from the size the core sends (chat/5), at most 248 x 330 dp, at least 120
 *  wide; 4:3 when unknown (photos from older apps). */
internal fun photoBox(w: UInt, h: UInt): Pair<androidx.compose.ui.unit.Dp, androidx.compose.ui.unit.Dp> {
    val (pw, ph) = if (w > 0u && h > 0u) w.toFloat() to h.toFloat() else 4f to 3f
    val s = minOf(248f / pw, 330f / ph)
    return maxOf(120f, pw * s).dp to (ph * s).dp
}

/** The core's 4x3 colour preview as a tiny bitmap (drawn scaled up, smooth and blurred). */
internal fun previewBitmap(p: ByteArray): androidx.compose.ui.graphics.ImageBitmap? {
    if (p.size != 36) return null
    val px = IntArray(12) { i -> (0xFF shl 24) or ((p[i * 3].toInt() and 0xFF) shl 16) or ((p[i * 3 + 1].toInt() and 0xFF) shl 8) or (p[i * 3 + 2].toInt() and 0xFF) }
    return android.graphics.Bitmap.createBitmap(px, 4, 3, android.graphics.Bitmap.Config.ARGB_8888).asImageBitmap()
}

/**
 * PhotoBody.svelte (Framed, decided 2026-10-14): the photo inside its bubble's 3 dp rim, its corners following the
 * bubble's (so they tighten in a run). Until it is here the core's colour preview fills the box, soft; sending shows a
 * ring with cancel, waiting a chip, not downloaded the size and Download. The time sits under it (the bubble's meta).
 */
@Composable
private fun PhotoBox(m: Message, a: Attachment, out: Boolean, pos: BubblePos, first: String, waiting: Boolean, prog: Pair<Long, Long>?, source: ChatSource,
    onDownload: () -> Unit, onCancel: () -> Unit, onOpen: () -> Unit, onBroken: () -> Unit) {
    val c = Tin.c
    val ctx = LocalContext.current
    val ready = a.state == TransferState.READY || out
    val bmp by produceState<androidx.compose.ui.graphics.ImageBitmap?>(null, m.id, ready) {
        value = if (!ready) null else withContext(Dispatchers.IO) { runCatching { ChatMedia.bitmap(ChatMedia.decrypt(ctx, source, m), 720) }.getOrNull() }
        // Ready but undecodable (odd GIF/WebP/HEIC): fall back to a file bubble with Open/Save.
        if (ready && value == null) onBroken()
    }
    val pv = remember(a.hash) { previewBitmap(a.preview) }
    val (w, h) = photoBox(a.width, a.height)
    val frac = prog?.let { if (it.second > 0) it.first.toFloat() / it.second else 0f }
        ?: if (a.state == TransferState.DOWNLOADING && a.size > 0UL) a.transferred.toFloat() / a.size.toFloat() else null
    val busy = frac != null && !(ready && prog == null)
    Box(Modifier.size(w, h).clip(bubbleShape(pos, out, 3.dp)).background(c.sf3)
        .clickable(role = Role.Button) { if (ready && bmp != null) onOpen() else if (busy) onCancel() else if (!ready) onDownload() },
        contentAlignment = Alignment.Center) {
        pv?.let {
            Image(it, null, Modifier.fillMaxSize().graphicsLayer { scaleX = 1.24f; scaleY = 1.24f }.blur(14.dp),
                contentScale = ContentScale.FillBounds, filterQuality = androidx.compose.ui.graphics.FilterQuality.High)
        }
        bmp?.let { Image(it, a.name, Modifier.fillMaxSize().alpha(if (a.state == TransferState.FAILED) .55f else 1f), contentScale = ContentScale.Crop) }
        when {
            busy -> {
                Box(Modifier.fillMaxSize().background(Color.Black.copy(alpha = .18f)))
                Ring {
                    CircularProgressIndicator({ frac }, Modifier.size(48.dp), color = Color.White, trackColor = Color.White.copy(alpha = .28f), strokeWidth = 3.dp)
                    Icon(PgIcons.X, if (out) "Cancel sending" else "Cancel download", Modifier.size(18.dp), tint = Color.White)
                }
            }
            a.state == TransferState.REMOTE && !out -> Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)) {
                PhotoChip("Photo · ${sizeText(a.size.toLong())}")
                Row(Modifier.clip(RoundedCornerShape(18.dp)).background(c.accent).clickable(role = Role.Button, onClick = onDownload).padding(horizontal = 16.dp, vertical = 7.dp),
                    verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    Icon(PgIcons.Chev, null, Modifier.size(16.dp), tint = c.onAccent)
                    Text("Download", style = TinType.caption.copy(fontSize = 13.sp, fontWeight = FontWeight.SemiBold), color = c.onAccent)
                }
            }
            a.state == TransferState.FAILED -> Ring { Icon(PgIcons.Retry, "Retry download", tint = Color.White) }
            bmp == null && ready && pv == null -> Icon(PgIcons.Image, null, tint = c.ink2)
        }
        if (waiting && !busy) Box(Modifier.align(Alignment.BottomStart).padding(8.dp)) { PhotoChip("Sends when $first is online") }
    }
}

@Composable
private fun PhotoChip(text: String) =
    Text(text, Modifier.clip(RoundedCornerShape(10.dp)).background(Color.Black.copy(alpha = .42f)).padding(horizontal = 8.dp, vertical = 2.dp),
        style = TinType.caption.copy(fontSize = 11.sp, lineHeight = 15.sp, fontWeight = FontWeight.Normal), color = Color.White)

@Composable
private fun Ring(content: @Composable BoxScope.() -> Unit) =
    Box(Modifier.size(56.dp).clip(CircleShape).background(Color(0x8C0F1513)), contentAlignment = Alignment.Center, content = content)

@OptIn(androidx.compose.foundation.ExperimentalFoundationApi::class)
@Composable
private fun FileRow(a: Attachment, out: Boolean, prog: Pair<Long, Long>?, link: Link, first: String, asked0: Boolean, onDownload: () -> Unit, onCancel: () -> Unit, onSave: () -> Unit, onOpen: () -> Unit, onLong: () -> Unit) {
    val c = Tin.c
    // "starting…" only until the core reports Downloading; never hides a failure (tap to retry).
    val asked = asked0 && a.state == TransferState.REMOTE
    val ready = a.state == TransferState.READY || out
    val size = sizeText(a.size.toLong())
    val frac = prog?.let { if (it.second > 0) it.first.toFloat() / it.second else 0f }
        ?: if (a.state == TransferState.DOWNLOADING && a.size > 0UL) a.transferred.toFloat() / a.size.toFloat() else null
    val paused = link == Link.Offline
    val canOpen = FileOpen.plan(a.name, a.mime).kind != OpenKind.NONE
    val sub = when {
        ready && canOpen -> "$size · ${typeLabel(a)} · tap to open"
        ready -> "$size · ${typeLabel(a)} · tap to save"
        a.state == TransferState.DOWNLOADING && !paused -> "Downloading · ${sizeText((frac ?: 0f).times(a.size.toLong()).toLong())} of $size"
        (a.state == TransferState.DOWNLOADING || asked) && paused -> "$size · paused — waiting for $first to come online"
        asked -> "$size · starting…"
        a.state == TransferState.FAILED -> "$size · couldn’t download — tap to retry"
        else -> "$size · not downloaded"
    }
    val canDownload = !ready && (a.state == TransferState.REMOTE || a.state == TransferState.FAILED) && !asked
    Row(
        Modifier.widthIn(min = 240.dp).clip(SmoothCornerShape(8.dp)).combinedClickable(role = Role.Button, onLongClick = onLong) { if (ready) (if (canOpen) onOpen() else onSave()) else if (canDownload) onDownload() },
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Box(Modifier.size(44.dp).clip(SmoothCornerShape(12.dp)).background(if (out) Color.White.copy(alpha = .55f) else c.sf2), contentAlignment = Alignment.Center) {
            Icon(if (a.isVideo()) PgIcons.Video else PgIcons.File, null, tint = c.pr)
        }
        Column(Modifier.weight(1f, fill = false)) {
            Text(a.name, style = TinType.bodyL.copy(fontSize = 15.sp, lineHeight = 20.sp, fontWeight = FontWeight.SemiBold), color = if (out) c.onPrc else c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(sub, style = TinType.caption.copy(fontWeight = FontWeight.Normal), color = if (out) c.onPrc else c.ink2)
            if (frac != null && !ready && !paused) Box(Modifier.padding(top = 6.dp).fillMaxWidth().height(4.dp).clip(RoundedCornerShape(2.dp)).background(Color.Black.copy(alpha = .1f))) {
                Box(Modifier.fillMaxWidth(frac.coerceIn(0f, 1f)).fillMaxHeight().background(c.pr))
            }
        }
        if (canDownload && a.state == TransferState.REMOTE) IconBtn(PgIcons.Download, "Download ${a.name}", onDownload, tint = c.pr)
        if (!ready && (a.state == TransferState.DOWNLOADING || asked)) IconBtn(PgIcons.X, "Cancel download of ${a.name}", onCancel, tint = c.ink2)
        if (ready && canOpen) IconBtn(PgIcons.Save, "Save ${a.name} to phone", onSave, tint = if (out) c.onPrc else c.ink2)
    }
}

// ------------------------------------------------------------------ sheets

@Composable
private fun ActionsSheet(
    m: Message, first: String, isSelf: Boolean, receipt: MessageReceiptStatus?, name: String, onDismiss: () -> Unit, onReply: () -> Unit, onCopy: () -> Unit, onEdit: () -> Unit, onDelete: () -> Unit, onSave: () -> Unit,
) {
    val c = Tin.c
    val at = msClock(m.at.toLong())
    TinSheet(onDismiss) {
        // Only the core's signed receipts say "on" or "read on" their device; a signature is the device's claim, not a person's attention.
        Hint(
            if (isSelf) "Saved $at · on your devices" else if (!m.outgoing) "Received $at" else when (receipt?.tick) {
                ReceiptTick.READ -> "Sent $at · read on $first’s device"
                ReceiptTick.DELIVERED -> "Sent $at · on $first’s device"
                ReceiptTick.ONLY_HERE -> "Sent $at · only on this phone so far"
                else -> "Sent $at · waiting for $first’s device to confirm"
            },
            Modifier.padding(start = 8.dp, bottom = if (m.outgoing && !isSelf && receipt?.recipients?.any { it.proofs.isNotEmpty() } == true) 2.dp else 8.dp),
        )
        if (m.outgoing && !isSelf) receipt?.recipients?.forEach { r ->
            r.proofs.forEach { p ->
                val what = if (p.kind == ReceiptProofKind.READ) "Read" else "Delivered"
                Hint("$what · signed by ${firstName(name)} · device ${p.signerDevice.take(8)}", Modifier.padding(start = 8.dp, bottom = 2.dp))
            }
        }
        if (m.outgoing && !isSelf && receipt?.recipients?.any { it.proofs.isNotEmpty() } == true) Spacer(Modifier.height(6.dp))
        SheetAction(PgIcons.Reply, "Reply", onClick = onReply)
        if (m.text.isNotBlank()) SheetAction(PgIcons.Copy, "Copy text", onClick = onCopy)
        if (m.attachment?.let { it.state == TransferState.READY || m.outgoing } == true) SheetAction(PgIcons.Save, "Save to phone", onClick = onSave)
        if (m.outgoing && m.attachment == null) SheetAction(PgIcons.Edit, "Edit", onClick = onEdit)
        if (m.outgoing) SheetAction(PgIcons.Trash, if (isSelf) "Delete on all your devices" else "Delete for both of you", color = c.er, onClick = onDelete)
        if (m.outgoing) Hint(if (isSelf) "Edits and deletes reach your other devices when they’re online." else "Edits and deletes reach $first’s phone the next time you’re both online.", Modifier.padding(start = 8.dp, top = 8.dp))
    }
}

@Composable
private fun SheetAction(icon: ImageVector, label: String, color: Color = Tin.c.ink, onClick: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().heightIn(min = 56.dp).clip(SmoothCornerShape(12.dp)).clickable(role = Role.Button, onClick = onClick).padding(horizontal = 8.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Icon(icon, null, tint = if (color == Tin.c.ink) Tin.c.ink2 else color)
        Text(label, style = TinType.bodyL, color = color)
    }
}

@Composable
private fun AttachSheet(first: String, isSelf: Boolean, onDismiss: () -> Unit, onPhotos: () -> Unit, onCamera: () -> Unit, onFile: () -> Unit) {
    val c = Tin.c
    TinSheet(onDismiss) {
        Text(if (isSelf) "Save for yourself" else "Send to $first", Modifier.padding(start = 4.dp, bottom = 8.dp), style = TinType.titleM, color = c.ink)
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            listOf(Triple(PgIcons.Image, "Photos & videos", onPhotos), Triple(PgIcons.Camera, "Camera", onCamera), Triple(PgIcons.File, "File", onFile)).forEach { (icon, label, go) ->
                Column(Modifier.weight(1f).clip(SmoothCornerShape(12.dp)).clickable(role = Role.Button, onClick = go).padding(vertical = 8.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Box(Modifier.size(60.dp).clip(SmoothCornerShape(18.dp)).background(c.prc), contentAlignment = Alignment.Center) { Icon(icon, null, tint = c.onPrc) }
                    Text(label, style = TinType.bodyM.copy(fontWeight = FontWeight.Medium), color = c.ink)
                }
            }
        }
        Row(Modifier.padding(start = 4.dp, top = 12.dp), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.Top) {
            Icon(PgIcons.Lock, null, tint = c.ink2, modifier = Modifier.size(18.dp))
            Hint(if (isSelf) "Files stay encrypted on your devices and copy to the others when they’re online. Up to 2 GB each." else "Files are end-to-end encrypted. Up to 2 GB each. Both of you need to be online while it transfers.")
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
            IconBtn(PgIcons.X, "Close", onClose, tint = Color.White)
            Column(Modifier.weight(1f)) {
                Text(if (m.outgoing) "You" else name, style = TinType.bodyL.copy(fontWeight = FontWeight.SemiBold), color = Color.White)
                Text(viewerWhen(m.at.toLong(), System.currentTimeMillis()), style = TinType.caption, color = Color(0xFFA6B2AD))
            }
            IconBtn(PgIcons.Share, "Share", onShare, tint = Color.White)
            IconBtn(PgIcons.Save, "Save to phone", onSave, tint = Color.White)
        }
        Box(Modifier.weight(1f).fillMaxWidth(), contentAlignment = Alignment.Center) {
            bmp?.let { Image(it, m.attachment?.name, Modifier.fillMaxSize(), contentScale = ContentScale.Fit) }
        }
        Row(Modifier.padding(start = 20.dp, end = 20.dp, top = 16.dp, bottom = 24.dp), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.Top) {
            Icon(PgIcons.Lock, null, tint = Color(0xFFA6B2AD), modifier = Modifier.size(18.dp))
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
        ) { Icon(if (remote) PgIcons.Download else PgIcons.Play, if (remote) "Download voice message" else "Voice message loading", tint = c.onPr) }
        Text(formatMs(a.durationMs.toInt()), style = TinType.caption.copy(fontFamily = PlexMono, fontSize = 12.sp), color = if (out) c.onPrc else c.ink2)
    }
}

// ------------------------------------------------------------------ blocking

/** Under the last message: a quiet line when we block them, their signed notice when they block us. */
@Composable
private fun BlockNote(name: String, v: BlockView) {
    val c = Tin.c
    val f = firstName(name)
    val blockedMeAt = v.blockedMeAtSecs
    if (v.blockedByMe) Box(Modifier.fillMaxWidth().padding(vertical = 8.dp), contentAlignment = Alignment.Center) {
        Text("You blocked $f" + (v.blockedAtSecs?.let { " · ${BlockCopy.date(it)}" } ?: ""),
            Modifier.clip(RoundedCornerShape(50)).background(c.sf2).padding(horizontal = 12.dp, vertical = 4.dp),
            style = TinType.caption.copy(fontWeight = FontWeight.SemiBold), color = c.ink2)
    } else if (blockedMeAt != null) Column(
        Modifier.fillMaxWidth().padding(vertical = 8.dp).clip(SmoothCornerShape(16.dp)).background(c.erc).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        Text(BlockCopy.blockedYouTitle(name), style = TinType.bodyL.copy(fontWeight = FontWeight.Bold), color = c.onErc)
        Text(BlockCopy.blockedYouBody(name), style = TinType.bodyM, color = c.onErc)
        Text("Signed by $f · ${BlockCopy.date(blockedMeAt)}", style = TinType.caption, color = c.onErc.copy(alpha = .8f))
    }
}

/** In place of the composer while a block is on, or for a kept chat with someone no longer a contact. */
@Composable
private fun BlockedFooter(name: String, state: ComposerState, onUnblock: () -> Unit, onAddContact: () -> Unit) {
    val c = Tin.c
    Box(Modifier.fillMaxWidth().height(1.dp).background(c.ln))
    Row(
        Modifier.fillMaxWidth().padding(start = 20.dp, end = 12.dp, top = 12.dp, bottom = 16.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text(
            when (state) {
                ComposerState.BlockedByMe -> BlockCopy.blockedByMeLine(name)
                ComposerState.BlockedMe -> BlockCopy.cantSend(name)
                else -> BlockCopy.notContact(name)
            },
            Modifier.weight(1f), style = TinType.bodyM, color = c.ink2,
        )
        when (state) {
            ComposerState.BlockedByMe -> TinButton("Unblock", onUnblock, fill = false, height = 44.dp)
            ComposerState.NotContact -> TinButton("Add", onAddContact, style = BtnStyle.Tonal, fill = false, height = 44.dp)
            else -> {}
        }
    }
}
