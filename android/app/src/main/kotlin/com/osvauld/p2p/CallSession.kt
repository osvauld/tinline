package com.osvauld.p2p

import android.content.Context
import android.net.Uri
import android.telecom.DisconnectCause
import android.util.Log
import androidx.core.telecom.CallAttributesCompat
import androidx.core.telecom.CallControlResult
import androidx.core.telecom.CallControlScope
import androidx.core.telecom.CallEndpointCompat
import androidx.core.telecom.CallsManager
import kotlinx.coroutines.CoroutineExceptionHandler
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.launch

/**
 * One call as Android sees it, and the audio that goes with it.
 *
 * The call is registered with Telecom (androidx core-telecom, a self-managed call), which then owns
 * audio mode, focus and routing: Bluetooth/wired/hearing-aid headsets with their names, headset
 * buttons, cars and watches, and hold when a phone call takes over. If Telecom refuses the call
 * (no telecom feature, an OEM quirk, a timeout) the [engine] takes routing itself instead
 * ([AudioEngine.takeRouting]); the call itself never depends on Telecom.
 *
 * The address handed to Telecom is a constant: only the display name (the peer's or group's name)
 * is shown by the system, cars and watches, never a DID.
 *
 * Lifecycle: create when the call starts ringing or dialing ([incoming]); [connected] when the
 * core reports it active; [end] exactly once when it is over. Telecom operations run in order on
 * one coroutine; anything issued before Telecom answers is queued.
 */
class CallSession(
    private val scope: CoroutineScope,
    private val ctx: Context,
    private val engine: AudioEngine,
    private val name: String,
    private val incoming: Boolean,
    /**
     * A video call from the start (placed with the camera on, or rung as one): Telecom gets
     * [CallAttributesCompat.CALL_TYPE_VIDEO_CALL] (so the system treats it as one: loudspeaker, no
     * ear detection). A voice call that turns into video later keeps its Telecom type (core-telecom
     * cannot change it) and moves to the speaker with [speakerIfEarpiece].
     */
    private val video: Boolean = false,
    /** Group and video calls: start on the loudspeaker when no headset is connected. */
    @Volatile var preferSpeaker: Boolean,
    /** Answered from outside the app (headset button, car, watch). */
    private val onAnswer: () -> Unit,
    /** Ended or rejected from outside the app. */
    private val onDisconnect: () -> Unit,
    /** The system microphone mute changed (e.g. a car's mute button). */
    private val onSystemMute: (Boolean) -> Unit,
) {
    private enum class Mode { Pending, Telecom, Fallback, Ended }

    private val _routes = MutableStateFlow(RouteState())
    val routes: StateFlow<RouteState> = _routes
    private val _held = MutableStateFlow(false)
    /** Telecom put this call on hold (e.g. a phone call was answered). */
    val held: StateFlow<Boolean> = _held

    @Volatile private var mode = Mode.Pending
    @Volatile private var audioOn = false
    /** Telecom has been told the call is answered/active (answer for incoming, setActive for outgoing). */
    @Volatile private var activated = false
    @Volatile private var endedHere = false
    @Volatile private var placedSpeaker = false
    /** Telecom disconnected the call itself (car, watch, headset): no fallback, the core call is ending. */
    @Volatile private var remoteEnded = false
    private val ops = Channel<suspend CallControlScope.() -> Unit>(Channel.UNLIMITED)
    private var endpoints: List<CallEndpointCompat> = emptyList()
    private var job: Job? = null

    private val errors = CoroutineExceptionHandler { _, e -> Log.w(TAG, "telecom session: $e") }

    init {
        engine.onRoutes = { st -> if (mode == Mode.Fallback) _routes.value = st }
        val cm = manager(ctx)
        if (cm == null) fallback("telecom unavailable") else job = scope.launch(errors) { run(cm) }
    }

    private suspend fun run(cm: CallsManager) {
        val attrs = CallAttributesCompat(
            displayName = name.ifBlank { appString(R.string.call_default_name) },
            address = ADDRESS,
            direction = if (incoming) CallAttributesCompat.DIRECTION_INCOMING else CallAttributesCompat.DIRECTION_OUTGOING,
            callType = callType(),
            callCapabilities = CallAttributesCompat.SUPPORTS_SET_INACTIVE,
        )
        try {
            cm.addCall(
                attrs,
                onAnswer = { _ -> activated = true; guard("answer") { onAnswer() } },
                onDisconnect = { _ ->
                    CallTrace.add("telecom disconnected${if (endedHere) "" else " by system"}")
                    remoteEnded = true
                    ops.close()
                    if (!endedHere) guard("disconnect") { onDisconnect() }
                },
                onSetActive = { setHeld(false) },
                onSetInactive = { setHeld(true) },
            ) {
                synchronized(this@CallSession) {
                    if (mode == Mode.Ended) { launch { disconnect(DisconnectCause(DisconnectCause.LOCAL)) }; return@addCall }
                    mode = Mode.Telecom
                }
                Log.i(TAG, "telecom call added: incoming=$incoming video=$video"); CallTrace.add("telecom added")
                val collectors = mutableListOf<Job>()
                launch {
                    for (op in ops) try { op() } catch (e: Exception) { Log.w(TAG, "telecom op: $e") }
                    // Ops close when the call ends either way; the flows would otherwise keep addCall open.
                    collectors.forEach { it.cancel() }
                }
                collectors += launch {
                    combine(availableEndpoints, currentCallEndpoint) { a, c -> a to c }.collect { (a, c) ->
                        endpoints = a
                        _routes.value = RouteState(routeOf(c), a.map(::routeOf))
                        maybeSpeaker(a, c)
                    }
                }
                collectors += launch {
                    var last: Boolean? = null
                    // Only changes count: our own mute button does not go through Telecom.
                    isMuted.collect { m -> if (last != null && m != last) guard("mute") { onSystemMute(m) }; last = m }
                }
            }
        } catch (e: Exception) {
            fallback("addCall failed: $e")
        } finally {
            finish()
        }
    }

    /** The core says the call is up: answer/activate it in Telecom and start the audio. */
    fun connected() {
        synchronized(this) {
            if (mode == Mode.Ended || audioOn) return
            audioOn = true
            engine.start()
            if (mode == Mode.Fallback) engine.takeRouting(preferSpeaker)
        }
        activate()
    }

    /** The user answered in the app: tell Telecom right away so the audio mode is set before media flows. */
    fun answering() { if (incoming) activate() }

    private fun activate() {
        if (activated) return
        activated = true
        if (incoming) op { log("answer", answer(callType())) }
        else op { log("active", setActive()) }
    }

    private fun callType() = if (video) CallAttributesCompat.CALL_TYPE_VIDEO_CALL else CallAttributesCompat.CALL_TYPE_AUDIO_CALL

    /** The call is over (any reason). Stops the audio and releases Telecom; idempotent. */
    fun end(cause: Int) {
        synchronized(this) {
            if (mode == Mode.Ended) return
            endedHere = true
            val wasTelecom = mode == Mode.Telecom
            mode = Mode.Ended
            if (wasTelecom) ops.trySend { log("disconnect", disconnect(DisconnectCause(cause))) }
            CallTrace.add("telecom end ($cause${if (wasTelecom) "" else ", not in telecom"})")
        }
        ops.close()
        engine.stop()
        audioOn = false
        _held.value = false
    }

    fun select(route: AudioRoute) {
        when (mode) {
            Mode.Telecom -> {
                val ep = endpoints.firstOrNull { it.identifier.toString() == route.key } ?: return
                op { log("route", requestEndpointChange(ep)) }
            }
            Mode.Fallback -> engine.selectRoute(route)
            else -> {}
        }
    }

    /** The speaker button: earpiece/headset <-> loudspeaker. */
    fun toggleSpeaker() { Routes.toggleTarget(_routes.value)?.let(::select) }

    /** Video: the phone leaves the ear, so the earpiece moves to the loudspeaker (headsets stay). */
    fun speakerIfEarpiece() {
        preferSpeaker = true
        if (_routes.value.earpiece) toggleSpeaker()
    }

    private fun maybeSpeaker(available: List<CallEndpointCompat>, current: CallEndpointCompat) {
        // Telecom starts on a headset if there is one, else the earpiece; group calls want the speaker.
        if (!preferSpeaker || placedSpeaker || current.type != CallEndpointCompat.TYPE_EARPIECE) return
        placedSpeaker = true
        available.firstOrNull { it.type == CallEndpointCompat.TYPE_SPEAKER }?.let { ep -> op { log("speaker", requestEndpointChange(ep)) } }
    }

    private fun setHeld(h: Boolean) {
        Log.i(TAG, "telecom ${if (h) "hold" else "resume"}")
        engine.held = h
        _held.value = h
    }

    private fun op(block: suspend CallControlScope.() -> Unit) { ops.trySend(block) }

    private fun fallback(why: String) {
        synchronized(this) {
            if (mode == Mode.Ended || mode == Mode.Fallback || remoteEnded) return
            Log.w(TAG, "$why; routing in-app"); CallTrace.add("telecom fallback")
            mode = Mode.Fallback
            ops.close()
            if (audioOn) engine.takeRouting(preferSpeaker)
        }
    }

    /** Telecom's side is over (disconnected or failed). */
    private fun finish() {
        if (!remoteEnded && mode == Mode.Telecom) fallback("telecom session closed")
    }

    private inline fun guard(what: String, f: () -> Unit) {
        // Exceptions in these callbacks would surface inside Telecom's session; never let them.
        try { f() } catch (e: Exception) { Log.w(TAG, "$what callback: $e") }
    }

    private fun log(what: String, r: CallControlResult) {
        if (r is CallControlResult.Error) { Log.w(TAG, "$what failed: ${r.errorCode}"); CallTrace.add("telecom $what failed ${r.errorCode}") }
    }

    private fun routeOf(e: CallEndpointCompat) = AudioRoute(
        when (e.type) {
            CallEndpointCompat.TYPE_EARPIECE -> AudioRoute.Kind.Earpiece
            CallEndpointCompat.TYPE_SPEAKER -> AudioRoute.Kind.Speaker
            CallEndpointCompat.TYPE_BLUETOOTH -> AudioRoute.Kind.Bluetooth
            CallEndpointCompat.TYPE_WIRED_HEADSET -> AudioRoute.Kind.Wired
            else -> AudioRoute.Kind.Other
        },
        e.name.toString(), e.identifier.toString(),
    )

    companion object {
        const val TAG = "TinCall"
        private val ADDRESS: Uri = Uri.fromParts("tinline", "call", null)
        @Volatile private var cm: CallsManager? = null
        @Volatile private var registered: Boolean? = null

        /** Registers the app with Telecom once; null if this device cannot (the fallback is used). */
        @Synchronized
        fun manager(ctx: Context): CallsManager? {
            if (registered == false) return null
            cm?.let { return it }
            return try {
                CallsManager(ctx.applicationContext).also {
                    // Video calling too: without it Telecom turns a video call into an audio one (earpiece).
                    it.registerAppWithTelecom(CallsManager.CAPABILITY_BASELINE or CallsManager.CAPABILITY_SUPPORTS_VIDEO_CALLING)
                    cm = it; registered = true
                }
            } catch (e: Throwable) {
                Log.w(TAG, "registerAppWithTelecom failed: $e")
                registered = false
                null
            }
        }
    }
}
