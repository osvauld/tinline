package com.osvauld.p2p

import android.os.Trace
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow

/** Startup marks: Perfetto sections (`atrace -a com.osvauld.tinline`) and when the chat list first has real data. */
object Startup {
    private val _chatsLoaded = MutableStateFlow(false)
    /** The core answered the chat list once this process: Home reports "fully drawn" on the next frame. */
    val chatsLoaded: StateFlow<Boolean> = _chatsLoaded
    fun chatsLoaded() { _chatsLoaded.value = true }

    inline fun <T> trace(name: String, block: () -> T): T {
        Trace.beginSection(name)
        try { return block() } finally { Trace.endSection() }
    }
}
