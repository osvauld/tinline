package com.osvauld.p2p

/** UI safeguard only: core unavailable refuses calls, whereas the decided Busy state must not. */
object ProfilePolicy {
    /** The refusal for Busy, in the app's language. */
    val BUSY_UNSUPPORTED: String get() = appString(R.string.profile_busy_unsupported)
    fun availabilityProblem(available: Boolean): String? = if (available) null else BUSY_UNSUPPORTED
}
