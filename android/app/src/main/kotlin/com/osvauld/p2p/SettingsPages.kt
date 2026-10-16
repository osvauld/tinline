package com.osvauld.p2p

import android.content.Context
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.*
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow

/*
 * Settings (playground screens/settings: Settings.svelte, Notifications.svelte, Sounds.svelte, PermBanner.svelte, perms.ts) and the
 * Privacy landing (screens/privacy/Privacy.svelte, CallsAllowed.svelte). Profile card, accounts, then Devices / Privacy /
 * Notifications / Sounds. Page furniture is PeopleKit's Pf*.
 */

/** perms.ts Problem: one OS-permission problem with its calm copy (same wording on the landing strip and the Notifications page). */
data class PermProblem(val need: Need, val icon: ImageVector, val title: String, val body: String, val fix: String = "Open settings")

/** perms.ts problems(), in its order. Android adds battery. */
fun permProblems(missing: List<Need>): List<PermProblem> = buildList {
    if (Need.Notifications in missing) add(PermProblem(Need.Notifications, PgIcons.Bell, "Notifications are off",
        "Tinline can’t alert you to messages or calls. They still arrive; you’ll see them when you open the app. Turn notifications on in system settings."))
    if (Need.Mic in missing) add(PermProblem(Need.Mic, PgIcons.MicOff, "Microphone is off",
        "You can take and place calls, but people won’t hear you until Tinline can use the microphone. Turn it on in system settings."))
    if (Need.Battery in missing) add(PermProblem(Need.Battery, PgIcons.Battery, "Battery saving may delay calls",
        "Android can pause Tinline in the background, so calls and messages can arrive late. Set Tinline’s battery use to Unrestricted."))
}

/** PermBanner.svelte: warn-bg card (r14, padding 12 14, gap 12), icon 20, title 650/20, body 13/19, an outline button in the ink colour. */
@Composable
fun PermBanner(p: PermProblem, onAction: () -> Unit, action: String = p.fix) {
    val c = Tin.c
    Column(Modifier.fillMaxWidth().padding(bottom = 12.dp).semantics(mergeDescendants = false) { liveRegion = LiveRegionMode.Polite }) {
        Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(c.warn).padding(horizontal = 14.dp, vertical = 12.dp),
            horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Icon(p.icon, null, Modifier.padding(top = 1.dp).size(20.dp), tint = c.onWarn)
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(p.title, style = PgType.bodyB.copy(fontWeight = W650), color = c.onWarn)
                Text(p.body, style = PgType.sub19, color = c.onWarn)
                PBtn(action, onAction, Modifier.padding(top = 6.dp), height = 36.dp, colorOverride = c.onWarn)
            }
        }
    }
}

data class AccountRow(val did: String, val name: String, val current: Boolean)

/** Settings.svelte landing. */
@Composable
fun SettingsLanding(
    store: ProfileStore, fallbackName: String, accounts: List<AccountRow>, problems: List<PermProblem>,
    onBack: () -> Unit, onMe: () -> Unit, onSwitch: (String) -> Unit, onAdd: () -> Unit,
    onDevices: () -> Unit, onPrivacy: () -> Unit, onNotifications: () -> Unit, onSounds: () -> Unit,
    onVideo: () -> Unit = {}, onStorage: () -> Unit = {}, onAbout: () -> Unit = {},
    extra: @Composable ColumnScope.() -> Unit = {},
) {
    val c = Tin.c
    val me by store.self.collectAsState()
    val view = rememberProfileView(me)
    val name = view?.text("name")?.takeIf { it.isNotBlank() } ?: fallbackName
    val status = view?.text("status")?.takeIf { it.isNotEmpty() }
    var now by remember { mutableLongStateOf(System.currentTimeMillis()) }
    val av = effectiveAvailability(view?.fields, now)
    LaunchedEffect(av) { av.untilMs?.let { kotlinx.coroutines.delay((it - System.currentTimeMillis()).coerceAtLeast(0) + 50); now = System.currentTimeMillis() } }
    val avText = if (av.available) "Available" else av.untilMs?.let { "Busy until ${ProfileInfo.untilLabel(it)}" } ?: "Busy"

    PfPage("Settings", onBack) {
        // Permission problems: said once here, with the details on the Notifications page.
        problems.firstOrNull()?.let { p -> PfSec { PermBanner(p, onNotifications, "See details") } }
        PfSec(first = true) {
            PfCard {
                act {
                    PfAct(name, onClick = onMe, minHeight = 88.dp, gap = 16.dp, padV = 12.dp, chevron = true,
                        titleStyle = PgType.bodyB.copy(fontSize = PgType.title.fontSize, lineHeight = PgType.title.lineHeight),
                        leading = { PgAvatar(name, 0, 64.dp, person = me) }, extraLines = {
                            if (status != null) Text(status, style = PgType.sub, color = c.ink2)
                            Text(avText, style = PgType.sub.copy(fontWeight = FontWeight.SemiBold, fontFeatureSettings = "tnum"), color = if (av.available) c.ink2 else c.onWarn)
                        })
                }
            }
        }
        PfSec {
            PfCard {
                accounts.filter { it.current }.forEach { a ->
                    act {
                        PfAct(a.name, leading = { PgAvatar(a.name, 0, 36.dp, person = me) }, sub = "Selected · online", subColor = c.accentInk, subBold = true,
                            trailing = { Icon(PgIcons.Check, null, Modifier.size(18.dp), tint = c.ink2) })
                    }
                }
                accounts.filter { !it.current }.forEach { a ->
                    act { PfAct(a.name, onClick = { onSwitch(a.did) }, leading = { PgAvatar(a.name, hueOf(a.did), 36.dp) }, sub = "Paused · tap to switch") }
                }
                act { PfAct("Add another account", onClick = onAdd, icon = PgIcons.Plus, chevron = true) }
            }
            Text("Only the selected account is online on this phone. Switching pauses the other one: it receives no calls until you select it again.",
                Modifier.padding(start = 16.dp, end = 16.dp, top = 8.dp), style = PgType.sub, color = c.ink2)
        }
        // Appearance (Settings.svelte LOOKS, 2026-10-14): the same choice the sign-up switch makes; switches at once.
        PfSec("Appearance") {
            val ctx = androidx.compose.ui.platform.LocalContext.current
            val look by Appearance.dark.collectAsState()
            PfCard {
                listOf(Triple(null as Boolean?, "Match my phone", "Light or dark, as this phone is set"), Triple(false, "Light", null), Triple(true, "Dark", null)).forEach { (v, label, sub) ->
                    act {
                        PfAct(label, onClick = { if (v == null) Appearance.follow(ctx) else Appearance.set(ctx, v) }, sub = sub, role = Role.RadioButton, minHeight = 56.dp,
                            leading = { PgRadio(look == v) })
                    }
                }
            }
        }
        PfSec {
            PfCard {
                act { PfAct("Devices", onClick = onDevices, icon = PgIcons.Devices, chevron = true) }
                act { PfAct("Privacy", onClick = onPrivacy, icon = PgIcons.Shield, chevron = true) }
                act { PfAct("Notifications", onClick = onNotifications, icon = PgIcons.Bell, chevron = true) }
                act { PfAct("Sounds", onClick = onSounds, icon = PgIcons.Vol, chevron = true) }
                act { PfAct("Video", onClick = onVideo, icon = PgIcons.Video, chevron = true) }
                act { PfAct("Storage", onClick = onStorage, icon = PgIcons.Save, chevron = true) }
                act { PfAct("About Tinline", onClick = onAbout, icon = PgIcons.Info, chevron = true) }
            }
        }
        extra()
    }
}

/** Stable avatar hue for a key (the same hash as Components.kt's Avatar). */
fun hueOf(key: String): Int { var h = 0; for (ch in key) h = h * 31 + ch.code; return (h and 0x7fffffff) % 6 }

// ------------------------------------------------------------------ app-level alert and sound settings (stored on this device)

/** Settings › Notifications "Alert me about" and Settings › Sounds Quiet. Device-local preferences; the core has none of these. */
object AlertPrefs {
    private fun p(c: Context) = c.getSharedPreferences("alerts", Context.MODE_PRIVATE)
    private val state = MutableStateFlow<Map<String, Boolean>>(emptyMap())
    fun flow(c: Context): StateFlow<Map<String, Boolean>> {
        if (state.value.isEmpty()) state.value = listOf("messages", "calls", "groups", "quiet").associateWith { p(c).getBoolean(it, it != "quiet") }
        return state
    }
    fun get(c: Context, key: String): Boolean = p(c).getBoolean(key, key != "quiet")
    fun set(c: Context, key: String, on: Boolean) { p(c).edit().putBoolean(key, on).apply(); state.value = flow(c).value + (key to on) }
    fun messages(c: Context) = get(c, "messages")
    fun calls(c: Context) = get(c, "calls")
    fun groups(c: Context) = get(c, "groups")
    fun quiet(c: Context) = get(c, "quiet")
}

/** Notifications.svelte. [missing] is what Android reports; [fix] opens the system setting. */
@Composable
fun NotificationsPage(missing: List<Need>, onBack: () -> Unit, fix: (Need) -> Unit) {
    val c = Tin.c
    val ctx = androidx.compose.ui.platform.LocalContext.current
    val prefs by AlertPrefs.flow(ctx).collectAsState()
    val probs = permProblems(missing)
    val off = Need.Notifications in missing
    PfPage("Notifications", onBack) {
        if (probs.isNotEmpty()) PfSec(first = true) { probs.forEach { p -> key(p.need) { PermBanner(p, { fix(p.need) }) } } }
        PfSec("Alert me about", top = 12.dp) {
            Box(Modifier.alpha(if (off) .55f else 1f)) {
                PfCard {
                    listOf(Triple("messages", PgIcons.Chat, "Messages" to "From people, not groups"),
                        Triple("calls", PgIcons.Phone, "Calls" to "Ringing when someone calls"),
                        Triple("groups", PgIcons.Users, "Groups" to "Posts, invites and requests")).forEach { (k, icon, t) ->
                        val on = prefs[k] ?: true
                        act {
                            PfAct(t.first, onClick = if (off) null else ({ AlertPrefs.set(ctx, k, !on) }), icon = icon, sub = t.second, role = Role.Switch, plain = true,
                                trailing = { PgSwitch(on, { AlertPrefs.set(ctx, k, it) }, when (k) { "messages" -> "Message notifications"; "calls" -> "Call notifications"; else -> "Group notifications" }, enabled = !off) })
                        }
                    }
                }
            }
            PfNote("A muted chat never alerts you, whatever is switched on here. Missed calls from one person fold into one notification.")
        }
        PfSec("Permissions") {
            PfCard {
                listOf(
                    Triple(Need.Notifications, PgIcons.Bell, Triple("Notifications", "Allowed", "Off in system settings")),
                    Triple(Need.Mic, PgIcons.Mic, Triple("Microphone", "Allowed", "Off in system settings")),
                    Triple(Need.Battery, PgIcons.Battery, Triple("Battery use", "Unrestricted", "Restricted")),
                ).forEach { (need, icon, t) ->
                    val ok = need !in missing
                    act {
                        PfAct(t.first, icon = icon, sub = if (ok) t.second else t.third, subColor = if (ok) null else if (c.dark) c.goldInk else c.onWarn, subBold = !ok,
                            trailing = { Box(Modifier.widthIn(min = 64.dp), contentAlignment = Alignment.CenterEnd) { if (!ok) PBtn("Fix", { fix(need) }, height = 36.dp, padH = 14.dp) } })
                    }
                }
                // Native: Android 14 also needs the full-screen permission for a call to ring over the lock screen (not in the playground).
                if (Need.FullScreen in missing) act {
                    PfAct("Ring over lock screen", icon = PgIcons.Lock, sub = "Off in system settings", subColor = if (c.dark) c.goldInk else c.onWarn, subBold = true,
                        trailing = { Box(Modifier.widthIn(min = 64.dp), contentAlignment = Alignment.CenterEnd) { PBtn("Fix", { fix(Need.FullScreen) }, height = 36.dp, padH = 14.dp) } })
                }
            }
            PfNote("These are set by your system, not by Tinline. Tinline asks only when it needs one.")
        }
    }
}

/** Sounds.svelte: Quiet, and the call sound set with previews (the core renders the same cues as the desktop). */
@Composable
fun SoundsPage(onBack: () -> Unit) {
    val ctx = androidx.compose.ui.platform.LocalContext.current
    val prefs by AlertPrefs.flow(ctx).collectAsState()
    val quiet = prefs["quiet"] ?: false
    var palette by remember { mutableStateOf(CallSounds.palette(ctx)) }
    val sets = listOf(Triple("classic", "Classic chime", "A bright four-note chime."),
        Triple("slack", "Slack \u2192 taut", "Two close notes beat like a loose string, then one pulls tight."),
        Triple("triad", "Triad knock", "Three rising notes, like a knock."))
    PfPage("Sounds", onBack) {
        PfSec("Quiet", first = true) {
            PfCard {
                act {
                    PfAct("Quiet", onClick = { AlertPrefs.set(ctx, "quiet", !quiet) }, icon = PgIcons.Mute, sub = "No sounds from Tinline: ringing, call tones, joins", role = Role.Switch, plain = true,
                        trailing = { PgSwitch(quiet, { AlertPrefs.set(ctx, "quiet", it) }, "Quiet") })
                }
            }
        }
        PfSec("Call sounds") {
            PfCard {
                sets.forEach { (k, label, sub) ->
                    act { PfRadioRow(label, sub, palette == k, { palette = k; CallSounds.setPalette(ctx, k); CallSounds.preview(ctx, "ring") }, minHeight = 64.dp) }
                }
            }
            Row(Modifier.fillMaxWidth().padding(start = 4.dp, end = 4.dp, top = 12.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                listOf("ring" to "Ringtone", "connected" to "Connected", "ended" to "Call ended").forEach { (cue, label) ->
                    PgChip(label, false, { CallSounds.preview(ctx, cue) }, icon = PgIcons.Vol)
                }
            }
            PfNote(if (quiet) "Quiet is on, so previews are silent." else "Tap a sound to hear it. The choice applies to the next call.")
        }
    }
}

/** One choice among several (Settings radio rows): ring + dot, title, optional muted line. */
@Composable
fun PfRadioRow(title: String, sub: String?, on: Boolean, onClick: () -> Unit, minHeight: androidx.compose.ui.unit.Dp = 56.dp) =
    PfAct(title, onClick = onClick, sub = sub, role = Role.RadioButton, minHeight = minHeight, leading = { PgRadio(on) })

/**
 * CallsVideo.svelte (Android form): "Sharper video" only; the phone picks its own cameras (Flip in a call). The preference is
 * [VideoQualityPreference]; each video call starts this way (the HD chip in a call changes only that call).
 */
@Composable
internal fun VideoSettingsPage(quality: VideoQualityPreference, onQuality: (VideoQualityPreference) -> Unit, onBack: () -> Unit) {
    val hd = quality == VideoQualityPreference.PREFER_HD
    val set = { on: Boolean -> onQuality(if (on) VideoQualityPreference.PREFER_HD else VideoQualityPreference.AUTO) }
    PfPage("Video", onBack) {
        PfSec("Video quality", first = true) {
            PfCard {
                act {
                    PfAct("Sharper video", onClick = { set(!hd) }, icon = PgIcons.Video, role = Role.Switch, plain = true,
                        sub = if (hd) "Up to 540p when the connection allows. Uses more data and battery." else "Adjusts by itself for smooth calls.",
                        trailing = { PgSwitch(hd, set, "Sharper video") })
                }
            }
            PfNote("Every video call starts this way. During a call, tap HD to change it for that call.")
        }
    }
}

private fun fmtBytes(n: Long): String = when {
    n >= 1_000_000_000L -> "%.1f GB".format(n / 1e9)
    n >= 1_000_000L -> "${Math.round(n / 1e6)} MB"
    n >= 1_000L -> "${Math.round(n / 1e3)} KB"
    else -> "$n B"
}

/** Storage.svelte: what Tinline keeps here (one bar), when pictures download, and clearing the cache. */
@Composable
fun StoragePage(pictures: Long, files: Long, messages: Long, cache: Long, download: uniffi.p2pcore.PictureDownload, cleared: Boolean,
                onDownload: (uniffi.p2pcore.PictureDownload) -> Unit, onClear: () -> Unit, onBack: () -> Unit) {
    val c = Tin.c
    val kinds = listOf(Triple("Pictures", pictures, c.accent), Triple("Files", files, c.thread), Triple("Messages", messages, c.ink2), Triple("Cache", cache, c.ln))
    val total = kinds.sumOf { it.second }
    PfPage("Storage", onBack) {
        PfSec("On this phone", first = true) {
            PfCard {
                act {
                    Column(Modifier.fillMaxWidth().padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                        Text(fmtBytes(total), style = PgType.title, color = c.ink)
                        Row(Modifier.fillMaxWidth().height(10.dp).clip(RoundedCornerShape(5.dp)).background(c.sf2), horizontalArrangement = Arrangement.spacedBy(2.dp)) {
                            kinds.filter { it.second > 0 }.forEach { (_, n, col) -> Box(Modifier.weight(n.toFloat()).fillMaxHeight().background(col)) }
                        }
                        Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                            kinds.forEach { (label, n, col) ->
                                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                    Box(Modifier.size(10.dp).clip(RoundedCornerShape(3.dp)).background(col))
                                    Text(label, Modifier.weight(1f), style = PgType.sub, color = c.ink)
                                    Text(fmtBytes(n), style = PgType.sub.copy(fontFeatureSettings = "tnum"), color = c.ink2)
                                }
                            }
                        }
                    }
                }
            }
        }
        PfSec("Download pictures") {
            PfCard {
                listOf(Triple(uniffi.p2pcore.PictureDownload.ALWAYS, "Always", "On Wi-Fi and mobile data"),
                    Triple(uniffi.p2pcore.PictureDownload.WIFI, "On Wi-Fi", "On mobile data they wait for a tap"),
                    Triple(uniffi.p2pcore.PictureDownload.TAP, "When I tap them", "Each shows a soft preview until then")).forEach { (d, label, sub) ->
                    act { PfRadioRow(label, sub, download == d, { onDownload(d) }) }
                }
            }
        }
        PfSec {
            PfCard {
                act {
                    PfAct("Clear cache", onClick = if (cache > 0) onClear else null, icon = PgIcons.Trash,
                        sub = if (cache > 0) "${fmtBytes(cache)} of pictures and files you’ve opened. They’re prepared again when you open them." else if (cleared) "Cleared" else "Nothing to clear")
                }
            }
            PfNote("To free more space, remove pictures or files from their chat.")
        }
    }
}

/** Privacy.svelte landing. */
@Composable
fun PrivacyLanding(blockedN: Int, msgOnly: Int, hasPassphrase: Boolean, onBack: () -> Unit, onBlocked: () -> Unit, onCalls: () -> Unit, onPhrase: () -> Unit, onPassphrase: () -> Unit,
                   readReceipts: Boolean = true, onReadReceipts: (Boolean) -> Unit = {}) {
    PfPage("Privacy", onBack) {
        PfSec(first = true) {
            PfCard {
                act { PfAct("Blocked people", onClick = onBlocked, icon = PgIcons.Off, chevron = true,
                    sub = if (blockedN > 0) "$blockedN ${if (blockedN == 1) "person" else "people"}" else "No one") }
                act { PfAct("Who can call me", onClick = onCalls, icon = PgIcons.Phone, chevron = true,
                    sub = if (msgOnly > 0) "$msgOnly ${if (msgOnly == 1) "contact is" else "contacts are"} messages only" else "All contacts can call") }
                // Native: without a passphrase the phrase is behind the phone's screen lock instead.
                act { PfAct("Account recovery", onClick = onPhrase, icon = PgIcons.Key, chevron = true, sub = if (hasPassphrase) "Needs your passphrase" else "Needs your screen lock") }
                act { PfAct("Passphrase", onClick = onPassphrase, icon = PgIcons.Lock, chevron = true, sub = "Protects Tinline on this device") }
            }
            PfNote("Your contacts see your name, picture and profile. Tinline doesn’t hide who you talk to, and every device linked to your account can read your chats.")
        }
        // Core account setting (default on). Same words as desktop.
        PfSec {
            PfCard {
                act {
                    PfAct("Read receipts", onClick = { onReadReceipts(!readReceipts) }, icon = PgIcons.Check, role = Role.Switch, plain = true,
                        sub = if (readReceipts) "People see when you’ve read their messages" else "Your reads stay with you",
                        trailing = { PgSwitch(readReceipts, onReadReceipts, "Read receipts") })
                }
            }
            PfNote("Delivered ticks still go out either way, and you still see when others read yours. Messages you read while this is off stay unconfirmed.")
        }
    }
}

data class CallsRow(val did: String, val name: String, val allowed: Boolean)

/** CallsAllowed.svelte: each contact's Allow calls (the same switch as on their profile). */
@Composable
fun CallsAllowedPage(rows: List<CallsRow>, onBack: () -> Unit, onAllow: (String, Boolean) -> Unit) {
    PfPage("Who can call me", onBack) {
        PfSec("Contacts", first = true) {
            PfCard {
                rows.sortedBy { it.name.lowercase() }.forEach { r ->
                    row {
                        PersonRowPg(r.name, r.did, if (r.allowed) "Can call you" else "Messages only", pad = 12.dp) {
                            PgSwitch(r.allowed, { onAllow(r.did, it) }, "Allow calls from ${r.name}")
                        }
                    }
                }
            }
            PfNote("Someone you allowed can ring you; someone on messages only can still write. People you blocked can do neither.")
        }
    }
}
