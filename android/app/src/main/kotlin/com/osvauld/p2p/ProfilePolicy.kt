package com.osvauld.p2p

/** UI safeguard only: core unavailable refuses calls, whereas the decided Busy state must not. */
object ProfilePolicy {
    const val BUSY_UNSUPPORTED = "Busy is not available yet. Your call settings are unchanged."
    fun availabilityProblem(available: Boolean): String? = if (available) null else BUSY_UNSUPPORTED
}
