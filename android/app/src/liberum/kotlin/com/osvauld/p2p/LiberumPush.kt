package com.osvauld.p2p

import android.content.Context
import uniffi.p2pcore.PushTarget

/**
 * The `liberum` edition's [PushProvider]: none. No Google or Firebase code is in this edition; it stays
 * always-on, as before. UnifiedPush goes here in a later slice (docs/push.md "Android (Tinline Liberum)").
 */
internal object NoPush : PushProvider {
    override fun available(ctx: Context) = false
    override fun setEnabled(ctx: Context, on: Boolean) {}
    override fun fetchTarget(ctx: Context, done: (PushTarget?) -> Unit) = done(null)
}

internal fun platformPushProvider(): PushProvider = NoPush
