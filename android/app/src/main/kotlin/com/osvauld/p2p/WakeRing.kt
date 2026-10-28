package com.osvauld.p2p

import android.util.Log
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch

/**
 * A call wake that rings before the caller's call has arrived (docs/push.md "Calls"). [account]: the name of the account of
 * this phone it rings when that is not the open one ("Asha → Work"); answering switches to it (the core does).
 */
data class WakeRingUi(val did: String, val name: String, val answered: Boolean = false, val account: String? = null) {
    /** What the ring shows as the caller. */
    val title: String get() = AccountLabels.ringName(name, account)
}

/** The ring's state changes, pure so the handover to the real call is unit tested. The core owns the timing. */
object WakeRingLogic {
    private const val PREFIX = "wake:"

    /** The `call_id` the core reports a wake ring under. */
    fun callId(did: String) = PREFIX + did
    fun isWakeCallId(id: String) = id.startsWith(PREFIX)

    /** A ring (or a repeat wake for the same contact: one ring only, an early answer is kept). */
    fun ring(cur: WakeRingUi?, did: String, name: String, account: String? = null): WakeRingUi =
        if (cur?.did == did) cur.copy(name = name, account = account) else WakeRingUi(did, name, account = account)

    /**
     * Whether a wake for [did] starts a ring that should alert (notification, full-screen, ringtone). A repeat wake for
     * the contact already ringing alerts nothing: it would restart the ringtone and bring an answered ring
     * ("Connecting…") back as a fresh incoming call.
     */
    fun alerts(cur: WakeRingUi?, did: String): Boolean = cur?.did != did

    fun answered(cur: WakeRingUi?): WakeRingUi? = cur?.copy(answered = true)

    /** The core ended the ring (`on_call_state("wake:<did>", Ended)`); another contact's end changes nothing. */
    fun ended(cur: WakeRingUi?, callId: String): WakeRingUi? =
        if (cur != null && callId == callId(cur.did)) null else cur

    /** What the real incoming call from [peerDid] does with the ring: it takes it over (once), keeping an early answer. */
    data class Handover(val matched: Boolean, val answeredEarly: Boolean)

    fun handover(cur: WakeRingUi?, peerDid: String): Handover =
        if (cur != null && cur.did == peerDid) Handover(true, cur.answered) else Handover(false, false)
}

/** The wake ring's side: notification, ringtone and the core calls. Thin; the rules are in [WakeRingLogic]. */
object WakeRing {
    private const val TAG = "WakeRing"
    private val _state = MutableStateFlow<WakeRingUi?>(null)
    val state: StateFlow<WakeRingUi?> = _state
    val active: Boolean get() = _state.value != null

    /** A call wake: ring now (calls alerts off: the call still rings as an ordinary one when it arrives). */
    fun ring(app: P2pApp, did: String, name: String, account: String? = null) {
        if (!AlertPrefs.calls(app)) return
        val cur = _state.value
        val ring = WakeRingLogic.ring(cur, did, name, account)
        _state.value = ring
        if (!WakeRingLogic.alerts(cur, did)) return
        Notifications.wakeRing(app, ring.title)
        app.calls.ringForWake()
    }

    /** Answer (the microphone is already allowed): the call is accepted the moment it arrives; "Connecting…" meanwhile. */
    fun answer(app: P2pApp) {
        val cur = _state.value ?: return
        _state.value = WakeRingLogic.answered(cur)
        app.calls.stopRingForWake()
        Notifications.cancelIncoming(app)
        app.scope.launch {
            try { app.node.answerWakeRing(cur.did) } catch (e: uniffi.p2pcore.Exception.NotFound) {
                // The ring ended, or the call already arrived: answer that one the ordinary way, else it is over.
                if (app.calls.ui.value?.info?.peerDid == cur.did) app.calls.answer() else _state.value = null
            } catch (e: Exception) { Log.w(TAG, "answer: ${e.javaClass.simpleName}") }
        }
    }

    fun decline(app: P2pApp) {
        val cur = _state.value ?: return
        _state.value = null
        app.calls.stopRingForWake()
        Notifications.cancelIncoming(app)
        app.scope.launch {
            try { app.node.declineWakeRing(cur.did) } catch (e: uniffi.p2pcore.Exception.NotFound) {
                // The call has arrived meanwhile: decline that one.
                if (app.calls.ui.value?.info?.peerDid == cur.did) app.calls.decline()
            } catch (e: Exception) { Log.w(TAG, "decline: ${e.javaClass.simpleName}") }
        }
    }

    /** `on_call_state("wake:<did>", Ended)`: the ring ran out (the caller gave up). */
    fun onEnded(app: P2pApp, callId: String) {
        val before = _state.value
        val after = WakeRingLogic.ended(before, callId)
        if (before === after) return
        _state.value = after
        app.calls.stopRingForWake()
        Notifications.cancelIncoming(app)
    }

    /** Read-only: does the arriving call from [peerDid] take over the ring, and was the ring answered? */
    fun match(peerDid: String): WakeRingLogic.Handover = WakeRingLogic.handover(_state.value, peerDid)

    /** After the call is in the controller's state (so the screen never sees neither): the ring is consumed silently. */
    fun consumed(peerDid: String) {
        if (WakeRingLogic.handover(_state.value, peerDid).matched) _state.value = null
    }
}
