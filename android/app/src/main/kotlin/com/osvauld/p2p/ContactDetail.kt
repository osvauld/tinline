package com.osvauld.p2p

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.*
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import uniffi.p2pcore.Contact

/** Contact page: name (alias first), Verified pill, Call / Rename / Verify / Remove, and this contact's call history. */
@Composable
fun ContactScreen(
    contact: Contact, onBack: () -> Unit, onCall: () -> Unit, onRemove: () -> Unit,
    history: List<RecentCall> = emptyList(), onRename: (String?) -> Unit = {}, onVerify: () -> Unit = {},
) {
    val alias = contact.alias?.takeIf { it.isNotBlank() }
    val verified = contact.verified
    val c = Tin.c
    var confirmRemove by remember { mutableStateOf(false) }
    var renaming by remember { mutableStateOf(false) }
    val shown = contact.display()
    Page {
        TopBar(null, onBack)
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState())) {
            Column(Modifier.fillMaxWidth().padding(horizontal = 24.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Avatar(shown, contact.did, 96.dp)
                Text(shown, Modifier.padding(top = 6.dp), style = TinType.h1, color = c.ink, textAlign = TextAlign.Center)
                if ((alias != null && alias != contact.name) || verified) Row(
                    horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically,
                ) {
                    if (alias != null && alias != contact.name) Text("Calls themselves “${contact.name}”", style = TinType.bodyM, color = c.ink2)
                    if (verified) Row(
                        Modifier.heightIn(min = 22.dp).clip(RoundedCornerShape(50)).background(c.prc).padding(start = 6.dp, end = 8.dp),
                        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp),
                    ) {
                        Icon(Icons.Rounded.VerifiedUser, null, tint = c.onPrc, modifier = Modifier.size(14.dp))
                        Text("Verified", style = TinType.caption.copy(fontWeight = FontWeight.SemiBold), color = c.onPrc)
                    }
                }
                TinButton("Call", onCall, Modifier.padding(top = 14.dp), icon = Icons.Rounded.Call)
                Row(Modifier.padding(top = 10.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    ActionChip(Icons.Rounded.Edit, "Rename", c.pr) { renaming = true }
                    ActionChip(Icons.Rounded.VerifiedUser, "Verify", c.pr, onVerify)
                    ActionChip(Icons.Rounded.PersonRemove, "Remove", c.er) { confirmRemove = true }
                }
            }
            if (history.isNotEmpty()) {
                SectionLabel("CALLS WITH ${shown.uppercase()}", Modifier.padding(top = 8.dp))
                history.forEach { HistoryRow(it) }
                Hint("History stays on this phone only.", Modifier.padding(horizontal = 20.dp, vertical = 8.dp))
            }
        }
    }
    if (confirmRemove) TinDialog(
        "Remove $shown?", { confirmRemove = false }, "Remove", { confirmRemove = false; onRemove() }, destructive = true, icon = Icons.Rounded.PersonRemove,
    ) {
        DialogText("They won’t be able to call you, and you won’t be able to call them." + " Your call history with them is deleted from this phone.")
        DialogText("To talk again, you’ll need to add each other again.")
    }
    if (renaming) RenameSheet(alias ?: contact.name, contact.name, hasAlias = alias != null, onDismiss = { renaming = false }) { onRename(it); renaming = false }
}

@Composable
private fun ActionChip(icon: ImageVector, label: String, tint: androidx.compose.ui.graphics.Color, onClick: () -> Unit) {
    val c = Tin.c
    Column(
        Modifier.width(88.dp).clip(RoundedCornerShape(12.dp)).clickable(role = Role.Button, onClick = onClick),
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
        Icon(when (r.kind) { RecentCall.Kind.Incoming -> Icons.Rounded.CallReceived; RecentCall.Kind.Missed -> Icons.Rounded.CallMissed; else -> Icons.Rounded.CallMade },
            null, tint = if (red) c.er else c.ink2, modifier = Modifier.size(20.dp))
        Column(Modifier.weight(1f)) {
            Text(r.title.ifBlank { "Call" }, style = TinType.bodyL.copy(fontSize = 15.sp), color = if (red) c.er else c.ink)
            Text(r.whenLong, style = TinType.bodyM.copy(fontSize = 13.sp), color = c.ink2)
        }
        when (r.direct) {
            true -> Badge("Direct", BadgeKind.Direct)
            false -> Badge("Relayed", BadgeKind.Relayed)
            null -> Text("—", style = TinType.bodyM, color = c.ink2)
        }
    }
}

@Composable
fun RenameSheet(current: String, theirName: String, hasAlias: Boolean, onDismiss: () -> Unit, onSave: (String?) -> Unit) {
    val c = Tin.c
    var text by remember { mutableStateOf(current) }
    TinSheet(onDismiss) {
        Text("Rename", Modifier.padding(top = 6.dp, bottom = 14.dp), style = TinType.titleL.copy(fontSize = 22.sp), color = c.ink)
        TinField(text, { text = it }, "Name on this phone")
        Hint("Only you see this. They still call themselves “$theirName”.", Modifier.padding(top = 14.dp))
        Row(Modifier.fillMaxWidth().padding(top = 18.dp), horizontalArrangement = Arrangement.spacedBy(10.dp, Alignment.End)) {
            if (hasAlias) TinButton("Use their name", { onSave(null) }, style = BtnStyle.Text, fill = false)
            TinButton("Cancel", onDismiss, style = BtnStyle.Text, fill = false)
            TinButton("Save", { text.trim().let { t -> onSave(if (t == theirName) null else t) } }, fill = false, enabled = text.isNotBlank())
        }
    }
}

/** Safety-number comparison. [groups] are the 12 five-digit groups, same on both phones. */
@Composable
fun VerifyScreen(
    me: String, contact: Contact, groups: List<String>?, onBack: () -> Unit, onMatch: () -> Unit, onRemove: () -> Unit,
) {
    val c = Tin.c
    var mismatch by remember { mutableStateOf(false) }
    val name = contact.display()
    Page {
        TopBar("Verify $name", onBack)
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(start = 24.dp, end = 24.dp, top = 8.dp, bottom = 16.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            Lead("Compare these numbers with $name, in person or on a Tinline call. If they match, your calls go only to them — nobody is in the middle.")
            CardBox(Modifier.fillMaxWidth(), radius = 20.dp) {
                Column(Modifier.padding(horizontal = 16.dp, vertical = 20.dp).fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(16.dp)) {
                    PairAvatars(me, name, contact.did, mine = 40.dp, theirs = 40.dp)
                    if (groups == null) Text("Not available yet.", style = TinType.bodyM, color = c.ink2)
                    else groups.chunked(4).forEach { row ->
                        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                            row.forEach { Text(it, style = TinType.mono.copy(fontSize = 19.sp, letterSpacing = 0.04.sp), color = c.ink) }
                        }
                    }
                    Hint("Same on both phones. Read them aloud in groups.", align = TextAlign.Center)
                }
            }
        }
        Column(Modifier.padding(start = 24.dp, end = 24.dp, bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            TinButton("They match", onMatch, icon = Icons.Rounded.Check, enabled = groups != null)
            TinButton("They don’t match", { mismatch = true }, style = BtnStyle.Text)
            Hint("If they don’t match, remove $name and add them again face to face.", Modifier.fillMaxWidth(), align = TextAlign.Center)
        }
    }
    if (mismatch) TinDialog(
        "The numbers don’t match", { mismatch = false }, "Remove $name", { mismatch = false; onRemove() }, dismiss = "Check again",
        destructive = true, icon = Icons.Rounded.GppMaybe,
    ) {
        DialogText("Someone may be in the middle of your calls, or a digit was misread. Look once more, group by group.")
        DialogText("If they still differ, remove $name and add them again face to face.")
    }
}
