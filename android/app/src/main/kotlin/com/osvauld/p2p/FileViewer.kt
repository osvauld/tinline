package com.osvauld.p2p

import android.graphics.Bitmap
import android.graphics.Color as AColor
import android.graphics.pdf.PdfRenderer
import android.os.ParcelFileDescriptor
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.*
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import uniffi.p2pcore.Message
import java.io.File

private val Dim = Color(0xFFA6B2AD)

private sealed interface Loaded {
    data object Loading : Loaded
    class Pdf(val doc: PdfDoc) : Loaded
    class Text(val lines: List<String>) : Loaded
    class Error(val msg: String) : Loaded
}

/** A [PdfRenderer] guarded so only one page is open at a time (the renderer allows no more) and never used concurrently. */
class PdfDoc(private val pfd: ParcelFileDescriptor, private val r: PdfRenderer) {
    private val lock = Mutex()
    val pageCount = r.pageCount

    /** Renders [index] at about [widthPx] wide, capped to ~3 MP (12 MB). Null if the page cannot be drawn. */
    suspend fun render(index: Int, widthPx: Int): Bitmap? = withContext(Dispatchers.IO) {
        lock.withLock {
            runCatching {
                r.openPage(index).use { p ->
                    var w = widthPx.coerceIn(200, 1600)
                    var h = (w.toLong() * p.height / p.width.coerceAtLeast(1)).toInt().coerceAtLeast(1)
                    val maxPx = 3_000_000L
                    if (w.toLong() * h > maxPx) {
                        val s = Math.sqrt(maxPx.toDouble() / (w.toLong() * h))
                        w = (w * s).toInt().coerceAtLeast(1); h = (h * s).toInt().coerceAtLeast(1)
                    }
                    Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888).also {
                        it.eraseColor(AColor.WHITE)
                        p.render(it, null, null, PdfRenderer.Page.RENDER_MODE_FOR_DISPLAY)
                    }
                }
            }.getOrNull()
        }
    }

    suspend fun close() = withContext(Dispatchers.IO) { lock.withLock { runCatching { r.close() }; runCatching { pfd.close() } } }

    companion object {
        /** Throws SecurityException for password-protected, IOException for damaged files. */
        fun open(f: File): PdfDoc {
            val pfd = ParcelFileDescriptor.open(f, ParcelFileDescriptor.MODE_READ_ONLY)
            return try { PdfDoc(pfd, PdfRenderer(pfd)) } catch (e: Throwable) { runCatching { pfd.close() }; throw e }
        }
    }
}

/** Full-screen document viewer (PDF or plain text), always dark like the photo viewer. Unreadable files get a friendly message and Save. */
@Composable
fun FileViewer(m: Message, plan: OpenPlan, source: ChatSource, onClose: () -> Unit, onSave: () -> Unit, onShare: () -> Unit) {
    val ctx = LocalContext.current
    BackHandler(onBack = onClose)
    val a = m.attachment
    val state by produceState<Loaded>(Loaded.Loading, m.id) {
        value = withContext(Dispatchers.IO) {
            runCatching {
                val f = ChatMedia.decrypt(ctx, source, m)
                if (plan.kind == OpenKind.PDF) {
                    try { Loaded.Pdf(PdfDoc.open(f)) }
                    catch (e: SecurityException) { Loaded.Error("This PDF is password-protected, so Tinline can’t show it. Save it to open it in another app.") }
                    catch (e: Exception) { Loaded.Error("Tinline can’t read this PDF. It may be damaged. You can still save it.") }
                } else loadText(f)
            }.getOrElse { Loaded.Error("Couldn’t open that file.") }
        }
    }
    DisposableEffect(state) {
        val d = (state as? Loaded.Pdf)?.doc
        onDispose { if (d != null) CoroutineScope(Dispatchers.IO).launch { d.close() } }
    }

    Column(Modifier.fillMaxSize().background(Color.Black).systemBarsPadding()) {
        val s = state
        var page by remember { mutableIntStateOf(1) }
        Row(Modifier.fillMaxWidth().height(64.dp).padding(horizontal = 4.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            IconBtn(Icons.Rounded.Close, "Close", onClose, tint = Color.White)
            Column(Modifier.weight(1f)) {
                Text(a?.name ?: "File", style = TinType.bodyL.copy(fontWeight = FontWeight.SemiBold), color = Color.White, maxLines = 1, overflow = TextOverflow.Ellipsis)
                val sub = if (s is Loaded.Pdf) "Page $page of ${s.doc.pageCount}" else viewerWhen(m.at.toLong(), System.currentTimeMillis())
                Text(sub, style = TinType.caption, color = Dim)
            }
            IconBtn(Icons.Rounded.Share, "Share", onShare, tint = Color.White)
            IconBtn(Icons.Rounded.SaveAlt, "Save to phone", onSave, tint = Color.White)
        }
        Box(Modifier.weight(1f).fillMaxWidth(), contentAlignment = Alignment.Center) {
            when (s) {
                Loaded.Loading -> Text("Opening…", style = TinType.bodyM, color = Dim)
                is Loaded.Error -> Column(Modifier.padding(32.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(16.dp)) {
                    Icon(Icons.Rounded.ErrorOutline, null, tint = Dim)
                    Text(s.msg, style = TinType.bodyL, color = Color.White)
                    Box(Modifier.clip(RoundedCornerShape(50)).background(Color.White).clickable(role = Role.Button, onClick = onSave).padding(horizontal = 24.dp, vertical = 12.dp)) {
                        Text("Save to phone", style = TinType.bodyL.copy(fontWeight = FontWeight.SemiBold), color = Color.Black)
                    }
                }
                is Loaded.Pdf -> PdfPages(s.doc) { page = it }
                is Loaded.Text -> TextLines(s.lines)
            }
        }
    }
}

private fun loadText(f: File): Loaded {
    if (f.length() > FileOpen.TEXT_LIMIT) return Loaded.Error("This file is too big to show here. Save it to open it in another app.")
    val bytes = f.readBytes()
    if (bytes.take(4096).any { it.toInt() == 0 }) return Loaded.Error("This doesn’t look like a text file. Save it to open it in another app.")
    return Loaded.Text(String(bytes, Charsets.UTF_8).removePrefix("﻿").lines())
}

@Composable
private fun PdfPages(doc: PdfDoc, onPage: (Int) -> Unit) {
    val ls = rememberLazyListState()
    val density = LocalDensity.current
    val widthPx = with(density) { (LocalConfiguration.current.screenWidthDp.dp - 16.dp).roundToPx() }
    LaunchedEffect(ls) { snapshotFlow { ls.firstVisibleItemIndex }.collect { onPage(it + 1) } }
    var aspect by remember { mutableFloatStateOf(1.414f) }
    LazyColumn(Modifier.fillMaxSize(), state = ls, contentPadding = PaddingValues(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        itemsIndexed(List(doc.pageCount) { it }, key = { _, i -> i }) { _, i ->
            var bmp by remember { mutableStateOf<Bitmap?>(null) }
            LaunchedEffect(i) { bmp = doc.render(i, widthPx)?.also { if (i == 0) aspect = it.height.toFloat() / it.width } }
            DisposableEffect(i) { onDispose { val b = bmp; bmp = null; b?.recycle() } }
            val b = bmp
            val h = with(density) { ((b?.let { it.height.toFloat() / it.width } ?: aspect) * widthPx).toDp() }
            Box(Modifier.fillMaxWidth().height(h).background(Color(0xFF1E2522)), contentAlignment = Alignment.Center) {
                if (b != null && !b.isRecycled) Image(b.asImageBitmap(), "Page ${i + 1}", Modifier.fillMaxSize(), contentScale = ContentScale.FillWidth)
            }
        }
    }
}

@Composable
private fun TextLines(lines: List<String>) {
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(16.dp)) {
        itemsIndexed(lines) { _, l ->
            Text(l.ifEmpty { " " }, style = TinType.bodyM.copy(fontFamily = PlexMono, fontSize = 13.sp, lineHeight = 19.sp), color = Color(0xFFE6ECE9))
        }
    }
}
