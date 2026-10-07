package com.osvauld.p2p

import android.Manifest
import android.annotation.SuppressLint
import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioRecord
import android.media.AudioTrack
import android.media.MediaRecorder
import android.util.Log
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.waitForUpOrCancellation
import androidx.compose.runtime.collectAsState
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.io.File
import kotlin.math.abs
import kotlin.math.roundToInt
import kotlin.math.sqrt
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.Send
import androidx.compose.material.icons.rounded.Delete
import androidx.compose.material.icons.rounded.KeyboardArrowUp
import androidx.compose.material.icons.rounded.Lock
import androidx.compose.material.icons.rounded.Mic
import androidx.compose.material.icons.rounded.Pause
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.automirrored.rounded.KeyboardArrowLeft
import uniffi.p2pcore.VoiceDecoder
import uniffi.p2pcore.VoiceRecorder

private const val TAG = "Voice"
private const val RATE = 16_000
const val VOICE_MIN_MS = 500
const val VOICE_MAX_MS = 15 * 60 * 1000

/** A finished recording: the Ogg Opus file plus the metadata the chat message carries. */
class RecordedVoice(val path: String, val durationMs: Int, val waveform: ByteArray)

// ---------------------------------------------------------------------------------------------
// Recording
// ---------------------------------------------------------------------------------------------

/** One AudioRecord (16 kHz mono, VOICE_COMMUNICATION: echo/noise processing on) feeding the core recorder. */
class VoiceCapture private constructor(private val rec: AudioRecord, private val core: VoiceRecorder, private val file: File) {
    @Volatile private var running = true
    private val thread = Thread(::loop, "voice-capture")
    @Volatile var onLimit: (() -> Unit)? = null

    val elapsedMs: Int get() = core.elapsedMs().toInt()

    private fun loop() {
        val buf = ShortArray(RATE / 50)
        try {
            rec.startRecording()
            while (running) {
                val n = rec.read(buf, 0, buf.size)
                if (n <= 0) { if (n < 0) { Log.w(TAG, "read error $n"); break }; continue }
                core.push((if (n == buf.size) buf.copyOf() else buf.copyOf(n)).asList())
                if (core.elapsedMs().toInt() >= VOICE_MAX_MS) { running = false; onLimit?.invoke() }
            }
        } catch (e: Exception) { Log.w(TAG, "capture: $e") }
    }

    private fun halt() {
        running = false
        thread.join(1000)
        try { rec.stop() } catch (_: Exception) {}
        rec.release()
    }

    /** Stops and returns the file, or null (file deleted) if it is shorter than [VOICE_MIN_MS]. */
    fun finish(): RecordedVoice? {
        halt()
        return try {
            val info = core.finish()
            if (info.durationMs.toInt() < VOICE_MIN_MS) { file.delete(); null }
            else RecordedVoice(file.absolutePath, info.durationMs.toInt(), info.waveform)
        } catch (e: Exception) { Log.w(TAG, "finish: $e"); file.delete(); null }
    }

    fun cancel() {
        halt()
        core.cancel()
    }

    companion object {
        /** Null if the mic cannot be opened (permission missing, or a call holds it). */
        @SuppressLint("MissingPermission")
        fun start(ctx: Context): VoiceCapture? {
            val min = AudioRecord.getMinBufferSize(RATE, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT)
            val rec = try {
                AudioRecord(MediaRecorder.AudioSource.VOICE_COMMUNICATION, RATE, AudioFormat.CHANNEL_IN_MONO,
                    AudioFormat.ENCODING_PCM_16BIT, maxOf(min, RATE / 5 * 2))
            } catch (e: Exception) { Log.w(TAG, "AudioRecord: $e"); return null }
            if (rec.state != AudioRecord.STATE_INITIALIZED) { rec.release(); return null }
            val dir = File(ctx.cacheDir, "voice").also { it.mkdirs() }
            val file = File(dir, "rec-${System.currentTimeMillis()}.opus")
            val core = try { VoiceRecorder.start(file.absolutePath) } catch (e: Exception) { rec.release(); Log.w(TAG, "core: $e"); return null }
            return VoiceCapture(rec, core, file).also { it.thread.start() }
        }
    }
}

/**
 * Hold-to-record state for one composer. [onSend] gets the finished file. Drive it with [VoiceMicButton]
 * and show [VoiceRecordingBar] in place of the text field while [recording].
 */
@Stable
class VoiceRecState(private val ctx: Context, private val onSend: (RecordedVoice) -> Unit, private val canRecord: () -> Boolean) {
    var recording by mutableStateOf(false); private set
    var locked by mutableStateOf(false); internal set
    /** 0..1 how far the finger has slid left; at 1 releasing cancels. */
    var cancelPull by mutableFloatStateOf(0f); internal set
    var elapsedMs by mutableIntStateOf(0); internal set
    private var capture: VoiceCapture? = null

    /** False if the mic is busy (in a call) or could not be opened. */
    fun begin(): Boolean {
        if (recording || !canRecord()) return false
        val c = VoiceCapture.start(ctx) ?: return false
        c.onLimit = { android.os.Handler(android.os.Looper.getMainLooper()).post { send() } }
        capture = c; recording = true; locked = false; cancelPull = 0f; elapsedMs = 0
        return true
    }

    fun send() {
        val c = capture ?: return
        reset()
        c.finish()?.let(onSend)
    }

    fun cancel() {
        val c = capture ?: return
        reset()
        c.cancel()
    }

    internal fun tick() { elapsedMs = capture?.elapsedMs ?: 0 }

    private fun reset() { capture = null; recording = false; locked = false; cancelPull = 0f }
}

@Composable
fun rememberVoiceRecState(onSend: (RecordedVoice) -> Unit): VoiceRecState {
    val ctx = LocalContext.current
    val app = remember { P2pApp.get(ctx) }
    val state = remember { VoiceRecState(ctx, onSend) { app.calls.ui.value == null } }
    androidx.compose.runtime.DisposableEffect(state) { onDispose { state.cancel() } }
    LaunchedEffect(state.recording) { while (state.recording) { state.tick(); delay(100) } }
    return state
}

fun formatMs(ms: Int): String {
    val s = (ms + 500) / 1000
    return "%d:%02d".format(s / 60, s % 60)
}

/** The mic button. Hold to record, release to send, slide left to cancel, up to lock (then it becomes Send). */
@Composable
fun VoiceMicButton(state: VoiceRecState, modifier: Modifier = Modifier) {
    val c = Tin.c
    val ctx = LocalContext.current
    val haptic = LocalHapticFeedback.current
    val density = LocalDensity.current
    val cancelPx = with(density) { 96.dp.toPx() }
    val lockPx = with(density) { 64.dp.toPx() }
    var askedPermission by remember { mutableStateOf(false) }
    val permission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { askedPermission = false }
    val size = if (state.recording && !state.locked) 64.dp else 48.dp
    Box(modifier.size(48.dp), contentAlignment = Alignment.Center) {
        if (state.recording && !state.locked) {
            // The lock pill the finger slides up into.
            Column(
                Modifier.offset(y = (-120).dp).size(40.dp, 96.dp).clip(RoundedCornerShape(50)).background(c.sf)
                    .border(1.dp, c.ln, RoundedCornerShape(50)).padding(vertical = 12.dp),
                horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.SpaceBetween,
            ) {
                Icon(Icons.Rounded.Lock, null, tint = c.ink2, modifier = Modifier.size(20.dp))
                Icon(Icons.Rounded.KeyboardArrowUp, null, tint = c.ink2, modifier = Modifier.size(18.dp))
            }
        }
        Box(
            Modifier.size(size).then(if (state.recording && !state.locked) Modifier.background(c.pr.copy(alpha = 0.18f), CircleShape) else Modifier)
                .clip(CircleShape).background(c.pr)
                .semantics { contentDescription = if (state.locked) "Send voice message" else if (state.recording) "Recording, release to send" else "Hold to record a voice message" }
                .pointerInput(state) {
                    awaitEachGesture {
                        val down = awaitFirstDown()
                        if (state.locked) {
                            // Locked: a tap on the button sends.
                            val up = waitForUpOrCancellation()
                            if (up != null) state.send()
                            return@awaitEachGesture
                        }
                        if (!Perms.granted(ctx, Manifest.permission.RECORD_AUDIO)) {
                            if (!askedPermission) { askedPermission = true; permission.launch(Manifest.permission.RECORD_AUDIO) }
                            waitForUpOrCancellation()
                            return@awaitEachGesture
                        }
                        if (!state.begin()) { haptic.performHapticFeedback(HapticFeedbackType.Reject); waitForUpOrCancellation(); return@awaitEachGesture }
                        haptic.performHapticFeedback(HapticFeedbackType.LongPress)
                        var cancelled = false
                        while (true) {
                            val ev = awaitPointerEvent()
                            val p = ev.changes.firstOrNull { it.id == down.id } ?: break
                            if (!p.pressed) break
                            val d = p.position - down.position
                            state.cancelPull = (-d.x / cancelPx).coerceIn(0f, 1f)
                            if (-d.x >= cancelPx) { cancelled = true; break }
                            if (-d.y >= lockPx && abs(d.y) > abs(d.x)) { state.locked = true; state.cancelPull = 0f; break }
                            p.consume()
                        }
                        when {
                            cancelled -> { haptic.performHapticFeedback(HapticFeedbackType.LongPress); state.cancel() }
                            state.locked -> haptic.performHapticFeedback(HapticFeedbackType.ContextClick)
                            else -> state.send()
                        }
                        // A cancelled or locked gesture may still have the finger down: swallow the rest of it.
                        if (cancelled || state.locked) while (true) { val e = awaitPointerEvent(); if (e.changes.none { it.pressed }) break }
                    }
                },
            contentAlignment = Alignment.Center,
        ) {
            Icon(if (state.locked) Icons.AutoMirrored.Rounded.Send else Icons.Rounded.Mic, null, tint = c.onPr, modifier = Modifier.size(if (size == 64.dp) 28.dp else 24.dp))
        }
    }
}

/** Replaces the text field while recording: red dot, timer, "Slide to cancel" (or a trash button once locked). */
@Composable
fun VoiceRecordingBar(state: VoiceRecState, modifier: Modifier = Modifier) {
    val c = Tin.c
    Row(
        modifier.heightIn(min = 48.dp).clip(RoundedCornerShape(24.dp)).background(c.sf).border(1.dp, c.ln, RoundedCornerShape(24.dp))
            .padding(start = 16.dp, end = 6.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Box(Modifier.size(10.dp).background(c.er, CircleShape))
        Text(formatMs(state.elapsedMs), color = c.ink, style = TinType.mono.copy(fontSize = 15.sp))
        if (state.locked) {
            Box(Modifier.weight(1f))
            Box(
                Modifier.size(40.dp).clip(CircleShape).semantics { contentDescription = "Discard recording" }
                    .pointerInput(state) { detectTapGestures { state.cancel() } },
                contentAlignment = Alignment.Center,
            ) { Icon(Icons.Rounded.Delete, null, tint = c.er) }
        } else {
            Row(
                Modifier.weight(1f).offset(x = (-state.cancelPull * 48).dp), horizontalArrangement = Arrangement.Center,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(Icons.AutoMirrored.Rounded.KeyboardArrowLeft, null, tint = c.ink2, modifier = Modifier.size(16.dp))
                Text("Slide to cancel", color = c.ink2, fontSize = 14.sp)
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Playback
// ---------------------------------------------------------------------------------------------

data class PlayState(val id: String, val playing: Boolean, val positionMs: Int, val durationMs: Int)

/**
 * One message plays at a time. A call starting, or any other app taking audio focus, pauses it.
 * Not done: routing to the earpiece when the phone is held to the ear (proximity sensor).
 */
object VoicePlayer {
    private val _state = MutableStateFlow<PlayState?>(null)
    val state: StateFlow<PlayState?> = _state

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
    private var app: P2pApp? = null
    private var audio: AudioManager? = null
    private var focus: AudioFocusRequest? = null
    private var decoder: VoiceDecoder? = null
    private var id: String? = null
    private var thread: Thread? = null
    @Volatile private var gen = 0

    private fun attach(ctx: Context) {
        if (app != null) return
        val a = P2pApp.get(ctx).also { app = it }
        audio = ctx.applicationContext.getSystemService(Context.AUDIO_SERVICE) as AudioManager
        scope.launch { a.calls.ui.collect { if (it != null) pause() } }
    }

    /** Starts [id]'s file at [fromMs] (or continues if it is the current message and [fromMs] is null). */
    fun play(ctx: Context, id: String, path: String, fromMs: Int? = null) {
        attach(ctx)
        if (this.id != id) {
            stopThread()
            decoder = try { VoiceDecoder.open(path) } catch (e: Exception) { Log.w(TAG, "open: $e"); return }
            this.id = id
        }
        val d = decoder ?: return
        if (fromMs != null) { stopThread(); d.seek(fromMs.toUInt()) }
        else if (thread != null) return
        if (d.positionMs() >= d.durationMs()) d.seek(0u)
        if (!requestFocus()) return
        start(d, id)
    }

    fun pause() {
        stopThread()
        abandonFocus()
        val d = decoder ?: return
        _state.value = id?.let { PlayState(it, false, d.positionMs().toInt(), d.durationMs().toInt()) }
    }

    fun stop() {
        stopThread(); abandonFocus()
        decoder = null; id = null; _state.value = null
    }

    private fun requestFocus(): Boolean {
        val am = audio ?: return true
        val req = AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
            .setAudioAttributes(attrs())
            .setOnAudioFocusChangeListener { if (it == AudioManager.AUDIOFOCUS_LOSS || it == AudioManager.AUDIOFOCUS_LOSS_TRANSIENT) scope.launch { pause() } }
            .build()
        focus = req
        return am.requestAudioFocus(req) == AudioManager.AUDIOFOCUS_REQUEST_GRANTED
    }

    private fun abandonFocus() { focus?.let { audio?.abandonAudioFocusRequest(it) }; focus = null }

    private fun attrs() = AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA).setContentType(AudioAttributes.CONTENT_TYPE_SPEECH).build()

    private fun stopThread() {
        gen++
        thread?.let { if (it !== Thread.currentThread()) it.join(1000) }
        thread = null
    }

    private fun start(d: VoiceDecoder, id: String) {
        val my = ++gen
        val dur = d.durationMs().toInt()
        _state.value = PlayState(id, true, d.positionMs().toInt(), dur)
        thread = Thread({
            val min = AudioTrack.getMinBufferSize(RATE, AudioFormat.CHANNEL_OUT_MONO, AudioFormat.ENCODING_PCM_16BIT)
            val track = AudioTrack.Builder()
                .setAudioAttributes(attrs())
                .setAudioFormat(AudioFormat.Builder().setSampleRate(RATE).setChannelMask(AudioFormat.CHANNEL_OUT_MONO).setEncoding(AudioFormat.ENCODING_PCM_16BIT).build())
                .setBufferSizeInBytes(maxOf(min, RATE / 5 * 2)).setTransferMode(AudioTrack.MODE_STREAM).build()
            var finished = false
            try {
                track.play()
                var tick = 0
                while (gen == my) {
                    val pcm = d.read(RATE / 20)
                    if (pcm.isEmpty()) { finished = true; break }
                    val arr = ShortArray(pcm.size) { pcm[it] }
                    track.write(arr, 0, arr.size)
                    if (++tick % 2 == 0) _state.value = PlayState(id, true, d.positionMs().toInt(), dur)
                }
                if (finished) track.stop()
            } catch (e: Exception) { Log.w(TAG, "playback: $e") }
            finally { track.release() }
            if (finished && gen == my) scope.launch {
                abandonFocus()
                d.seek(0u)
                _state.value = PlayState(id, false, 0, dur)
                thread = null
            }
        }, "voice-play").also { it.start() }
    }
}

// ---------------------------------------------------------------------------------------------
// Bubble
// ---------------------------------------------------------------------------------------------

/** Peaks resampled to [n] bars (max of each slice), 0..1. */
private fun barsFor(wave: ByteArray, n: Int): FloatArray = FloatArray(n) { i ->
    if (wave.isEmpty()) return@FloatArray 0f
    val lo = i * wave.size / n
    val hi = maxOf(lo + 1, (i + 1) * wave.size / n).coerceAtMost(wave.size)
    var m = 0
    for (k in lo until hi) m = maxOf(m, wave[k].toInt() and 0xFF)
    m / 255f
}

/**
 * The inside of a voice message bubble (the chat supplies the bubble itself): play/pause, waveform
 * that fills with progress, and the time. Tap the waveform to seek. [id] identifies the message so
 * only one plays at a time.
 */
@Composable
fun VoiceBubbleContent(id: String, path: String, durationMs: Int, waveform: ByteArray, outgoing: Boolean, modifier: Modifier = Modifier) {
    val c = Tin.c
    val ctx = LocalContext.current
    val st by VoicePlayer.state.collectAsState()
    val mine = st?.takeIf { it.id == id }
    val playing = mine?.playing == true
    val progress = if (mine != null && durationMs > 0) (mine.positionMs.toFloat() / durationMs).coerceIn(0f, 1f) else 0f
    val idle = if (outgoing) c.onPrc.copy(alpha = 0.35f) else c.ln2
    val shown = if (mine != null && (playing || mine.positionMs > 0)) mine.positionMs else durationMs
    Row(modifier.widthIn(min = 230.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        Box(
            Modifier.size(40.dp).clip(CircleShape).background(c.pr)
                .semantics { contentDescription = if (playing) "Pause voice message" else "Play voice message" }
                .pointerInput(id, path) { detectTapGestures { if (playing) VoicePlayer.pause() else VoicePlayer.play(ctx, id, path) } },
            contentAlignment = Alignment.Center,
        ) { Icon(if (playing) Icons.Rounded.Pause else Icons.Rounded.PlayArrow, null, tint = c.onPr, modifier = Modifier.size(22.dp)) }
        val barW = with(LocalDensity.current) { 3.dp.toPx() }
        val gap = with(LocalDensity.current) { 2.dp.toPx() }
        Canvas(
            Modifier.weight(1f).height(28.dp)
                .semantics { contentDescription = "Voice message waveform, tap to seek" }
                .pointerInput(id, path, durationMs) { detectTapGestures { o -> VoicePlayer.play(ctx, id, path, (o.x / size.width * durationMs).roundToInt().coerceIn(0, durationMs)) } },
        ) {
            val n = maxOf(8, ((size.width + gap) / (barW + gap)).toInt())
            val bars = barsFor(waveform, n)
            val min = 4.dp.toPx()
            val used = n * (barW + gap) - gap
            val x0 = (size.width - used) / 2
            for (i in 0 until n) {
                val h = min + (size.height - min) * sqrt(bars[i])
                val x = x0 + i * (barW + gap)
                drawRoundRect(if ((i + 0.5f) / n <= progress) c.pr else idle, Offset(x, (size.height - h) / 2), Size(barW, h), CornerRadius(barW / 2))
            }
        }
        Text(formatMs(shown), fontFamily = PlexMono, fontSize = 12.sp, color = if (outgoing) c.onPrc else c.ink2)
    }
}
