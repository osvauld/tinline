package com.osvauld.p2p

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.ui.text.input.KeyboardType
import com.journeyapps.barcodescanner.ScanContract
import com.journeyapps.barcodescanner.ScanOptions
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.p2pcore.Contact

/** Copies [text]; flagged sensitive so Android 13+ hides it from the clipboard preview. */
private fun copy(c: Context, label: String, text: String) {
    val clip = ClipData.newPlainText(label, text)
    if (android.os.Build.VERSION.SDK_INT >= 33) {
        clip.description.extras = android.os.PersistableBundle().apply {
            putBoolean(android.content.ClipDescription.EXTRA_IS_SENSITIVE, true)
        }
    }
    (c.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager).setPrimaryClip(clip)
}

private fun clipboardText(c: Context): String? =
    (c.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager).primaryClip?.getItemAt(0)?.coerceToText(c)?.toString()

// ---------------------------------------------------------------- onboarding

@Composable
fun OnboardingScreen(app: P2pApp, forgot: Boolean = false, onDone: () -> Unit) {
    val scope = rememberCoroutineScope()
    var name by rememberSaveable { mutableStateOf(if (forgot) app.node.profile()?.name ?: "" else "") }
    var restore by rememberSaveable { mutableStateOf(forgot) }
    // Secrets are never saved into the instance-state bundle.
    var pass by remember { mutableStateOf("") }
    var confirm by remember { mutableStateOf("") }
    var phraseIn by remember { mutableStateOf("") }
    var shownPhrase by remember { mutableStateOf<String?>(null) }
    var saved by rememberSaveable { mutableStateOf(false) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    // After process death the recovery phrase is gone but the identity exists: do not show the
    // create form again over it (the phrase can be viewed in Settings with the passphrase).
    val has by app.hasIdentity.collectAsState()
    LaunchedEffect(has, shownPhrase) { if (!forgot && has && shownPhrase == null && !busy) onDone() }

    Column(
        Modifier.fillMaxSize().systemBarsPadding().verticalScroll(rememberScrollState()).padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Spacer(Modifier.height(24.dp))
        Box(
            Modifier.size(64.dp).clip(CircleShape).background(MaterialTheme.colorScheme.primaryContainer),
            contentAlignment = Alignment.Center,
        ) { Icon(Icons.Default.Call, null, tint = MaterialTheme.colorScheme.onPrimaryContainer, modifier = Modifier.size(32.dp)) }
        if (shownPhrase != null) {
            Text("Your recovery phrase", style = MaterialTheme.typography.headlineMedium, fontWeight = FontWeight.SemiBold)
            Text(
                "These 24 words are your identity. Write them down and keep them private: they are the only way to restore your account on a new phone.",
                style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            PhraseGrid(shownPhrase.orEmpty())
            Row(verticalAlignment = Alignment.CenterVertically) {
                Checkbox(saved, { saved = it })
                Text("I have saved my recovery phrase")
            }
            Button(onClick = onDone, enabled = saved, modifier = Modifier.fillMaxWidth().height(52.dp)) { Text("Continue") }
        } else {
            Text("Private voice calls", style = MaterialTheme.typography.headlineMedium, fontWeight = FontWeight.SemiBold)
            Text(
                "Peer-to-peer, end-to-end encrypted. No accounts, no servers that hear you. Choose a name your contacts will see.",
                style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            OutlinedTextField(
                name, { name = it }, label = { Text("Your name") }, singleLine = true, modifier = Modifier.fillMaxWidth(),
            )
            if (restore) {
                if (forgot) Text(
                    "Restoring keeps the same identity: use the recovery phrase of this account. Your contacts stay.",
                    style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.error,
                )
                OutlinedTextField(
                    phraseIn, { phraseIn = it }, label = { Text("24-word recovery phrase") },
                    modifier = Modifier.fillMaxWidth(), minLines = 3, enabled = !busy,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password, autoCorrectEnabled = false),
                )
            }
            NewPassphraseFields(pass, confirm, { pass = it; error = null }, { confirm = it; error = null }, !busy)
            error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            if (busy) LinearProgressIndicator(Modifier.fillMaxWidth())
            Button(
                onClick = {
                    busy = true; error = null
                    scope.launch {
                        val r = withContext(Dispatchers.IO) {
                            runCatching {
                                if (forgot) { app.restoreOverLocked(phraseIn, name.trim(), pass); null }
                                else if (restore) { app.node.restoreIdentity(phraseIn, name.trim(), pass); null }
                                else app.node.createIdentity(name.trim(), pass)
                            }
                        }
                        busy = false
                        r.onFailure { error = friendly(it) }
                        r.onSuccess { p ->
                            pass = ""; confirm = ""
                            app.identityReady()
                            if (p == null) onDone() else shownPhrase = p
                        }
                    }
                },
                enabled = name.isNotBlank() && !busy && (!restore || phraseIn.isNotBlank()) && passphraseProblem(pass, confirm) == null,
                modifier = Modifier.fillMaxWidth().height(52.dp),
            ) { Text(if (busy) "Encrypting..." else if (restore) "Restore identity" else "Create identity") }
            if (!forgot) TextButton(onClick = { restore = !restore; error = null }, modifier = Modifier.align(Alignment.CenterHorizontally)) {
                Text(if (restore) "Create a new identity instead" else "I already have a recovery phrase")
            }
        }
    }
}

@Composable
fun PhraseGrid(phrase: String) {
    val words = phrase.trim().split(Regex("\\s+"))
    Surface(shape = RoundedCornerShape(16.dp), color = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.5f)) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            words.chunked(3).forEachIndexed { r, row ->
                Row(Modifier.fillMaxWidth()) {
                    row.forEachIndexed { i, w ->
                        Text(
                            "${r * 3 + i + 1}. $w", Modifier.weight(1f),
                            fontFamily = FontFamily.Monospace, style = MaterialTheme.typography.bodyMedium,
                        )
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------- home

@OptIn(ExperimentalMaterial3Api::class, ExperimentalFoundationApi::class)
@Composable
fun HomeScreen(
    app: P2pApp, missing: List<Need>, onFix: (Need) -> Unit, onAdd: () -> Unit, onSettings: () -> Unit,
) {
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    val status by app.status.collectAsState()
    val contacts by app.contacts.collectAsState()
    var ticket by remember { mutableStateOf<String?>(null) }
    var ticketErr by remember { mutableStateOf<String?>(null) }
    var toRemove by remember { mutableStateOf<Contact?>(null) }
    var callErr by remember { mutableStateOf<String?>(null) }
    val online = status?.online == true
    LaunchedEffect(online, contacts.size) {
        withContext(Dispatchers.IO) {
            runCatching { app.node.myTicket() }.onSuccess { ticket = it; ticketErr = null }.onFailure { ticketErr = it.message }
        }
    }
    val profile = remember(contacts) { app.node.profile() }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Tinline", fontWeight = FontWeight.SemiBold) },
                actions = { IconButton(onSettings) { Icon(Icons.Default.Settings, "Settings") } },
            )
        },
        floatingActionButton = {
            ExtendedFloatingActionButton(onClick = onAdd, icon = { Icon(Icons.Default.Add, null) }, text = { Text("Add contact") })
        },
    ) { pad ->
        LazyColumn(
            Modifier.fillMaxSize().padding(pad),
            contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 8.dp, bottom = 96.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            item {
                StatusChip(online, status?.relay)
            }
            if (missing.isNotEmpty()) item {
                Card(colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.errorContainer.copy(alpha = 0.6f))) {
                    Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            Icon(Icons.Default.Warning, null)
                            Text("Finish setup to receive calls", fontWeight = FontWeight.SemiBold)
                        }
                        missing.forEach { n ->
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Column(Modifier.weight(1f)) {
                                    Text(n.title, style = MaterialTheme.typography.bodyLarge)
                                    Text(n.why, style = MaterialTheme.typography.bodySmall)
                                }
                                FilledTonalButton(onClick = { onFix(n) }) { Text("Allow") }
                            }
                        }
                    }
                }
            }
            item {
                Card {
                    Column(Modifier.padding(16.dp).fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally) {
                        Text("My contact card", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
                        Text(
                            profile?.name ?: "", style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        Spacer(Modifier.height(12.dp))
                        val t = ticket
                        if (t != null) {
                            val bmp = remember(t) { qrBitmap(t) }
                            Image(
                                bmp.asImageBitmap(), "QR code of your contact ticket",
                                Modifier.size(220.dp).clip(RoundedCornerShape(12.dp)).background(Color.White).padding(6.dp),
                            )
                            Spacer(Modifier.height(12.dp))
                            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                OutlinedButton(onClick = { copy(ctx, "ticket", t) }) { Text("Copy") }
                                Button(onClick = {
                                    try {
                                        ctx.startActivity(Intent.createChooser(
                                            Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, t), "Share contact card"))
                                    } catch (_: Exception) { callErr = "Nothing to share with" }
                                }) {
                                    Icon(Icons.Default.Share, null, Modifier.size(18.dp)); Spacer(Modifier.width(6.dp)); Text("Share")
                                }
                            }
                        } else {
                            Text(ticketErr ?: "Preparing your card...", color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    }
                }
            }
            item {
                Text("Contacts", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold,
                    modifier = Modifier.padding(top = 8.dp))
                callErr?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            }
            if (contacts.isEmpty()) item {
                Text(
                    "No contacts yet. Add someone by scanning their QR code or pasting their contact card.",
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            items(contacts, key = { it.did }) { c ->
                Card(
                    Modifier.fillMaxWidth().combinedClickable(
                        onClick = {
                            callErr = null
                            if (!Perms.granted(ctx, android.Manifest.permission.RECORD_AUDIO)) {
                                callErr = "Microphone permission needed to place a call"
                                onFix(Need.Mic)
                            } else scope.launch {
                                val r = withContext(Dispatchers.IO) { app.calls.place(c.did) }
                                r.onFailure { callErr = it.message }
                            }
                        },
                        onLongClick = { toRemove = c },
                    ),
                ) {
                    Row(Modifier.padding(horizontal = 16.dp, vertical = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                        Box(
                            Modifier.size(44.dp).clip(CircleShape).background(MaterialTheme.colorScheme.primaryContainer),
                            contentAlignment = Alignment.Center,
                        ) {
                            Text(c.name.take(1).uppercase().ifEmpty { "?" }, color = MaterialTheme.colorScheme.onPrimaryContainer,
                                fontWeight = FontWeight.Bold)
                        }
                        Spacer(Modifier.width(14.dp))
                        Column(Modifier.weight(1f)) {
                            Text(c.name.ifBlank { "Unnamed" }, style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.Medium)
                            Text(c.did, style = MaterialTheme.typography.bodySmall, maxLines = 1, overflow = TextOverflow.Ellipsis,
                                color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                        Icon(Icons.Default.Call, "Call", tint = MaterialTheme.colorScheme.primary)
                    }
                }
            }
        }
    }
    toRemove?.let { c ->
        AlertDialog(
            onDismissRequest = { toRemove = null },
            title = { Text("Remove ${c.name}?") },
            text = { Text("They will no longer be able to call you.") },
            confirmButton = {
                TextButton(onClick = {
                    toRemove = null
                    scope.launch(Dispatchers.IO) { runCatching { app.node.removeContact(c.did) }; app.refresh() }
                }) { Text("Remove") }
            },
            dismissButton = { TextButton(onClick = { toRemove = null }) { Text("Cancel") } },
        )
    }
}

@Composable
fun StatusChip(online: Boolean, relay: String?) {
    val color = if (online) MaterialTheme.colorScheme.tertiary else MaterialTheme.colorScheme.error
    Surface(shape = RoundedCornerShape(50), color = color.copy(alpha = 0.12f)) {
        Row(Modifier.padding(horizontal = 14.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            Canvas(Modifier.size(10.dp)) { drawCircle(color) }
            Spacer(Modifier.width(8.dp))
            Text(
                if (online) "Online" else "Offline", color = color, fontWeight = FontWeight.Medium,
                style = MaterialTheme.typography.labelLarge,
            )
        }
    }
}

// ---------------------------------------------------------------- add contact

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AddContactScreen(app: P2pApp, onBack: () -> Unit) {
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    var text by rememberSaveable { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val scan = rememberLauncherForScan { text = it }

    fun add() {
        busy = true; error = null
        scope.launch {
            val r = withContext(Dispatchers.IO) { runCatching { app.node.addContact(text.trim()) } }
            busy = false
            r.onSuccess { testLog("added name=${it.name} did=${it.did}"); app.refresh(); onBack() }
                .onFailure { error = it.message ?: "Could not add contact" }
        }
    }

    Scaffold(topBar = {
        TopAppBar(title = { Text("Add contact") }, navigationIcon = {
            IconButton(onBack) { Icon(Icons.Default.ArrowBack, "Back") }
        })
    }) { pad ->
        Column(Modifier.padding(pad).padding(16.dp).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(14.dp)) {
            Text(
                "Scan their QR code, or paste the contact card they sent you. Both of you need to be online while adding.",
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Button(onClick = { scan() }, modifier = Modifier.fillMaxWidth().height(52.dp)) { Text("Scan QR code") }
            OutlinedTextField(
                text, { text = it }, label = { Text("Contact card (OSVC2:...)") },
                modifier = Modifier.fillMaxWidth(), minLines = 3, maxLines = 6,
            )
            OutlinedButton(onClick = { clipboardText(ctx)?.let { text = it } }, modifier = Modifier.fillMaxWidth()) { Text("Paste from clipboard") }
            error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            if (busy) LinearProgressIndicator(Modifier.fillMaxWidth())
            Button(onClick = { add() }, enabled = text.isNotBlank() && !busy, modifier = Modifier.fillMaxWidth().height(52.dp)) {
                Text(if (busy) "Connecting..." else "Add contact")
            }
        }
    }
}

@Composable
private fun rememberLauncherForScan(onResult: (String) -> Unit): () -> Unit {
    val l = androidx.activity.compose.rememberLauncherForActivityResult(ScanContract()) { r -> r.contents?.let(onResult) }
    return {
        l.launch(ScanOptions().setDesiredBarcodeFormats(ScanOptions.QR_CODE).setPrompt("Scan a contact QR code")
            .setBeepEnabled(false).setOrientationLocked(false))
    }
}

// ---------------------------------------------------------------- settings

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(app: P2pApp, onBack: () -> Unit) {
    val scope = rememberCoroutineScope()
    var name by rememberSaveable { mutableStateOf(app.node.profile()?.name ?: "") }
    var saved by remember { mutableStateOf(false) }
    var phrase by remember { mutableStateOf<String?>(null) }
    var askPhrase by remember { mutableStateOf(false) }
    var changePass by remember { mutableStateOf(false) }
    var tone by remember { mutableStateOf(app.testToneHz != null) }
    Scaffold(topBar = {
        TopAppBar(title = { Text("Settings") }, navigationIcon = {
            IconButton(onBack) { Icon(Icons.Default.ArrowBack, "Back") }
        })
    }) { pad ->
        Column(Modifier.padding(pad).padding(16.dp).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            Text("Profile", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
            OutlinedTextField(name, { name = it; saved = false }, label = { Text("Display name") }, singleLine = true, modifier = Modifier.fillMaxWidth())
            Button(onClick = {
                scope.launch(Dispatchers.IO) { runCatching { app.node.setName(name.trim()) }; saved = true }
            }, enabled = name.isNotBlank()) { Text(if (saved) "Saved" else "Save name") }
            HorizontalDivider()
            Text("Recovery phrase", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
            if (phrase != null) {
                PhraseGrid(phrase.orEmpty())
                TextButton(onClick = { phrase = null }) { Text("Hide") }
            } else {
                OutlinedButton(onClick = { askPhrase = true }) { Icon(Icons.Default.Lock, null, Modifier.size(18.dp)); Spacer(Modifier.width(8.dp)); Text("Show recovery phrase") }
            }
            HorizontalDivider()
            Text("Passphrase", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
            OutlinedButton(onClick = { changePass = true }) { Text("Change passphrase") }
            if (BuildConfig.DEBUG) {
            HorizontalDivider()
            Text("Diagnostics", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text("Test tone")
                    Text("Send a 440 Hz tone instead of the microphone", style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                Switch(tone, { tone = it; app.testToneHz = if (it) 440f else null; app.node.setTestTone(app.testToneHz) })
            }
            }
            Text("p2pcore ${uniffi.p2pcore.coreVersion()}", style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
    if (askPhrase) ShowPhraseDialog(app, { phrase = it; askPhrase = false }, { askPhrase = false })
    if (changePass) ChangePassphraseDialog(app) { changePass = false }
}
