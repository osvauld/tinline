package com.osvauld.p2p

import android.media.MediaCodecList
import android.os.Build

/** Diagnostic inventory only. Does not create codecs or change our software AV1 call path. */
internal object VideoHardware {
    fun inventory(): List<String> = runCatching {
        MediaCodecList(MediaCodecList.REGULAR_CODECS).codecInfos.flatMap { codec ->
            codec.supportedTypes.filter { it.equals("video/av01", true) || it.equals("video/avc", true) }.map { mime ->
                runCatching {
                    val video = codec.getCapabilitiesForType(mime).videoCapabilities
                    // On API 28, don't guess acceleration from vendor codec names.
                    val hardware = if (Build.VERSION.SDK_INT >= 29) codec.isHardwareAccelerated.toString() else "unknown"
                    val software = if (Build.VERSION.SDK_INT >= 29) codec.isSoftwareOnly.toString() else "unknown"
                    "codec=${codec.name} mime=$mime role=${if (codec.isEncoder) "encode" else "decode"} " +
                        "hardware=$hardware software=$software " +
                        "720p30=${video?.areSizeAndRateSupported(1280, 720, 30.0)}"
                }.getOrElse { "codec=${codec.name} mime=$mime capabilities=unavailable" }
            }
        }
    }.getOrElse { listOf("inventory=unavailable") }
}
