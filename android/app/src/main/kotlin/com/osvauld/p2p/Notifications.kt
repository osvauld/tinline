package com.osvauld.p2p

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Person
import android.content.Context
import android.content.Intent
import android.graphics.drawable.Icon
import android.os.Build
import uniffi.p2pcore.CallInfo

object Notifications {
    const val CH_SERVICE = "service"
    const val CH_CALLS = "calls"
    const val CH_MISSED = "missed"
    const val CH_LOCKED = "locked"
    const val ID_SERVICE = 1
    const val ID_INCOMING = 2
    const val ID_MISSED = 3
    const val ID_LOCKED = 4
    const val ID_WAITING = 5

    private fun nm(c: Context) = c.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager

    fun createChannels(c: Context) {
        val n = nm(c)
        n.createNotificationChannel(NotificationChannel(CH_SERVICE, "Call listener", NotificationManager.IMPORTANCE_LOW).apply {
            description = "Keeps the app reachable for incoming calls"
            setShowBadge(false)
        })
        n.createNotificationChannel(NotificationChannel(CH_CALLS, "Incoming calls", NotificationManager.IMPORTANCE_HIGH).apply {
            description = "Rings when a contact calls you"
            setSound(null, null)
            enableVibration(false)
            lockscreenVisibility = Notification.VISIBILITY_PUBLIC
        })
        n.createNotificationChannel(NotificationChannel(CH_MISSED, "Missed calls", NotificationManager.IMPORTANCE_DEFAULT))
        n.createNotificationChannel(NotificationChannel(CH_LOCKED, "Locked", NotificationManager.IMPORTANCE_LOW).apply {
            description = "Shown while the app needs your passphrase to receive calls"
            setShowBadge(false)
        })
    }

    private fun flags() = PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE

    private fun activity(c: Context, cls: Class<*>, code: Int, extra: (Intent) -> Unit = {}): PendingIntent =
        PendingIntent.getActivity(c, code, Intent(c, cls).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK).also(extra), flags())

    private fun service(c: Context, action: String, code: Int): PendingIntent =
        PendingIntent.getService(c, code, Intent(c, CoreService::class.java).setAction(action), flags())

    private fun idleTitle(): String {
        val app = P2pApp.instance
        return when {
            !app.availability.value.available -> "Not available"
            app.status.value?.online == false -> "Offline — reconnecting"
            else -> "Available for calls"
        }
    }

    private fun idleText(): String {
        val app = P2pApp.instance
        return when {
            !app.availability.value.available -> "Calls won’t ring. Tap to change."
            app.status.value?.online == false -> "Calls can’t reach you right now."
            else -> "Your line is open. Tap to change."
        }
    }

    /** The always-on foreground notification; text reflects call state, availability and connectivity. */
    fun service(c: Context, inCall: String?): Notification {
        val b = Notification.Builder(c, CH_SERVICE)
            .setSmallIcon(R.drawable.ic_stat_tinline)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setColor(0xFF0B6B5B.toInt())
            .setContentTitle(inCall ?: idleTitle())
            .setContentText(if (inCall != null) "Tap to return to the call" else idleText())
            .setContentIntent(
                if (inCall != null) activity(c, CallActivity::class.java, 10)
                else activity(c, MainActivity::class.java, 11))
        if (Build.VERSION.SDK_INT >= 31) b.setForegroundServiceBehavior(Notification.FOREGROUND_SERVICE_IMMEDIATE)
        if (inCall != null) b.setCategory(Notification.CATEGORY_CALL)
        return b.build()
    }

    fun incoming(c: Context, call: CallInfo) {
        val name = call.peerName.ifBlank { "Unknown" }
        val full = activity(c, IncomingCallActivity::class.java, 20)
        val answer = activity(c, CallActivity::class.java, 21) { it.putExtra(CallActivity.EXTRA_ANSWER, true) }
        val decline = service(c, CoreService.ACTION_DECLINE, 22)
        val b = Notification.Builder(c, CH_CALLS)
            .setSmallIcon(R.drawable.ic_stat_tinline)
            .setCategory(Notification.CATEGORY_CALL)
            .setOngoing(true)
            .setAutoCancel(false)
            .setFullScreenIntent(full, true)
            .setContentIntent(full)
            // Never ring forever (matches CallController.RING_TIMEOUT_MS).
            .setTimeoutAfter(CallController.RING_TIMEOUT_MS)
        if (Build.VERSION.SDK_INT >= 31) {
            val p = Person.Builder().setName(name).setImportant(true).build()
            b.style = Notification.CallStyle.forIncomingCall(p, decline, answer)
        } else {
            b.setContentTitle(name).setContentText("Incoming call")
            b.addAction(Notification.Action.Builder(Icon.createWithResource(c, android.R.drawable.ic_menu_close_clear_cancel), "Decline", decline).build())
            b.addAction(Notification.Action.Builder(Icon.createWithResource(c, android.R.drawable.sym_action_call), "Answer", answer).build())
        }
        try { nm(c).notify(ID_INCOMING, b.build()) } catch (e: SecurityException) { }
    }

    fun cancelIncoming(c: Context) = nm(c).cancel(ID_INCOMING)

    /** A second call during a call: a quiet heads-up with two actions, no full-screen intent. */
    fun waiting(c: Context, call: CallInfo) {
        val name = call.peerName.ifBlank { "Unknown" }
        val b = Notification.Builder(c, CH_CALLS)
            .setSmallIcon(R.drawable.ic_stat_tinline)
            .setCategory(Notification.CATEGORY_CALL)
            .setAutoCancel(false)
            .setOnlyAlertOnce(true)
            .setContentTitle("$name is calling")
            .setContentText("Your call continues until you choose.")
            .setContentIntent(activity(c, CallActivity::class.java, 50))
            .setTimeoutAfter(30_000)
            .addAction(Notification.Action.Builder(Icon.createWithResource(c, android.R.drawable.ic_menu_close_clear_cancel), "Decline", service(c, CoreService.ACTION_DECLINE_WAITING, 51)).build())
            .addAction(Notification.Action.Builder(Icon.createWithResource(c, android.R.drawable.sym_action_call), "End & answer", service(c, CoreService.ACTION_END_ANSWER, 52)).build())
        try { nm(c).notify(ID_WAITING, b.build()) } catch (e: SecurityException) { }
    }

    fun cancelWaiting(c: Context) = nm(c).cancel(ID_WAITING)

    fun locked(c: Context) {
        val n = Notification.Builder(c, CH_LOCKED)
            .setSmallIcon(R.drawable.ic_stat_tinline)
            .setOnlyAlertOnce(true)
            .setContentTitle("Unlock to receive calls")
            .setContentText("Tap to enter your passphrase")
            .setOngoing(true)
            .setContentIntent(activity(c, MainActivity::class.java, 40))
            .build()
        try { nm(c).notify(ID_LOCKED, n) } catch (e: SecurityException) { }
    }

    fun cancelLocked(c: Context) = nm(c).cancel(ID_LOCKED)

    fun missed(c: Context, name: String) {
        val n = Notification.Builder(c, CH_MISSED)
            .setSmallIcon(R.drawable.ic_stat_tinline)
            .setContentTitle("Missed call")
            .setContentText(name.ifBlank { "Unknown" })
            .setAutoCancel(true)
            .setContentIntent(activity(c, MainActivity::class.java, 30))
            .build()
        try { nm(c).notify(ID_MISSED, n) } catch (e: SecurityException) { }
    }
}
