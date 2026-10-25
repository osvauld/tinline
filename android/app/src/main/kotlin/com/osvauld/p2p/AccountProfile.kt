package com.osvauld.p2p

import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext

/** Private account actions are navigation, never fields in the synchronized profile. */
@Composable
internal fun AccountProfileScreen(
    store: ProfileStore, fallbackName: String, onBack: () -> Unit,
    onEdit: () -> Unit, onShare: () -> Unit, onRecovery: () -> Unit,
) {
    val me by store.self.collectAsState()
    val profile = rememberProfileView(me)
    val name = profile?.text("name")?.takeIf { it.isNotBlank() } ?: fallbackName
    PfPage(stringResource(R.string.profile_my_profile), onBack, bottomPad = 24.dp) {
        Column(Modifier.fillMaxWidth().padding(horizontal = 24.dp, vertical = 24.dp),
            horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(16.dp)) {
            ViewablePgAvatar(name, 0, 104.dp, person = me)
            Text(name, style = TinType.titleL, color = Tin.c.ink, textAlign = TextAlign.Center)
            PBtn(stringResource(R.string.profile_edit_profile), onEdit, icon = PgIcons.Edit)
        }
        PfSec(first = true) {
            PfCard { act { PfAct(stringResource(R.string.profile_share_my_code), onClick = onShare, icon = PgIcons.UserPlus, chevron = true, sub = stringResource(R.string.profile_share_my_code_sub)) } }
        }
        PfSec(stringResource(R.string.profile_only_you)) {
            PfCard { act { PfAct(stringResource(R.string.profile_account_recovery), onClick = onRecovery, icon = PgIcons.Key, chevron = true, sub = stringResource(R.string.profile_account_recovery_sub)) } }
        }
    }
}

/** Contact-only sharing. There is deliberately no recovery-secret parameter. */
@Composable
internal fun MyContactCodeScreen(store: ProfileStore, fallbackName: String, code: String?, failed: Boolean, onBack: () -> Unit, onRetry: () -> Unit) {
    val ctx = LocalContext.current
    val me by store.self.collectAsState()
    val profile = rememberProfileView(me)
    val name = profile?.text("name")?.takeIf { it.isNotBlank() } ?: fallbackName
    val copyFailedMsg = stringResource(R.string.profile_code_copy_failed)
    val shareFailedMsg = stringResource(R.string.profile_code_share_failed)
    val codeLabel = stringResource(R.string.profile_code_clip_label)
    var qrMode by remember { mutableStateOf(false) }
    var copied by remember(code) { mutableStateOf(false) }
    var error by remember(code) { mutableStateOf("") }
    LaunchedEffect(copied) { if (copied) { delay(1600); copied = false } }
    val qr by produceState<ImageBitmap?>(null, code, qrMode) {
        value = null
        if (qrMode && code != null) value = withContext(Dispatchers.Default) { runCatching { qrBitmap(code).asImageBitmap() }.getOrNull() }
    }
    Page {
        TopBar(stringResource(R.string.profile_code_title), onBack)
        Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = 24.dp, vertical = 16.dp),
            horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(20.dp)) {
            Lead(if (qrMode) stringResource(R.string.profile_code_lead_qr) else stringResource(R.string.profile_code_lead_text))
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                ViewablePgAvatar(name, 0, 48.dp, person = me)
                Text(name, Modifier.weight(1f, fill = false), style = TinType.titleM, color = Tin.c.ink)
            }
            if (code == null) {
                Hint(if (failed) stringResource(R.string.profile_code_load_failed) else stringResource(R.string.profile_code_loading))
                if (failed) PBtn(stringResource(R.string.common_try_again), onRetry)
            } else if (qrMode) {
                Box(Modifier.sizeIn(maxWidth = 260.dp, maxHeight = 260.dp).aspectRatio(1f).clip(RoundedCornerShape(16.dp)).background(Color.White).padding(8.dp), contentAlignment = Alignment.Center) {
                    qr?.let { Image(it, stringResource(R.string.profile_code_qr_desc), Modifier.fillMaxSize()) }
                }
            } else KeyCard(code)
            if (code != null) PBtn(if (qrMode) stringResource(R.string.profile_code_use_code) else stringResource(R.string.profile_code_show_qr), {
                qrMode = !qrMode; error = ""; copied = false
            }, variant = PVariant.Quiet)
        }
        Column(Modifier.fillMaxWidth().padding(start = 24.dp, end = 24.dp, bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(error.ifEmpty { if (copied) stringResource(R.string.common_copied) else "" }, Modifier.fillMaxWidth().semantics { liveRegion = LiveRegionMode.Polite }, style = TinType.bodyM, color = Tin.c.ink2)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Box(Modifier.weight(1f)) { TinButton(if (copied) stringResource(R.string.common_copied) else stringResource(R.string.profile_code_copy), {
                    if (code != null) {
                        copied = runCatching { copyToClipboard(ctx, codeLabel, code) }.isSuccess
                        error = if (copied) "" else copyFailedMsg
                    }
                }, enabled = code != null) }
                Box(Modifier.weight(1f)) { TinButton(stringResource(R.string.common_share), {
                    if (code != null) { copied = false; error = if (shareCard(ctx, code)) "" else shareFailedMsg }
                }, enabled = code != null, style = BtnStyle.Outlined) }
            }
        }
    }
}
