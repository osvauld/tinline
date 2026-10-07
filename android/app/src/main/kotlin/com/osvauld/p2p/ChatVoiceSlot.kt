package com.osvauld.p2p

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.*
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import uniffi.p2pcore.Message
import uniffi.p2pcore.TransferState

// lead: replace with VoiceMessage.kt. Everything in this file is a stand-in for the recorder and the
// player that VoiceMessage.kt provides; Conversation.kt calls only the two composables below.

/**
 * Placeholder for the hold-to-record mic. The real one records on press, slides left to cancel, and calls
 * [onSend] with the Ogg Opus file, its length and about 64 waveform peaks (see `Node.send_voice`).
 */
@Composable
fun VoiceMicSlot(onSend: (path: String, durationMs: Int, waveform: ByteArray) -> Unit, modifier: Modifier = Modifier) {
    val c = Tin.c
    Box(
        modifier.size(48.dp).clip(CircleShape).background(c.pr).clickable(role = Role.Button) { /* lead: hold to record */ },
        contentAlignment = Alignment.Center,
    ) { Icon(Icons.Rounded.Mic, "Hold to record a voice message", tint = c.onPr) }
}

/**
 * Placeholder voice bubble body: play button, a static waveform from the attachment, and the length. The
 * real one plays the decrypted file (`Node.save_attachment`) with a moving head. [ready] is false while
 * the bytes are not on this phone; [onDownload] then fetches them.
 */
@Composable
fun VoiceBubbleSlot(message: Message, ready: Boolean, onDownload: () -> Unit) {
    val c = Tin.c
    val a = message.attachment ?: return
    val mine = message.outgoing
    val peaks = a.waveform.map { it.toInt() }
    val bars = 34
    // About 64 peaks in the data; the board draws 34 bars, so average neighbours.
    val heights = List(bars) { i ->
        if (peaks.isEmpty()) 8 else {
            val from = i * peaks.size / bars
            val to = ((i + 1) * peaks.size / bars).coerceAtLeast(from + 1).coerceAtMost(peaks.size)
            (4 + peaks.subList(from, to).average() / 255.0 * 24).toInt()
        }
    }
    Row(Modifier.widthIn(min = 230.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        val remote = !ready && a.state != TransferState.DOWNLOADING
        Box(
            Modifier.size(40.dp).clip(CircleShape).background(c.pr).clickable(role = Role.Button) { if (remote) onDownload() },
            contentAlignment = Alignment.Center,
        ) { Icon(if (remote) Icons.Rounded.Download else Icons.Rounded.PlayArrow, if (remote) "Download voice message" else "Play voice message", tint = c.onPr) }
        Row(Modifier.weight(1f).height(28.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(2.dp)) {
            heights.forEachIndexed { i, h ->
                val played = mine && i < 12
                Box(Modifier.width(3.dp).height(h.dp).clip(RoundedCornerShape(2.dp)).background(
                    if (mine) (if (played) c.pr else c.onPrc.copy(alpha = .35f)) else c.ln2))
            }
        }
        Text(clock(a.durationMs.toInt()), style = TinType.caption.copy(fontFamily = PlexMono, fontSize = 12.sp), color = if (mine) c.onPrc else c.ink2)
    }
}

/** 14000 -> "0:14". */
fun clock(ms: Int): String = "${ms / 60000}:${(ms / 1000 % 60).toString().padStart(2, '0')}"
