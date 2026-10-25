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
    const val CH_ONGOING = "ongoing_calls"
    const val CH_MISSED = "missed"
    const val CH_LOCKED = "locked"
    // A channel's sound is fixed when it is created, so Tinline's own `received` tone needed new ids; the first-version channels
    // ("messages", "groups", which used the system default sound) are deleted in createChannels.
    const val CH_MESSAGES = "messages2"
    // "requests2": High, so a contact request pops up like a message (a channel's importance is fixed once created; the first
    // "requests" channel, Default, only showed an icon in the status bar and is deleted in createChannels).
    const val CH_REQUESTS = "requests2"
    const val CH_GROUPS = "groups2"
    const val EXTRA_REQUEST = "contact_request"
    /** Open Home on the Groups tab (a group invitation). */
    const val EXTRA_GROUPS = "open_groups"
    /** Join this group's running call (the ringing notification's Join). */
    const val EXTRA_GCALL_JOIN = "gcall_join"
    const val EXTRA_GCALL_OPEN = "gcall_open"
    const val ID_SERVICE = 1
    const val ID_INCOMING = 2
    const val ID_MISSED = 3
    const val ID_LOCKED = 4
    const val ID_WAITING = 5
    const val ID_GROUP_RING = 6
    /**
     * The always-on notification while in a call. A separate id from [ID_SERVICE]: moving the
     * foreground service between the two ids makes Android remove the old one outright, so no
     * call style, timer or status-bar call chip survives the call (an in-place update of one id
     * left "in call" showing on Pixel 8 / moto after the log said idle, 2026-10-19).
     */
    const val ID_CALL = 7
    /** The content-free "you may have a new message" notice after a wake the app could not open (locked). */
    const val ID_WAKE_NOTICE = 8
    /** Opens the wake ring screen and answers it (the wake notification's Answer). */
    const val EXTRA_WAKE_ANSWER = "wake_answer"

    private fun nm(c: Context) = c.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager

    fun createChannels(c: Context) {
        val n = nm(c)
        n.createNotificationChannel(NotificationChannel(CH_SERVICE, c.getString(R.string.notif_ch_service), NotificationManager.IMPORTANCE_LOW).apply {
            description = c.getString(R.string.notif_ch_service_desc)
            setShowBadge(false)
        })
        n.createNotificationChannel(NotificationChannel(CH_CALLS, c.getString(R.string.notif_ch_calls), NotificationManager.IMPORTANCE_HIGH).apply {
            description = c.getString(R.string.notif_ch_calls_desc)
            setSound(null, null)
            enableVibration(false)
            lockscreenVisibility = Notification.VISIBILITY_PUBLIC
        })
        n.createNotificationChannel(NotificationChannel(CH_ONGOING, c.getString(R.string.notif_ch_ongoing), NotificationManager.IMPORTANCE_DEFAULT).apply {
            description = c.getString(R.string.notif_ch_ongoing_desc)
            setSound(null, null)
            enableVibration(false)
            setShowBadge(false)
        })
        n.createNotificationChannel(NotificationChannel(CH_MISSED, c.getString(R.string.notif_ch_missed), NotificationManager.IMPORTANCE_DEFAULT))
        listOf("messages", "groups", "requests").forEach { old -> if (n.getNotificationChannel(old) != null) n.deleteNotificationChannel(old) }
        val tone = receivedTone(c)
        val toneAttrs = android.media.AudioAttributes.Builder().setUsage(android.media.AudioAttributes.USAGE_NOTIFICATION)
            .setContentType(android.media.AudioAttributes.CONTENT_TYPE_SONIFICATION).build()
        n.createNotificationChannel(NotificationChannel(CH_MESSAGES, c.getString(R.string.notif_messages), NotificationManager.IMPORTANCE_HIGH).apply {
            description = c.getString(R.string.notif_ch_messages_desc)
            if (tone != null) setSound(tone, toneAttrs)
        })
        n.createNotificationChannel(NotificationChannel(CH_GROUPS, c.getString(R.string.notif_groups), NotificationManager.IMPORTANCE_DEFAULT).apply {
            description = c.getString(R.string.notif_ch_groups_desc)
            if (tone != null) setSound(tone, toneAttrs)
        })
        n.createNotificationChannel(NotificationChannel(CH_REQUESTS, c.getString(R.string.notif_ch_requests), NotificationManager.IMPORTANCE_HIGH).apply {
            description = c.getString(R.string.notif_ch_requests_desc)
        })
        n.createNotificationChannel(NotificationChannel(CH_LOCKED, c.getString(R.string.notif_ch_locked), NotificationManager.IMPORTANCE_LOW).apply {
            description = c.getString(R.string.notif_ch_locked_desc)
            setShowBadge(false)
        })
    }

    /**
     * Tinline's own `received` tone as a file the system can play for a channel: the core renders it (`toy_cue_pcm`), it is written once
     * as a 16-bit mono WAV in files/tones and shared by FileProvider (SystemUI is granted read access). Null if anything fails, and the
     * channel then keeps the system default sound.
     */
    private fun receivedTone(c: Context): android.net.Uri? = try {
        val dir = java.io.File(c.filesDir, "tones").apply { mkdirs() }
        val f = java.io.File(dir, "received.wav")
        if (!f.exists() || f.length() < 100) {
            val pcm = uniffi.p2pcore.toyCuePcm("received", 4.0) ?: return null   // a notification plays at the phone's volume: lift the desktop level
            val data = java.nio.ByteBuffer.allocate(pcm.samples.size * 2).order(java.nio.ByteOrder.LITTLE_ENDIAN)
            pcm.samples.forEach { data.putShort(it) }
            val rate = pcm.rate.toInt()
            val h = java.nio.ByteBuffer.allocate(44).order(java.nio.ByteOrder.LITTLE_ENDIAN)
            h.put("RIFF".toByteArray()).putInt(36 + data.capacity()).put("WAVEfmt ".toByteArray()).putInt(16).putShort(1).putShort(1)
                .putInt(rate).putInt(rate * 2).putShort(2).putShort(16).put("data".toByteArray()).putInt(data.capacity())
            f.outputStream().use { it.write(h.array()); it.write(data.array()) }
        }
        val uri = androidx.core.content.FileProvider.getUriForFile(c, c.packageName + ".files", f)
        c.grantUriPermission("com.android.systemui", uri, Intent.FLAG_GRANT_READ_URI_PERMISSION)
        uri
    } catch (e: Exception) { android.util.Log.w(P2pApp.TAG, "received tone: $e"); null }

    private fun flags() = PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE

    private fun activity(c: Context, cls: Class<*>, code: Int, extra: (Intent) -> Unit = {}): PendingIntent =
        PendingIntent.getActivity(c, code, Intent(c, cls).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK).also(extra), flags())

    private fun service(c: Context, action: String, code: Int): PendingIntent =
        PendingIntent.getService(c, code, Intent(c, CoreService::class.java).setAction(action), flags())

    /** Push mode (no always-on service): the service only runs for a wake, so it says what it is doing. */
    private fun wakeOnly(c: Context) = PushMode.pushOn(c)

    private fun idleTitle(c: Context): String {
        val app = P2pApp.instance
        return when {
            wakeOnly(c) -> c.getString(R.string.notif_wake_service_title)
            !app.availability.value.available -> appString(R.string.notif_idle_unavailable)
            app.status.value?.online == false -> appString(R.string.notif_idle_offline)
            else -> appString(R.string.notif_idle_available)
        }
    }

    private fun idleText(c: Context): String {
        val app = P2pApp.instance
        return when {
            wakeOnly(c) -> c.getString(R.string.notif_wake_service_text)
            !app.availability.value.available -> appString(R.string.notif_idle_unavailable_text)
            app.status.value?.online == false -> appString(R.string.notif_idle_offline_text)
            else -> appString(R.string.notif_idle_available_text)
        }
    }

    /** The always-on foreground notification; text reflects call state, availability and connectivity. */
    fun service(c: Context, inCall: String?): Notification {
        // A group call (no 1:1 call): its name as the title, the timer, Mute and Leave.
        val app = P2pApp.instance
        val groupCall = inCall != null && app.calls.ui.value == null && app.groupCalls.active
        val b = Notification.Builder(c, if (inCall != null) CH_ONGOING else CH_SERVICE)
            .setSmallIcon(R.drawable.ic_stat_tinline)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setColor(0xFF0B6B5B.toInt())
            .setContentTitle(inCall ?: idleTitle(c))
            .setContentText(if (inCall != null) c.getString(R.string.notif_return_to_call) else idleText(c))
            .setContentIntent(
                if (groupCall) activity(c, MainActivity::class.java, 13) { it.putExtra(EXTRA_GCALL_OPEN, true) }
                else if (inCall != null) activity(c, CallActivity::class.java, 10)
                else activity(c, MainActivity::class.java, 11))
        if (Build.VERSION.SDK_INT >= 31) b.setForegroundServiceBehavior(Notification.FOREGROUND_SERVICE_IMMEDIATE)
        if (inCall != null) {
            b.setCategory(Notification.CATEGORY_CALL)
                .setAutoCancel(false)
            val hangup = service(c, CoreService.ACTION_HANGUP, 12)
            val call = P2pApp.instance.calls.ui.value
            if (Build.VERSION.SDK_INT >= 31) {
                val person = Person.Builder().setName(inCall.ifBlank { c.getString(R.string.call_notif_fallback_title) }).setImportant(true).build()
                b.style = Notification.CallStyle.forOngoingCall(person, hangup)
            } else {
                b.addAction(Notification.Action.Builder(
                    Icon.createWithResource(c, android.R.drawable.ic_menu_close_clear_cancel),
                    if (groupCall) c.getString(R.string.notif_leave) else c.getString(R.string.notif_end_call), hangup).build())
            }
            if (groupCall) {
                val g = app.groupCallUi.state.value
                g.sinceMs?.takeIf { g.connected }?.let { b.setWhen(it).setShowWhen(true).setUsesChronometer(true) }
                b.addAction(Notification.Action.Builder(
                    Icon.createWithResource(c, android.R.drawable.ic_lock_silent_mode),
                    if (g.muted) c.getString(R.string.notif_unmute) else c.getString(R.string.notif_mute), service(c, CoreService.ACTION_GCALL_MUTE, 14)).build())
            }
            call?.activeSinceMs?.let { since ->
                b.setWhen(since).setShowWhen(true).setUsesChronometer(true)
            }
        }
        return b.build()
    }

    fun incoming(c: Context, call: CallInfo) {
        if (!AlertPrefs.calls(c)) return   // Settings › Notifications › Calls off
        ring(c, call.peerName.ifBlank { c.getString(R.string.common_unknown) }, call.video,
            answer = activity(c, CallActivity::class.java, 21) { it.putExtra(CallActivity.EXTRA_ANSWER, true) },
            decline = service(c, CoreService.ACTION_DECLINE, 22))
    }

    /**
     * A call wake (docs/push.md): rings before the call itself has arrived, with the user's own name for the contact.
     * Same look and id as [incoming], so the real call replaces it in place. Answer opens the wake screen (it asks for the
     * microphone first); Decline goes to the service. Nothing here carries message text; the name is looked up locally.
     */
    fun wakeRing(c: Context, name: String) {
        if (!AlertPrefs.calls(c)) return
        ring(c, name.ifBlank { c.getString(R.string.common_someone) }, false,
            answer = activity(c, IncomingCallActivity::class.java, 23) { it.putExtra(EXTRA_WAKE_ANSWER, true) },
            decline = service(c, CoreService.ACTION_WAKE_DECLINE, 24))
    }

    /** A wake the app could not open (still locked): names nobody and says nothing, only that something may be waiting. */
    fun wakeNotice(c: Context) {
        if (!AlertPrefs.messages(c)) return
        val n = Notification.Builder(c, CH_MESSAGES)
            .setSmallIcon(R.drawable.ic_stat_tinline)
            .setAutoCancel(true)
            .setContentTitle(c.getString(R.string.notif_wake_notice_title))
            .setContentText(c.getString(R.string.notif_wake_notice_text))
            .setContentIntent(activity(c, MainActivity::class.java, 41))
            .build()
        try { nm(c).notify(ID_WAKE_NOTICE, n) } catch (e: SecurityException) { }
    }

    private fun ring(c: Context, name: String, video: Boolean, answer: PendingIntent, decline: PendingIntent) {
        val full = activity(c, IncomingCallActivity::class.java, 20)
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
            b.style = Notification.CallStyle.forIncomingCall(p, decline, answer).setIsVideo(video)
        } else {
            b.setContentTitle(name).setContentText(if (video) c.getString(R.string.notif_incoming_video) else c.getString(R.string.notif_incoming))
            b.addAction(Notification.Action.Builder(Icon.createWithResource(c, android.R.drawable.ic_menu_close_clear_cancel), c.getString(R.string.common_decline), decline).build())
            b.addAction(Notification.Action.Builder(Icon.createWithResource(c, android.R.drawable.sym_action_call), c.getString(R.string.common_answer), answer).build())
        }
        try { nm(c).notify(ID_INCOMING, b.build()) } catch (e: SecurityException) { }
    }

    /**
     * A group call someone else started rings (up to 30 s): the group's name over "<First name> is calling the group",
     * Join / Not now, on the calls channel (heads-up, no full-screen). Posted only while the app is not in front.
     */
    fun groupRing(c: Context, gid: String, group: String, by: String?, ringMs: Long) {
        if (!AlertPrefs.calls(c)) return
        val b = Notification.Builder(c, CH_CALLS)
            .setSmallIcon(R.drawable.ic_stat_tinline)
            .setCategory(Notification.CATEGORY_CALL)
            .setAutoCancel(true)
            .setOnlyAlertOnce(true)
            .setContentTitle(group)
            .setContentText(if (by != null) c.getString(R.string.notif_group_calling, by) else c.getString(R.string.notif_group_calling_unnamed))
            .setContentIntent(activity(c, MainActivity::class.java, 60) { it.putExtra(EXTRA_GROUPS, true) })
            .setTimeoutAfter(ringMs)
            .addAction(Notification.Action.Builder(Icon.createWithResource(c, android.R.drawable.sym_action_call), c.getString(R.string.common_join),
                activity(c, MainActivity::class.java, 61) { it.putExtra(EXTRA_GCALL_JOIN, gid) }).build())
            .addAction(Notification.Action.Builder(Icon.createWithResource(c, android.R.drawable.ic_menu_close_clear_cancel), c.getString(R.string.common_not_now),
                service(c, CoreService.ACTION_GCALL_NOT_NOW, 62)).build())
        try { nm(c).notify(ID_GROUP_RING, b.build()) } catch (e: SecurityException) { }
    }

    fun cancelGroupRing(c: Context) = nm(c).cancel(ID_GROUP_RING)

    fun cancelIncoming(c: Context) = nm(c).cancel(ID_INCOMING)

    /** Process start: the call notifications a dead process may have left behind. */
    fun cancelCallLeftovers(c: Context) {
        val n = nm(c)
        val left = try { n.activeNotifications.map { it.id }.filter { it == ID_INCOMING || it == ID_WAITING || it == ID_GROUP_RING } }
            catch (_: Exception) { emptyList() }
        if (left.isNotEmpty()) CallTrace.add("leftover call notifications $left cancelled")
        listOf(ID_INCOMING, ID_WAITING, ID_GROUP_RING).forEach { try { n.cancel(it) } catch (_: Exception) {} }
    }

    /** A second call during a call: a quiet heads-up with two actions, no full-screen intent. */
    fun waiting(c: Context, call: CallInfo) {
        val name = call.peerName.ifBlank { c.getString(R.string.common_unknown) }
        val b = Notification.Builder(c, CH_CALLS)
            .setSmallIcon(R.drawable.ic_stat_tinline)
            .setCategory(Notification.CATEGORY_CALL)
            .setAutoCancel(false)
            .setOnlyAlertOnce(true)
            .setContentTitle(c.getString(R.string.notif_waiting_title, name))
            .setContentText(c.getString(R.string.notif_waiting_text))
            .setContentIntent(activity(c, CallActivity::class.java, 50))
            .setTimeoutAfter(30_000)
            .addAction(Notification.Action.Builder(Icon.createWithResource(c, android.R.drawable.ic_menu_close_clear_cancel), c.getString(R.string.common_decline), service(c, CoreService.ACTION_DECLINE_WAITING, 51)).build())
            .addAction(Notification.Action.Builder(Icon.createWithResource(c, android.R.drawable.sym_action_call), c.getString(R.string.notif_end_answer), service(c, CoreService.ACTION_END_ANSWER, 52)).build())
        try { nm(c).notify(ID_WAITING, b.build()) } catch (e: SecurityException) { }
    }

    fun cancelWaiting(c: Context) = nm(c).cancel(ID_WAITING)

    fun locked(c: Context) {
        val n = Notification.Builder(c, CH_LOCKED)
            .setSmallIcon(R.drawable.ic_stat_tinline)
            .setOnlyAlertOnce(true)
            .setContentTitle(c.getString(R.string.notif_locked_title))
            .setContentText(c.getString(R.string.notif_locked_text))
            .setOngoing(true)
            .setContentIntent(activity(c, MainActivity::class.java, 40))
            .build()
        try { nm(c).notify(ID_LOCKED, n) } catch (e: SecurityException) { }
    }

    fun cancelLocked(c: Context) = nm(c).cancel(ID_LOCKED)

    /** Someone used my code while Tinline was in the background (SIGNUP-HANDOFF § 6f ask 2). A tap opens the request sheet. */
    fun request(c: Context, did: String, name: String) {
        val n = androidx.core.app.NotificationCompat.Builder(c, CH_REQUESTS)
            .setSilent(AlertPrefs.quiet(c))
            .setSmallIcon(R.drawable.ic_stat_tinline)
            .setContentTitle(c.getString(R.string.notif_request_title))
            .setContentText(if (name.isBlank()) c.getString(R.string.notif_request_text_unnamed) else c.getString(R.string.notif_request_text, name))
            .setCategory(Notification.CATEGORY_SOCIAL)
            .setPriority(androidx.core.app.NotificationCompat.PRIORITY_HIGH)
            .setAutoCancel(true)
            .setContentIntent(activity(c, MainActivity::class.java, 40 + (did.hashCode() and 0xfff)) { it.putExtra(EXTRA_REQUEST, did) })
            .build()
        try { nm(c).notify(2000 + (did.hashCode() and 0xffff), n) } catch (e: SecurityException) { }
    }

    /** They accepted my request while Tinline was out of sight: the line is open. A tap opens our chat. */
    fun accepted(c: Context, did: String, name: String) {
        val n = androidx.core.app.NotificationCompat.Builder(c, CH_REQUESTS)
            .setSilent(AlertPrefs.quiet(c))
            .setSmallIcon(R.drawable.ic_stat_tinline)
            .setContentTitle(c.getString(R.string.notif_accepted_title))
            .setContentText(if (name.isBlank()) c.getString(R.string.notif_accepted_text_unnamed) else c.getString(R.string.notif_accepted_text, name))
            .setAutoCancel(true)
            .setContentIntent(activity(c, MainActivity::class.java, 60 + (did.hashCode() and 0xfff)) { it.putExtra(ChatNotifier.EXTRA_PEER, did) })
            .build()
        try { nm(c).notify(2000 + (did.hashCode() and 0xffff), n) } catch (e: SecurityException) { }
    }

    fun missed(c: Context, name: String) {
        val n = androidx.core.app.NotificationCompat.Builder(c, CH_MISSED)
            .setSilent(AlertPrefs.quiet(c))
            .setSmallIcon(R.drawable.ic_stat_tinline)
            .setContentTitle(c.getString(R.string.notif_missed_title))
            .setContentText(name.ifBlank { c.getString(R.string.common_unknown) })
            .setAutoCancel(true)
            .setContentIntent(activity(c, MainActivity::class.java, 30))
            .build()
        try { nm(c).notify(ID_MISSED, n) } catch (e: SecurityException) { }
    }
}
