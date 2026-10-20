package com.osvauld.p2p

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Locale

/*
 * Contact blocking on the core's real API (Node.block / unblock / blockState / blockedContacts /
 * notDeliveredReason; crates/core/src/block.rs). Decisions (rotation-open-decisions.md §0):
 * blocking is visible to the blocked person (U5 revised); a contact's action is Block, there is no
 * "Remove contact" (U19); every block asks the other app to delete its contact entry for us, so after
 * an unblock they must add us again (U20); chats are never deleted (U24). Groups are not touched.
 *
 * The model below is plain Kotlin (no uniffi types) so the JVM unit tests can run it; [P2pApp]
 * fills it from the Node.
 */

/** One conversation's block state. Times are unix seconds. */
data class BlockView(val blockedByMe: Boolean = false, val blockedAtSecs: Long? = null, val blockedMeAtSecs: Long? = null)

/** A row of Settings › Blocked people. */
data class BlockedRow(val did: String, val name: String, val blockedAtSecs: Long)

/** What sits where the composer goes. */
enum class ComposerState {
    Open,
    /** We block them: "You blocked X" and Unblock. */
    BlockedByMe,
    /** They block us: no composer, just why. */
    BlockedMe,
    /** A kept chat with someone who is no longer a contact (their block deleted our entry, then they unblocked): add them again. */
    NotContact,
}

/** Why the call button does or doesn't dial. */
enum class CallGate { Ok, Closed, NotAllowed }

data class Blocks(
    /** Only conversations with a block on either side. */
    val states: Map<String, BlockView> = emptyMap(),
    /** Newest first; one list across all our devices. */
    val list: List<BlockedRow> = emptyList(),
    /** Contacts we let call us ("Allow calls" on; P6 Node.callsAllowed). */
    val allowed: Set<String> = emptySet(),
    /** Contacts whose call token we hold: they let us call them (P6 Node.canCall). */
    val tokens: Set<String> = emptySet(),
) {
    fun of(did: String): BlockView = states[did] ?: list.firstOrNull { it.did == did }?.let { BlockView(true, it.blockedAtSecs) } ?: BlockView()
    fun blockedByMe(did: String) = of(did).blockedByMe
    fun blockedMeAt(did: String): Long? = of(did).blockedMeAtSecs

    /** [contact]: they are in our contacts (or it is the "You" conversation). */
    fun composer(did: String, contact: Boolean): ComposerState = when {
        blockedByMe(did) -> ComposerState.BlockedByMe
        blockedMeAt(did) != null -> ComposerState.BlockedMe
        !contact -> ComposerState.NotContact
        else -> ComposerState.Open
    }

    /** Closed while either side blocks or they are not a contact; NotAllowed when they let us message, not call. */
    fun callGate(did: String, contact: Boolean): CallGate = when {
        composer(did, contact) != ComposerState.Open -> CallGate.Closed
        did !in tokens -> CallGate.NotAllowed
        else -> CallGate.Ok
    }

    fun canCall(did: String, contact: Boolean) = callGate(did, contact) == CallGate.Ok

    /** The "Allow calls" switch: whether we let them call us. */
    fun callsAllowed(did: String) = did in allowed

    /** The red line under the name in lists and headers. */
    fun status(did: String): String? = when (composer(did, true)) {
        ComposerState.BlockedByMe -> "Blocked"
        ComposerState.BlockedMe -> "Blocked you"
        else -> null
    }

    companion object {
        /** Builds the state from per-DID reads; DIDs with no block on either side are dropped. */
        fun from(views: Map<String, BlockView>, list: List<BlockedRow>, allowed: Set<String> = emptySet(), tokens: Set<String> = emptySet()) =
            Blocks(views.filterValues { it.blockedByMe || it.blockedMeAtSecs != null }, list.sortedByDescending { it.blockedAtSecs }, allowed, tokens)
    }
}

/** What a conversation needs to know about blocking. */
data class ConvBlock(val view: BlockView, val composer: ComposerState, val callGate: CallGate = CallGate.Ok)

/** The words of blocking are `uniffi.present` (block*, cantSendText, notContactLine, unblockNote...). */
object BlockCopy {
    /** Why the call button is closed ([ComposerState] mapped to the shared words). */
    fun cantCall(name: String, state: ComposerState) = uniffi.present.cantCallText(name, when (state) {
        ComposerState.Open -> uniffi.present.ComposerState.OPEN
        ComposerState.BlockedByMe -> uniffi.present.ComposerState.BLOCKED_BY_ME
        ComposerState.BlockedMe -> uniffi.present.ComposerState.BLOCKED_ME
        ComposerState.NotContact -> uniffi.present.ComposerState.NOT_CONTACT
    })

    fun date(secs: Long, zone: ZoneId = ZoneId.systemDefault()): String =
        DateTimeFormatter.ofPattern("MMM d, HH:mm", Locale.ENGLISH).format(Instant.ofEpochSecond(secs).atZone(zone))
}

/** "Block X?" with every consequence listed before the tap. */
@Composable
fun BlockDialog(name: String, onDismiss: () -> Unit, onBlock: () -> Unit) {
    TinDialog(uniffi.present.blockDialogTitle(name), onDismiss, "Block", { onDismiss(); onBlock() }, destructive = true, icon = PgIcons.Off) {
        uniffi.present.blockDialogLines(name).forEach { DialogText("• $it") }
    }
}

/** Settings › Blocked people: everyone we block (real data), with Unblock. */
@Composable
fun BlockedPeopleScreen(app: P2pApp, onBack: () -> Unit, say: (String) -> Unit) {
    val c = Tin.c
    val blocks by app.blocks.collectAsState()
    Page {
        TopBar("Blocked people", onBack)
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState())) {
            Hint("One list across all your devices: a block made on another device shows here too.", Modifier.padding(horizontal = 20.dp, vertical = 8.dp))
            if (blocks.list.isEmpty()) Hint("Nobody is blocked. To block someone, open their contact page and tap Block.", Modifier.padding(horizontal = 20.dp, vertical = 12.dp))
            blocks.list.forEach { b ->
                val name = b.name.ifBlank { "Someone" }
                Row(Modifier.fillMaxWidth().heightIn(min = 72.dp).padding(start = 20.dp, end = 8.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp)) {
                    Avatar(name, b.did, 44.dp)
                    Column(Modifier.weight(1f)) {
                        Text(name, style = TinType.bodyL.copy(fontWeight = FontWeight.SemiBold), color = c.ink)
                        Hint("Blocked ${BlockCopy.date(b.blockedAtSecs)}")
                    }
                    Box(Modifier.heightIn(min = 48.dp).clip(RoundedCornerShape(50)).clickable(role = Role.Button, onClickLabel = "Unblock $name") {
                        app.unblock(b.did); say("${firstName(name)} is told you unblocked them")
                    }.padding(horizontal = 12.dp), contentAlignment = Alignment.Center) {
                        Text("Unblock", style = TinType.button.copy(fontSize = 15.sp), color = c.pr)
                    }
                }
            }
            Column(Modifier.padding(20.dp).fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("Unblocking", style = TinType.bodyL.copy(fontSize = 15.sp, fontWeight = FontWeight.Bold), color = c.ink)
                Text(uniffi.present.unblockNote(), style = TinType.bodyM, color = c.ink2)
            }
        }
    }
}
