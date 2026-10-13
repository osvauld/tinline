package com.osvauld.p2p

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.material3.ripple
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.semantics.Role
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.isShiftPressed
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.text.AnnotatedString
import java.text.Normalizer

/** One message found in this chat; `day` is the core's UTC day to load it with (`chat_day`). */
data class FindHit(val id: String, val day: String)

/**
 * Find in this chat (playground screens/search/FindBar.svelte, docs/search.md "Find in chat"). Hits come newest first, so
 * [cur] = 0 is the newest and the count reads 1/m for it. Older = cur + 1, newer = cur - 1, both wrap.
 */
class FindState {
    var open by mutableStateOf(false)
    var q by mutableStateOf("")
    var hits by mutableStateOf<List<FindHit>>(emptyList())
    /** The core's cursor for the next (older) page; non-null shows "m+". */
    var next by mutableStateOf<String?>(null)
    var cur by mutableIntStateOf(0)
    var focusReq by mutableIntStateOf(0)
    var error by mutableStateOf<String?>(null)
    fun show() { open = true; focusReq++ }
    fun close() { open = false; q = ""; hits = emptyList(); next = null; cur = 0; error = null }
    val current: FindHit? get() = hits.getOrNull(cur)
}

object ChatFind {
    // Highlighting only: the real core decides which messages match. Keep Indic vowel marks; fold Latin accents only.
    private fun fold(s: String): String {
        val compatible = Normalizer.normalize(s.lowercase(java.util.Locale.ROOT), Normalizer.Form.NFKC)
        return buildString {
            compatible.codePoints().forEach { cp ->
                val part = String(Character.toChars(cp))
                append(if (Character.UnicodeScript.of(cp) == Character.UnicodeScript.LATIN)
                    Normalizer.normalize(part, Normalizer.Form.NFD).replace(Regex("\\p{Mn}+"), "") else part)
            }
        }
    }

    /** Where each word of [q] matches the start of a word in [text] (the core's rule), as ranges into [text]. */
    fun ranges(text: String, q: String): List<IntRange> {
        val words = fold(q).split(Regex("\\s+")).filter { it.isNotEmpty() }
        if (words.isEmpty() || text.isEmpty()) return emptyList()
        // Folding can change lengths (é -> e + mark -> e); map folded indices back per character.
        val folded = StringBuilder(); val back = ArrayList<Int>()
        var index = 0
        while (index < text.length) {
            val cp = text.codePointAt(index)
            fold(String(Character.toChars(cp))).forEach { f -> folded.append(f); back.add(index) }
            index += Character.charCount(cp)
        }
        val f = folded.toString()
        val out = ArrayList<IntRange>()
        for (w in words) {
            var from = 0
            while (true) {
                val at = f.indexOf(w, from); if (at < 0) break
                val script = Character.UnicodeScript.of(w.codePointAt(0))
                val substring = script in setOf(Character.UnicodeScript.HAN, Character.UnicodeScript.HIRAGANA, Character.UnicodeScript.KATAKANA,
                    Character.UnicodeScript.THAI, Character.UnicodeScript.LAO, Character.UnicodeScript.KHMER, Character.UnicodeScript.MYANMAR)
                val start = substring || at == 0 || !f[at - 1].isLetterOrDigit() && Character.getType(f[at - 1]) != Character.NON_SPACING_MARK.toInt()
                if (start) { val end = back[at + w.length - 1]; out.add(back[at] until end + Character.charCount(text.codePointAt(end))) }
                from = at + 1
            }
        }
        return out.sortedBy { it.first }
    }

    /** Local fallback (debug sources): newest first over what is loaded. */
    fun local(msgs: List<uniffi.p2pcore.Message>, q: String): List<FindHit> =
        msgs.filter { !it.deleted && ranges(it.text.ifEmpty { it.attachment?.name ?: "" }, q).isNotEmpty() }
            .sortedWith(compareByDescending<uniffi.p2pcore.Message> { it.at }.thenByDescending { it.id }).map { FindHit(it.id, "") }

    /** The core's find-in-chat page (`search_in`), or null when this source is not the core (debug gallery). */
    fun core(app: P2pApp?, source: ChatSource, peerDid: String, q: String, cursor: String?): Pair<List<FindHit>, String?>? {
        if (app == null) return null
        val real = try { app.realChat } catch (e: UninitializedPropertyAccessException) { return null }
        if (source !== real) return null
        val r = app.node.searchIn(uniffi.p2pcore.SearchConvKind.DM, peerDid, q, 20u, cursor)
        return r.messages.map { FindHit(it.id, it.day) } to r.next
    }

    /** [text] with each match washed in gold, the current message's stronger (Marked.svelte variant "mark"). */
    fun marked(text: AnnotatedString, q: String, cur: Boolean, gold: androidx.compose.ui.graphics.Color): AnnotatedString {
        val rs = ranges(text.text, q)
        if (rs.isEmpty()) return text
        return buildAnnotatedString {
            append(text)
            rs.forEach { addStyle(SpanStyle(background = gold.copy(alpha = if (cur) .70f else .38f)), it.first, it.last + 1) }
        }
    }
}

/** Covers the conversation header in the same box: field (magnifier, query, n/m), older, newer, close. */
@Composable
fun FindBar(state: FindState, onQuery: (String) -> Unit, onOlder: () -> Unit, onNewer: () -> Unit, onClose: () -> Unit, modifier: Modifier = Modifier) {
    val c = Tin.c
    val focus = remember { FocusRequester() }
    var focused by remember { mutableStateOf(false) }
    LaunchedEffect(state.focusReq) { runCatching { focus.requestFocus() } }
    val count = state.hits.size
    val shown = if (count == 0) 0 else state.cur + 1
    val ring by animateColorAsState(if (focused) c.accentFill else c.sf2, tween(PgMotion.SWITCH, easing = PgMotion.Out), label = "findRing")
    Row(modifier.fillMaxSize().background(c.sf).padding(horizontal = 8.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(2.dp)) {
        Row(
            Modifier.weight(1f).height(44.dp).clip(RoundedCornerShape(22.dp)).background(c.sf2).border(2.dp, ring, RoundedCornerShape(22.dp)),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(Modifier.width(44.dp), contentAlignment = Alignment.Center) { Icon(Pg.Search, null, tint = c.ink2, modifier = Modifier.size(18.dp)) }
            Box(Modifier.weight(1f), contentAlignment = Alignment.CenterStart) {
                if (state.q.isEmpty()) Text("Find in this chat", style = TinType.bodyL.copy(fontSize = 15.sp, lineHeight = 21.sp), color = c.ink2, maxLines = 1)
                BasicTextField(
                    state.q, { state.q = it; onQuery(it) },
                    Modifier.fillMaxWidth().focusRequester(focus).onFocusChanged { focused = it.isFocused }
                        .semantics { contentDescription = "Find in this chat" }
                        .onPreviewKeyEvent { e ->
                            if (e.type != KeyEventType.KeyDown) return@onPreviewKeyEvent false
                            when (e.key) {
                                Key.Escape -> { onClose(); true }
                                Key.Enter, Key.NumPadEnter -> { if (e.isShiftPressed) onNewer() else onOlder(); true }
                                Key.DirectionUp -> { onOlder(); true }
                                Key.DirectionDown -> { onNewer(); true }
                                else -> false
                            }
                        },
                    singleLine = true, textStyle = TinType.bodyL.copy(fontSize = 15.sp, lineHeight = 21.sp, color = c.ink), cursorBrush = SolidColor(c.pr),
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search), keyboardActions = KeyboardActions(onSearch = { onOlder() }),
                )
            }
            Text(
                if (state.error != null) "—" else "$shown/$count" + if (state.next != null) "+" else "",
                Modifier.width(64.dp).padding(end = 14.dp).semantics {
                    liveRegion = LiveRegionMode.Polite
                    contentDescription = state.error ?: if (count > 0) "Match $shown of $count" else "No matches"
                },
                style = TinType.bodyM.copy(fontSize = 13.sp, lineHeight = 18.sp, fontFeatureSettings = "tnum"), color = c.ink2, textAlign = TextAlign.End, maxLines = 1,
            )
        }
        PgIconBtn(Pg.ChevUp, "Older match", onOlder, enabled = count > 0 || state.next != null, iconSize = 20.dp)
        PgIconBtn(Pg.Chev, "Newer match", onNewer, enabled = count > 0, iconSize = 20.dp)
        PgIconBtn(Pg.X, "Close find", onClose, iconSize = 20.dp)
    }
}

/** The playground's `.ib`: a 44 dp round icon button, 22 dp icon, dimmed (.4) when disabled. */
@Composable
fun PgIconBtn(
    icon: androidx.compose.ui.graphics.vector.ImageVector, desc: String, onClick: () -> Unit, modifier: Modifier = Modifier,
    tint: androidx.compose.ui.graphics.Color = Tin.c.ink, enabled: Boolean = true, iconSize: androidx.compose.ui.unit.Dp = 22.dp,
    dimAlpha: Float = .4f, clickableWhenDim: Boolean = false,
) {
    Box(
        modifier.size(44.dp).clip(CircleShape).alpha(if (enabled) 1f else dimAlpha)
            .let { m -> if (enabled || clickableWhenDim) m.pgPress(onClick, desc) else m.semantics { contentDescription = desc } },
        contentAlignment = Alignment.Center,
    ) { Icon(icon, null, tint = tint, modifier = Modifier.size(iconSize)) }
}

/** Click with the playground's `.press` feel: a quick scale to .97 while pressed, back on release. */
@Composable
fun Modifier.pgPress(onClick: () -> Unit, desc: String? = null): Modifier {
    val src = remember { MutableInteractionSource() }
    val pressed by src.collectIsPressedAsState()
    val s by animateFloatAsState(if (pressed) .97f else 1f, tween(if (pressed) PgMotion.PRESS else PgMotion.RELEASE, easing = PgMotion.Out), label = "press")
    return this.graphicsLayer { scaleX = s; scaleY = s }
        .clickable(src, ripple(), role = Role.Button, onClick = onClick)
        .let { if (desc != null) it.semantics { contentDescription = desc } else it }
}
