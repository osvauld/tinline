package com.osvauld.p2p

import android.app.KeyguardManager
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Call
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay
import uniffi.p2pcore.CallState

private fun ComponentActivity.showOverLock() {
    if (Build.VERSION.SDK_INT >= 27) { setShowWhenLocked(true); setTurnScreenOn(true) }
    else @Suppress("DEPRECATION") window.addFlags(
        WindowManager.LayoutParams.FLAG_SHOW_WHEN_LOCKED or WindowManager.LayoutParams.FLAG_TURN_SCREEN_ON)
    window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
}

/** Asks for the microphone (if missing) and then runs [then] whatever the answer: a call can still be heard without it. */
private fun ComponentActivity.registerMicThen(then: () -> Unit): () -> Unit {
    var pending: (() -> Unit)? = null
    val launcher = registerForActivityResult(androidx.activity.result.contract.ActivityResultContracts.RequestPermission()) {
        pending?.invoke(); pending = null
    }
    return {
        if (Perms.granted(this, android.Manifest.permission.RECORD_AUDIO)) then()
        else { pending = then; launcher.launch(android.Manifest.permission.RECORD_AUDIO) }
    }
}

class CallActivity : ComponentActivity() {
    private lateinit var answerWithMic: () -> Unit

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        secureWindow()
        showOverLock()
        val app = P2pApp.get(this)
        answerWithMic = registerMicThen { app.calls.answer() }
        // Only a fresh launch answers: a recreation (rotation) must not answer a second time.
        if (savedInstanceState == null) handle(intent)
        setContent {
            P2pTheme {
                val ui by app.calls.ui.collectAsState()
                var seen by remember { mutableStateOf(false) }
                if (ui != null) seen = true
                LaunchedEffect(ui, seen) { if (ui == null && seen) finish() }
                LaunchedEffect(Unit) { delay(1500); if (app.calls.ui.value == null) finish() }
                ui?.let { InCallScreen(it, app.calls) }
            }
        }
    }

    override fun onNewIntent(intent: Intent) { super.onNewIntent(intent); setIntent(intent); handle(intent) }

    private fun handle(i: Intent?) {
        if (i?.getBooleanExtra(EXTRA_ANSWER, false) == true) {
            i.removeExtra(EXTRA_ANSWER)
            answerWithMic()
        }
    }

    companion object { const val EXTRA_ANSWER = "answer" }
}

class IncomingCallActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        secureWindow()
        showOverLock()
        val app = P2pApp.get(this)
        val answerWithMic = registerMicThen {
            app.calls.answer()
            startActivity(Intent(this@IncomingCallActivity, CallActivity::class.java)); finish()
        }
        setContent {
            P2pTheme {
                val ui by app.calls.ui.collectAsState()
                val u = ui
                LaunchedEffect(u?.state) {
                    if (u == null) finish()
                    else if (u.state is CallState.Active) {
                        startActivity(Intent(this@IncomingCallActivity, CallActivity::class.java)); finish()
                    }
                }
                if (u != null) CallBackdrop {
                    Spacer(Modifier.weight(1f))
                    Avatar(u.info.peerName)
                    Spacer(Modifier.height(20.dp))
                    Text(u.info.peerName.ifBlank { "Unknown" }, fontSize = 32.sp, fontWeight = FontWeight.SemiBold, color = Color.White)
                    Text("Incoming call", color = Color.White.copy(alpha = 0.7f), fontSize = 16.sp)
                    Spacer(Modifier.weight(1f))
                    Row(Modifier.fillMaxWidth().padding(bottom = 56.dp), horizontalArrangement = Arrangement.SpaceEvenly) {
                        RoundAction(Color(0xFFD93025), "Decline", rotate = 135f) { app.calls.decline(); finish() }
                        RoundAction(Color(0xFF1E9E5A), "Answer") { answerWithMic() }
                    }
                }
            }
        }
    }
}

@Composable
private fun CallBackdrop(content: @Composable ColumnScope.() -> Unit) {
    Box(Modifier.fillMaxSize().background(Color(0xFF0F1220))) {
        Column(Modifier.fillMaxSize().systemBarsPadding().padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally, content = content)
    }
}

@Composable
private fun Avatar(name: String) {
    Box(Modifier.size(120.dp).clip(CircleShape).background(Color(0xFF2F5BEA)), contentAlignment = Alignment.Center) {
        Text(name.take(1).uppercase().ifEmpty { "?" }, color = Color.White, fontSize = 52.sp, fontWeight = FontWeight.Bold)
    }
}

@Composable
private fun RoundAction(color: Color, label: String, rotate: Float = 0f, onClick: () -> Unit) {
    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        FilledIconButton(onClick, Modifier.size(72.dp), colors = IconButtonDefaults.filledIconButtonColors(containerColor = color)) {
            Icon(Icons.Default.Call, label, tint = Color.White, modifier = Modifier.size(32.dp).rotate(rotate))
        }
        Spacer(Modifier.height(8.dp))
        Text(label, color = Color.White.copy(alpha = 0.8f))
    }
}

@Composable
private fun ToggleAction(label: String, icon: Int, on: Boolean, onClick: () -> Unit) {
    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        FilledIconToggleButton(on, { onClick() }, Modifier.size(64.dp), colors = IconButtonDefaults.filledIconToggleButtonColors(
            containerColor = Color.White.copy(alpha = 0.12f), contentColor = Color.White,
            checkedContainerColor = Color.White, checkedContentColor = Color(0xFF0F1220))) {
            Icon(painterResource(icon), label, Modifier.size(28.dp))
        }
        Spacer(Modifier.height(8.dp))
        Text(label, color = Color.White.copy(alpha = 0.8f))
    }
}

@Composable
fun InCallScreen(ui: CallUi, calls: CallController) {
    var now by remember { mutableLongStateOf(System.currentTimeMillis()) }
    LaunchedEffect(Unit) { while (true) { delay(500); now = System.currentTimeMillis() } }
    val status = when (ui.state) {
        is CallState.Dialing -> "Calling..."
        is CallState.Ringing -> if (ui.info.incoming) "Connecting..." else "Ringing..."
        is CallState.Active -> {
            val s = ((now - (ui.activeSinceMs ?: now)) / 1000).coerceAtLeast(0)
            "%02d:%02d".format(s / 60, s % 60)
        }
        is CallState.Ended -> "Call ended"
    }
    CallBackdrop {
        Spacer(Modifier.height(48.dp))
        Avatar(ui.info.peerName)
        Spacer(Modifier.height(20.dp))
        Text(ui.info.peerName.ifBlank { "Unknown" }, fontSize = 30.sp, fontWeight = FontWeight.SemiBold, color = Color.White)
        Text(status, color = Color.White.copy(alpha = 0.7f), fontSize = 18.sp)
        ui.micProblem?.let {
            Spacer(Modifier.height(8.dp))
            Text(it, color = Color(0xFFFFB4AB), fontSize = 14.sp, textAlign = TextAlign.Center)
        }
        ui.stats?.let { s ->
            Spacer(Modifier.height(8.dp))
            val tot = (s.received + s.lost).toDouble()
            val loss = if (tot > 0) 100.0 * s.lost.toDouble() / tot else 0.0
            Text(
                "${if (s.direct) "Direct P2P" else "Relayed (encrypted)"} · rtt ${s.rttMs} ms · loss ${"%.1f".format(loss)}%",
                color = Color.White.copy(alpha = 0.5f), fontSize = 12.sp, textAlign = TextAlign.Center,
            )
        }
        Spacer(Modifier.weight(1f))
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceEvenly) {
            ToggleAction("Mute", android.R.drawable.ic_lock_silent_mode, ui.muted) { calls.toggleMute() }
            ToggleAction("Speaker", android.R.drawable.ic_lock_silent_mode_off, ui.speaker) { calls.toggleSpeaker() }
        }
        Spacer(Modifier.height(40.dp))
        RoundAction(Color(0xFFD93025), "Hang up", rotate = 135f) { calls.hangup() }
        Spacer(Modifier.height(48.dp))
    }
}
