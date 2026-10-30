package com.osvauld.p2p

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.systemBarsPadding
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/** Pinch and double-tap zoom for the photo viewer: 1x (fit) up to [MAX]x, never panned past the picture's edge. */
object PhotoZoom {
    const val MAX = 5f
    const val DOUBLE_TAP = 2.5f

    /** Largest pan either way at [scale] in a [w] x [h] box (the picture fills the box at 1x). */
    fun clamp(pan: Offset, scale: Float, w: Float, h: Float): Offset {
        val mx = (scale - 1f) * w / 2f
        val my = (scale - 1f) * h / 2f
        return Offset(pan.x.coerceIn(-mx, mx), pan.y.coerceIn(-my, my))
    }

    /** Zooms from [scale] to [next] keeping the point under [focus] (relative to the box's centre) where it is. */
    fun zoomAt(pan: Offset, scale: Float, next: Float, focus: Offset): Offset = focus - (focus - pan) * (next / scale)
}

/**
 * Full-screen photo, always dark: who sent it and when, Share and Save to phone, the picture (pinch or double-tap to zoom,
 * drag to look around), and the note that it stays encrypted in Tinline until saved. Shared by 1:1 chats and groups;
 * [load] reads the picture (an IO thread) and [key] says which one it is.
 */
@Composable
fun PhotoViewer(key: Any, who: String, atMs: Long, alt: String?, load: () -> ImageBitmap?, onClose: () -> Unit, onSave: () -> Unit, onShare: () -> Unit) {
    BackHandler(onBack = onClose)
    val bmp by produceState<ImageBitmap?>(null, key) {
        value = withContext(Dispatchers.IO) { runCatching { load() }.getOrNull() }
    }
    var scale by remember(key) { mutableFloatStateOf(1f) }
    var pan by remember(key) { mutableStateOf(Offset.Zero) }
    Column(Modifier.fillMaxSize().background(Color.Black).systemBarsPadding()) {
        Row(Modifier.fillMaxWidth().height(64.dp).padding(horizontal = 4.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            IconBtn(PgIcons.X, stringResource(R.string.common_close), onClose, tint = Color.White)
            Column(Modifier.weight(1f)) {
                Text(who, style = TinType.bodyL.copy(fontWeight = FontWeight.SemiBold), color = Color.White, maxLines = 1)
                Text(viewerWhen(atMs, System.currentTimeMillis()), style = TinType.caption, color = Color(0xFFA6B2AD))
            }
            IconBtn(PgIcons.Share, stringResource(R.string.common_share), onShare, tint = Color.White)
            IconBtn(PgIcons.Save, stringResource(R.string.conv_save_to_phone), onSave, tint = Color.White)
        }
        BoxWithConstraints(Modifier.weight(1f).fillMaxWidth().clipToBounds(), contentAlignment = Alignment.Center) {
            val w = constraints.maxWidth.toFloat()
            val h = constraints.maxHeight.toFloat()
            val centre = Offset(w / 2f, h / 2f)
            Image(
                bmp ?: return@BoxWithConstraints, alt,
                Modifier.fillMaxSize()
                    .pointerInput(key, w, h) {
                        detectTapGestures(onDoubleTap = { p ->
                            if (scale > 1f) { scale = 1f; pan = Offset.Zero }
                            else { pan = PhotoZoom.clamp(PhotoZoom.zoomAt(pan, 1f, PhotoZoom.DOUBLE_TAP, p - centre), PhotoZoom.DOUBLE_TAP, w, h); scale = PhotoZoom.DOUBLE_TAP }
                        })
                    }
                    .pointerInput(key, w, h) {
                        detectTransformGestures { centroid, move, zoom, _ ->
                            val next = (scale * zoom).coerceIn(1f, PhotoZoom.MAX)
                            pan = PhotoZoom.clamp(PhotoZoom.zoomAt(pan, scale, next, centroid - centre) + move, next, w, h)
                            scale = next
                        }
                    }
                    .graphicsLayer { scaleX = scale; scaleY = scale; translationX = pan.x; translationY = pan.y },
                contentScale = ContentScale.Fit,
            )
        }
        Row(Modifier.padding(start = 20.dp, end = 20.dp, top = 16.dp, bottom = 24.dp), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.Top) {
            Icon(PgIcons.Lock, null, tint = Color(0xFFA6B2AD), modifier = Modifier.size(18.dp))
            Text(stringResource(R.string.conv_viewer_note), style = TinType.bodyM.copy(fontSize = 13.sp, lineHeight = 18.sp), color = Color(0xFFA6B2AD))
        }
    }
}
