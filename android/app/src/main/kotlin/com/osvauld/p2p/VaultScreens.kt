package com.osvauld.p2p

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.p2pcore.Exception as CoreError

const val MIN_PASSPHRASE = 8

const val PASSPHRASE_WHY =
    "Protects your recovery phrase and keys on this phone. You'll need it to view your recovery phrase " +
        "or if this phone forgets it. It can't be recovered — your recovery phrase can restore your account."

/** Friendly text for a core error; never includes secrets. */
fun friendly(e: Throwable): String = when (e) {
    is CoreError.WrongPassphrase -> "Wrong passphrase"
    is CoreError.WeakPassphrase -> "At least $MIN_PASSPHRASE characters"
    is CoreError.Locked -> "Locked — enter your passphrase to unlock"
    is CoreError.BadPhrase -> "That recovery phrase is not valid"
    else -> e.message ?: "Something went wrong"
}

/** Null when the pair is acceptable, else what to tell the user. */
fun passphraseProblem(pass: String, confirm: String): String? = when {
    pass.length < MIN_PASSPHRASE -> "At least $MIN_PASSPHRASE characters"
    pass != confirm -> "Passphrases don't match"
    else -> null
}

@Composable
fun PassField(value: String, onChange: (String) -> Unit, label: String, enabled: Boolean = true) {
    OutlinedTextField(
        value, onChange, label = { Text(label) }, singleLine = true, enabled = enabled,
        visualTransformation = PasswordVisualTransformation(), modifier = Modifier.fillMaxWidth(),
    )
}

/** Passphrase + confirm with the rule shown up front. */
@Composable
fun NewPassphraseFields(pass: String, confirm: String, onPass: (String) -> Unit, onConfirm: (String) -> Unit, enabled: Boolean = true) {
    Text(PASSPHRASE_WHY, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
    PassField(pass, onPass, "Passphrase (at least $MIN_PASSPHRASE characters)", enabled)
    PassField(confirm, onConfirm, "Confirm passphrase", enabled)
    if (confirm.isNotEmpty()) passphraseProblem(pass, confirm)?.let { Text(it, color = MaterialTheme.colorScheme.error) }
}

/** Shown while the identity is Locked and no device key could unlock it. */
@Composable
fun UnlockScreen(app: P2pApp, onForgot: () -> Unit) {
    val scope = rememberCoroutineScope()
    var pass by rememberSaveable { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val name = remember { app.node.profile()?.name ?: "" }
    Column(
        Modifier.fillMaxSize().systemBarsPadding().verticalScroll(rememberScrollState()).padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Spacer(Modifier.height(48.dp))
        Text("Unlock", style = MaterialTheme.typography.headlineMedium, fontWeight = FontWeight.SemiBold)
        Text(
            if (name.isNotBlank()) "Enter the passphrase for $name to receive calls."
            else "Enter your passphrase to receive calls.",
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        PassField(pass, { pass = it; error = null }, "Passphrase", !busy)
        error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        if (busy) LinearProgressIndicator(Modifier.fillMaxWidth())
        Button(
            onClick = {
                busy = true; error = null
                scope.launch {
                    val r = withContext(Dispatchers.IO) { runCatching { app.node.unlock(pass) } }
                    busy = false
                    r.onFailure { error = friendly(it) }
                    r.onSuccess { pass = ""; app.identityReady() }
                }
            },
            enabled = pass.isNotEmpty() && !busy, modifier = Modifier.fillMaxWidth().height(52.dp),
        ) { Text(if (busy) "Unlocking..." else "Unlock") }
        TextButton(onClick = onForgot, enabled = !busy, modifier = Modifier.align(Alignment.CenterHorizontally)) {
            Text("Forgot passphrase? Restore with recovery phrase")
        }
    }
}

/** Legacy installs: usable already, but the user must seal the identity under a passphrase. */
@Composable
fun SetPassphraseScreen(app: P2pApp) {
    val scope = rememberCoroutineScope()
    var pass by rememberSaveable { mutableStateOf("") }
    var confirm by rememberSaveable { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    Column(
        Modifier.fillMaxSize().systemBarsPadding().verticalScroll(rememberScrollState()).padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Spacer(Modifier.height(24.dp))
        Text("Set a passphrase", style = MaterialTheme.typography.headlineMedium, fontWeight = FontWeight.SemiBold)
        Text(
            "Your recovery phrase is currently stored unprotected on this phone. Choose a passphrase to encrypt it. " +
                "Calls keep working while you do this.",
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        NewPassphraseFields(pass, confirm, { pass = it; error = null }, { confirm = it; error = null }, !busy)
        error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        if (busy) LinearProgressIndicator(Modifier.fillMaxWidth())
        Button(
            onClick = {
                busy = true; error = null
                scope.launch {
                    val r = withContext(Dispatchers.IO) { runCatching { app.node.setPassphrase(null, pass) } }
                    busy = false
                    r.onFailure { error = friendly(it) }
                    r.onSuccess { pass = ""; confirm = ""; app.identityReady() }
                }
            },
            enabled = passphraseProblem(pass, confirm) == null && !busy,
            modifier = Modifier.fillMaxWidth().height(52.dp),
        ) { Text(if (busy) "Encrypting..." else "Set passphrase") }
    }
}

/** Settings dialog: ask for the passphrase, then reveal the phrase via the callback. */
@Composable
fun ShowPhraseDialog(app: P2pApp, onPhrase: (String) -> Unit, onDismiss: () -> Unit) {
    val scope = rememberCoroutineScope()
    var pass by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    AlertDialog(
        onDismissRequest = { if (!busy) onDismiss() },
        title = { Text("Enter your passphrase") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                PassField(pass, { pass = it; error = null }, "Passphrase", !busy)
                error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
                if (busy) LinearProgressIndicator(Modifier.fillMaxWidth())
            }
        },
        confirmButton = {
            TextButton(enabled = pass.isNotEmpty() && !busy, onClick = {
                busy = true; error = null
                scope.launch {
                    val r = withContext(Dispatchers.IO) { runCatching { app.node.recoveryPhrase(pass) } }
                    busy = false
                    r.onFailure { error = friendly(it) }
                    r.onSuccess { onPhrase(it) }
                }
            }) { Text("Show") }
        },
        dismissButton = { TextButton(onClick = onDismiss, enabled = !busy) { Text("Cancel") } },
    )
}

@Composable
fun ChangePassphraseDialog(app: P2pApp, onDone: () -> Unit) {
    val scope = rememberCoroutineScope()
    var old by remember { mutableStateOf("") }
    var pass by remember { mutableStateOf("") }
    var confirm by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    AlertDialog(
        onDismissRequest = { if (!busy) onDone() },
        title = { Text("Change passphrase") },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                PassField(old, { old = it; error = null }, "Current passphrase", !busy)
                PassField(pass, { pass = it; error = null }, "New passphrase (at least $MIN_PASSPHRASE characters)", !busy)
                PassField(confirm, { confirm = it; error = null }, "Confirm new passphrase", !busy)
                if (confirm.isNotEmpty()) passphraseProblem(pass, confirm)?.let { Text(it, color = MaterialTheme.colorScheme.error) }
                error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
                if (busy) LinearProgressIndicator(Modifier.fillMaxWidth())
            }
        },
        confirmButton = {
            TextButton(enabled = old.isNotEmpty() && passphraseProblem(pass, confirm) == null && !busy, onClick = {
                busy = true; error = null
                scope.launch {
                    val r = withContext(Dispatchers.IO) { runCatching { app.node.setPassphrase(old, pass) } }
                    busy = false
                    r.onFailure { error = friendly(it) }
                    r.onSuccess { app.rememberKey(); onDone() }
                }
            }) { Text("Change") }
        },
        dismissButton = { TextButton(onClick = onDone, enabled = !busy) { Text("Cancel") } },
    )
}
