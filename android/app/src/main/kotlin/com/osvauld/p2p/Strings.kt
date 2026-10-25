package com.osvauld.p2p

import androidx.annotation.PluralsRes
import androidx.annotation.StringRes

/**
 * Localized text for code that has no Context at hand (model and controller code, top-level helpers).
 * Composables use stringResource; services and notification builders use their own Context.
 * Reads the running app. Before it exists (JVM unit tests) there are no resources, so the id is returned
 * as text; real code always runs after Application.onCreate.
 */
fun appString(@StringRes id: Int, vararg args: Any): String {
    val app = P2pApp.instanceOrNull() ?: return "string#$id"
    return if (args.isEmpty()) app.getString(id) else app.getString(id, *args)
}

fun appPlural(@PluralsRes id: Int, count: Int, vararg args: Any): String {
    val app = P2pApp.instanceOrNull() ?: return "plural#$id"
    return app.resources.getQuantityString(id, count, *args)
}
