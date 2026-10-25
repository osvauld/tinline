package com.osvauld.p2p

import android.content.Context
import android.util.Log
import kotlinx.coroutines.launch
import uniffi.p2pcore.LockState
import uniffi.p2pcore.PushTarget
import uniffi.p2pcore.WakeOutcome

/** What a wake push asks the app to do. Pure mapping of the core's answer; the rules live in Rust. */
sealed interface WakePlan {
    /** Not for us, refused, or nothing to show (Ignore, EndCall). */
    data object Nothing : WakePlan
    /** A message wake: make sure the node runs so the dial-back and the sync happen; the usual notifications follow. */
    data object StartNode : WakePlan
    /** A call wake: ring now with the user's own name for the contact, and keep the node up for the caller's dial. */
    data class Ring(val did: String, val name: String) : WakePlan

    companion object {
        fun of(o: WakeOutcome): WakePlan = when (o) {
            is WakeOutcome.Ignore, is WakeOutcome.EndCall -> Nothing
            is WakeOutcome.DialedBack -> StartNode
            is WakeOutcome.Ring -> Ring(o.did, o.name)
        }
    }
}

/**
 * Wake-by-push on the app side (docs/push.md, Android). Thin: the core opens the hint and decides; this
 * unlocks with the remembered key, shows the ring or the generic notice, and keeps the process up.
 * Never logs a hint, a DID or a token.
 */
object WakeHandler {
    private const val TAG = "Wake"

    /** The platform gave a new wake target (FCM `onNewToken`): hand it to the core, or remember to when unlocked. */
    fun onNewTarget(ctx: Context, target: PushTarget) {
        val app = P2pApp.get(ctx)
        if (!PushMode.pushOn(app)) return
        if (app.node.lockState() == LockState.UNLOCKED) set(app, target) else PushMode.setRefreshPending(app, true)
    }

    /**
     * After every unlock/start: in push mode read the current target and give it to the core (a token refresh that
     * arrived while locked is picked up here); in always-on mode take any wake tokens we issued back.
     */
    fun syncTarget(app: P2pApp) {
        if (app.node.lockState() != LockState.UNLOCKED) return
        val provider = PushMode.provider()
        if (PushMode.pushOn(app)) {
            provider.setEnabled(app, true)
            provider.fetchTarget(app) { t ->
                if (t != null) app.scope.launch { set(app, t) } else Log.w(TAG, "no push target; contacts reach us directly")
            }
        } else {
            provider.setEnabled(app, false)
            if (app.node.pushTarget() != null) set(app, null)
        }
    }

    private fun set(app: P2pApp, target: PushTarget?) {
        try {
            app.node.setPushTarget(target)
            PushMode.setRefreshPending(app, false)
        } catch (e: Exception) { Log.w(TAG, "setPushTarget: ${e.javaClass.simpleName}") }
    }

    /**
     * A wake push arrived ([hint]: FCM data key `h`). Runs on the push thread inside its short window: it unlocks
     * with the remembered key, asks the core, rings or posts the generic notice, and raises the foreground
     * service (allowed from a high-priority push) so the node stays up past the window. The service stops itself
     * again in push mode ([CoreService]); WorkManager is not needed because the service already exists for this.
     */
    fun onWake(ctx: Context, hint: String) {
        val app = P2pApp.get(ctx)
        if (!PushMode.pushOn(app)) return   // always-on: the service is connected already; a stray push is ignored
        app.unlockForWake()
        val outcome = try {
            app.node.onWake(hint)
        } catch (e: uniffi.p2pcore.Exception.Locked) { Notifications.wakeNotice(app); return }
        catch (e: uniffi.p2pcore.Exception.NoIdentity) { Notifications.wakeNotice(app); return }
        catch (e: Exception) { Log.w(TAG, "onWake: ${e.javaClass.simpleName}"); return }
        when (val plan = WakePlan.of(outcome)) {
            WakePlan.Nothing -> {}
            WakePlan.StartNode -> keepUp(app)
            is WakePlan.Ring -> { WakeRing.ring(app, plan.did, plan.name); keepUp(app) }
        }
    }

    private fun keepUp(app: P2pApp) {
        CoreService.ensureRunning(app, CoreService.ACTION_WAKE)
        app.startNode()
    }
}
