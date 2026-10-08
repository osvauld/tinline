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
    lateinit var realChat: CoreChatSource
        private set
    /** The chat backend: the node's, or the in-memory fake the debug gallery swaps in. */
    val chat: ChatSource get() = ChatBackend.current(this)

    private val _status = MutableStateFlow<NodeStatus?>(null)
    val status: StateFlow<NodeStatus?> = _status
    private val _contacts = MutableStateFlow<List<Contact>>(emptyList())
    val contacts: StateFlow<List<Contact>> = _contacts
    private val _lock = MutableStateFlow(LockState.NO_IDENTITY)
    val lockState: StateFlow<LockState> = _lock
    private val _hasIdentity = MutableStateFlow(false)
    val hasIdentity: StateFlow<Boolean> = _hasIdentity

    private val _accountDid = MutableStateFlow<String?>(null)
    /** The selected account's DID (changes when the user switches); null with no account. */
    val accountDid: StateFlow<String?> = _accountDid

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
        ChatMedia.clear(this)
        calls = CallController(this)
        node = newNode()
        realChat = CoreChatSource(this).also { it.attach() }
        selectLastAccountIfNone()
        UnlockStore.migrateLegacy(this, currentDid())
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
        _accountDid.value = currentDid()
        _contacts.value = node.contacts()
        _status.value = node.status()
        if (_lock.value == LockState.UNLOCKED) { refreshHistory(); refreshAvailability(); chat.refresh() }
        else { _history.value = emptyList(); _availability.value = Availability(true, null) }
        // The account to come back to if the app dies while a new one is being added.
        if (_hasIdentity.value && node.identityCommitted()) currentDid()?.let { lastPrefs.edit().putString("did", it).apply() }
    }

    private val lastPrefs by lazy { getSharedPreferences("accounts", Context.MODE_PRIVATE) }

    fun currentDid(): String? = try { node.profile()?.did } catch (_: Exception) { null }

    /**
     * Adding an account deselects the current one in the core. If the process died in that state the
     * accounts would be invisible, so select the last used one (or the first) again.
     */
    private fun selectLastAccountIfNone() {
        try {
            if (node.hasIdentity()) return
            val all = node.accounts()
            if (all.isEmpty()) return
            val want = lastPrefs.getString("did", null)
            node.switchAccount((all.firstOrNull { it.did == want } ?: all.first()).did)
        } catch (e: Exception) { Log.w(TAG, "selecting an account: ${e.javaClass.simpleName}") }
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
        val did = currentDid() ?: return false
        if (!UnlockStore.exists(this, did)) return false
        when (val l = UnlockStore.load(this, did)) {
            UnlockStore.Loaded.Gone -> { UnlockStore.clear(this, did); false }
            UnlockStore.Loaded.Transient -> false
            is UnlockStore.Loaded.Key -> try {
                node.unlockWithKey(l.bytes); true
            } catch (e: uniffi.p2pcore.Exception.WrongPassphrase) {
                Log.w(TAG, "unlockWithKey: key rejected")
                UnlockStore.clear(this, did); false
            } catch (e: Exception) {
                Log.w(TAG, "unlockWithKey failed (kept for retry): ${e.javaClass.simpleName}")
                false
            } finally { l.bytes.fill(0) }
        }
    }

    /**
     * Wraps the vault data key under the Keystore for the current account. False if it could not be
     * saved (nothing is remembered then). Blocking (Keystore).
     */
    fun rememberKey(): Boolean {
        val did = currentDid() ?: return false
        val k = node.unlockKey() ?: return false
        return try { UnlockStore.save(this, did, k) } finally { k.fill(0) }
    }

    /**
     * S1: makes a freshly created/restored identity durable. A no-passphrase identity exists only in
     * memory until its data key is saved under the Keystore; only then is `commitIdentity` called.
     * Returns null on success, else a message for the user (the identity then stays uncommitted: the
     * caller offers Retry or a passphrase). A passphrase identity is already on disk; failing to
     * remember its key only means the passphrase is asked after a restart. Blocking.
     */
    fun persistIdentity(): String? {
        val saved = rememberKey()
        if (!saved && !node.hasPassphrase()) return "Couldn’t save your account safely on this phone."
        if (!saved) Log.w(TAG, "key not remembered; passphrase will be asked at startup")
        return try { node.commitIdentity(); null } catch (e: Exception) { Log.w(TAG, "commitIdentity: ${e.javaClass.simpleName}"); "Couldn’t save your account on this phone." }
    }

    private fun newNode() = Node(filesDir.resolve("core").also { it.mkdirs() }.absolutePath, this)

    // ---- accounts ----

    /** Set while a new account is being created/restored next to the existing ones. */
    data class Adding(val from: String?, val restore: Boolean)
    private val _adding = MutableStateFlow<Adding?>(null)
    val adding: StateFlow<Adding?> = _adding

    /** Leaves the current account (offline until we come back) and lets onboarding add another. Blocking; throws InCall. */
    fun beginAdding(restore: Boolean) {
        try {
            lifecycle.submit {
                val from = currentDid()
                node.beginNewAccount()
                _adding.value = Adding(from, restore)
                refresh()
            }.get()
        } catch (e: java.util.concurrent.ExecutionException) { throw e.cause ?: e }
    }

    /** Back out of adding: select the account we left and bring it online. Blocking. */
    fun cancelAdding() {
        val from = _adding.value?.from
        try {
            if (from != null) switchTo(from, null) else _adding.value = null
        } finally { _adding.value = null; refresh() }
    }

    fun finishAdding() { _adding.value = null }

    /**
     * Selects [did] and unlocks it: with [passphrase], else with that account's remembered key (else it
     * stays locked and the unlock screen shows). A wrong passphrase puts the previous account back and
     * rethrows. Throws InCall during a call. Blocking.
     */
    fun switchTo(did: String, passphrase: String?) {
        try { lifecycle.submit { switchToNow(did, passphrase) }.get() }
        catch (e: java.util.concurrent.ExecutionException) { throw e.cause ?: e }
        refresh()
        Notifications.cancelLocked(this)
        CoreService.ensureRunning(this)
        startNode()
    }

    private fun switchToNow(did: String, passphrase: String?) {
        val prev = currentDid()
        node.switchAccount(did)
        if (passphrase == null) { tryAutoUnlock(); return }
        try {
            node.unlock(passphrase)
        } catch (e: Exception) {
            if (prev != null && prev != did) try { node.switchAccount(prev); tryAutoUnlock() } catch (_: Exception) {}
            throw e
        }
        rememberKey()
    }

    /** Deletes a non-current account and its remembered key. */
    fun removeAccount(did: String) {
        node.removeAccount(did)
        UnlockStore.clear(this, did)
    }

    /**
     * "Forgot passphrase": restore the same identity from its phrase. The core never restores over an
     * existing account, so the locked account is deselected, and if the phrase turns out to be this
     * very account (AccountExists, and it is the only one) its directory is replaced: the passphrase is
     * new, but contacts and history of that account are NOT kept (a core change could keep them).
     * A phrase of a different identity changes nothing. Blocking.
     */
    fun restoreOverLocked(phrase: String, name: String, passphrase: String) {
        try { lifecycle.submit { restoreOverLockedNow(phrase, name, passphrase) }.get() }
        catch (e: java.util.concurrent.ExecutionException) { throw e.cause ?: e }
    }

    private fun restoreOverLockedNow(phrase: String, name: String, passphrase: String) {
        val oldDid = currentDid() ?: throw IllegalStateException("no account")
        val others = node.accounts().any { it.did != oldDid }
        node.beginNewAccount()
        try {
            try {
                node.restoreIdentity(phrase, name, passphrase)
            } catch (e: uniffi.p2pcore.Exception.AccountExists) {
                if (others) throw IllegalArgumentException("That recovery phrase belongs to an account that is already on this phone. Switch to it in Settings > Switch.")
                node.removeAccount(oldDid)
                UnlockStore.clear(this, oldDid)
                node.restoreIdentity(phrase, name, passphrase)
            }
            if (node.profile()?.did != oldDid) {
                // A different identity was just created next to the locked one: take it back out.
                val fresh = node.profile()?.did
                node.beginNewAccount()
                if (fresh != null && node.accounts().any { it.did == fresh }) node.removeAccount(fresh)
                node.switchAccount(oldDid)
                throw IllegalArgumentException("That recovery phrase belongs to a different identity")
            }
            UnlockStore.clear(this, oldDid)
        } catch (e: Exception) {
            if (!node.hasIdentity() && node.accounts().any { it.did == oldDid }) runCatching { node.switchAccount(oldDid) }
            throw e
        }
    }

    /**
     * Called after unlock/set-passphrase (and by debug hooks). The state refresh is immediate; remembering
     * the key and committing (S1) and starting the node run off the main thread.
     */
    fun identityReady() {
        refresh()
        scope.launch {
            // A committed account is started whatever the Keystore said; an uncommitted one is not
            // running yet (the caller that created it handles the failure, see persistIdentity).
            val err = persistIdentity()
            if (err != null && !node.identityCommitted()) { Log.w(TAG, "identity not committed: $err"); return@launch }
            startServices()
        }
    }

    /** Starts the foreground service and the node for the current account. */
    fun startServices() {
        refresh()
        Notifications.cancelLocked(this)
        CoreService.ensureRunning(this)
        startNode()
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
