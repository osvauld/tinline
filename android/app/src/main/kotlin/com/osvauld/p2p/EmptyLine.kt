package com.osvauld.p2p

import androidx.compose.foundation.layout.*
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
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
        EmptyCopy("Your line is ready", "Your request to ${firstName(sent[0].name)} is waiting. Your line opens when they accept.", "Add another contact")
    contacts.isEmpty() -> EmptyCopy("Your line is ready", "Add someone you’d like to talk to, and you can message and call each other.", "Add my first contact")
    contacts.size == 1 -> contacts[0].display().trim().split(Regex("\\s+")).first().let { f -> EmptyCopy("Your line is open", "Say hello to $f whenever you like.", "Message $f") }
    else -> EmptyCopy("Your line is open", "Pick someone and say hello whenever you like.", "Start a chat")
}

object EmptyQuiet {
    const val CHATS = "Your chats show up here."
    const val UNREAD = "You’re all caught up."
    const val CALLS = "Calls you make and receive show up here."
    const val CONTACTS = "The people you add show up here."
    const val GROUPS_PHONE = "When someone invites you to a group, it shows up here."
}

@Composable
fun EmptyHero(contacts: List<Contact>, onAdd: () -> Unit, onMessage: (String) -> Unit, onNewChat: () -> Unit, modifier: Modifier = Modifier,
              sent: List<ContactRequest> = emptyList()) {
    val c = Tin.c
    val copy = emptyHeroCopy(contacts, sent)
    Column(modifier.fillMaxWidth().heightIn(min = 360.dp).padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.Center) {
        TinMark(88.dp, state = if (contacts.isEmpty()) MarkState.Slack else MarkState.Taut, live = true, lineColor = true, spring = MarkSpring.Line,
            description = "Tinline", modifier = Modifier.padding(bottom = 18.dp))
        Text(copy.title, Modifier.padding(bottom = 6.dp), style = PgType.title.copy(fontSize = 17.sp, lineHeight = 26.sp, fontWeight = FontWeight.SemiBold), color = c.ink, maxLines = 1)
        Text(copy.line, Modifier.widthIn(max = 300.dp).heightIn(min = 38.dp).padding(bottom = 0.dp), style = PgType.sub19, color = c.ink2, textAlign = TextAlign.Center)
        Spacer(Modifier.height(20.dp))
        PBtn(copy.act, {
            when (contacts.size) { 0 -> onAdd(); 1 -> onMessage(contacts[0].did); else -> onNewChat() }
        }, variant = PVariant.Primary, minWidth = 220.dp)
    }
}

@Composable
fun EmptyQuietLine(text: String, modifier: Modifier = Modifier) {
    val c = Tin.c
    Column(modifier.fillMaxWidth().padding(top = 40.dp, start = 24.dp, end = 24.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(10.dp)) {
        TinMark(40.dp, dim = true)
        Text(text, Modifier.widthIn(max = 300.dp), style = PgType.sub19, color = c.ink2, textAlign = TextAlign.Center)
    }
}
