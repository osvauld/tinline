package com.osvauld.p2p

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import uniffi.p2pcore.Contact

/**
 * Contact page: name (alias first), Call / Rename / Block, and this contact's call history.
 * U19: a contact's action is Block (kept contact, kept chat, Unblock available); there is no Remove.
 */
@Composable
fun ContactScreen(
    contact: Contact, onBack: () -> Unit, onCall: () -> Unit, onBlock: () -> Unit,
    history: List<RecentCall> = emptyList(), onRename: (String?) -> Unit = {}, onMessage: (() -> Unit)? = null,
    block: BlockView = BlockView(), onUnblock: () -> Unit = {},
    /** P6: whether they let us call (NotAllowed: the button is crossed out and says why), and our "Allow calls" switch. */
    callGate: CallGate = CallGate.Ok, callsAllowed: Boolean? = null, onAllowCalls: (Boolean) -> Unit = {},
) {
    val alias = contact.alias?.takeIf { it.isNotBlank() }
    val c = Tin.c
    var confirmBlock by remember { mutableStateOf(false) }
    val blockedMe = block.blockedMeAtSecs != null
    val closed = block.blockedByMe || blockedMe || callGate == CallGate.Closed
    val canCall = !closed && callGate == CallGate.Ok
    var renaming by remember { mutableStateOf(false) }
    val shown = contact.display()
    Page {
        TopBar(null, onBack)
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState())) {
            Column(Modifier.fillMaxWidth().padding(horizontal = 24.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)) {
                // Tap the photo to see it large.
                ViewablePgAvatar(shown, 0, 96.dp, person = contact.did)
                Text(shown, Modifier.padding(top = 6.dp), style = TinType.h1, color = c.ink, textAlign = TextAlign.Center)
                if ((alias != null && alias != contact.name) || block.blockedByMe || blockedMe) Row(
                    horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically,
                ) {
                    if (alias != null && alias != contact.name) Text(stringResource(R.string.contact_calls_themselves, contact.name), style = TinType.bodyM, color = c.ink2)
                    if (block.blockedByMe || blockedMe) Text(if (block.blockedByMe) stringResource(R.string.common_blocked) else stringResource(R.string.contact_blocked_you),
                        style = TinType.caption.copy(fontWeight = FontWeight.SemiBold), color = c.er)
                }
                TinButton(stringResource(R.string.common_call), onCall, Modifier.padding(top = 14.dp), icon = if (canCall) PgIcons.Phone else PgIcons.Callend, enabled = !closed)
                Row(Modifier.padding(top = 10.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    if (onMessage != null) ActionChip(PgIcons.Chat, stringResource(R.string.contact_message), c.pr, onMessage)
                    ActionChip(PgIcons.Edit, stringResource(R.string.contact_rename), c.pr) { renaming = true }
                    if (block.blockedByMe) ActionChip(PgIcons.Off, stringResource(R.string.contact_unblock), c.pr, onUnblock)
                    else ActionChip(PgIcons.Off, stringResource(R.string.contact_block), c.er) { confirmBlock = true }
                }
            }
            if (callsAllowed != null) {
                val f = firstName(shown)
                SectionLabel(stringResource(R.string.contact_what_they_can_do, f).uppercase(), Modifier.padding(top = 8.dp))
                ListItem(stringResource(R.string.contact_allow_calls), icon = PgIcons.Phone,
                    sub = when { block.blockedByMe -> stringResource(R.string.contact_calls_sub_blocked); callsAllowed -> stringResource(R.string.contact_calls_sub_on, f); else -> stringResource(R.string.contact_calls_sub_off, f) },
                    trailing = { TinSwitch(callsAllowed, { if (!block.blockedByMe) onAllowCalls(it) }, stringResource(R.string.contact_allow_calls)) })
            }
            if (history.isNotEmpty()) {
                SectionLabel(stringResource(R.string.contact_calls_with, shown).uppercase(), Modifier.padding(top = 8.dp))
                history.forEach { HistoryRow(it) }
                Hint(stringResource(R.string.contact_history_local), Modifier.padding(horizontal = 20.dp, vertical = 8.dp))
            }
        }
    }
    if (confirmBlock) BlockDialog(shown, onDismiss = { confirmBlock = false }, onBlock = onBlock)
    if (renaming) RenameSheet(alias ?: contact.name, contact.name, hasAlias = alias != null, onDismiss = { renaming = false }) { onRename(it); renaming = false }
}

@Composable
private fun ActionChip(icon: ImageVector, label: String, tint: androidx.compose.ui.graphics.Color, onClick: () -> Unit) {
    val c = Tin.c
    Column(
        Modifier.width(88.dp).clip(SmoothCornerShape(12.dp)).clickable(role = Role.Button, onClick = onClick),
        horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Box(Modifier.size(52.dp).clip(CircleShape).background(c.sf2), contentAlignment = Alignment.Center) { Icon(icon, null, tint = tint) }
        Text(label, style = TinType.caption.copy(fontSize = 13.sp), color = c.ink)
    }
}

@Composable
private fun HistoryRow(r: RecentCall) {
    val c = Tin.c
    val red = r.kind == RecentCall.Kind.Missed || r.kind == RecentCall.Kind.Unreached
    Row(Modifier.fillMaxWidth().heightIn(min = 56.dp).padding(horizontal = 20.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp)) {
        Icon(when (r.kind) { RecentCall.Kind.Incoming -> PgIcons.CallIn; RecentCall.Kind.Missed -> PgIcons.CallIn; else -> PgIcons.CallOut },
            null, tint = if (red) c.er else c.ink2, modifier = Modifier.size(20.dp))
        Column(Modifier.weight(1f)) {
            Text(r.title.ifBlank { stringResource(R.string.common_call) }, style = TinType.bodyL.copy(fontSize = 15.sp), color = if (red) c.er else c.ink)
            Text(r.whenLong, style = TinType.bodyM.copy(fontSize = 13.sp), color = c.ink2)
        }
        when (r.direct) {
            true -> Badge(stringResource(R.string.contact_direct), BadgeKind.Direct)
            false -> Badge(stringResource(R.string.contact_relayed), BadgeKind.Relayed)
            null -> Text("—", style = TinType.bodyM, color = c.ink2)
        }
    }
}

@Composable
fun RenameSheet(current: String, theirName: String, hasAlias: Boolean, onDismiss: () -> Unit, onSave: (String?) -> Unit) {
    val c = Tin.c
    var text by remember { mutableStateOf(current) }
    TinSheet(onDismiss) {
        Text(stringResource(R.string.contact_rename), Modifier.padding(top = 6.dp, bottom = 14.dp), style = TinType.titleL.copy(fontSize = 22.sp), color = c.ink)
        TinField(text, { text = it }, stringResource(R.string.contact_name_here))
        Hint(stringResource(R.string.contact_rename_hint, theirName), Modifier.padding(top = 14.dp))
        Row(Modifier.fillMaxWidth().padding(top = 18.dp), horizontalArrangement = Arrangement.spacedBy(10.dp, Alignment.End)) {
            if (hasAlias) TinButton(stringResource(R.string.contact_use_their_name), { onSave(null) }, style = BtnStyle.Text, fill = false)
            TinButton(stringResource(R.string.common_cancel), onDismiss, style = BtnStyle.Text, fill = false)
            TinButton(stringResource(R.string.contact_save), { text.trim().let { t -> onSave(if (t == theirName) null else t) } }, fill = false, enabled = text.isNotBlank())
        }
    }
}

