package com.osvauld.p2p

import android.content.Context
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow

/**
 * Gate for the groups / blocking / calls-permission / address-change screens, which run on PLACEHOLDER
 * data ([FakeGroupsPrivacy]) until the core implements them. Debug builds only: in a release build
 * [enabled] is always false and the developer switch is not shown, so placeholder screens can't be
 * mistaken for working features. Default ON in debug builds.
 */
object PreviewFlags {
    const val LABEL = "Preview groups & privacy (placeholder data)"
    private const val KEY = "preview_groups_privacy"
    private val _on = MutableStateFlow<Boolean?>(null)

    /** Whether the switch can exist at all in this build. */
    val available: Boolean get() = BuildConfig.DEBUG

    fun enabled(ctx: Context): StateFlow<Boolean?> {
        if (_on.value == null) _on.value = available && prefs(ctx).getBoolean(KEY, true)
        return _on
    }

    fun set(ctx: Context, on: Boolean) {
        if (!available) return
        prefs(ctx).edit().putBoolean(KEY, on).apply()
        _on.value = on
    }

    private fun prefs(ctx: Context) = ctx.getSharedPreferences("dev_flags", Context.MODE_PRIVATE)
}

/** The one placeholder source the preview screens share (state survives leaving and re-entering the preview). */
object PreviewBackend {
    @Volatile private var fake: FakeGroupsPrivacy? = null
    fun get(myName: String): FakeGroupsPrivacy = fake ?: synchronized(this) {
        fake ?: FakeGroupsPrivacy(myName = myName.ifBlank { "You" }).also { fake = it }
    }
}
