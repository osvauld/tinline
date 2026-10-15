package com.osvauld.p2p

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.material3.ripple
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.key.*
import androidx.compose.ui.layout.layout
import androidx.compose.ui.semantics.*
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import org.json.JSONArray
import org.json.JSONObject

/*
 * Profile furniture ported from playground src/screens/profile: FieldRow.svelte (one piece of information: view / edit / remove),
 * Value.svelte (any stored value, readably), and the shared input (FieldRow / AddInfo `input, textarea`).
 * Page furniture (pf-bar, pf-sec, pf-card, pf-act, pf-note, pf-strip, pf-err) is PeopleKit's Pf*; buttons are PBtn (groups/Btn).
 */

/** "Saved" stays on a row this long (FieldRow / Availability SAVED_MS: a UI constant, not a motion timing). */
const val SAVED_MS = 1800L

/** The "✓ Saved" tag: check 13 + 600 accent-ink, fading in over --dur-enter (ease-out). */
@Composable
fun SavedTag(offline: Boolean, modifier: Modifier = Modifier) {
    val reduced = rememberReducedMotion()
    val a = remember { Animatable(if (reduced) 1f else 0f) }
    LaunchedEffect(Unit) { a.animateTo(1f, tween(PgMotion.ENTER, easing = PgMotion.easeOut)) }
    Row(modifier.graphicsLayer { alpha = a.value }, verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(3.dp)) {
        Icon(PgIcons.Check, null, Modifier.size(13.dp), tint = Tin.c.accentInk)
        Text(if (offline) "Saved · syncs when connected" else "Saved", style = PgType.sub.copy(fontWeight = FontWeight.SemiBold), color = Tin.c.accentInk, maxLines = 1, softWrap = false)
    }
}

/**
 * FieldRow / AddInfo input: padding 10 14, r-ctl 14 (CSS radius), 1 px line border on surface, 15/21 ink, at least 44 tall.
 * Focus: accent border and the soft focus ring (`--ring-focus`: 3 px of accent-ink at 14 %, outside the box). [rows] > 1 is the
 * textarea (rows × 21 + padding). [selectAll] selects the text when it gains focus the first time (FieldRow focusIn).
 */
@Composable
fun PfInput(
    value: TextFieldValue, onChange: (TextFieldValue) -> Unit, label: String, modifier: Modifier = Modifier, placeholder: String = "",
    rows: Int = 1, link: Boolean = false, focus: Boolean = false, onSubmit: (() -> Unit)? = null, onEscape: (() -> Unit)? = null,
    plain: Boolean = false,
) {
    val c = Tin.c
    val src = remember { MutableInteractionSource() }
    val focused by src.collectIsFocusedAsState()
    val req = remember { FocusRequester() }
    LaunchedEffect(focus) { if (focus) runCatching { req.requestFocus() } }
    val shape = RoundedCornerShape(14.dp)
    val ring = c.accentInk.copy(alpha = 0.14f)
    val multi = rows > 1
    BasicTextField(
        value, onChange,
        modifier.fillMaxWidth().focusRequester(req)
            .onPreviewKeyEvent { e ->
                if (e.type != KeyEventType.KeyDown) return@onPreviewKeyEvent false
                when {
                    e.key == Key.Escape && onEscape != null -> { onEscape(); true }
                    e.key == Key.Enter && multi && (e.isCtrlPressed || e.isMetaPressed) && onSubmit != null -> { onSubmit(); true }
                    else -> false
                }
            }
            .semantics { contentDescription = label },
        singleLine = !multi, minLines = rows, maxLines = if (multi) rows else 1,
        textStyle = PgType.body.copy(color = c.ink), cursorBrush = SolidColor(c.ink), interactionSource = src,
        keyboardOptions = KeyboardOptions(
            capitalization = if (plain || link) KeyboardCapitalization.None else KeyboardCapitalization.Sentences,
            autoCorrectEnabled = !(plain || link), keyboardType = if (link) KeyboardType.Uri else KeyboardType.Text,
            imeAction = if (multi) ImeAction.Default else ImeAction.Done,
        ),
        keyboardActions = KeyboardActions(onDone = { onSubmit?.invoke() }),
        decorationBox = { inner ->
            Box(
                Modifier.fillMaxWidth().heightIn(min = 44.dp)
                    .drawBehind {
                        if (focused) {
                            val w = 3.dp.toPx(); val r = 14.dp.toPx()
                            drawRoundRect(ring, Offset(-w / 2, -w / 2), Size(size.width + w, size.height + w), CornerRadius(r + w / 2), style = Stroke(w))
                        }
                    }
                    .background(c.sf, shape).border(1.dp, if (focused) c.accent else c.ln, shape)
                    .padding(horizontal = 14.dp, vertical = 10.dp),
                contentAlignment = if (multi) Alignment.TopStart else Alignment.CenterStart,
            ) {
                if (value.text.isEmpty() && placeholder.isNotEmpty()) Text(placeholder, style = PgType.body, color = c.ink2.copy(alpha = 0.75f), maxLines = rows)
                inner()
            }
        },
    )
}

enum class FieldKind { Text, Multiline, Link }

/**
 * FieldRow.svelte. View: label over value, the whole row edits (the pencil is a pointer convenience). Edit: label, field, the
 * always-present hint / error line (36 min, so a refusal never moves the buttons), the byte count once past 80 % of the limit,
 * then [Remove] … Cancel · Save. A failed save keeps the draft. Read-only rows show the value; values this version can't edit
 * (structured data from another app version) can only be removed, with an inline confirm.
 * [onSave] / [onRemove] return null when accepted, else the message.
 */
@Composable
fun FieldRow(
    label: String, value: Any?, kind: FieldKind = FieldKind.Text, hint: String = "", placeholder: String = "", optional: Boolean = false,
    readonly: Boolean = false, limit: Int = 0, offline: Boolean = false, empty: String = "Not set", note: String = "",
    onSave: (suspend (String) -> String?)? = null, onRemove: (suspend () -> String?)? = null, onOpen: ((String) -> Unit)? = null,
    startEditing: Boolean = false,
) {
    val c = Tin.c
    val scope = rememberCoroutineScope()
    var mode by remember { mutableStateOf("view") }
    var draft by remember { mutableStateOf(TextFieldValue("")) }
    var error by remember { mutableStateOf("") }
    var saved by remember { mutableStateOf(0) }
    var busy by remember { mutableStateOf(false) }
    val isNull = value == null || value == JSONObject.NULL
    val text = when { value is String -> value; isNull -> ""; ProfileInfo.isScalar(value) -> value.toString(); else -> "" }
    val editable = !readonly && onSave != null && (isNull || value is String || ProfileInfo.isScalar(value))
    val removable = !readonly && onRemove != null && optional && !editable
    val url = if (kind == FieldKind.Link) ProfileInfo.safeUrl(text) else null
    val bytes = ProfileRules.utf8(draft.text)
    val near = limit > 0 && bytes > limit * 0.8

    LaunchedEffect(saved) { if (saved > 0) { delay(SAVED_MS); saved = 0 } }
    LaunchedEffect(Unit) { if (startEditing && editable) { draft = TextFieldValue(text, TextRange(text.length)); mode = "edit" } }
    fun edit() { draft = TextFieldValue(text, TextRange(0, if (kind == FieldKind.Multiline) 0 else text.length)); error = ""; mode = "edit" }
    fun cancel() { mode = "view"; error = "" }
    fun save() {
        if (busy) return
        val run = onSave ?: return
        busy = true
        scope.launch {
            val r = run(draft.text); busy = false
            if (r == null) { mode = "view"; error = ""; saved++ } else error = r
        }
    }
    fun drop() {
        if (busy) return
        val run = onRemove ?: return
        busy = true
        scope.launch {
            val r = run(); busy = false
            if (r != null) error = r else { mode = "view"; error = "" }
        }
    }

    Box(Modifier.fillMaxWidth()) {
        when (mode) {
            "edit" -> FadeIn {
                Box(Modifier.fillMaxWidth()) {
                    Column(Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 14.dp, bottom = 12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        Text(label, style = PgType.sub.copy(fontWeight = FontWeight.SemiBold), color = c.ink2)
                        PfInput(draft, { draft = it }, label, placeholder = placeholder, rows = if (kind == FieldKind.Multiline) 4 else 1,
                            link = kind == FieldKind.Link, focus = true, onSubmit = ::save, onEscape = ::cancel, plain = kind != FieldKind.Multiline)
                        Column(Modifier.fillMaxWidth().heightIn(min = 36.dp).semantics { liveRegion = LiveRegionMode.Polite }, verticalArrangement = Arrangement.spacedBy(2.dp)) {
                            if (error.isNotEmpty()) Text(error, style = PgType.sub, color = c.danger)
                            else if (hint.isNotEmpty()) Text(hint, style = PgType.sub, color = c.ink2)
                        }
                        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                            if (optional && !isNull) PBtn("Remove", ::drop, variant = PVariant.DangerQuiet)
                            Spacer(Modifier.weight(1f))
                            PBtn("Cancel", ::cancel, variant = PVariant.Quiet)
                            PBtn("Save", ::save, variant = PVariant.Primary)
                        }
                    }
                    if (near) Text("$bytes of $limit bytes", Modifier.align(Alignment.TopEnd).padding(top = 14.dp, end = 16.dp),
                        style = PgType.sub.copy(fontFeatureSettings = "tnum"), color = if (bytes > limit) c.danger else c.ink2, maxLines = 1, softWrap = false)
                }
            }
            "remove" -> FadeIn {
                Column(Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 14.dp, bottom = 12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    Text("Remove “$label” from your profile?", style = PgType.body.copy(fontWeight = FontWeight.SemiBold), color = c.ink)
                    Text("It disappears from your profile. Earlier versions can remain in synced history.", style = PgType.sub19, color = c.ink2)
                    if (error.isNotEmpty()) Text(error, Modifier.semantics { liveRegion = LiveRegionMode.Assertive }, style = PgType.sub, color = c.danger)
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End), verticalAlignment = Alignment.CenterVertically) {
                        PBtn("Cancel", ::cancel, variant = PVariant.Quiet)
                        PBtn("Remove", ::drop, variant = PVariant.Danger)
                    }
                }
            }
            else -> Row(Modifier.fillMaxWidth().height(IntrinsicSize.Min)) {
                val src = remember { MutableInteractionSource() }
                Row(
                    Modifier.weight(1f).heightIn(min = if (!editable && label.isEmpty()) 0.dp else 64.dp)
                        .let { if (editable) it.clickable(src, ripple(), role = Role.Button, onClickLabel = "Edit") { edit() } else it }
                        .let { if (editable) it.semantics(mergeDescendants = true) { contentDescription = "$label: ${text.ifEmpty { empty }}. Edit" } else it }
                        .padding(horizontal = 16.dp, vertical = if (editable) 10.dp else if (label.isEmpty()) 14.dp else 12.dp),
                    verticalAlignment = if (editable) Alignment.CenterVertically else Alignment.Top,
                ) {
                    Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(1.dp)) {
                        if (label.isNotEmpty() || saved > 0) Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            Text(label, style = PgType.sub, color = c.ink2)
                            if (saved > 0) SavedTag(offline && editable)
                        }
                        when {
                            editable -> Text(text.ifEmpty { empty }, style = PgType.body, color = if (text.isEmpty()) c.ink2 else c.ink)
                            isNull || value == "" -> Text(empty, style = PgType.body, color = c.ink2)
                            value is String -> Text(value, style = PgType.body, color = c.ink)
                            else -> ProfileValue(value)
                        }
                        if (note.isNotEmpty()) Text(note, Modifier.padding(top = 2.dp), style = PgType.sub, color = c.ink2)
                    }
                }
                if (url != null && onOpen != null) ToolBtn(PgIcons.Link, 20.dp, "Open $label: ${ProfileInfo.shortUrl(url)}") { onOpen(url) }
                if (editable) Box(Modifier.padding(end = 6.dp).let { if (url != null && onOpen != null) it.marginStart((-8).dp) else it }) {
                    ToolBtn(PgIcons.Edit, 18.dp, null, alpha = 0.8f) { edit() }
                }
                if (removable) ToolBtn(PgIcons.Trash, 20.dp, "Remove $label") { error = ""; mode = "remove" }
            }
        }
    }
}

/** FieldRow `.tool`: a full-height 44 wide icon button, muted; [label] null = decorative for assistive tech (the row is the control). */
@Composable
private fun ToolBtn(icon: ImageVector, iconSize: Dp, label: String?, alpha: Float = 1f, onClick: () -> Unit) {
    val src = remember { MutableInteractionSource() }
    Box(
        Modifier.width(44.dp).fillMaxHeight().press(src).clickable(src, ripple(bounded = false, radius = 22.dp), role = Role.Button, onClick = onClick)
            .semantics { if (label != null) contentDescription = label else hideFromAccessibility() },
        contentAlignment = Alignment.Center,
    ) { Icon(icon, null, Modifier.size(iconSize).graphicsLayer { this.alpha = alpha }, tint = Tin.c.ink2) }
}

/** Fades content in over --dur-crossfade (FieldRow `.edit { animation: fadein }`). */
@Composable
private fun FadeIn(content: @Composable () -> Unit) {
    val reduced = rememberReducedMotion()
    val a = remember { Animatable(if (reduced) 1f else 0f) }
    LaunchedEffect(Unit) { a.animateTo(1f, tween(PgMotion.CROSSFADE, easing = PgMotion.easeOut)) }
    Box(Modifier.graphicsLayer { alpha = a.value }) { content() }
}

/**
 * Value.svelte: text and numbers as text, lists as lists, nested objects as indented label/value pairs. Text only, never fetched.
 * Deeper than 4 levels stops at "…", so a hostile value can't grow the page.
 */
@Composable
fun ProfileValue(v: Any?, depth: Int = 0) {
    val c = Tin.c
    fun prim(x: Any?) = x == null || x == JSONObject.NULL || x is String || x is Number || x is Boolean
    fun show(x: Any?) = when (x) { null, JSONObject.NULL -> "—"; is Boolean -> if (x) "Yes" else "No"; else -> x.toString() }
    val nest = Modifier.padding(top = 6.dp).drawBehind {
        val w = 2.dp.toPx()
        val x = if (layoutDirection == androidx.compose.ui.unit.LayoutDirection.Rtl) size.width - w / 2 else w / 2
        drawLine(c.hairline, Offset(x, 0f), Offset(x, size.height), w)
    }.padding(start = 12.dp)
    when {
        depth > 4 -> Text("…", style = PgType.body, color = c.ink2)
        v is JSONArray && (0 until v.length()).all { prim(v.opt(it)) } ->
            Text((0 until v.length()).joinToString(", ") { show(v.opt(it)) }.ifEmpty { "—" }, style = PgType.body, color = c.ink)
        v is JSONArray -> Column(nest, verticalArrangement = Arrangement.spacedBy(8.dp)) { for (i in 0 until v.length()) ProfileValue(v.opt(i), depth + 1) }
        v is JSONObject -> Column(nest, verticalArrangement = Arrangement.spacedBy(8.dp)) {
            v.keys().forEach { k ->
                Column(verticalArrangement = Arrangement.spacedBy(1.dp)) {
                    Text(ProfileInfo.humanize(k), style = PgType.sub, color = c.ink2)
                    ProfileValue(v.opt(k), depth + 1)
                }
            }
        }
        else -> Text(show(v), style = PgType.body, color = c.ink, overflow = TextOverflow.Clip)
    }
}

/** CSS `margin-inline-start: n` (may be negative): shifts the element and shrinks / grows the space it takes. */
fun Modifier.marginStart(n: Dp) = this.layout { m, cs ->
    val d = n.roundToPx()
    val p = m.measure(cs)
    layout((p.width + d).coerceAtLeast(0), p.height) { p.placeRelative(d, 0) }
}
