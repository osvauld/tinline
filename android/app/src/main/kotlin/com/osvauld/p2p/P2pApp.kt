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
import uniffi.p2pcore.CallInfo
import uniffi.p2pcore.CallState
import uniffi.p2pcore.Contact
import uniffi.p2pcore.LockState
import uniffi.p2pcore.Node
import uniffi.p2pcore.NodeEvents
import uniffi.p2pcore.NodeStatus

/** Process-wide singleton: owns the one [Node] and the observable state the UI renders. */
class P2pApp : Application(), NodeEvents {
    lateinit var node: Node
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

    override fun onCreate() {
        super.onCreate()
        instance = this
        Notifications.createChannels(this)
        calls = CallController(this)
        node = newNode()
        tryAutoUnlock()
        refresh()
    }

    fun refresh() {
        _lock.value = node.lockState()
        _hasIdentity.value = node.hasIdentity()
        _contacts.value = node.contacts()
        _status.value = node.status()
    }

    /**
     * If the vault is locked and a device-wrapped key exists, unlock with it (fast, no passphrase).
     * A key that no longer works is deleted and the node stays Locked. Returns true if usable.
     */
    @Synchronized
    fun tryAutoUnlock(): Boolean {
        if (node.lockState() != LockState.LOCKED) return node.lockState() != LockState.NO_IDENTITY
        if (!UnlockStore.exists(this)) return false
        val key = UnlockStore.load(this)
        if (key == null) { UnlockStore.clear(this); return false }
        return try {
            node.unlockWithKey(key); true
        } catch (e: Exception) {
            Log.w(TAG, "unlockWithKey failed: ${e.javaClass.simpleName}")
            UnlockStore.clear(this); false
        } finally { key.fill(0) }
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
    @Synchronized
    fun restoreOverLocked(phrase: String, name: String, passphrase: String) {
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

    /** Called after create/restore/unlock/set-passphrase: remember the key, bring up service and node. */
    fun identityReady() {
        rememberKey()
        Notifications.cancelLocked(this)
        refresh()
        CoreService.ensureRunning(this)
        scope.launch { startNode() }
    }

    @Synchronized
    fun startNode() {
        if (!node.hasIdentity()) return
        if (!tryAutoUnlock()) {
            Log.w(TAG, "locked; not starting node")
            Notifications.locked(this)
            refresh()
            return
        }
        try { node.start() } catch (e: Exception) { Log.e(TAG, "node.start failed", e) }
        _status.value = node.status()
    }

    // ---- NodeEvents (core threads) ----
    override fun onStatus(status: NodeStatus) { _status.value = status }
    override fun onContactsChanged() {
        val list = node.contacts()
        val known = _contacts.value.map { it.did }.toSet()
        list.filter { it.did !in known }.forEach { testLog("added name=${it.name} did=${it.did}") }
        _contacts.value = list
    }
    override fun onIncomingCall(call: CallInfo) = calls.onIncoming(call)
    override fun onCallState(callId: String, state: CallState) = calls.onState(callId, state)
    override fun onLog(line: String) { Log.d("p2pcore", line) }

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
