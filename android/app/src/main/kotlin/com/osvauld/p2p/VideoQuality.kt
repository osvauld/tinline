package com.osvauld.p2p

/** Requested sizes, NOT promises about CameraX's negotiated buffers or encoded resolution. */
internal enum class CapturePreference(val width: Int, val height: Int, val wide: Boolean) {
    SD(640, 480, false),
    HD(1280, 720, true),
}

/** Device-local choice. Auto preserves the existing SD capture/adaptation policy. */
internal enum class VideoQualityPreference(val label: String) {
    AUTO("Auto"), PREFER_HD("Prefer HD");

    companion object {
        fun fromStored(value: String?): VideoQualityPreference = entries.firstOrNull { it.name == value } ?: AUTO
    }
}

internal fun capturePreferences(preferHd: Boolean): List<CapturePreference> =
    if (preferHd) listOf(CapturePreference.HD, CapturePreference.SD) else listOf(CapturePreference.SD)

/** This call's HD chip choice wins over Settings > Video "Sharper video" (which the chip never changes). */
internal fun effectivePreferHd(setting: Boolean, callChoice: Boolean?): Boolean = callChoice ?: setting

/** Shortest side of the picture the phone aims for with sharper video (540p). */
internal const val HD_TARGET_SHORT_PHONE = 540

/**
 * Sharper video is on but the encoder is sending below the HD size: the sender's own ladder stepped down (weak link,
 * a throttled phone or a lower cap on the peer's side; the core does not say which). No frame sent yet is not paused.
 */
internal fun hdPaused(hdOn: Boolean, sendWidth: Int, sendHeight: Int): Boolean =
    hdOn && sendWidth > 0 && sendHeight > 0 && minOf(sendWidth, sendHeight) < HD_TARGET_SHORT_PHONE

/** The HD chip's short line (shown for 2.6 s in the line under the header). */
internal fun hdLine(on: Boolean, paused: Boolean): String = when {
    !on -> "Sharper video off"
    paused -> "HD paused \u00b7 weak connection"
    else -> "Sharper video on"
}

/**
 * How long a sharper-video suggestion or the mobile-data note stays in the short line (ms): longer than the chip's
 * 2.6 s echo, since it asks the person to decide something and they need time to read it and reach the chip.
 */
internal const val HD_SUGGEST_MS = 6000L

/** The chip's own echo of a tap (ms). */
internal const val HD_NOTE_MS = 2600L

/** Sharper video is on while the phone is on mobile data: said once per call, calmly; the chip turns it off. */
internal const val HD_METERED_LINE = "On mobile data, HD uses more \u00b7 tap HD"

/**
 * Whether to say [HD_METERED_LINE] now: HD on, my camera sending, the active network metered, not said yet in this
 * call. No metered exception exists for the suggestions themselves (the core's rule ignores the network type).
 */
internal fun hdMeteredNow(hdOn: Boolean, sending: Boolean, metered: Boolean, saidThisCall: Boolean): Boolean =
    hdOn && sending && metered && !saidThisCall

/**
 * Whether to show the core's suggestion line now ([uniffi.p2pcore.videoHdSuggestion], "" = none): at most once per
 * call, and never over another line that is still showing (the chip's echo wins; the suggestion waits a second).
 */
internal fun hdSuggestNow(line: String, shownThisCall: Boolean, otherLineShowing: Boolean): Boolean =
    line.isNotEmpty() && !shownThisCall && !otherLineShowing

/** Actual encoded/decoded dimensions, not a capture preference or a bitrate-based guess. */
internal fun videoResolutionLabel(width: Int, height: Int): String? {
    if (width <= 0 || height <= 0) return null
    val hd = maxOf(width, height) >= 1280 && minOf(width, height) >= 720
    return "${if (hd) "HD · " else ""}${width}×${height}"
}

/** Only unsupported-resolution/use-case failures retry; never hide permission/lifecycle errors. */
internal fun bindCapture(preferences: List<CapturePreference>, bind: (CapturePreference) -> Unit): CapturePreference {
    require(preferences.isNotEmpty())
    for ((index, preference) in preferences.withIndex()) {
        try { bind(preference); return preference }
        catch (e: IllegalArgumentException) { if (index == preferences.lastIndex) throw e }
    }
    error("unreachable")
}

internal data class VideoRateSnapshot(
    val callId: String,
    val admittedFrames: ULong,
    val receivedFrames: ULong,
    val decodedFrames: ULong,
    val admittedPayloadBytes: ULong,
    val receivedPayloadBytes: ULong,
) {
    fun counters() = listOf(admittedFrames, receivedFrames, decodedFrames, admittedPayloadBytes, receivedPayloadBytes)
}

internal data class VideoRates(
    val admittedFps: Double,
    val receivedFps: Double,
    val decodedFps: Double,
    val admittedPayloadKbps: Double,
    val receivedPayloadKbps: Double,
)

/** One bounded previous sample. No oracle capacity/target bitrate; idle time counts. */
internal class VideoRateMeter {
    private data class Sample(val nanos: Long, val counters: VideoRateSnapshot)
    private var previous: Sample? = null

    @Synchronized fun reset() { previous = null }

    @Synchronized fun sample(nowNanos: Long, counters: VideoRateSnapshot): VideoRates? {
        val old = previous
        if (old == null || old.counters.callId != counters.callId || nowNanos < old.nanos ||
            counters.counters().zip(old.counters.counters()).any { (now, before) -> now < before }) {
            previous = Sample(nowNanos, counters)
            return null
        }
        if (nowNanos == old.nanos) return null // Keep accumulated deltas for the next valid window.
        val seconds = (nowNanos - old.nanos).toDouble() / 1e9
        if (seconds <= 0.0) { previous = Sample(nowNanos, counters); return null }
        val before = old.counters
        previous = Sample(nowNanos, counters)
        fun rate(now: ULong, old: ULong) = (now - old).toDouble() / seconds
        return VideoRates(
            rate(counters.admittedFrames, before.admittedFrames),
            rate(counters.receivedFrames, before.receivedFrames),
            rate(counters.decodedFrames, before.decodedFrames),
            rate(counters.admittedPayloadBytes, before.admittedPayloadBytes) * 8.0 / 1000.0,
            rate(counters.receivedPayloadBytes, before.receivedPayloadBytes) * 8.0 / 1000.0,
        )
    }
}
