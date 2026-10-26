package com.osvauld.p2p

import androidx.compose.ui.res.stringResource
import android.content.Context
import androidx.activity.compose.BackHandler
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.filter
import kotlinx.coroutines.withContext
import uniffi.p2pcore.HitRange
import uniffi.p2pcore.MessageHit
import uniffi.p2pcore.NameHit
import uniffi.p2pcore.SearchConvKind
import uniffi.p2pcore.SearchNameKind

/*
 * Search (screens/search/, docs/search.md): Android opens a full-screen search from the dock's pill, with the field at the
 * BOTTOM (thumb reach). Results: "People & groups" (one fixed-height strip), then "Messages" (fixed 64 dp rows, newest
 * first, 20 a page). Rows are keyed by position, so a new query changes the rows' text instead of pushing them around;
 * hints are overlays in reserved space. Names from 1 character, messages from 2; 150 ms debounce. Wired to Node.search.
 */

object SearchConst {
    const val LIMIT = 20
    const val MIN_CHARS = 2
    const val DEBOUNCE_MS = 150L
    /** Characters kept before the first match in a one-line snippet (Marked.svelte `lead`). */
    const val LEAD = 18
}

/**
 * A message hit the user opened: the CHAT area (DM: chat_day(conv, day), find `id`) and the GROUPS area
 * (group_messages(conv, before = "{at}:{id}~")) read it, scroll to the message, highlight it and clear it.
 */
object SearchJump {
    val pending = MutableStateFlow<MessageHit?>(null)
}

/** Recents live only for this search screen; never persist message queries in plaintext preferences. */

private fun cps(s: String) = s.codePointCount(0, s.length)

/** Core ranges are in code points; Compose wants UTF-16 offsets. */
private fun utf16(s: String, cp: Int): Int = runCatching { s.offsetByCodePoints(0, cp.coerceIn(0, cps(s))) }.getOrDefault(s.length)

/** Text with bold hits (Marked.svelte, variant bold). [lead] keeps at most that many characters before the first hit. */
fun marked(text: String, ranges: List<IntRange>, ink: Color, lead: Int = 0): AnnotatedString {
    var t = text
    var rs = ranges.filter { it.first < it.last + 1 }.sortedBy { it.first }
    if (lead > 0 && rs.isNotEmpty() && rs[0].first > 0) {
        val headCp = cps(t.substring(0, rs[0].first))
        if (headCp > lead) {
            var cut = t.offsetByCodePoints(rs[0].first, -lead)
            val sp = t.substring(cut, rs[0].first).indexOfFirst { it.isWhitespace() }
            if (sp >= 0 && sp < lead - 4) cut += sp + 1
            val shift = cut - 1
            t = "…" + t.substring(cut)
            rs = rs.map { (it.first - shift)..(it.last - shift) }
        }
    }
    return buildAnnotatedString {
        var p = 0
        for (r in rs) {
            val a = r.first.coerceIn(p, t.length); val b = (r.last + 1).coerceIn(a, t.length)
            append(t.substring(p, a))
            withStyle(SpanStyle(fontWeight = FontWeight.Bold, color = ink)) { append(t.substring(a, b)) }
            p = b
        }
        if (p < t.length) append(t.substring(p))
    }
}

internal fun hitRanges(text: String, hs: List<HitRange>): List<IntRange> = hs.map { utf16(text, it.start.toInt()) until utf16(text, it.end.toInt()) }

/** Names: the core matches folded substrings; mark the same stretch here (case-insensitive). */
private fun nameRanges(name: String, q: String): List<IntRange> {
    val i = name.indexOf(q.trim(), ignoreCase = true)
    return if (i < 0 || q.isBlank()) emptyList() else listOf(i until i + q.trim().length)
}

@Composable
fun SearchScreen(app: P2pApp, onBack: () -> Unit, onChat: (String) -> Unit, onGroup: (String) -> Unit, onContact: (String) -> Unit) {
    val c = Tin.c
    val ctx = LocalContext.current
    var q by rememberSaveable { mutableStateOf("") }
    var applied by remember { mutableStateOf(q) }
    var recents by remember { mutableStateOf<List<String>>(emptyList()) }
    LaunchedEffect(q) { if (q.isBlank()) applied = "" else { delay(SearchConst.DEBOUNCE_MS); applied = q } }

    var names by remember { mutableStateOf<List<NameHit>>(emptyList()) }
    var msgs by remember { mutableStateOf<List<MessageHit>>(emptyList()) }
    var next by remember { mutableStateOf<String?>(null) }
    var doneFor by remember { mutableStateOf<String?>(null) }
    var paging by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val chats by app.chat.chats.collectAsState()
    val groups by app.groups.list.collectAsState()
    val t = applied.trim()
    val msgsOk = cps(t) >= SearchConst.MIN_CHARS
    LaunchedEffect(t, chats, groups) {
        error = null
        if (t.isEmpty()) { names = emptyList(); msgs = emptyList(); next = null; doneFor = t; return@LaunchedEffect }
        val ok = cps(t) >= SearchConst.MIN_CHARS
        val result = withContext(Dispatchers.IO) { runCatching { app.node.search(t, if (ok) SearchConst.LIMIT.toUInt() else 1u, null) } }
        error = result.exceptionOrNull()?.let(::friendly)
        val r = result.getOrNull()
        names = r?.names ?: emptyList()
        msgs = if (ok) r?.messages ?: emptyList() else emptyList()
        next = if (ok) r?.next else null
        doneFor = t
    }
    val listState = rememberLazyListState()
    LaunchedEffect(t) { listState.scrollToItem(0) }
    // Next page when the end of the list is near.
    LaunchedEffect(listState, t) {
        snapshotFlow { Triple(listState.layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: 0, msgs.size, next) }
            .distinctUntilChanged().filter { it.first >= it.second - 4 }.collect { (_, _, cur) ->
                if (cur == null || paging || doneFor != t || error != null) return@collect
                paging = true
                try {
                    val result = withContext(Dispatchers.IO) { runCatching { app.node.search(t, SearchConst.LIMIT.toUInt(), cur) } }
                    error = result.exceptionOrNull()?.let(::friendly)
                    result.getOrNull()?.let { r ->
                        if (doneFor == t) { msgs = (msgs + r.messages).distinctBy { it.kind to (it.conv to it.id) }; next = r.next }
                    }
                } finally { paging = false }
            }
    }

    fun keepRecent() {
        val s = q.trim()
        if (cps(s) < SearchConst.MIN_CHARS) return
        recents = (listOf(s) + recents.filter { !it.equals(s, ignoreCase = true) }).take(8)
    }
    val me = remember { app.currentDid() }
    fun openName(n: NameHit) {
        keepRecent()
        when (n.kind) {
            SearchNameKind.ME -> onChat(n.id)
            SearchNameKind.GROUP -> onGroup(n.id)
            SearchNameKind.CONTACT -> onChat(n.id)
        }
    }
    fun openHit(h: MessageHit) {
        keepRecent()
        SearchJump.pending.value = h
        if (h.kind == SearchConvKind.GROUP) onGroup(h.conv) else onChat(h.conv)
    }

    val focus = remember { FocusRequester() }
    val fm = LocalFocusManager.current
    LaunchedEffect(Unit) { runCatching { focus.requestFocus() } }
    BackHandler { fm.clearFocus(); onBack() }

    Column(Modifier.fillMaxSize().background(c.bg).imePadding()) {
        Box(Modifier.weight(1f).fillMaxWidth().statusBarsPadding().padding(top = 12.dp)) {
            if (q.isBlank()) RecentResults(recents, onPick = { q = it; applied = it }, onClear = { recents = emptyList() })
            else Column(Modifier.fillMaxSize()) {
                ResultsHeader(stringResource(R.string.search_people_groups))
                Box(Modifier.fillMaxWidth().height(100.dp)) {
                    LazyRow(Modifier.fillMaxSize(), contentPadding = PaddingValues(start = 8.dp, end = 8.dp, bottom = 8.dp), horizontalArrangement = Arrangement.spacedBy(2.dp)) {
                        items(names.size, key = { it }) { i -> NameTile(names[i], t, me) { openName(names[i]) } }
                    }
                    val none by animateFloatAsState(if (error == null && doneFor == t && t.isNotEmpty() && names.isEmpty()) 1f else 0f, tween(Motion.CROSSFADE), label = "none")
                    Text(stringResource(R.string.search_no_people), Modifier.align(Alignment.Center).alpha(none), style = TinFs.sub, color = c.ink2)
                }
                ResultsHeader(stringResource(R.string.search_messages))
                Box(Modifier.weight(1f).fillMaxWidth()) {
                    LazyColumn(Modifier.fillMaxSize(), state = listState, contentPadding = PaddingValues(start = 8.dp, end = 8.dp, bottom = 12.dp)) {
                        items(msgs.size, key = { it }) { i -> MessageRow(msgs[i], me) { openHit(msgs[i]) } }
                    }
                    val hint = when {
                        error != null -> error!!
                        t.isEmpty() -> ""
                        !msgsOk -> stringResource(R.string.search_type_more)
                        doneFor == t && msgs.isEmpty() -> stringResource(R.string.search_no_messages, t)
                        else -> ""
                    }
                    var shownHint by remember { mutableStateOf(hint) }
                    LaunchedEffect(hint) { if (hint.isNotEmpty()) shownHint = hint }
                    val ha by animateFloatAsState(if (hint.isNotEmpty()) 1f else 0f, tween(Motion.CROSSFADE), label = "hint")
                    Text(shownHint, Modifier.align(Alignment.Center).padding(horizontal = 32.dp).alpha(ha), style = TinFs.sub.copy(lineHeight = 19.sp), color = c.ink2, textAlign = TextAlign.Center)
                }
            }
        }
        // The field at the bottom: Back + the 48 dp field, on the surface, a hairline above.
        Column(Modifier.fillMaxWidth().background(c.sf)) {
            Box(Modifier.fillMaxWidth().height(1.dp).background(c.hairline))
            Row(Modifier.fillMaxWidth().navigationBarsPadding().padding(start = 4.dp, end = 12.dp, top = 8.dp, bottom = 8.dp),
                verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                PgIconButton(PgIcons.Back, stringResource(R.string.common_back), { fm.clearFocus(); onBack() })
                SearchField(q, { q = it }, Modifier.weight(1f), focus, onSubmit = {
                    if (names.isNotEmpty()) openName(names[0]) else if (msgs.isNotEmpty()) openHit(msgs[0]) else keepRecent()
                })
            }
        }
    }
}

/** SearchField.svelte: 48 tall pill on --surface-2; focused = --surface with a 2 px --accent ring. The clear button stays in the layout and fades. */
@Composable
fun SearchField(value: String, onChange: (String) -> Unit, modifier: Modifier, focus: FocusRequester, height: Int = 48, onSubmit: () -> Unit = {}) {
    val c = Tin.c
    var focused by remember { mutableStateOf(false) }
    val bg by animateColorAsState(if (focused) c.sf else c.sf2, tween(Motion.SWITCH, easing = Motion.EaseOut), label = "fbg")
    val ring by animateColorAsState(if (focused) c.accent else Color.Transparent, tween(Motion.SWITCH, easing = Motion.EaseOut), label = "fring")
    val shape = RoundedCornerShape((height / 2).dp)
    val searchLabel = stringResource(R.string.common_search)
    Row(modifier.height(height.dp).clip(shape).background(bg).border(2.dp, ring, shape), verticalAlignment = Alignment.CenterVertically) {
        Box(Modifier.width(44.dp), contentAlignment = Alignment.Center) { Icon(PgIcons.Search, null, tint = c.ink2, modifier = Modifier.size(20.dp)) }
        Box(Modifier.weight(1f), contentAlignment = Alignment.CenterStart) {
            if (value.isEmpty()) Text(searchLabel, style = TinFs.body, color = c.ink2, maxLines = 1)
            BasicTextField(
                value, onChange, Modifier.fillMaxWidth().focusRequester(focus).onFocusChanged { focused = it.isFocused }.semantics { contentDescription = searchLabel },
                singleLine = true, textStyle = TinFs.body.copy(color = c.ink), cursorBrush = SolidColor(c.accentInk),
                keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.None, autoCorrectEnabled = false, imeAction = ImeAction.Search),
                keyboardActions = KeyboardActions(onSearch = { onSubmit() }),
            )
        }
        val a by animateFloatAsState(if (value.isNotEmpty()) 1f else 0f, tween(Motion.CROSSFADE, easing = Motion.EaseOut), label = "clr")
        Box(Modifier.padding(start = 2.dp, end = 4.dp).size(44.dp).alpha(a)) {
            if (value.isNotEmpty()) PgIconButton(PgIcons.X, stringResource(R.string.search_clear_field), { onChange(""); runCatching { focus.requestFocus() } }, tint = c.ink2, size = 44.dp, iconSize = 18.dp)
        }
    }
}

@Composable
private fun ResultsHeader(title: String, action: (@Composable () -> Unit)? = null) {
    Row(Modifier.fillMaxWidth().height(36.dp).padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
        ListSection(title, Modifier.weight(1f))
        action?.invoke()
    }
}

@Composable
private fun RecentResults(recents: List<String>, onPick: (String) -> Unit, onClear: () -> Unit) {
    val c = Tin.c
    Box(Modifier.fillMaxSize()) {
        Column(Modifier.fillMaxSize()) {
            ResultsHeader(stringResource(R.string.search_recent)) {
                if (recents.isNotEmpty()) Text(stringResource(R.string.search_clear), Modifier.pressable(onClear, RoundedCornerShape(8.dp), wash = Color.Transparent).padding(horizontal = 10.dp, vertical = 12.dp),
                    style = TinFs.sub.copy(fontWeight = FontWeight(650)), color = c.accentInk)
            }
            Column(Modifier.padding(horizontal = 8.dp)) {
                recents.forEach { r ->
                    Row(Modifier.fillMaxWidth().heightIn(min = 48.dp).pressable({ onPick(r) }, RoundedCornerShape(14.dp)).padding(horizontal = 8.dp),
                        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                        Box(Modifier.width(32.dp), contentAlignment = Alignment.Center) { Icon(PgIcons.Clock, null, tint = c.ink2, modifier = Modifier.size(18.dp)) }
                        Text(r, style = TinFs.body, color = c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                }
            }
        }
        val a by animateFloatAsState(if (recents.isEmpty()) 1f else 0f, tween(Motion.CROSSFADE), label = "blank")
        Column(Modifier.align(Alignment.Center).padding(horizontal = 32.dp).alpha(a), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(stringResource(R.string.search_blank_1), style = TinFs.sub.copy(lineHeight = 19.sp), color = c.ink2, textAlign = TextAlign.Center)
            Text(stringResource(R.string.search_blank_2), style = TinFs.sub.copy(lineHeight = 19.sp), color = c.ink2, textAlign = TextAlign.Center)
        }
    }
}

/** A tile in the People & groups strip: 78 x 92, avatar 48, the name in 12/15 over two lines with the match bold. */
@Composable
private fun NameTile(n: NameHit, q: String, me: String?, onClick: () -> Unit) {
    val c = Tin.c
    val what = stringResource(when (n.kind) { SearchNameKind.GROUP -> R.string.search_kind_group; SearchNameKind.ME -> R.string.search_kind_me; SearchNameKind.CONTACT -> R.string.search_kind_contact })
    val tileDesc = stringResource(R.string.search_tile_desc, n.name, what)
    Column(
        Modifier.size(78.dp, 92.dp).pressable(onClick, RoundedCornerShape(14.dp)).semantics(mergeDescendants = true) { contentDescription = tileDesc }
            .padding(start = 4.dp, end = 4.dp, top = 6.dp),
        horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        if (n.kind == SearchNameKind.GROUP) ListGroupAvatar(48.dp, gid = n.id) else Avatar(n.name, n.id, 48.dp)
        Text(marked(n.name, nameRanges(n.name, q), c.ink), style = TinFs.sub.copy(fontSize = 12.sp, lineHeight = 15.sp), color = c.ink,
            textAlign = TextAlign.Center, maxLines = 2, overflow = TextOverflow.Ellipsis)
    }
}

/** A message hit: 64 dp fixed; avatar 44; name 600 (20) and time 11 (min 56, end); a one-line snippet with the match kept in view. */
@Composable
private fun MessageRow(h: MessageHit, me: String?, onClick: () -> Unit) {
    val c = Tin.c
    val mine = h.author == me
    val you = stringResource(R.string.common_you)
    val someone = stringResource(R.string.common_someone)
    val by = if (h.kind == SearchConvKind.GROUP) stringResource(R.string.search_author_prefix, if (mine) you else firstName(h.authorName.ifBlank { someone })) + " " else if (mine) stringResource(R.string.search_author_prefix, you) + " " else ""
    val snip = h.snippet.replace('\n', ' ')
    Row(
        Modifier.fillMaxWidth().height(64.dp).pressable(onClick, RoundedCornerShape(14.dp)).padding(horizontal = 8.dp, vertical = 11.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        if (h.kind == SearchConvKind.GROUP) ListGroupAvatar(44.dp, gid = h.conv) else Avatar(h.convName, h.conv, 44.dp)
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(h.convName, Modifier.weight(1f), style = TinFs.body.copy(fontWeight = FontWeight.SemiBold, lineHeight = 20.sp), color = c.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(listTime(h.at.toLong(), System.currentTimeMillis()), Modifier.widthIn(min = 56.dp), style = TinFs.meta, color = c.ink2, textAlign = TextAlign.End)
            }
            Text(buildAnnotatedString { append(by); append(marked(snip, hitRanges(snip, h.highlights), c.ink, SearchConst.LEAD)) },
                style = TinFs.sub, color = c.ink2, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
    }
}
