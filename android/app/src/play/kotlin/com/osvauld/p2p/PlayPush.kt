package com.osvauld.p2p

import android.content.Context
import android.util.Log
import com.google.android.gms.common.ConnectionResult
import com.google.android.gms.common.GoogleApiAvailability
import com.google.firebase.FirebaseApp
import com.google.firebase.FirebaseOptions
import com.google.firebase.messaging.FirebaseMessaging
import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage
import uniffi.p2pcore.PushTarget

/**
 * The `play` edition's [PushProvider]: FCM. Firebase is started by hand from [BuildConfig] values (no
 * google-services plugin, no google-services.json in git). Without those values, or without Play
 * services on the phone, [available] is false and the app stays always-on. No analytics SDK is
 * present; only firebase-messaging.
 */
internal object PlayPush : PushProvider {
    private const val TAG = "PlayPush"
    @Volatile private var checked: Boolean? = null

    /** The settings are all present (the project id has a default, the other three do not). */
    internal fun configured(): Boolean =
        BuildConfig.FIREBASE_APP_ID.isNotBlank() && BuildConfig.FIREBASE_API_KEY.isNotBlank() &&
            BuildConfig.FIREBASE_GCM_SENDER_ID.isNotBlank() && BuildConfig.FIREBASE_PROJECT_ID.isNotBlank()

    /** Firebase by hand, at once (FCM's receiver can be what started the process); the Play services check off the main thread. */
    override fun start(ctx: Context) {
        if (!configured()) return
        try { initFirebase(ctx) } catch (e: Throwable) { Log.w(TAG, "firebase: ${e.javaClass.simpleName}") }
        Thread({ available(ctx) }, "push-check").start()
    }

    @Synchronized
    override fun available(ctx: Context): Boolean {
        checked?.let { return it }
        val t0 = android.os.SystemClock.elapsedRealtime()
        val ok = try {
            configured() &&
                GoogleApiAvailability.getInstance().isGooglePlayServicesAvailable(ctx) == ConnectionResult.SUCCESS &&
                initFirebase(ctx)
        } catch (e: Throwable) { Log.w(TAG, "unavailable: ${e.javaClass.simpleName}"); false }
        checked = ok
        Log.i(TAG, "available=$ok in ${android.os.SystemClock.elapsedRealtime() - t0} ms on ${Thread.currentThread().name}")
        return ok
    }

    private fun initFirebase(ctx: Context): Boolean {
        val app = ctx.applicationContext
        if (FirebaseApp.getApps(app).isNotEmpty()) return true
        val o = FirebaseOptions.Builder()
            .setProjectId(BuildConfig.FIREBASE_PROJECT_ID)
            .setApplicationId(BuildConfig.FIREBASE_APP_ID)
            .setApiKey(BuildConfig.FIREBASE_API_KEY)
            .setGcmSenderId(BuildConfig.FIREBASE_GCM_SENDER_ID)
            .build()
        return FirebaseApp.initializeApp(app, o) != null
    }

    override fun setEnabled(ctx: Context, on: Boolean) {
        if (!available(ctx)) return
        try { FirebaseMessaging.getInstance().isAutoInitEnabled = on } catch (e: Throwable) { Log.w(TAG, "auto-init: ${e.javaClass.simpleName}") }
    }

    override fun fetchTarget(ctx: Context, done: (PushTarget?) -> Unit) {
        if (!available(ctx)) { done(null); return }
        try {
            FirebaseMessaging.getInstance().token.addOnCompleteListener { t ->
                // t.result throws on a failed task (e.g. INVALID_SENDER), and this runs on the main thread.
                if (!t.isSuccessful) { Log.w(TAG, "token: ${t.exception?.javaClass?.simpleName}"); done(null); return@addOnCompleteListener }
                done(t.result?.takeIf { it.isNotBlank() }?.let { PushTarget.Fcm(it) })
            }
        } catch (e: Throwable) { Log.w(TAG, "token: ${e.javaClass.simpleName}"); done(null) }
    }
}

internal fun platformPushProvider(): PushProvider = PlayPush

/** FCM entry points. Thin: the work is in [WakeHandler]. Never logs the token or the hint. */
class WakeMessagingService : FirebaseMessagingService() {
    override fun onNewToken(token: String) {
        WakeHandler.onNewTarget(this, PushTarget.Fcm(token))
    }

    override fun onMessageReceived(message: RemoteMessage) {
        val hint = message.data["h"] ?: return
        // Runs on FCM's worker thread inside its short window; WakeHandler only unlocks, asks the core
        // and posts, then hands the long part to the foreground service.
        WakeHandler.onWake(this, hint)
    }
}
