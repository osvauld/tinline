package com.osvauld.p2p

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.RadioButton
import androidx.compose.material3.RadioButtonDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/**
 * The sheet behind the status pill ("Availability sheet" on AndroidHome.dc.html). [onPick] gets the
 * new state: (true, null) = open the line, (false, until) = a break until epoch seconds, or
 * (false, null) = until switched back on.
 */
@Composable
fun AvailabilitySheet(
    available: Boolean, until: Long?, myName: String, onDismiss: () -> Unit,
    onPick: (available: Boolean, until: Long?) -> Unit,
) {
    val c = Tin.c
    TinSheet(onDismiss) {
        Row(Modifier.fillMaxWidth().padding(top = 8.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp)) {
            Column(Modifier.weight(1f)) {
                Text("Available for calls", style = TinType.titleL.copy(fontSize = 20.sp, lineHeight = 26.sp), color = c.ink)
                Text(
                    "Your line stays open in the background so calls ring like a normal phone.",
                    Modifier.padding(top = 4.dp), style = TinType.bodyM, color = c.ink2,
                )
            }
            TinSwitch(available, { onPick(it, null) }, "Available for calls")
        }
        if (!available) Text(
            if (until != null) "Not available until ${clockTime(until)}${if (until - System.currentTimeMillis() / 1000 > 12 * 3600) " tomorrow" else ""}."
            else "Not available until you turn it back on.",
            Modifier.padding(top = 12.dp), style = TinType.bodyM.copy(fontWeight = FontWeight.SemiBold), color = c.ink,
        )
        HorizontalDivider(Modifier.padding(vertical = 14.dp), color = c.ln)
        Text("TAKE A BREAK", style = TinType.caption.copy(fontSize = 13.sp, fontWeight = FontWeight.SemiBold, letterSpacing = 0.04.sp), color = c.ink2)
        val now = System.currentTimeMillis() / 1000
        BreakOption("For 1 hour", false) { onPick(false, now + 3600) }
        BreakOption("Until tomorrow morning", false) { onPick(false, tomorrowMorning(now)) }
        BreakOption("Until I turn it back on", !available && until == null) { onPick(false, null) }
        val who = myName.ifBlank { "you" }
        Hint(
            "While you’re away, people who call see “Couldn’t reach $who”. Nothing is queued and nobody is told why.",
            Modifier.padding(top = 8.dp),
        )
    }
}

@Composable
private fun BreakOption(label: String, selected: Boolean, onClick: () -> Unit) {
    val c = Tin.c
    Row(
        Modifier.fillMaxWidth().heightIn(min = 52.dp).clip(RoundedCornerShape(12.dp)).clickable(role = Role.RadioButton, onClick = onClick),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        RadioButton(selected, null, Modifier.padding(start = 4.dp), colors = RadioButtonDefaults.colors(selectedColor = c.pr, unselectedColor = c.ln2))
        Text(label, style = TinType.bodyL, color = c.ink)
    }
}
