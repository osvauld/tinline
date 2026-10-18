package com.osvauld.p2p

/**
 * Where call audio plays (and, for headsets, where the mic is). Telecom ([CallSession]) or the
 * fallback in [AudioEngine] fills these in; [key] is only meaningful to whichever made it.
 */
data class AudioRoute(val kind: Kind, val name: String, val key: String) {
    enum class Kind { Earpiece, Speaker, Bluetooth, Wired, Other }
}

/** The routes on offer right now and the one in use (null until known). */
data class RouteState(val current: AudioRoute? = null, val available: List<AudioRoute> = emptyList()) {
    val speaker: Boolean get() = current?.kind == AudioRoute.Kind.Speaker
    /** Held to the ear: the proximity sensor may switch the screen off. */
    val earpiece: Boolean get() = current?.kind == AudioRoute.Kind.Earpiece
    /** More than phone and speaker to choose from: the route button opens a list instead of toggling. */
    val hasHeadset: Boolean get() = available.any { it.kind == AudioRoute.Kind.Bluetooth || it.kind == AudioRoute.Kind.Wired }
}

object Routes {
    /**
     * The route a call starts on: a connected headset first (Bluetooth, then wired), else the
     * loudspeaker for group and video calls ([preferSpeaker]) or the earpiece for 1:1 calls.
     */
    fun pickDefault(available: List<AudioRoute>, preferSpeaker: Boolean): AudioRoute? =
        available.firstOrNull { it.kind == AudioRoute.Kind.Bluetooth }
            ?: available.firstOrNull { it.kind == AudioRoute.Kind.Wired }
            ?: available.firstOrNull { it.kind == if (preferSpeaker) AudioRoute.Kind.Speaker else AudioRoute.Kind.Earpiece }
            ?: available.firstOrNull { it.kind == AudioRoute.Kind.Earpiece || it.kind == AudioRoute.Kind.Speaker }
            ?: available.firstOrNull()

    /** The speaker button: earpiece/headset <-> speaker. Back from speaker goes to the default non-speaker route. */
    fun toggleTarget(state: RouteState): AudioRoute? =
        if (state.speaker) pickDefault(state.available.filter { it.kind != AudioRoute.Kind.Speaker }, preferSpeaker = false)
        else state.available.firstOrNull { it.kind == AudioRoute.Kind.Speaker }

    /**
     * A video call right now (the phone is held away from the ear). Until video is [decided] for the call (the
     * core's first negotiated VideoInfo, or the call active without one) that is how it was placed or rung
     * ([uniffi.p2pcore.CallInfo.video]); from then on, video runs and either camera is on, so a voice call turns into
     * a video call when a camera goes on, and a video call to a device without video is a voice call.
     */
    fun isVideo(placedAsVideo: Boolean, decided: Boolean, negotiated: Boolean, localCamera: Boolean, remoteCamera: Boolean): Boolean =
        if (!decided) placedAsVideo else negotiated && (localCamera || remoteCamera)

    /**
     * The call just became a video call: time to move to the loudspeaker ([CallSession.speakerIfEarpiece]: only from
     * the earpiece; a headset or Bluetooth stays). Once per change, so a route picked during the video call is kept.
     */
    fun becameVideo(wasVideo: Boolean, isVideo: Boolean): Boolean = isVideo && !wasVideo

    /** The proximity wake lock (screen off at the ear): only for a voice call on the earpiece, never in a video call. */
    fun proximityWanted(state: RouteState, video: Boolean): Boolean = state.earpiece && !video

    /** core-telecom's name for a Bluetooth device it may not read (no BLUETOOTH_CONNECT). */
    const val GENERIC_BT = "Bluetooth Device"

    /** A Bluetooth route shows without its own name: allowing Nearby devices would name it. */
    fun unnamedBluetooth(st: RouteState): Boolean =
        st.available.any { it.kind == AudioRoute.Kind.Bluetooth && (it.name.isBlank() || it.name == GENERIC_BT) }

    /** Short label for the route button and sheet. */
    fun label(r: AudioRoute): String = when (r.kind) {
        AudioRoute.Kind.Earpiece -> "Phone"
        AudioRoute.Kind.Speaker -> "Speaker"
        AudioRoute.Kind.Wired -> r.name.ifBlank { "Headset" }
        AudioRoute.Kind.Bluetooth -> r.name.takeUnless { it.isBlank() || it == GENERIC_BT } ?: "Bluetooth"
        AudioRoute.Kind.Other -> r.name.ifBlank { "Other" }
    }
}
