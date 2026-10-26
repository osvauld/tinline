package com.osvauld.p2p

import androidx.compose.ui.res.stringResource
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathFillType
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.key.*
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.*
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt

/*
 * My profile (playground screens/profile/MyProfile.svelte, route `#/me`): picture, name, status, about, availability and
 * additional information. Each piece saves on its own and patches one key (ProfileOp -> one core call). Accepted state lives in the
 * profile adapter; drafts live in the rows. Layers: PictureFlow (+ Cropper), Remove picture, AddInfo, OpenLink.
 */

/** Debug-gallery start states (never set in production). */
data class MyProfilePreview(val picked: Picked? = null, val removeAsk: Boolean = false, val addOpen: Boolean = false, val addPick: String? = null,
                            val link: String? = null, val picError: String? = null, val pending: Boolean = false, val editField: String? = null)

private sealed interface ProfileLayer {
    class Picture(val picked: Picked) : ProfileLayer
    data object Remove : ProfileLayer
    data class Add(val pick: String?) : ProfileLayer
    data class Link(val url: String) : ProfileLayer
}

@Composable
fun MyProfileScreen(store: ProfileStore, fallbackName: String, onBack: () -> Unit, preview: MyProfilePreview? = null, title: String = stringResource(R.string.profile_my_profile)) {
    val c = Tin.c
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    val me by store.self.collectAsState()
    val online by store.online.collectAsState()
    val view = rememberProfileView(me)
    val fields = view?.fields ?: JSONObject()
    val extras = ProfileInfo.extraKeys(fields)
    val hasPic = view?.hash != null
    val offline = !online
    val hasOther = extras.any { it !in ProfileInfo.KNOWN }
    val name = view?.text("name")?.takeIf { it.isNotBlank() } ?: fallbackName
    val badLinkMsg = stringResource(R.string.profile_err_bad_link)
    val tooDetailedMsg = stringResource(R.string.profile_err_too_detailed)
    val pictureUpdatedMsg = stringResource(R.string.profile_announce_updated)
    val pictureRemovedMsg = stringResource(R.string.profile_announce_removed)
    val pictureDesc = stringResource(R.string.profile_picture_desc)

    var layer by remember(me) { mutableStateOf<ProfileLayer?>(when {
        preview?.picked != null -> ProfileLayer.Picture(preview.picked)
        preview?.removeAsk == true -> ProfileLayer.Remove
        preview?.addOpen == true -> ProfileLayer.Add(preview.addPick)
        preview?.link != null -> ProfileLayer.Link(preview.link)
        else -> null
    }) }
    var picError by remember(me) { mutableStateOf(preview?.picError ?: "") }
    var removeError by remember(me) { mutableStateOf("") }
    var announce by remember { mutableStateOf("") }
    // px.pending: saves accepted while offline, waiting for the network. No per-contact delivery state exists.
    var pending by remember(me) { mutableIntStateOf(if (preview?.pending == true) 1 else 0) }
    LaunchedEffect(online) { if (online && preview?.pending != true) pending = 0 }

    suspend fun accepted(r: String?): String? { if (r == null && !store.online.value) pending++; return r }
    suspend fun op(o: ProfileOp): String? = accepted(store.apply(o))

    fun choose(uri: android.net.Uri) {
        picError = ""
        scope.launch {
            when (val r = withContext(Dispatchers.IO) { inspectUri(ctx.contentResolver, uri) }) {
                is PickResult.Refused -> picError = r.error
                is PickResult.Ok -> layer = ProfileLayer.Picture(r.picked)
            }
        }
    }
    val picker = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri -> if (uri != null) choose(uri) }
    fun pick() = picker.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly))

    /* ---- field saves (MyProfile.svelte optionalText / saveLink / drop) ---- */
    fun optionalText(key: String): suspend (String) -> String? = { v ->
        val t = v.trim()
        if (t.isEmpty()) setField(store, fields, key, null, ::op) else setField(store, fields, key, t, ::op)
    }
    fun saveLink(key: String): suspend (String) -> String? = { v ->
        if (v.trim().isEmpty()) setField(store, fields, key, null, ::op)
        else ProfileInfo.normaliseLink(v)?.let { setField(store, fields, key, it, ::op) } ?: badLinkMsg
    }
    fun drop(key: String): suspend () -> String? = { setField(store, fields, key, null, ::op) }

    PfPage(title, onBack, bottomPad = 96.dp, overlay = {
        // Floating, not in the page flow: these appear on their own and must not move the page.
        Box(Modifier.fillMaxSize().padding(start = 16.dp, end = 16.dp, bottom = 16.dp), contentAlignment = Alignment.BottomCenter) {
            val ms = if (rememberReducedMotion()) 0 else PgMotion.CROSSFADE
            AnimatedVisibility(picError.isNotEmpty(), enter = fadeIn(tween(ms)), exit = fadeOut(tween(ms))) {
                FloatingError(picError) { picError = "" }
            }
            AnimatedVisibility(picError.isEmpty() && pending > 0, enter = fadeIn(tween(ms)), exit = fadeOut(tween(ms))) {
                Box(Modifier.widthIn(max = 520.dp)) { PfStrip(stringResource(R.string.profile_saved_pending), PgIcons.Clock, floating = true) }
            }
        }
        Text(announce, Modifier.size(1.dp).semantics { liveRegion = LiveRegionMode.Polite }, color = Color.Transparent)
    }) {
        // hero
        Column(Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, top = 24.dp, bottom = 4.dp).semantics { contentDescription = pictureDesc },
            horizontalAlignment = Alignment.CenterHorizontally) {
            ViewablePgAvatar(name, 0, 112.dp, person = me)
            Row(Modifier.padding(top = 18.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                if (hasPic) {
                    PBtn(stringResource(R.string.profile_change_photo), ::pick, icon = PgIcons.Camera, minWidth = 140.dp)
                    PBtn(stringResource(R.string.common_remove), { removeError = ""; layer = ProfileLayer.Remove }, minWidth = 140.dp)
                } else PBtn(stringResource(R.string.profile_add_photo), ::pick, icon = PgIcons.Camera, minWidth = 140.dp)
            }
        }

        PfSec(stringResource(R.string.profile_section_profile)) {
            PfCard {
                row { key(me, "name") { FieldRow(stringResource(R.string.profile_field_name), name, limit = ProfileRules.NAME_LIMIT, offline = offline, placeholder = stringResource(R.string.profile_name_placeholder), startEditing = preview?.editField == "name",
                    onSave = { v -> ProfileRules.checkName(v) ?: op(ProfileOp.SetName(v.trim())) }) } }
                row { key(me, "status") { FieldRow(stringResource(R.string.profile_field_status), fields.opt("status"), optional = true, limit = ProfileRules.TEXT_LIMIT, offline = offline,
                    empty = stringResource(R.string.profile_status_empty), placeholder = stringResource(R.string.profile_status_placeholder), startEditing = preview?.editField == "status", onSave = optionalText("status"), onRemove = drop("status")) } }
                row { key(me, "about") { FieldRow(stringResource(R.string.profile_field_about), fields.opt("about"), FieldKind.Multiline, optional = true, limit = ProfileRules.TEXT_LIMIT, offline = offline,
                    empty = stringResource(R.string.profile_about_empty), startEditing = preview?.editField == "about", onSave = optionalText("about"), onRemove = drop("about")) } }
            }
        }

        PfSec(stringResource(R.string.profile_section_availability)) {
            PfCard { row { key(me) { AvailabilityCard(fields, offline) { a, until -> op(ProfileOp.SetAvailable(a, until)) } } } }
            PfNote(stringResource(R.string.profile_availability_note))
        }

        PfSec(stringResource(R.string.profile_section_additional)) {
            PfCard {
                extras.forEach { k ->
                    val known = ProfileInfo.KNOWN[k]
                    row { key(me, k) {
                        FieldRow(ProfileInfo.labelOf(k), fields.opt(k), if (known?.link == true) FieldKind.Link else FieldKind.Text, hint = known?.hint ?: "",
                            optional = true, offline = offline, limit = ProfileRules.TEXT_LIMIT,
                            onSave = if (known?.link == true) saveLink(k) else optionalText(k), onRemove = drop(k), onOpen = { layer = ProfileLayer.Link(it) })
                    } }
                }
                act { PfAct(stringResource(R.string.profile_add_information), onClick = { layer = ProfileLayer.Add(null) }, icon = PgIcons.Plus) }
            }
            PfNote(stringResource(R.string.profile_field_help))
            if (hasOther) PfNote(stringResource(R.string.profile_other_info_note))
        }
    }

    // Layers (one at a time, as the {#if} chain).
    PgLayer(layer as? ProfileLayer.Picture, { layer = null }, stringResource(R.string.profile_layer_choose_picture)) { l ->
        key(l.picked) { PictureFlow(l.picked, hasPic, onClose = { layer = null }) { png ->
            if (png.size > ImageLimits.OUT_MAX_BYTES) tooDetailedMsg
            else op(ProfileOp.ReplacePicture(png)).also { if (it == null) { layer = null; announce = pictureUpdatedMsg } }
        } }
    }
    PgLayer(layer as? ProfileLayer.Remove, { layer = null }, stringResource(R.string.profile_remove_picture_title)) {
        Column(Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, top = 20.dp, bottom = 16.dp)) {
            DlgTitle(stringResource(R.string.profile_remove_picture_q))
            DlgP(stringResource(R.string.profile_remove_picture_body), lead = true)
            DlgP(stringResource(R.string.profile_picture_help))
            if (removeError.isNotEmpty()) PfErr(removeError)
            PfActs {
                PBtn(stringResource(R.string.common_cancel), { layer = null }, variant = PVariant.Quiet)
                PBtn(stringResource(R.string.profile_remove_picture_button), {
                    scope.launch {
                        val r = op(ProfileOp.RemovePicture)
                        if (r == null) { layer = null; removeError = ""; announce = pictureRemovedMsg } else removeError = r
                    }
                }, variant = PVariant.Danger)
            }
        }
    }
    PgLayer(layer as? ProfileLayer.Add, { layer = null }, stringResource(R.string.profile_add_information)) { l ->
        AddInfo(fields, l.pick, onClose = { layer = null }) { k, v -> setField(store, fields, k, v, ::op) }
    }
    PgLayer(layer as? ProfileLayer.Link, { layer = null }, stringResource(R.string.profile_open_link_layer)) { l -> OpenLink(l.url) { layer = null } }
}

/** profile.ts setField: the playground's checks first (same messages), then `set_profile_field(key, json?)`. */
private suspend fun setField(store: ProfileStore, fields: JSONObject, key: String, value: String?, op: suspend (ProfileOp) -> String?): String? {
    ProfileRules.checkField(key, value, fields)?.let { return it }
    if (key == "name") return op(ProfileOp.SetName(value!!.trim()))
    if (value == null && !fields.has(key)) return null
    return op(ProfileOp.SetField(key, value?.let { JSONObject.quote(it) }))
}

/** MyProfile `.perr`: the picture refusal, floating over the bottom of the page (danger-bg, r12, pop shadow), with a dismiss. */
@Composable
private fun FloatingError(text: String, onDismiss: () -> Unit) {
    val c = Tin.c
    val shape = RoundedCornerShape(12.dp)
    Row(
        Modifier.widthIn(max = 520.dp).fillMaxWidth().depth(Depth.Pop, shape).clip(shape).background(c.erc)
            .padding(start = 12.dp, end = 4.dp, top = 4.dp, bottom = 4.dp).semantics { liveRegion = LiveRegionMode.Assertive },
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text(text, Modifier.weight(1f), style = PgType.sub19, color = c.danger)
        PgIconBtn(PgIcons.X, stringResource(R.string.profile_dismiss), onDismiss, tint = c.danger, iconSize = 18.dp)
    }
}

// ------------------------------------------------------------------ Availability.svelte

/**
 * Available | Busy segmented control (radio group), the line under it (Available / Busy until … / the refusal) which always holds
 * its 19 px and carries the Saved tag at its end, and, when busy, the "for how long" chips. One account-wide setting, separate
 * from being online. A deadline that passes reads Available again.
 */
@OptIn(androidx.compose.foundation.layout.ExperimentalLayoutApi::class)
@Composable
fun AvailabilityCard(fields: JSONObject, offline: Boolean, save: suspend (available: Boolean, untilSecs: Long?) -> String?) {
    val c = Tin.c
    val scope = rememberCoroutineScope()
    var now by remember { mutableLongStateOf(System.currentTimeMillis()) }
    val av = effectiveAvailability(fields, now)
    LaunchedEffect(av) { av.untilMs?.let { delay((it - System.currentTimeMillis()).coerceAtLeast(0) + 50); now = System.currentTimeMillis() } }
    val howLongDesc = stringResource(R.string.profile_busy_how_long)
    var choice by remember { mutableStateOf("1h") }
    var error by remember { mutableStateOf("") }
    var saved by remember { mutableIntStateOf(0) }
    LaunchedEffect(saved) { if (saved > 0) { delay(SAVED_MS); saved = 0 } }
    val active = if (av.available) null else if (av.untilMs == null) "open" else choice
    val line = if (av.available) stringResource(R.string.profile_available)
        else av.untilMs?.let { stringResource(R.string.profile_busy_until, ProfileInfo.untilLabel(it)) } ?: stringResource(R.string.profile_busy_until_open)
    val pastMsg = stringResource(R.string.profile_err_time_passed)

    fun apply(available: Boolean, untilMs: Long?) {
        if (!available && untilMs != null && untilMs <= System.currentTimeMillis()) { error = pastMsg; return }
        scope.launch {
            val r = save(available, untilMs?.let { it / 1000 })
            if (r != null) error = r else { error = ""; saved++; now = System.currentTimeMillis() }
        }
    }
    fun setBusy(id: String) { choice = id; apply(false, BUSY_CHOICES.first { it.id == id }.until(System.currentTimeMillis())) }

    Column(Modifier.fillMaxWidth().padding(16.dp, 14.dp, 16.dp, 14.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        PgSegmented(listOf(stringResource(R.string.profile_available), stringResource(R.string.profile_busy)), if (av.available) 0 else 1, { i ->
            if (i == 0) apply(true, null) else if (av.available) setBusy(choice)
        }, label = stringResource(R.string.profile_section_availability))
        Row(Modifier.fillMaxWidth().height(19.dp).semantics(mergeDescendants = true) { liveRegion = LiveRegionMode.Polite },
            verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            if (error.isNotEmpty()) Text(error, Modifier.weight(1f, fill = false), style = PgType.sub19, color = c.danger,
                maxLines = 1, softWrap = false, overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis)
            else Text(line, Modifier.weight(1f, fill = false), style = PgType.sub19.copy(fontFeatureSettings = "tnum"), color = c.ink, maxLines = 1,
                overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis, softWrap = false)
            if (saved > 0) { Spacer(Modifier.weight(1f)); SavedTag(offline) }
        }
        if (!av.available) FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp),
            modifier = Modifier.semantics { contentDescription = howLongDesc }) {
            BUSY_CHOICES.forEach { b -> PgChip(b.label, active == b.id, { setBusy(b.id) }, minHeight = 36.dp) }
        }
    }
}

// ------------------------------------------------------------------ AddInfo.svelte

@Composable
private fun AddInfo(fields: JSONObject, startPick: String?, onClose: () -> Unit, save: suspend (key: String, value: String) -> String?) {
    val c = Tin.c
    val scope = rememberCoroutineScope()
    val have = fields.keys().asSequence().toSet()
    val icon = mapOf("pronouns" to PgIcons.Users, "location" to PgIcons.Info, "website" to PgIcons.Link)
    val choices = ProfileInfo.KNOWN.keys.filter { it !in have }
    val dupMsg = stringResource(R.string.profile_err_name_taken)
    val needValueMsg = stringResource(R.string.profile_err_need_value)
    val badLinkMsg = stringResource(R.string.profile_err_bad_link)
    val kindsDesc = stringResource(R.string.profile_add_kinds_desc)
    var pick by remember { mutableStateOf(startPick) }   // a KNOWN key, or "" for something else
    var name by remember { mutableStateOf(TextFieldValue("")) }
    var v by remember { mutableStateOf(TextFieldValue("")) }
    var error by remember { mutableStateOf("") }
    val key = pick?.ifEmpty { null } ?: name.text.trim().lowercase().replace(Regex("\\s+"), "-")
    val link = pick != null && ProfileInfo.KNOWN[pick]?.link == true

    fun submit() {
        if (pick == "") {
            ProfileRules.checkKey(name.text)?.let { error = it; return }
            if (key in ProfileRules.reserved || key in ProfileInfo.KNOWN || key in have) { error = dupMsg; return }
        }
        var value: String? = v.text.trim()
        if (value.isNullOrEmpty()) { error = needValueMsg; return }
        if (link) { value = ProfileInfo.normaliseLink(v.text); if (value == null) { error = badLinkMsg; return } }
        scope.launch { val r = save(key, value); if (r == null) onClose() else error = r }
    }

    Column(Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, top = 20.dp, bottom = 16.dp)) {
        val p = pick
        if (p == null) {
            DlgTitle(stringResource(R.string.profile_add_information))
            DlgP(stringResource(R.string.profile_add_info_lead))
            Column(Modifier.fillMaxWidth().padding(top = 10.dp).bleedX(12.dp).semantics { contentDescription = kindsDesc }) {
                val shape = RoundedCornerShape(14.dp)
                choices.forEach { k -> PfAct(ProfileInfo.labelOf(k), onClick = { pick = k }, icon = icon[k] ?: PgIcons.Info, padH = 12.dp, shape = shape) }
                PfAct(stringResource(R.string.profile_something_else), onClick = { pick = "" }, icon = PgIcons.Plus, sub = stringResource(R.string.profile_name_it_yourself), padH = 12.dp, shape = shape)
            }
            PfActs { PBtn(stringResource(R.string.common_cancel), onClose, variant = PVariant.Quiet) }
        } else {
            DlgTitle(if (p.isNotEmpty()) ProfileInfo.labelOf(p) else stringResource(R.string.profile_something_else))
            Column(Modifier.fillMaxWidth().padding(top = 12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                val labelStyle = PgType.sub.copy(fontWeight = androidx.compose.ui.text.font.FontWeight.SemiBold)
                if (p.isEmpty()) {
                    Text(stringResource(R.string.profile_what_called), Modifier.padding(top = 6.dp), style = labelStyle, color = c.ink2)
                    PfInput(name, { name = it }, stringResource(R.string.profile_what_called), placeholder = stringResource(R.string.profile_what_called_placeholder), focus = true, onSubmit = ::submit)
                }
                Text(if (p.isNotEmpty()) ProfileInfo.labelOf(p) else stringResource(R.string.profile_value), Modifier.padding(top = 6.dp), style = labelStyle, color = c.ink2)
                PfInput(v, { v = it }, if (p.isNotEmpty()) ProfileInfo.labelOf(p) else stringResource(R.string.profile_value), placeholder = ProfileInfo.KNOWN[p]?.hint ?: "",
                    link = link, focus = p.isNotEmpty(), onSubmit = ::submit, plain = true)
                if (error.isNotEmpty()) PfErr(error, Modifier.padding(top = 0.dp))
            }
            PfActs {
                PBtn(stringResource(R.string.common_back), { pick = null; error = "" }, variant = PVariant.Quiet)
                PBtn(stringResource(R.string.profile_add), ::submit, variant = PVariant.Primary)
            }
        }
    }
}

// ------------------------------------------------------------------ OpenLink.svelte

@Composable
private fun OpenLink(url: String, onClose: () -> Unit) {
    val ctx = LocalContext.current
    Column(Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, top = 20.dp, bottom = 16.dp)) {
        DlgTitle(stringResource(R.string.profile_open_link_q))
        androidx.compose.runtime.CompositionLocalProvider(androidx.compose.ui.platform.LocalLayoutDirection provides androidx.compose.ui.unit.LayoutDirection.Ltr) {
            Text(url, Modifier.fillMaxWidth().padding(top = 8.dp), style = TextStyle(fontFamily = FontFamily.Monospace, fontSize = 13.sp, lineHeight = 18.sp), color = Tin.c.ink)
        }
        DlgP(stringResource(R.string.profile_open_link_body))
        PfActs {
            PBtn(stringResource(R.string.common_cancel), onClose, variant = PVariant.Quiet)
            PBtn(stringResource(R.string.profile_open_link_button), { openUrl(ctx, url); onClose() }, variant = PVariant.Primary)
        }
    }
}

// ------------------------------------------------------------------ PictureFlow.svelte + Cropper.svelte

private const val VIEW = 264f
private val SIZES = listOf(72, 48, 40)

@Composable
private fun PictureFlow(picked: Picked, replacing: Boolean, onClose: () -> Unit, use: suspend (ByteArray) -> String?) {
    val c = Tin.c
    val scope = rememberCoroutineScope()
    val image = remember(picked) { picked.bitmap.asImageBitmap() }
    var crop by remember(picked) { mutableStateOf(centred(picked.w, picked.h, VIEW)) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf("") }
    val prepareFailedMsg = stringResource(R.string.profile_err_prepare)
    Column(Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, top = 20.dp, bottom = 16.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        Text(stringResource(R.string.profile_crop_title), Modifier.fillMaxWidth().padding(bottom = 14.dp).semantics { heading() }, style = PgType.title, color = c.ink)
        Cropper(image, picked.w, picked.h, crop) { crop = it }
        Row(Modifier.padding(top = 6.dp).clearAndSetSemantics {}, verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(14.dp)) {
            SIZES.forEach { px -> CropPreview(image, picked.w, picked.h, crop, px) }
        }
        if (replacing) Text(stringResource(R.string.profile_picture_help), Modifier.fillMaxWidth().padding(top = 14.dp), style = PgType.sub19, color = c.ink2)
        if (error.isNotEmpty()) PfErr(error)
        PfActs {
            PBtn(stringResource(R.string.common_cancel), onClose, variant = PVariant.Quiet)
            PBtn(stringResource(R.string.profile_use_photo), {
                if (!busy) {
                    busy = true; error = ""
                    scope.launch {
                        val r = runCatching { withContext(Dispatchers.Default) { renderSquare(picked, crop, VIEW) } }
                        error = r.fold({ use(it) ?: "" }, { prepareFailedMsg })
                        busy = false
                    }
                }
            }, variant = PVariant.Primary, enabled = !busy)
        }
    }
}

/** Cropper: drag to move, arrow keys move (Shift = 40), +/- or the slider zoom (1..4), Home recentres. The window is the preview. */
@Composable
private fun Cropper(image: ImageBitmap, w: Int, h: Int, crop: Crop, set: (Crop) -> Unit) {
    val c = Tin.c
    val cur by rememberUpdatedState(crop)
    fun move(dx: Float, dy: Float) = set(clampCrop(cur.copy(ox = cur.ox + dx, oy = cur.oy + dy), w, h, VIEW))
    fun zoomTo(z: Float) = set(zoomAt(cur, min(4f, max(1f, z)), w, h, VIEW))
    val cropDesc = stringResource(R.string.profile_crop_title)
    var focused by remember { mutableStateOf(false) }
    Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Canvas(
            Modifier.size(VIEW.dp)
                .drawBehind {
                    // :focus-visible outline: 2 px accent-ink, 3 px outside, following the radius.
                    if (focused) { val o = 4.dp.toPx(); drawRoundRect(c.accentInk, Offset(-o, -o), Size(size.width + 2 * o, size.height + 2 * o),
                        androidx.compose.ui.geometry.CornerRadius(18.dp.toPx()), style = Stroke(2.dp.toPx())) }
                }
                .clip(RoundedCornerShape(14.dp)).background(c.sf2)
                .onFocusChanged { focused = it.isFocused }
                .focusable()
                .onKeyEvent { e ->
                    if (e.type != KeyEventType.KeyDown) return@onKeyEvent false
                    val d = if (e.isShiftPressed) 40f else 10f
                    when (e.key) {
                        Key.DirectionLeft -> move(d, 0f); Key.DirectionRight -> move(-d, 0f)
                        Key.DirectionUp -> move(0f, d); Key.DirectionDown -> move(0f, -d)
                        Key.Plus, Key.Equals, Key.NumPadAdd -> zoomTo(cur.zoom + 0.1f)
                        Key.Minus, Key.NumPadSubtract -> zoomTo(cur.zoom - 0.1f)
                        Key.MoveHome -> set(centred(w, h, VIEW, cur.zoom))
                        else -> return@onKeyEvent false
                    }
                    true
                }
                .pointerInput(w, h) { detectDragGestures { ch, d -> ch.consume(); move(d.x / density, d.y / density) } }
                .semantics { contentDescription = cropDesc },
        ) {
            val k = density
            val s = coverScale(w, h, VIEW, cur.zoom)
            drawImage(image, srcOffset = IntOffset.Zero, srcSize = IntSize(w, h),
                dstOffset = IntOffset((cur.ox * k).roundToInt(), (cur.oy * k).roundToInt()), dstSize = IntSize((w * s * k).roundToInt(), (h * s * k).roundToInt()))
            // The round window: everything outside it dimmed, a thin light edge where the circle ends.
            val r = size.minDimension / 2
            val dim = Path().apply { fillType = PathFillType.EvenOdd; addRect(androidx.compose.ui.geometry.Rect(Offset.Zero, size)); addOval(androidx.compose.ui.geometry.Rect(center, r)) }
            drawPath(dim, PgScrim)
            val ring = 1.5f * k
            drawCircle(Color.White.copy(alpha = .85f), radius = r - ring / 2, style = Stroke(ring))
        }
        Text(stringResource(R.string.profile_crop_hint), style = PgType.sub19, color = c.ink2, textAlign = TextAlign.Center)
        Row(Modifier.widthIn(max = 260.dp).fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            ZoomGlyph(16)
            RangeInput(cur.zoom, 1f, 4f, { zoomTo(it) }, stringResource(R.string.profile_crop_zoom), Modifier.weight(1f))
            ZoomGlyph(22)
        }
    }
}

@Composable
private fun ZoomGlyph(px: Int) {
    val col = Tin.c.ink2
    Canvas(Modifier.size(px.dp)) {
        val u = size.width / 24f
        drawRoundRect(col, Offset(3 * u, 4 * u), Size(18 * u, 16 * u), androidx.compose.ui.geometry.CornerRadius(3 * u), style = Stroke(1.8f * u))
    }
}

/** A round preview at [px]: the same crop drawn at that size, with the avatar's hairline. */
@Composable
private fun CropPreview(image: ImageBitmap, w: Int, h: Int, crop: Crop, px: Int) {
    val c = Tin.c
    Canvas(Modifier.size(px.dp).clip(CircleShape).background(c.sf2)) {
        val r = px / VIEW * density
        val s = coverScale(w, h, VIEW, crop.zoom)
        drawImage(image, srcOffset = IntOffset.Zero, srcSize = IntSize(w, h),
            dstOffset = IntOffset((crop.ox * r).roundToInt(), (crop.oy * r).roundToInt()), dstSize = IntSize((w * s * r).roundToInt(), (h * s * r).roundToInt()))
        val line = 1f * density
        drawCircle(if (c.dark) Color.White.copy(alpha = .12f) else Color.Black.copy(alpha = .1f), radius = size.minDimension / 2 - line / 2, style = Stroke(line))
    }
}


/**
 * `<input type="range">` as Chromium draws it with `accent-color`: an 8 dp track (1 px #b2b2b2 edge on #efefef, fully round) filled
 * with the accent up to the thumb, and a 13 dp accent thumb; 44 dp tall to touch. Drag or tap to set; arrow keys step by .01.
 */
@Composable
fun RangeInput(value: Float, min: Float, max: Float, onChange: (Float) -> Unit, label: String, modifier: Modifier = Modifier) {
    val accent = Tin.c.accent
    val cur by rememberUpdatedState(value)
    val rtl = androidx.compose.ui.platform.LocalLayoutDirection.current == androidx.compose.ui.unit.LayoutDirection.Rtl
    fun at(x: Float, w: Float, r: Float): Float {
        val t = ((x - r) / (w - 2 * r)).coerceIn(0f, 1f)
        return min + (max - min) * (if (rtl) 1 - t else t)
    }
    Canvas(
        modifier.height(44.dp)
            .pointerInput(min, max) { detectTapGestures { o -> onChange(at(o.x, size.width.toFloat(), 6.5f * density)) } }
            .pointerInput(min, max) { detectHorizontalDragGestures { ch, _ -> ch.consume(); onChange(at(ch.position.x, size.width.toFloat(), 6.5f * density)) } }
            .semantics {
                contentDescription = label
                progressBarRangeInfo = ProgressBarRangeInfo(cur, min..max)
                setProgress { v -> onChange(v.coerceIn(min, max)); true }
            }
            .focusable()
            .onKeyEvent { e ->
                if (e.type != KeyEventType.KeyDown) return@onKeyEvent false
                when (e.key) {
                    Key.DirectionRight, Key.DirectionUp -> { onChange((cur + .01f).coerceAtMost(max)); true }
                    Key.DirectionLeft, Key.DirectionDown -> { onChange((cur - .01f).coerceAtLeast(min)); true }
                    else -> false
                }
            },
    ) {
        val r = 6.5f * density; val th = 8f * density; val y = size.height / 2
        val t = ((cur - min) / (max - min)).coerceIn(0f, 1f).let { if (rtl) 1 - it else it }
        val cx = r + (size.width - 2 * r) * t
        val corner = androidx.compose.ui.geometry.CornerRadius(th / 2)
        drawRoundRect(Color(0xFFEFEFEF), Offset(0f, y - th / 2), Size(size.width, th), corner)
        val fillW = if (rtl) size.width - cx else cx
        if (fillW > 0) drawRoundRect(accent, Offset(if (rtl) cx else 0f, y - th / 2), Size(fillW, th), corner)
        drawRoundRect(Color(0xFFB2B2B2), Offset(.5f * density, y - th / 2 + .5f * density), Size(size.width - density, th - density), corner, style = Stroke(density))
        drawCircle(accent, r, Offset(cx, y))
    }
}
