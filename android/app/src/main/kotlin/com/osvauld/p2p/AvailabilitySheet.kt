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
import androidx.compose.ui.res.stringResource
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
                Text(stringResource(R.string.avail_title), style = TinType.titleL.copy(fontSize = 20.sp, lineHeight = 26.sp), color = c.ink)
                Text(
                    stringResource(R.string.avail_subtitle),
                    Modifier.padding(top = 4.dp), style = TinType.bodyM, color = c.ink2,
                )
            }
            TinSwitch(available, { onPick(it, null) }, stringResource(R.string.avail_title))
        }
        if (!available) Text(
            if (until != null) stringResource(
                if (until - System.currentTimeMillis() / 1000 > 12 * 3600) R.string.avail_until_tomorrow else R.string.avail_until, clockTime(until))
            else stringResource(R.string.avail_until_manual),
            Modifier.padding(top = 12.dp), style = TinType.bodyM.copy(fontWeight = FontWeight.SemiBold), color = c.ink,
        )
        HorizontalDivider(Modifier.padding(vertical = 14.dp), color = c.ln)
        Text(stringResource(R.string.avail_take_break), style = TinType.caption.copy(fontSize = 13.sp, fontWeight = FontWeight.SemiBold, letterSpacing = 0.04.sp), color = c.ink2)
        val now = System.currentTimeMillis() / 1000
        BreakOption(stringResource(R.string.avail_one_hour), false) { onPick(false, now + 3600) }
        BreakOption(stringResource(R.string.avail_tomorrow_morning), false) { onPick(false, tomorrowMorning(now)) }
        BreakOption(stringResource(R.string.avail_until_on), !available && until == null) { onPick(false, null) }
        Hint(
            if (myName.isBlank()) stringResource(R.string.avail_note_unnamed) else stringResource(R.string.avail_note, myName),
            Modifier.padding(top = 8.dp),
        )
    }
}

@Composable
private fun BreakOption(label: String, selected: Boolean, onClick: () -> Unit) {
    val c = Tin.c
    Row(
        Modifier.fillMaxWidth().heightIn(min = 52.dp).clip(SmoothCornerShape(12.dp)).clickable(role = Role.RadioButton, onClick = onClick),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        RadioButton(selected, null, Modifier.padding(start = 4.dp), colors = RadioButtonDefaults.colors(selectedColor = c.pr, unselectedColor = c.ln2))
        Text(label, style = TinType.bodyL, color = c.ink)
    }
}
