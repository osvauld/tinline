package com.osvauld.p2p

import android.os.Trace
import android.util.Log
import android.view.View
import java.util.concurrent.atomic.AtomicBoolean
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow

/** Startup marks: Perfetto sections (`atrace -a com.osvauld.tinline`) and when the chat list first has real data. */
object Startup {
    private val _chatsLoaded = MutableStateFlow(false)
    /** The core answered the chat list once this process: Home reports "fully drawn" on the next frame. */
    val chatsLoaded: StateFlow<Boolean> = _chatsLoaded
    fun chatsLoaded() { _chatsLoaded.value = true }

    private val homeDrawn = AtomicBoolean(false)
    /** Logs `TinStartup: home drawn` once per process, after the frame showing the loaded chat list has finished drawing
     *  (reportFullyDrawn fires before that frame's draw). Called from composition once [chatsLoaded]. */
    fun homeDrawn(view: View) { if (homeDrawn.compareAndSet(false, true)) view.post { Log.i("TinStartup", "home drawn") } }

    inline fun <T> trace(name: String, block: () -> T): T {
        Trace.beginSection(name)
        try { return block() } finally { Trace.endSection() }
    }
}
