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

    /** Short label for the route button and sheet. */
    fun label(r: AudioRoute): String = when (r.kind) {
        AudioRoute.Kind.Earpiece -> "Phone"
        AudioRoute.Kind.Speaker -> "Speaker"
        AudioRoute.Kind.Wired -> r.name.ifBlank { "Headset" }
        AudioRoute.Kind.Bluetooth -> r.name.ifBlank { "Bluetooth" }
        AudioRoute.Kind.Other -> r.name.ifBlank { "Other" }
    }
}
