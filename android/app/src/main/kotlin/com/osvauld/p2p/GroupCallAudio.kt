package com.osvauld.p2p

import android.Manifest
import android.util.Log
import java.util.concurrent.Executors
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import uniffi.p2pcore.GroupCallEvent
import uniffi.p2pcore.GroupCallEventKind
import uniffi.p2pcore.GroupCallEvents
import uniffi.p2pcore.GroupCallHere

/**
 * Group-call audio and service (GA-A1): the same mic + speaker loop as a 1:1 call (its own
 * [AudioEngine], so nothing on the 1:1 path stops it), audio focus and communication mode, and the
 * foreground service in call mode (microphone type, wake + Wi-Fi locks) while this device is in a
 * group call or hosts one: a phone host keeps running while its own user is in the call.
 *
 * Driven by the core's [GroupCallEvents]. Each event only triggers a re-read of the core's state
 * ([uniffi.p2pcore.Node.groupCallHere]) on one worker thread: the callbacks run on separate core
 * threads and can arrive out of order (a superseded call's Left after the winner's Joined).
 * `Node.pushMicPcm16`/`pullSpeakerPcm16` reach the group call whenever no 1:1 call is active.
 *
 * The call screens (next packet) use [here], [events], [prepare], [leave] and [setSpeaker].
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

    /** Group calls default to the loudspeaker (no proximity handling yet). */
    @Volatile var speaker = true
        private set
    /** [prepare] raised the service for a call that has not shown up yet. */
    @Volatile private var prepared = false

    /** In a group call or hosting one. */
    val active: Boolean get() = _here.value != null

    override fun onGroupCall(event: GroupCallEvent) {
        testLog("group-call kind=${event.kind} group=${event.groupId} call=${event.callId} detail=${event.detail}")
        _events.tryEmit(event)
        val settles = event.kind == GroupCallEventKind.START_FAILED || event.kind == GroupCallEventKind.LEFT
        worker.execute { reconcile(settles) }
    }

    /**
     * Call right before the user starts or joins a group call: raises the service to the
     * microphone type while the app is in the foreground (Android 14+ refuses it later from the
     * background). Undone by `StartFailed`/`Left` if the call does not come about; call
     * [unprepare] when the start or join call itself throws.
     */
    fun prepare() {
        prepared = true
        CoreService.ensureRunning(app, CoreService.ACTION_IN_CALL, NAME)
    }

    fun unprepare() {
        worker.execute { reconcile(true) }
    }

    fun leave() {
        app.scope.launch { try { app.node.groupCallLeave() } catch (e: Exception) { Log.w(TAG, "leave: $e") } }
    }

    fun setSpeaker(on: Boolean) {
        speaker = on
        audio.setSpeaker(on)
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
        _here.value = h
        if (h?.inCall == true) {
            if (was?.inCall != true) _micProblem.value = null
            audio.start(speaker)
        } else audio.stop()
        if (h != null) {
            prepared = false
            if (was == null) CoreService.ensureRunning(app, CoreService.ACTION_IN_CALL, NAME)
        } else if (was != null || (prepared && settles)) {
            prepared = false
            // A 1:1 call (if any) owns the service state; the two never run at the same time.
            if (app.calls.ui.value == null) CoreService.ensureRunning(app, CoreService.ACTION_IDLE)
        }
    }

    companion object {
        const val TAG = "GroupCall"
        /** The in-call notification title. */
        const val NAME = "Group call"
    }
}
