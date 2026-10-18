package com.osvauld.p2p

import androidx.compose.foundation.layout.*
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.p2pcore.VideoChoice
import uniffi.p2pcore.VideoLive
import uniffi.p2pcore.VideoStatRow
import uniffi.p2pcore.videoStatRows

/**
 * "Stats for nerds" inside About this call (opened from the call screen's signal icon), only in
 * builds with [BuildConfig.VIDEO_TUNING]. The rows and their wording come from Rust
 * (`video_stat_rows`: always the same 11 rows, so nothing moves while they refresh once a second);
 * the controls apply to this call at once (`Node.setVideoLive`), Settings > Diagnostics stays the
 * next-call default. Nothing here turns the camera on.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun CallNerds(app: P2pApp, rttMs: Int?, lossPct: Double?) {
    if (!BuildConfig.VIDEO_TUNING) return
    val c = Tin.c
    val node = app.node
    // 11 blank rows until the first read: the sheet has its full height from the start.
    var rows by remember(node) { mutableStateOf(List(11) { VideoStatRow("", "", " ") }) }
    // First values read right away (small Rust reads), so the controls are there when the sheet opens.
    var live by remember(node) { mutableStateOf(runCatching { node.videoLive() }.getOrNull()) }
    var codecs by remember(node) { mutableStateOf(runCatching { node.videoLiveCodecs() }.getOrDefault(emptyList())) }
    var decoders by remember(node) { mutableStateOf(runCatching { node.videoLiveDecoders() }.getOrDefault(emptyList())) }
    var note by remember(node) { mutableStateOf<String?>(null) }
    var busy by remember(node) { mutableStateOf(false) }
    val rtt by rememberUpdatedState(rttMs)
    val loss by rememberUpdatedState(lossPct)
    val scope = rememberCoroutineScope()
    LaunchedEffect(node) {
        while (true) {
            withContext(Dispatchers.IO) {
                runCatching {
                    node.videoStats()?.let { rows = videoStatRows(it, (rtt ?: 0).toUInt(), loss ?: 0.0) }
                    if (!busy) live = node.videoLive()
                    codecs = node.videoLiveCodecs()
                    decoders = node.videoLiveDecoders()
                }
            }
            delay(1000)
        }
    }
    fun apply(next: VideoLive) {
        if (busy) return
        busy = true
        live = next
        scope.launch {
            val r = withContext(Dispatchers.IO) { runCatching { node.setVideoLive(next) } }
            r.onSuccess { note = null }.onFailure { note = it.message ?: "That didn’t apply" }
            live = withContext(Dispatchers.IO) { runCatching { node.videoLive() }.getOrNull() } ?: live
            busy = false
        }
    }
    Column(Modifier.padding(top = 14.dp)) {
        Text("Stats for nerds", style = TinFs.section, color = c.ink2)
        // The rows: label column, then a two-line value (reserved even when short).
        rows.forEachIndexed { i, r ->
            // A gap before "Their video" and the connection (rows 6 and 9), by position so blank rows keep it too.
            if (i == 6 || i == 9) Spacer(Modifier.height(6.dp))
            Row(Modifier.fillMaxWidth().padding(top = 2.dp), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                Text(r.label, Modifier.width(76.dp), style = TinFs.meta, color = c.ink2, maxLines = 1)
                Text(r.value, Modifier.weight(1f), style = TinFs.meta, color = c.ink, minLines = 2, maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
        }
        val l = live ?: return@Column
        Text("These apply to this call now. Settings > Diagnostics sets the next call.", Modifier.padding(top = 12.dp), style = TinFs.meta, color = c.ink2)
        Choices("Your video codec", codecs.map { Triple(it.id, it.label, it) }, l.sendCodec.ifEmpty { "auto" }, reserve = 2,
            onPick = { ch -> if (ch.available) apply(l.copy(sendCodec = ch.id)) else note = ch.why })
        Choices("Your size", listOf(0u to "Auto", 360u to "360p", 540u to "540p", 720u to "720p").map { (h, t) -> Triple(h.toString(), t, h) },
            l.fixedHeight.toString(), onPick = { h -> apply(l.copy(fixedHeight = h, ignoreLimits = l.ignoreLimits && h != 0u)) })
        Choices("Your frame rate", listOf(0u to "Auto", 15u to "15", 24u to "24", 30u to "30").map { (f, t) -> Triple(f.toString(), t, f) },
            l.fixedFps.toString(), onPick = { f -> apply(l.copy(fixedFps = f)) })
        Choices("Your bitrate cap (not a promised rate)", listOf(0u to "Auto", 500_000u to "0.5M", 1_000_000u to "1M", 2_000_000u to "2M", 3_000_000u to "3M")
            .map { (b, t) -> Triple(b.toString(), t, b) }, l.maxBps.toString(), onPick = { b -> apply(l.copy(maxBps = b)) })
        // Testing only: hold the pin against heat and their decoder. Off at the end of the call, never saved.
        Row(Modifier.fillMaxWidth().padding(top = 8.dp).alpha(if (l.fixedHeight != 0u) 1f else 0.5f), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text("Ignore limits (heat, their decoder)", style = TinFs.sub.copy(fontWeight = FontWeight.SemiBold), color = c.ink)
                Text(if (l.fixedHeight != 0u) "For testing. Your picture may stutter or your phone may heat up." else "Pin a size first.",
                    style = TinFs.meta, color = c.ink2, minLines = 1, maxLines = 1)
            }
            PgSwitch(l.ignoreLimits, { on -> apply(l.copy(ignoreLimits = on)) }, "Ignore limits (heat, their decoder)", enabled = l.fixedHeight != 0u)
        }
        val decoderNote = decoders.singleOrNull()?.why.orEmpty()
        Choices("Decoder for their video", decoders.map { Triple(it.id, it.label, it) }, l.recvDecoder.ifEmpty { "auto" }, reserve = 2,
            onPick = { ch -> apply(l.copy(recvDecoder = ch.id)) })
        // One reserved line for what didn't apply (or the decoder note): appears without moving anything.
        Text(note ?: decoderNote.ifEmpty { " " }, Modifier.padding(top = 6.dp), style = TinFs.meta, color = c.ink2, minLines = 2, maxLines = 2)
    }
}

/** A labelled row of chips; `reserve` rows of chip height are kept so a longer list never pushes the sheet. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun <T> Choices(label: String, items: List<Triple<String, String, T>>, selected: String, reserve: Int = 1, onPick: (T) -> Unit) {
    Text(label, Modifier.padding(top = 10.dp, bottom = 4.dp), style = TinFs.meta, color = Tin.c.ink2)
    FlowRow(Modifier.fillMaxWidth().heightIn(min = (44 * reserve + 6 * (reserve - 1)).dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        items.forEach { (id, text, v) ->
            val dim = (v as? VideoChoice)?.available == false
            Box(Modifier.alpha(if (dim) 0.5f else 1f)) { FilterChip(text, id == selected) { onPick(v) } }
        }
    }
}
