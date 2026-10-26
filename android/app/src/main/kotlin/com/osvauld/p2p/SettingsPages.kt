package com.osvauld.p2p

import androidx.compose.ui.res.stringResource
import androidx.compose.ui.res.pluralStringResource
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
data class PermProblem(val need: Need, val icon: ImageVector, val title: String, val body: String, val fix: String)

/** perms.ts problems(), in its order. Android adds battery. */
@Composable
fun permProblems(missing: List<Need>): List<PermProblem> {
    val fix = stringResource(R.string.perms_open_settings)
    return buildList {
        if (Need.Notifications in missing) add(PermProblem(Need.Notifications, PgIcons.Bell, stringResource(R.string.perms_notifications_off_title),
            stringResource(R.string.perms_notifications_off_body), fix))
        if (Need.Battery in missing) add(PermProblem(Need.Battery, PgIcons.Battery, stringResource(R.string.perms_battery_title),
            stringResource(R.string.perms_battery_body), fix))
    }
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
    val avText = if (av.available) stringResource(R.string.settings_available) else av.untilMs?.let { stringResource(R.string.settings_busy_until, ProfileInfo.untilLabel(it)) } ?: stringResource(R.string.settings_busy)

    PfPage(stringResource(R.string.settings_title), onBack) {
        // Permission problems: said once here, with the details on the Notifications page.
        problems.firstOrNull()?.let { p -> PfSec { PermBanner(p, onNotifications, stringResource(R.string.settings_see_details)) } }
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
                        PfAct(a.name, leading = { PgAvatar(a.name, 0, 36.dp, person = me) }, sub = stringResource(R.string.settings_account_selected), subColor = c.accentInk, subBold = true,
                            trailing = { Icon(PgIcons.Check, null, Modifier.size(18.dp), tint = c.ink2) })
                    }
                }
                accounts.filter { !it.current }.forEach { a ->
                    act { PfAct(a.name, onClick = { onSwitch(a.did) }, leading = { PgAvatar(a.name, avatarHue(a.did), 36.dp) }, sub = stringResource(R.string.settings_account_paused)) }
                }
                act { PfAct(stringResource(R.string.settings_add_account), onClick = onAdd, icon = PgIcons.Plus, chevron = true) }
            }
            Text(stringResource(R.string.settings_accounts_note),
                Modifier.padding(start = 16.dp, end = 16.dp, top = 8.dp), style = PgType.sub, color = c.ink2)
        }
        // Appearance (Settings.svelte LOOKS, 2026-10-14): the same choice the sign-up switch makes; switches at once.
        PfSec(stringResource(R.string.settings_appearance)) {
            val ctx = androidx.compose.ui.platform.LocalContext.current
            val look by Appearance.dark.collectAsState()
            val cardRows1 = listOf(Triple(null as Boolean?, stringResource(R.string.settings_look_match), stringResource(R.string.settings_look_match_sub)), Triple(false, stringResource(R.string.settings_look_light), null), Triple(true, stringResource(R.string.settings_look_dark), null))
            PfCard {
                cardRows1.forEach { (v, label, sub) ->
                    act {
                        PfAct(label, onClick = { if (v == null) Appearance.follow(ctx) else Appearance.set(ctx, v) }, sub = sub, role = Role.RadioButton, minHeight = 56.dp,
                            leading = { PgRadio(look == v) })
                    }
                }
            }
        }
        PfSec {
            PfCard {
                act { PfAct(stringResource(R.string.settings_devices), onClick = onDevices, icon = PgIcons.Devices, chevron = true) }
                act { PfAct(stringResource(R.string.settings_privacy), onClick = onPrivacy, icon = PgIcons.Shield, chevron = true) }
                act { PfAct(stringResource(R.string.settings_notifications), onClick = onNotifications, icon = PgIcons.Bell, chevron = true) }
                act { PfAct(stringResource(R.string.settings_sounds), onClick = onSounds, icon = PgIcons.Vol, chevron = true) }
                act { PfAct(stringResource(R.string.settings_video), onClick = onVideo, icon = PgIcons.Video, chevron = true) }
                act { PfAct(stringResource(R.string.settings_storage), onClick = onStorage, icon = PgIcons.Save, chevron = true) }
                act { PfAct(stringResource(R.string.settings_about), onClick = onAbout, icon = PgIcons.Info, chevron = true) }
            }
        }
        extra()
    }
}

/** Stable avatar hue for a key (the same hash as Components.kt's Avatar). */

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
    PfPage(stringResource(R.string.settings_notifications), onBack) {
        if (probs.isNotEmpty()) PfSec(first = true) { probs.forEach { p -> key(p.need) { PermBanner(p, { fix(p.need) }) } } }
        PfSec(stringResource(R.string.notif_alert_about), top = 12.dp) {
            Box(Modifier.alpha(if (off) .55f else 1f)) {
                val cardRows2 = listOf(Triple("messages", PgIcons.Chat, stringResource(R.string.notif_messages) to stringResource(R.string.notif_messages_sub)),
                        Triple("calls", PgIcons.Phone, stringResource(R.string.notif_calls) to stringResource(R.string.notif_calls_sub)),
                        Triple("groups", PgIcons.Users, stringResource(R.string.notif_groups) to stringResource(R.string.notif_groups_sub)))
                PfCard {
                    cardRows2.forEach { (k, icon, t) ->
                        val on = prefs[k] ?: true
                        act {
                            PfAct(t.first, onClick = if (off) null else ({ AlertPrefs.set(ctx, k, !on) }), icon = icon, sub = t.second, role = Role.Switch, plain = true,
                                trailing = { PgSwitch(on, { AlertPrefs.set(ctx, k, it) }, when (k) { "messages" -> stringResource(R.string.notif_messages_switch); "calls" -> stringResource(R.string.notif_calls_switch); else -> stringResource(R.string.notif_groups_switch) }, enabled = !off) })
                        }
                    }
                }
            }
            PfNote(stringResource(R.string.notif_muted_note))
        }
        PfSec(stringResource(R.string.notif_permissions)) {
            val cardRows3 = listOf(
                    Triple(Need.Notifications, PgIcons.Bell, Triple(stringResource(R.string.settings_notifications), stringResource(R.string.notif_perm_allowed), stringResource(R.string.notif_perm_off))),
                    Triple(Need.Mic, PgIcons.Mic, Triple(stringResource(R.string.notif_perm_mic), stringResource(R.string.notif_perm_allowed), stringResource(R.string.notif_perm_mic_asked))),
                    Triple(Need.Battery, PgIcons.Battery, Triple(stringResource(R.string.notif_perm_battery), stringResource(R.string.notif_perm_unrestricted), stringResource(R.string.notif_perm_restricted))),
                )
            PfCard {
                cardRows3.forEach { (need, icon, t) ->
                    // The microphone is asked for when a call needs it (also "Ask every time"), so it is never a problem to fix here.
                    val ok = need !in missing || need == Need.Mic
                    act {
                        PfAct(t.first, icon = icon, sub = if (need !in missing) t.second else t.third, subColor = if (ok) null else if (c.dark) c.goldInk else c.onWarn, subBold = !ok,
                            trailing = { Box(Modifier.widthIn(min = 64.dp), contentAlignment = Alignment.CenterEnd) { if (!ok) PBtn(stringResource(R.string.notif_fix), { fix(need) }, height = 36.dp, padH = 14.dp) } })
                    }
                }
                // Native: Android 14 also needs the full-screen permission for a call to ring over the lock screen (not in the playground).
                if (Need.FullScreen in missing) act {
                    PfAct(stringResource(R.string.notif_ring_lock), icon = PgIcons.Lock, sub = stringResource(R.string.notif_perm_off), subColor = if (c.dark) c.goldInk else c.onWarn, subBold = true,
                        trailing = { Box(Modifier.widthIn(min = 64.dp), contentAlignment = Alignment.CenterEnd) { PBtn(stringResource(R.string.notif_fix), { fix(Need.FullScreen) }, height = 36.dp, padH = 14.dp) } })
                }
            }
            PfNote(stringResource(R.string.notif_system_note))
        }
    }
}

/** Sounds.svelte: Quiet, and the call sound set with previews (the core renders the same cues as the desktop). */
/**
 * Settings › Language: follow the phone, or one of the languages this build ships (each named in its own language, with
 * the name in the current language under it). The choice applies at once.
 */
@Composable
fun LanguagePage(onBack: () -> Unit) {
    val ctx = androidx.compose.ui.platform.LocalContext.current
    val current = remember { AppLanguage.current(ctx) }
    val tags = remember { AppLanguage.shipped(ctx) }
    fun pick(tag: String) { if (tag != current) ctx.findActivity()?.let { AppLanguage.set(it, tag) } }
    PfPage(stringResource(R.string.settings_language), onBack) {
        PfSec(first = true) {
            PfCard {
                act {
                    PfAct(stringResource(R.string.settings_language_match), onClick = { pick("") }, sub = stringResource(R.string.settings_language_match_sub),
                        role = Role.RadioButton, minHeight = 56.dp, leading = { PgRadio(current.isEmpty()) })
                }
                tags.forEach { tag ->
                    act {
                        PfAct(AppLanguage.nativeName(tag), onClick = { pick(tag) }, sub = AppLanguage.localName(tag),
                            role = Role.RadioButton, minHeight = 56.dp, leading = { PgRadio(current == tag) })
                    }
                }
            }
        }
    }
}

@Composable
fun SoundsPage(onBack: () -> Unit) {
    val ctx = androidx.compose.ui.platform.LocalContext.current
    val prefs by AlertPrefs.flow(ctx).collectAsState()
    val quiet = prefs["quiet"] ?: false
    var palette by remember { mutableStateOf(CallSounds.palette(ctx)) }
    val sets = listOf(Triple("classic", stringResource(R.string.settings_sound_classic), stringResource(R.string.settings_sound_classic_sub)),
        Triple("slack", stringResource(R.string.settings_sound_slack), stringResource(R.string.settings_sound_slack_sub)),
        Triple("triad", stringResource(R.string.settings_sound_triad), stringResource(R.string.settings_sound_triad_sub)))
    PfPage(stringResource(R.string.settings_sounds), onBack) {
        PfSec(stringResource(R.string.settings_quiet), first = true) {
            PfCard {
                act {
                    PfAct(stringResource(R.string.settings_quiet), onClick = { AlertPrefs.set(ctx, "quiet", !quiet) }, icon = PgIcons.Mute, sub = stringResource(R.string.settings_quiet_sub), role = Role.Switch, plain = true,
                        trailing = { PgSwitch(quiet, { AlertPrefs.set(ctx, "quiet", it) }, stringResource(R.string.settings_quiet)) })
                }
            }
        }
        PfSec(stringResource(R.string.settings_call_sounds)) {
            PfCard {
                sets.forEach { (k, label, sub) ->
                    act { PfRadioRow(label, sub, palette == k, { palette = k; CallSounds.setPalette(ctx, k); CallSounds.preview(ctx, "ring") }, minHeight = 64.dp) }
                }
            }
            Row(Modifier.fillMaxWidth().padding(start = 4.dp, end = 4.dp, top = 12.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                listOf("ring" to stringResource(R.string.settings_cue_ring), "connected" to stringResource(R.string.settings_cue_connected), "ended" to stringResource(R.string.settings_cue_ended)).forEach { (cue, label) ->
                    PgChip(label, false, { CallSounds.preview(ctx, cue) }, icon = PgIcons.Vol)
                }
            }
            PfNote(if (quiet) stringResource(R.string.settings_sounds_note_quiet) else stringResource(R.string.settings_sounds_note))
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
    PfPage(stringResource(R.string.settings_video), onBack) {
        PfSec(stringResource(R.string.settings_video_quality), first = true) {
            PfCard {
                act {
                    PfAct(stringResource(R.string.settings_sharper_video), onClick = { set(!hd) }, icon = PgIcons.Video, role = Role.Switch, plain = true,
                        sub = if (hd) stringResource(R.string.settings_sharper_on) else stringResource(R.string.settings_sharper_off),
                        trailing = { PgSwitch(hd, set, stringResource(R.string.settings_sharper_video)) })
                }
            }
            PfNote(stringResource(R.string.settings_video_note))
        }
    }
}

@Composable
private fun fmtBytes(n: Long): String = when {
    n >= 1_000_000_000L -> stringResource(R.string.settings_size_gb, "%.1f".format(n / 1e9))
    n >= 1_000_000L -> stringResource(R.string.settings_size_mb, Math.round(n / 1e6).toString())
    n >= 1_000L -> stringResource(R.string.settings_size_kb, Math.round(n / 1e3).toString())
    else -> stringResource(R.string.settings_size_b, n.toString())
}

/** Storage.svelte: what Tinline keeps here (one bar), when pictures download, and clearing the cache. */
@Composable
fun StoragePage(pictures: Long, files: Long, messages: Long, cache: Long, download: uniffi.p2pcore.PictureDownload, cleared: Boolean,
                onDownload: (uniffi.p2pcore.PictureDownload) -> Unit, onClear: () -> Unit, onBack: () -> Unit) {
    val c = Tin.c
    val kinds = listOf(Triple(stringResource(R.string.settings_storage_pictures), pictures, c.accent), Triple(stringResource(R.string.settings_storage_files), files, c.thread), Triple(stringResource(R.string.settings_storage_messages), messages, c.ink2), Triple(stringResource(R.string.settings_storage_cache), cache, c.ln))
    val total = kinds.sumOf { it.second }
    PfPage(stringResource(R.string.settings_storage), onBack) {
        PfSec(stringResource(R.string.settings_storage_on_phone), first = true) {
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
        PfSec(stringResource(R.string.settings_download_pictures)) {
            val cardRows4 = listOf(Triple(uniffi.p2pcore.PictureDownload.ALWAYS, stringResource(R.string.settings_dl_always), stringResource(R.string.settings_dl_always_sub)),
                    Triple(uniffi.p2pcore.PictureDownload.WIFI, stringResource(R.string.settings_dl_wifi), stringResource(R.string.settings_dl_wifi_sub)),
                    Triple(uniffi.p2pcore.PictureDownload.TAP, stringResource(R.string.settings_dl_tap), stringResource(R.string.settings_dl_tap_sub)))
            PfCard {
                cardRows4.forEach { (d, label, sub) ->
                    act { PfRadioRow(label, sub, download == d, { onDownload(d) }) }
                }
            }
        }
        PfSec {
            PfCard {
                act {
                    PfAct(stringResource(R.string.settings_clear_cache), onClick = if (cache > 0) onClear else null, icon = PgIcons.Trash,
                        sub = if (cache > 0) stringResource(R.string.settings_clear_cache_sub, fmtBytes(cache)) else if (cleared) stringResource(R.string.settings_cleared) else stringResource(R.string.settings_nothing_to_clear))
                }
            }
            PfNote(stringResource(R.string.settings_storage_note))
        }
    }
}

/** Privacy.svelte landing. */
@Composable
fun PrivacyLanding(blockedN: Int, msgOnly: Int, hasPassphrase: Boolean, onBack: () -> Unit, onBlocked: () -> Unit, onCalls: () -> Unit, onPhrase: () -> Unit, onPassphrase: () -> Unit,
                   readReceipts: Boolean = true, onReadReceipts: (Boolean) -> Unit = {},
                   presenceVisible: Boolean = true, onPresenceVisible: (Boolean) -> Unit = {},
                   groupPictures: uniffi.p2pcore.PictureDownload = uniffi.p2pcore.PictureDownload.WIFI, onGroupPictures: (uniffi.p2pcore.PictureDownload) -> Unit = {}) {
    val receiptsOn = stringResource(R.string.settings_receipts_on)
    val receiptsOff = stringResource(R.string.settings_receipts_off)
    val presenceTitle = stringResource(R.string.settings_presence_title)
    val presenceOn = stringResource(R.string.settings_presence_on)
    val presenceOff = stringResource(R.string.settings_presence_off)
    PfPage(stringResource(R.string.settings_privacy), onBack) {
        PfSec(first = true) {
            PfCard {
                act { PfAct(stringResource(R.string.blocking_title), onClick = onBlocked, icon = PgIcons.Off, chevron = true,
                    sub = if (blockedN > 0) pluralStringResource(R.plurals.settings_blocked_count, blockedN, blockedN) else stringResource(R.string.settings_blocked_none)) }
                act { PfAct(stringResource(R.string.settings_calls_allowed_title), onClick = onCalls, icon = PgIcons.Phone, chevron = true,
                    sub = if (msgOnly > 0) pluralStringResource(R.plurals.settings_messages_only_count, msgOnly, msgOnly) else stringResource(R.string.settings_calls_all)) }
                // Native: without a passphrase the phrase is behind the phone's screen lock instead.
                act { PfAct(stringResource(R.string.settings_account_recovery), onClick = onPhrase, icon = PgIcons.Key, chevron = true, sub = if (hasPassphrase) stringResource(R.string.settings_recovery_needs_pass) else stringResource(R.string.settings_recovery_needs_lock)) }
                act { PfAct(stringResource(R.string.settings_passphrase), onClick = onPassphrase, icon = PgIcons.Lock, chevron = true, sub = stringResource(R.string.settings_passphrase_sub)) }
            }
            PfNote(stringResource(R.string.settings_privacy_note))
        }
        // Core account setting (default on). Same words as desktop.
        PfSec {
            PfCard {
                act {
                    PfAct(stringResource(R.string.settings_read_receipts), onClick = { onReadReceipts(!readReceipts) }, icon = PgIcons.Check, role = Role.Switch, plain = true,
                        sub = if (readReceipts) receiptsOn else receiptsOff,
                        subReserve = listOf(receiptsOn, receiptsOff), // both heights reserved: flipping moves nothing
                        trailing = { PgSwitch(readReceipts, onReadReceipts, stringResource(R.string.settings_read_receipts)) })
                }
            }
            PfNote(stringResource(R.string.settings_receipts_note))
        }
        // Public group §3: a setting of this install (group core, default on). Same words as desktop; the title may wrap.
        PfSec {
            PfCard {
                act {
                    PfAct(presenceTitle, onClick = { onPresenceVisible(!presenceVisible) }, icon = PgIcons.Eye, role = Role.Switch, plain = true, titleMaxLines = 2,
                        sub = if (presenceVisible) presenceOn else presenceOff,
                        subReserve = listOf(presenceOn, presenceOff),
                        trailing = { PgSwitch(presenceVisible, onPresenceVisible, presenceTitle) })
                }
            }
            PfNote(stringResource(R.string.settings_presence_note))
        }
        // Public group §5: when group pictures load by themselves on this phone (group core, default Wi-Fi only).
        PfSec(stringResource(R.string.settings_group_pictures)) {
            val cardRows5 = listOf(Triple(uniffi.p2pcore.PictureDownload.ALWAYS, stringResource(R.string.settings_dl_always), stringResource(R.string.settings_dl_always_sub)),
                    Triple(uniffi.p2pcore.PictureDownload.WIFI, stringResource(R.string.settings_dl_wifi), stringResource(R.string.settings_dl_wifi_sub)),
                    Triple(uniffi.p2pcore.PictureDownload.TAP, stringResource(R.string.settings_dl_tap), stringResource(R.string.settings_dl_tap_sub)))
            PfCard {
                cardRows5.forEach { (d, label, sub) ->
                    act { PfRadioRow(label, sub, groupPictures == d, { onGroupPictures(d) }) }
                }
            }
            PfNote(stringResource(R.string.settings_group_pictures_note))
        }
    }
}

data class CallsRow(val did: String, val name: String, val allowed: Boolean)

/** CallsAllowed.svelte: each contact's Allow calls (the same switch as on their profile). */
@Composable
fun CallsAllowedPage(rows: List<CallsRow>, onBack: () -> Unit, onAllow: (String, Boolean) -> Unit) {
    PfPage(stringResource(R.string.settings_calls_allowed_title), onBack) {
        PfSec(stringResource(R.string.settings_contacts), first = true) {
            PfCard {
                rows.sortedBy { it.name.lowercase() }.forEach { r ->
                    row {
                        PersonRowPg(r.name, r.did, if (r.allowed) stringResource(R.string.settings_can_call) else stringResource(R.string.settings_messages_only), pad = 12.dp) {
                            PgSwitch(r.allowed, { onAllow(r.did, it) }, stringResource(R.string.settings_allow_calls_from, r.name))
                        }
                    }
                }
            }
            PfNote(stringResource(R.string.settings_calls_allowed_note))
        }
    }
}
