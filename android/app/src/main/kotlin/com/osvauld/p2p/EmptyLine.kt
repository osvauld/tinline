package com.osvauld.p2p

import androidx.compose.foundation.layout.*
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.annotation.StringRes
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import uniffi.p2pcore.Contact
import uniffi.p2pcore.ContactRequest

/*
 * The one empty state (lib/ui/EmptyLine.svelte; SIGNUP-HANDOFF.md § 6e), in two sizes. All its copy is decided here.
 * Hero: a new account's first screen (Chats and Calls). The line is slack while you have nobody; when your first contact arrives
 * it pulls tight in two tugs (MarkSpring.Line, the same pull as sign-up Done) and the words turn from "ready" to "open". The hero
 * plays no cue: adding the contact already did. Fixed slots (one-line title, two reserved lines, a 220 min button): nothing moves.
 * Quiet: an empty list in an app you already use: a small dimmed mark and one muted line, no button.
 */

data class EmptyCopy(val title: String, val line: String, val act: String)

/** The one function that decides the hero's copy from who you have (and the requests you sent, waiting for an answer). */
fun emptyHeroCopy(contacts: List<Contact>, sent: List<ContactRequest> = emptyList()): EmptyCopy = when {
    contacts.isEmpty() && sent.isNotEmpty() ->
        EmptyCopy(appString(R.string.empty_hero_ready), appString(R.string.empty_hero_request_waiting, firstName(sent[0].name)), appString(R.string.empty_hero_add_another))
    contacts.isEmpty() -> EmptyCopy(appString(R.string.empty_hero_ready), appString(R.string.empty_hero_add_someone), appString(R.string.empty_hero_add_first))
    contacts.size == 1 -> contacts[0].display().trim().split(Regex("\\s+")).first().let { f -> EmptyCopy(appString(R.string.empty_hero_open), appString(R.string.empty_hero_say_hello, f), appString(R.string.empty_hero_message, f)) }
    else -> EmptyCopy(appString(R.string.empty_hero_open), appString(R.string.empty_hero_pick_someone), appString(R.string.empty_hero_start_chat))
}

object EmptyQuiet {
    val CHATS = R.string.empty_quiet_chats
    val UNREAD = R.string.empty_quiet_unread
    val CALLS = R.string.empty_quiet_calls
    val CONTACTS = R.string.empty_quiet_contacts
}

@Composable
fun EmptyHero(contacts: List<Contact>, onAdd: () -> Unit, onMessage: (String) -> Unit, onNewChat: () -> Unit, modifier: Modifier = Modifier,
              sent: List<ContactRequest> = emptyList()) {
    val c = Tin.c
    val copy = emptyHeroCopy(contacts, sent)
    Column(modifier.fillMaxWidth().heightIn(min = 360.dp).padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.Center) {
        TinMark(88.dp, state = if (contacts.isEmpty()) MarkState.Slack else MarkState.Taut, live = true, lineColor = true, spring = MarkSpring.Line,
            description = stringResource(R.string.app_name), modifier = Modifier.padding(bottom = 18.dp))
        Text(copy.title, Modifier.padding(bottom = 6.dp), style = PgType.title.copy(fontSize = 17.sp, lineHeight = 26.sp, fontWeight = FontWeight.SemiBold), color = c.ink, maxLines = 1)
        Text(copy.line, Modifier.widthIn(max = 300.dp).heightIn(min = 38.dp).padding(bottom = 0.dp), style = PgType.sub19, color = c.ink2, textAlign = TextAlign.Center)
        Spacer(Modifier.height(20.dp))
        PBtn(copy.act, {
            when (contacts.size) { 0 -> onAdd(); 1 -> onMessage(contacts[0].did); else -> onNewChat() }
        }, variant = PVariant.Primary, minWidth = 220.dp)
    }
}

@Composable
fun EmptyQuietLine(@StringRes text: Int, modifier: Modifier = Modifier) {
    val c = Tin.c
    Column(modifier.fillMaxWidth().padding(top = 40.dp, start = 24.dp, end = 24.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(10.dp)) {
        TinMark(40.dp, dim = true)
        Text(stringResource(text), Modifier.widthIn(max = 300.dp), style = PgType.sub19, color = c.ink2, textAlign = TextAlign.Center)
    }
}
