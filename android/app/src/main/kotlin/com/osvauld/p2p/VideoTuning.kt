package com.osvauld.p2p

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.p2pcore.VideoTuning
import uniffi.p2pcore.videoTuningStatsLabel

/** Session-only controls, only in builds with [BuildConfig.VIDEO_TUNING]. Rust owns validation/policy. */
@Composable
internal fun VideoTuningPanel(app: P2pApp) {
    if (!BuildConfig.VIDEO_TUNING) return
    val node = app.node
    var tuning by remember(node) { mutableStateOf<VideoTuning?>(null) }
    var codecs by remember(node) { mutableStateOf(emptyList<String>()) }
    var error by remember(node) { mutableStateOf<String?>(null) }
    var busy by remember(node) { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    LaunchedEffect(node) {
        runCatching { withContext(Dispatchers.IO) { node.videoTuning() to node.videoTuningCodecs() } }
            .onSuccess { tuning = it.first; codecs = it.second }
            .onFailure { error = it.message }
    }
    fun apply(next: VideoTuning) {
        if (busy) return
        busy = true
        scope.launch {
            val result = withContext(Dispatchers.IO) { runCatching { node.setVideoTuning(next) } }
            result.onSuccess { tuning = next; error = null }.onFailure { error = it.message }
            busy = false
        }
    }
    SectionLabel("VIDEO TUNING · DEBUG ONLY")
    Hint("Session only. Changes apply to the next call; the call screen’s About this call changes the current one. Fixed size disables rate/CPU resolution adaptation; peer and thermal caps still win. Enable HD capture for larger sizes. Compare actual payload rates and dimensions.", Modifier.padding(horizontal = 20.dp))
    tuning?.let { t ->
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Hint("Send codec")
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                codecs.forEach { codec ->
                    val selected = t.sendCodec == codec || (t.sendCodec.isEmpty() && codec == "auto")
                    TinButton((if (selected) "✓ " else "") + codec.uppercase(), { apply(t.copy(sendCodec = codec)) }, fill = false, height = 36.dp)
                }
            }
            Hint("Bitrate cap (not a guaranteed rate)")
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                listOf(0u to "Auto", 500_000u to "0.5M", 1_000_000u to "1M").forEach { (rate, label) ->
                    TinButton((if (t.maxBps == rate) "✓ " else "") + label, { apply(t.copy(maxBps = rate)) }, fill = false, height = 36.dp)
                }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                listOf(2_000_000u to "2M", 3_000_000u to "3M").forEach { (rate, label) ->
                    TinButton((if (t.maxBps == rate) "✓ " else "") + label, { apply(t.copy(maxBps = rate)) }, fill = false, height = 36.dp)
                }
            }
            Hint("Fixed rung (camera aspect retained; never upscaled)")
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                listOf(0u to "Auto", 360u to "360p").forEach { (height, label) ->
                    TinButton((if (t.fixedHeight == height) "✓ " else "") + label, { apply(t.copy(fixedHeight = height)) }, fill = false, height = 36.dp)
                }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                listOf(540u to "540p", 720u to "720p").forEach { (height, label) ->
                    TinButton((if (t.fixedHeight == height) "✓ " else "") + label, { apply(t.copy(fixedHeight = height)) }, fill = false, height = 36.dp)
                }
            }
            Hint("Decoder for their video (where this phone has both kinds; AV1 is software only)")
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                listOf("auto" to "Auto", "hw" to "Hardware", "sw" to "Software").forEach { (id, label) ->
                    val selected = t.recvDecoder == id || (t.recvDecoder.isEmpty() && id == "auto")
                    TinButton((if (selected) "✓ " else "") + label, { apply(t.copy(recvDecoder = id)) }, fill = false, height = 36.dp)
                }
            }
            Hint("During a call, About this call (the signal icon) has the same controls for that call.")
            if (busy) Hint("Applying…")
            error?.let { Hint(it) }
        }
    }
    DebugVideoStats(app)
}

/** Visible in calls of builds with video tuning, without turning a decoder's pipeline latency into a CPU claim. */
@Composable
internal fun DebugVideoStats(app: P2pApp, modifier: Modifier = Modifier) {
    if (!BuildConfig.VIDEO_TUNING) return
    val node = app.node
    var line by remember(node) { mutableStateOf("") }
    LaunchedEffect(node) {
        while (true) {
            line = withContext(Dispatchers.IO) {
                runCatching { node.videoStats()?.let(::videoTuningStatsLabel).orEmpty() }.getOrDefault("")
            }
            delay(1000)
        }
    }
    if (line.isNotEmpty()) Text(line, modifier.background(Color.Black.copy(alpha = 0.75f)).padding(8.dp), color = Color.White, fontSize = 10.sp)
}
