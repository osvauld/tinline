package com.osvauld.p2p

/** UI safeguard only: core unavailable refuses calls, whereas the decided Busy state must not. */
object ProfilePolicy {
    // i18n-deferred: JVM test ProfileRecoveryTest compares this constant and has no Android resources
    const val BUSY_UNSUPPORTED = "Busy is not available yet. Your call settings are unchanged."
    fun availabilityProblem(available: Boolean): String? = if (available) null else BUSY_UNSUPPORTED
}
