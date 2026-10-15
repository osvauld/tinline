package com.osvauld.p2p

/** Monotonic-time reveal policy used by the real backup screen. Contains no secret material. */
internal data class RecoveryBackupState(val asKey: Boolean = true, val revealUntil: Long? = null) {
    fun visible(now: Long): Boolean = revealUntil?.let { now < it } == true
    fun canCopyKey(now: Long): Boolean = asKey && visible(now)
    fun reveal(now: Long) = copy(revealUntil = now + REVEAL_MS)
    fun hide() = copy(revealUntil = null)
    fun select(key: Boolean) = RecoveryBackupState(asKey = key)

    companion object { const val REVEAL_MS = 30_000L }
}
