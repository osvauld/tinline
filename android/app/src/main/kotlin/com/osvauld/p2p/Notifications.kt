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
    const val ID_SERVICE = 1
    const val ID_INCOMING = 2
    const val ID_MISSED = 3
    const val ID_LOCKED = 4

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
    }

    private fun flags() = PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE

    private fun activity(c: Context, cls: Class<*>, code: Int, extra: (Intent) -> Unit = {}): PendingIntent =
        PendingIntent.getActivity(c, code, Intent(c, cls).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK).also(extra), flags())

    private fun service(c: Context, action: String, code: Int): PendingIntent =
        PendingIntent.getService(c, code, Intent(c, CoreService::class.java).setAction(action), flags())

    /** The always-on foreground notification; text reflects call state. */
    fun service(c: Context, inCall: String?): Notification {
        val b = Notification.Builder(c, CH_SERVICE)
            .setSmallIcon(android.R.drawable.sym_action_call)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setContentTitle(inCall ?: "Ready for calls")
            .setContentText(if (inCall != null) "Tap to return to the call" else "Listening for contacts")
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
            .setSmallIcon(android.R.drawable.sym_action_call)
            .setCategory(Notification.CATEGORY_CALL)
            .setOngoing(true)
            .setAutoCancel(false)
            .setFullScreenIntent(full, true)
            .setContentIntent(full)
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

    fun locked(c: Context) {
        val n = Notification.Builder(c, CH_MISSED)
            .setSmallIcon(android.R.drawable.ic_lock_lock)
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
            .setSmallIcon(android.R.drawable.sym_call_missed)
            .setContentTitle("Missed call")
            .setContentText(name.ifBlank { "Unknown" })
            .setAutoCancel(true)
            .setContentIntent(activity(c, MainActivity::class.java, 30))
            .build()
        try { nm(c).notify(ID_MISSED, n) } catch (e: SecurityException) { }
    }
}
