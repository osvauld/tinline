package com.osvauld.p2p

import androidx.annotation.PluralsRes
import androidx.annotation.StringRes

/**
 * Localized text for code that has no Context at hand (model and controller code, top-level helpers).
 * Composables use stringResource; services and notification builders use their own Context.
 * Needs the running app, so it is not for pure logic that JVM unit tests call.
 */
fun appString(@StringRes id: Int, vararg args: Any): String = P2pApp.instance.getString(id, *args)

fun appPlural(@PluralsRes id: Int, count: Int, vararg args: Any): String =
    P2pApp.instance.resources.getQuantityString(id, count, *args)
