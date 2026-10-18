package com.osvauld.p2p

import android.Manifest
import android.telecom.DisconnectCause
import android.util.Log
import java.util.concurrent.Executors
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import uniffi.p2pcore.GroupCallEvent
import uniffi.p2pcore.GroupCallEventKind
import uniffi.p2pcore.GroupCallEvents
import uniffi.p2pcore.GroupCallHere

/**
 * Group-call audio and service (GA-A1): the same mic + speaker loop as a 1:1 call (its own
 * [AudioEngine], so nothing on the 1:1 path stops it), audio focus and communication mode, and the
 * foreground service in call mode (microphone type, wake + Wi-Fi locks) while this device's user is
 * in a group call ([CallNotif] derives it; hosting alone keeps the always-on service, not call mode).
 *
 * Driven by the core's [GroupCallEvents]. Each event only triggers a re-read of the core's state
 * ([uniffi.p2pcore.Node.groupCallHere]) on one worker thread: the callbacks run on separate core
 * threads and can arrive out of order (a superseded call's Left after the winner's Joined).
 * `Node.pushMicPcm16`/`pullSpeakerPcm16` reach the group call whenever no 1:1 call is active.
 *
 * While in the call it is registered with Telecom as a [CallSession] named after the group, so
 * headsets, cars and watches see it; it starts on the loudspeaker unless a headset is connected.
 *
 * The call screens (next packet) use [here], [events], [prepare], [leave], [routes] and [setSpeaker]/[selectRoute].
 */
class GroupCallAudio(private val app: P2pApp) : GroupCallEvents {
    private val audio by lazy { AudioEngine(app, { app.node }, ::micUnavailable) }
    private val worker = Executors.newSingleThreadExecutor { r -> Thread(r, "p2p-group-call") }

    private val _here = MutableStateFlow<GroupCallHere?>(null)
    /** The group call this device is in or hosts, as of the last event. */
    val here: StateFlow<GroupCallHere?> = _here
    private val _events = MutableSharedFlow<GroupCallEvent>(extraBufferCapacity = 64)
    /** Every group-call event, for the call UI. */
    val events: SharedFlow<GroupCallEvent> = _events
    private val _micProblem = MutableStateFlow<String?>(null)
    val micProblem: StateFlow<String?> = _micProblem

    private val _routes = MutableStateFlow(RouteState())
    /** Where the group call's audio plays (empty while not in a call). */
    val routes: StateFlow<RouteState> = _routes
    /** Worker thread only. */
    private var session: CallSession? = null
    private var sessionWatch: Job? = null
    /**
     * When [prepare] raised the service for a call that has not shown up yet, else null. Read by
     * [CallNotif]; counts for [CallNotif.PREPARE_TIMEOUT_MS] at most, so a prepare that never settles
     * cannot keep the ongoing notification.
     */
    @Volatile var preparedAtMs: Long? = null
        private set
    /** The group's name for [callName], cached by [reconcile] (the service reads it on the main thread). */
    @Volatile private var hereName: String = NAME

    /** In a group call or hosting one. */
    val active: Boolean get() = _here.value != null

    override fun onGroupCall(event: GroupCallEvent) {
        testLog("group-call kind=${event.kind} group=${event.groupId} call=${event.callId} detail=${event.detail}")
        CallTrace.add("group ${event.kind} ${shortId(event.callId)}")
        _events.tryEmit(event)
        val settles = event.kind == GroupCallEventKind.START_FAILED || event.kind == GroupCallEventKind.LEFT
        worker.execute { reconcile(settles) }
    }

    /**
     * Call right before the user starts or joins a group call: raises the service to the
     * microphone type while the app is in the foreground (Android 14+ refuses it later from the
     * background). Undone by `StartFailed`/`Left` if the call does not come about, and by the
     * timeout if nothing comes at all; call [unprepare] when the start or join call itself throws.
     */
    fun prepare() {
        val at = System.currentTimeMillis()
        preparedAtMs = at
        CallTrace.add("group prepare")
        CoreService.ensureRunning(app, CoreService.ACTION_IN_CALL)
        app.scope.launch {
            delay(CallNotif.PREPARE_TIMEOUT_MS + 500)
            if (preparedAtMs == at) { CallTrace.add("group prepare timed out"); CoreService.poke("prepare timeout") }
        }
    }

    fun unprepare() {
        worker.execute { reconcile(true) }
    }

    fun leave() {
        app.scope.launch { try { app.node.groupCallLeave() } catch (e: Exception) { Log.w(TAG, "leave: $e") } }
    }

    /** Group calls start on the loudspeaker unless a headset is connected; this switches explicitly. */
    fun setSpeaker(on: Boolean) {
        worker.execute { if (_routes.value.speaker != on) session?.toggleSpeaker() }
    }

    fun selectRoute(route: AudioRoute) {
        worker.execute { session?.select(route) }
    }

    /** The group call's name for the ongoing notification: the group's, else "Group call". */
    fun callName(): String = hereName

    private fun groupName(groupId: String): String =
        try { app.node.groups().firstOrNull { it.id == groupId }?.name } catch (e: Exception) { null }?.ifBlank { null } ?: NAME

    /** Worker thread only. */
    private fun startSession(h: GroupCallHere) {
        if (session != null) return
        val s = CallSession(
            app.scope, app, audio, groupName(h.groupId), incoming = false, preferSpeaker = true,
            onAnswer = {}, onDisconnect = { leave() }, onSystemMute = { audio.muted = it },
        )
        session = s
        sessionWatch = app.scope.launch { s.routes.collect { _routes.value = it } }
        s.connected()
    }

    /** Worker thread only. */
    private fun endSession() {
        val s = session ?: run { audio.stop(); return }
        session = null
        sessionWatch?.cancel(); sessionWatch = null
        s.end(DisconnectCause.LOCAL)
        _routes.value = RouteState()
    }

    private fun micUnavailable() {
        _micProblem.value = if (Perms.granted(app, Manifest.permission.RECORD_AUDIO)) "Microphone unavailable - you can't be heard"
        else "Microphone permission needed - you can't be heard"
    }

    /**
     * Worker thread only. Start and stop are idempotent. [settles]: the event means a prepared
     * start or join is over (succeeded or not).
     */
    private fun reconcile(settles: Boolean = false) {
        val h = try { app.node.groupCallHere() } catch (e: Exception) { Log.w(TAG, "groupCallHere: $e"); null }
        val was = _here.value
        // The name first: the service re-derives (and reads it) as soon as [here] changes.
        if (h != null && h.groupId != was?.groupId) hereName = groupName(h.groupId)
        val wasPrepared = preparedAtMs != null
        // Hosting alone (not yet in it) does not settle a prepare: the join may still be on its way.
        if (h?.inCall == true || settles) preparedAtMs = null
        _here.value = h
        if (h?.inCall == true) {
            if (was?.inCall != true) _micProblem.value = null
            startSession(h)
        } else endSession()
        // Pokes only: the service derives the state itself ([CallNotif]), so their order is free.
        if (h != null) {
            if (was == null) CoreService.ensureRunning(app, CoreService.ACTION_IN_CALL)
        } else if (was != null || (wasPrepared && settles)) {
            CoreService.ensureRunning(app, CoreService.ACTION_IDLE)
        }
    }

    companion object {
        const val TAG = "GroupCall"
        /** The in-call notification title. */
        const val NAME = "Group call"
    }
}
