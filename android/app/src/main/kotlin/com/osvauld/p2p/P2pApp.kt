package com.osvauld.p2p

import android.app.Application
import android.content.Context
import android.util.Log
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged
import uniffi.p2pcore.Availability
import uniffi.p2pcore.CallInfo
import uniffi.p2pcore.CallRecord
import uniffi.p2pcore.CallState
import uniffi.p2pcore.Contact
import uniffi.p2pcore.LockState
import uniffi.p2pcore.Node
import uniffi.p2pcore.NodeEvents
import uniffi.p2pcore.NodeStatus

/** Process-wide singleton: owns the one [Node] and the observable state the UI renders. */
class P2pApp : Application(), NodeEvents {
    @Volatile lateinit var node: Node
        private set
    @Volatile var testToneHz: Float? = null
    val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    lateinit var calls: CallController
        private set

    private val _status = MutableStateFlow<NodeStatus?>(null)
    val status: StateFlow<NodeStatus?> = _status
    private val _contacts = MutableStateFlow<List<Contact>>(emptyList())
    val contacts: StateFlow<List<Contact>> = _contacts
    private val _lock = MutableStateFlow(LockState.NO_IDENTITY)
    val lockState: StateFlow<LockState> = _lock
    private val _hasIdentity = MutableStateFlow(false)
    val hasIdentity: StateFlow<Boolean> = _hasIdentity

    private val _history = MutableStateFlow<List<CallRecord>>(emptyList())
    /** The call log, newest first. Re-read on every call end (the core writes it before `Ended` fires). */
    val history: StateFlow<List<CallRecord>> = _history
    private val _availability = MutableStateFlow(Availability(true, null))
    val availability: StateFlow<Availability> = _availability
    @Volatile private var availJob: Job? = null

    override fun onCreate() {
        super.onCreate()
        instance = this
        Notifications.createChannels(this)
        calls = CallController(this)
        node = newNode()
        tryAutoUnlock()
        refresh()
        // The always-on notification says "Available for calls" / "Not available" / "Offline".
        scope.launch {
            combine(status, availability) { st, av -> (st?.online == true) to av.available }.distinctUntilChanged()
                .collect { CoreService.refreshNotification() }
        }
    }

    fun refreshHistory() {
        try { _history.value = node.recentCalls(200u) } catch (e: Exception) { Log.w(TAG, "recentCalls: $e") }
    }

    /** Re-reads availability and arms a timer for when a timed break ends (no polling). */
    fun refreshAvailability() {
        val a = try { node.availability() } catch (e: Exception) { return }
        _availability.value = a
        availJob?.cancel()
        val until = a.until
        if (!a.available && until != null) availJob = scope.launch {
            delay((until.toLong() * 1000 - System.currentTimeMillis()).coerceAtLeast(0) + 500)
            refreshAvailability()
        }
    }

    fun setAvailable(available: Boolean, until: Long? = null) {
        try { node.setAvailable(available, until?.toULong()) } catch (e: Exception) { Log.w(TAG, "setAvailable: $e") }
        refreshAvailability()
    }

    fun refresh() {
        _lock.value = node.lockState()
        _hasIdentity.value = node.hasIdentity()
        _contacts.value = node.contacts()
        _status.value = node.status()
        if (_lock.value == LockState.UNLOCKED) { refreshHistory(); refreshAvailability() }
    }

    private val unlockLock = Any()

    /**
     * If the vault is locked and a device-wrapped key exists, unlock with it (fast, no passphrase).
     * A key that is definitively unusable (bad tag, key gone, core says WrongPassphrase) is deleted
     * and the node stays Locked; on a transient Keystore/IO error unlock.bin is kept and the next
     * startNode retries. Returns true if usable. Blocking (Keystore): not for the main thread.
     */
    fun tryAutoUnlock(): Boolean = synchronized(unlockLock) {
        if (node.lockState() != LockState.LOCKED) return node.lockState() != LockState.NO_IDENTITY
        if (!UnlockStore.exists(this)) return false
        when (val l = UnlockStore.load(this)) {
            UnlockStore.Loaded.Gone -> { UnlockStore.clear(this); false }
            UnlockStore.Loaded.Transient -> false
            is UnlockStore.Loaded.Key -> try {
                node.unlockWithKey(l.bytes); true
            } catch (e: uniffi.p2pcore.Exception.WrongPassphrase) {
                Log.w(TAG, "unlockWithKey: key rejected")
                UnlockStore.clear(this); false
            } catch (e: Exception) {
                Log.w(TAG, "unlockWithKey failed (kept for retry): ${e.javaClass.simpleName}")
                false
            } finally { l.bytes.fill(0) }
        }
    }

    /** After any successful passphrase path: remember the data key under the Keystore. */
    fun rememberKey() {
        val k = node.unlockKey() ?: return
        UnlockStore.save(this, k)
        k.fill(0)
    }

    private fun newNode() = Node(filesDir.resolve("core").also { it.mkdirs() }.absolutePath, this)

    /**
     * "Forgot passphrase": restore the same identity from its phrase over the locked vault. The core
     * refuses to restore over an existing profile, so the sealed profile.json is moved aside first
     * (state.json, i.e. contacts, is untouched) and moved back if the restore fails. Blocking.
     */
    fun restoreOverLocked(phrase: String, name: String, passphrase: String) {
        try { lifecycle.submit { restoreOverLockedNow(phrase, name, passphrase) }.get() }
        catch (e: java.util.concurrent.ExecutionException) { throw e.cause ?: e }
    }

    private fun restoreOverLockedNow(phrase: String, name: String, passphrase: String) {
        val dir = filesDir.resolve("core")
        val prof = dir.resolve("profile.json")
        val bak = dir.resolve("profile.json.bak")
        val oldDid = node.profile()?.did
        node.stop()
        if (prof.exists()) { bak.delete(); prof.renameTo(bak) }
        node = newNode()
        try {
            node.restoreIdentity(phrase, name, passphrase)
            // The contacts in state.json belong to the locked identity; a different phrase must not
            // inherit them.
            if (oldDid != null && node.profile()?.did != oldDid) {
                throw IllegalArgumentException("That recovery phrase belongs to a different identity")
            }
            bak.delete()
            UnlockStore.clear(this)
        } catch (e: Exception) {
            if (bak.exists()) { prof.delete(); bak.renameTo(prof); node = newNode() }
            throw e
        }
    }

    /**
     * Called after create/restore/unlock/set-passphrase. The state refresh is immediate; remembering
     * the key (Keystore work) and starting the node run off the main thread.
     */
    fun identityReady() {
        refresh()
        scope.launch {
            rememberKey()
            Notifications.cancelLocked(this@P2pApp)
            CoreService.ensureRunning(this@P2pApp)
            startNode()
        }
    }

    /** Node start/stop and identity swaps run one at a time on this thread, never on the callers' locks. */
    private val lifecycle = java.util.concurrent.Executors.newSingleThreadExecutor { r -> Thread(r, "p2p-lifecycle") }

    /** Unlocks (if needed) and starts the node. Queued on the lifecycle thread; returns immediately. */
    fun startNode() {
        lifecycle.execute {
            try {
                if (!node.hasIdentity()) return@execute
                if (!tryAutoUnlock()) {
                    Log.w(TAG, "locked; not starting node")
                    Notifications.locked(this)
                    refresh()
                    return@execute
                }
                Notifications.cancelLocked(this)
                try { node.start() } catch (e: Exception) { Log.e(TAG, "node.start failed", e) }
                refresh()
            } catch (e: Throwable) { Log.e(TAG, "startNode", e) }
        }
    }

    // ---- NodeEvents (core threads) ----
    override fun onStatus(status: NodeStatus) { _status.value = status }
    override fun onContactsChanged() {
        val list = node.contacts()
        val known = _contacts.value.map { it.did }.toSet()
        list.filter { it.did !in known }.forEach { testLog("added name=${it.name} did=${it.did}") }
        _contacts.value = list
        refreshHistory()
    }
    override fun onIncomingCall(call: CallInfo) = calls.onIncoming(call)
    override fun onCallState(callId: String, state: CallState) {
        if (state is CallState.Ended) refreshHistory()
        calls.onState(callId, state)
    }
    override fun onLog(line: String) { if (BuildConfig.DEBUG) Log.d("p2pcore", line) }

    companion object {
        const val TAG = "P2P"
        lateinit var instance: P2pApp
            private set
        fun get(ctx: Context): P2pApp = ctx.applicationContext as P2pApp
    }
}

fun testLog(msg: String) {
    if (BuildConfig.DEBUG) Log.i("P2PTEST", msg)
}
