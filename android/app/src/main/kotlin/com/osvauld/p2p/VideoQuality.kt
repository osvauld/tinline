package com.osvauld.p2p

/** Experiment only: requested sizes, NOT promises about CameraX's negotiated buffers. */
internal enum class CapturePreference(val width: Int, val height: Int, val wide: Boolean) {
    SD(640, 480, false),
    HD(1280, 720, true),
}

/** Release and baseline debug builds retain existing capture preference. */
internal fun capturePreferences(debug: Boolean, hdRequested: Boolean): List<CapturePreference> =
    if (debug && hdRequested) listOf(CapturePreference.HD, CapturePreference.SD)
    else listOf(CapturePreference.SD)

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
