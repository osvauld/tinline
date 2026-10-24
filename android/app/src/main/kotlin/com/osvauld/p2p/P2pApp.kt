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
import uniffi.p2pcore.ContactRequest
import uniffi.p2pcore.ContactRequestEvents
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
    lateinit var video: CallVideo
        private set
    /** Group-call audio + service (GA-A1); the core's GroupCallEvents sink. */
    lateinit var groupCalls: GroupCallAudio
    /** The group call screens' state (GroupCallUi.kt). */
    lateinit var groupCallUi: GroupCallUi
        private set
    private lateinit var hostConditions: HostConditions
    lateinit var realChat: CoreChatSource
        private set
    /** Group chat on the core (GroupsCore.kt): the Groups tab. */
    lateinit var groups: GroupsController
        private set
    /** Device linking: the state machine behind the Link a device screens, and the core's LinkEvents. */
    lateinit var links: LinkController
        private set
    lateinit var profiles: ProfilesController
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
    private val _ready = MutableStateFlow(false)
    /**
     * The first account load (select, auto-unlock, refresh) is done. Until then [lockState] and [hasIdentity] are only
     * defaults, so the UI shows a quiet placeholder rather than onboarding or the unlock screen.
     */
    val ready: StateFlow<Boolean> = _ready

    private val _accountDid = MutableStateFlow<String?>(null)
    /** The selected account's DID (changes when the user switches); null with no account. */
    val accountDid: StateFlow<String?> = _accountDid

    private val _history = MutableStateFlow<List<CallRecord>>(emptyList())
    /** The call log, newest first. Re-read on every call end (the core writes it before `Ended` fires). */
    val history: StateFlow<List<CallRecord>> = _history
    private val _groupLog = MutableStateFlow<List<uniffi.p2pcore.GroupCallRecord>>(emptyList())
    /** Group calls this device was in or missed, newest first (the core's group call log; local to this device). */
    val groupLog: StateFlow<List<uniffi.p2pcore.GroupCallRecord>> = _groupLog
    private val _blocks = MutableStateFlow(Blocks())
    /** Contact blocking (Blocking.kt): who we block, who blocks us. Re-read on every contacts/chats change. */
    val blocks: StateFlow<Blocks> = _blocks
    private val _requestsIn = MutableStateFlow<List<ContactRequest>>(emptyList())
    /** Requests made with my code, waiting for my answer (Contacts › "Waiting for you"). */
    val requestsIn: StateFlow<List<ContactRequest>> = _requestsIn
    private val _requestsOut = MutableStateFlow<List<ContactRequest>>(emptyList())
    /** Requests I sent, waiting for theirs (Contacts › "Waiting for them", the empty state). */
    val requestsOut: StateFlow<List<ContactRequest>> = _requestsOut
    /**
     * The Add contact sheet as the other side of a handshake (people.svelte.ts `pp.incoming`): someone used my code
     * ("Contact request"), or accepted mine while the sheet was closed ("New contact"). MainActivity opens the sheet for it
     * when nothing else is in front (never over a call or another screen), else drops it: the request waits in Contacts.
     */
    val incoming = MutableStateFlow<IncomingSheet?>(null)
    /** The last request of mine they accepted: an open sheet on that person turns taut by itself. */
    val accepted = MutableStateFlow<Contact?>(null)
    /** The Add contact sheet is on screen (it sets this), so an acceptance doesn't open a second moment. */
    @Volatile var addSheetOpen = false
    private val _availability = MutableStateFlow(Availability(true, null))
    val availability: StateFlow<Availability> = _availability
    @Volatile private var availJob: Job? = null

    override fun onCreate() = Startup.trace("P2pApp.onCreate") {
        super.onCreate()
        instance = this
        CallTrace.init(filesDir)
        CallTrace.lastExit(this)
        Notifications.createChannels(this)
        // A new process has no call yet: a ringing or waiting notification left by a process that
        // died mid-call (posted with notify, not tied to the service) would otherwise stay forever.
        Notifications.cancelCallLeftovers(this)
        ChatMedia.clear(this)
        calls = CallController(this)
        video = CallVideo(this)
        node = Startup.trace("newNode") { newNode() }
        profiles = ProfilesController(this)
        node.setProfileEvents(profiles)
        node.setVideoEvents(video)
        node.setVideoEnabled(videoEnabled)
        links = LinkController(this)
        node.setLinkEvents(links)
        groupCalls = GroupCallAudio(this)
        node.setGroupCallEvents(groupCalls)
        groupCallUi = GroupCallUi(this)
        node.setContactRequestEvents(requestEvents)
        hostConditions = HostConditions(this).also { it.start() }
        realChat = CoreChatSource(this).also { it.attach() }
        // The cached DID: the group screens read it per row while composing (a core call there stalled the UI).
        groups = GroupsController(NodeGroupApi { node }) { _accountDid.value ?: currentDid() }.also { it.attach(node) }
        groups.onPost = { m -> GroupNotifier.post(this, m, groups.list.value.firstOrNull { it.id == m.groupId }?.name) }
        // Someone invited me (playground Notices "invite"): out of sight a notification; in front one knock, and the
        // Groups tab shows its count. Follows Notifications › Groups.
        groups.onInvitation = { i ->
            if (!i.outgoing && i.state == uniffi.p2pcore.GroupInvitationState.PENDING && AlertPrefs.groups(this)) {
                if (AppVisible.resumed) Cues.knock() else GroupNotifier.invitation(this, i)
            }
        }
        // Keystore unlock and the first state read run on the lifecycle thread (before any queued startNode), so the
        // first frame never waits for them.
        lifecycle.execute {
            try {
                Startup.trace("selectAccount") { selectLastAccountIfNone() }
                Startup.trace("tryAutoUnlock") { tryAutoUnlock() }
                Startup.trace("refresh") { refresh() }
            } catch (e: Throwable) { Log.e(TAG, "first load", e) } finally { _ready.value = true }
        }
        // Kept chats of people who are no longer contacts (their block deleted our entry) come only from the chat list.
        scope.launch { realChat.chats.collect { refreshBlocks() } }
        // The always-on notification says "Available for calls" / "Not available" / "Offline".
        scope.launch {
            combine(status, availability) { st, av -> (st?.online == true) to av.available }.distinctUntilChanged()
                .collect { CoreService.refreshNotification() }
        }
        Unit
    }

    fun refreshHistory() {
        try { _history.value = node.recentCalls(200u) } catch (e: Exception) { Log.w(TAG, "recentCalls: $e") }
        refreshGroupLog()
    }

    /** Re-reads the group call log (the core writes a record before the `Ended` / `Left` event fires). */
    fun refreshGroupLog() {
        try { _groupLog.value = node.groupCallHistory(null) } catch (e: Exception) { Log.w(TAG, "groupCallHistory: $e") }
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

    /** A contact's profile changed (their name may have): re-read the contact list only. */
    fun refreshContacts() {
        try { _contacts.value = node.contacts() } catch (e: Exception) { Log.w(TAG, "contacts: ${e.javaClass.simpleName}") }
    }

    fun setAvailable(available: Boolean, until: Long? = null) {
        try { node.setAvailable(available, until?.toULong()) } catch (e: Exception) { Log.w(TAG, "setAvailable: $e") }
        refreshAvailability()
    }

    /** [refresh] on the lifecycle thread, after any unlock or account switch already queued there. For UI callers. */
    fun refreshLater() {
        lifecycle.execute { try { refresh() } catch (e: Exception) { Log.w(TAG, "refresh: ${e.javaClass.simpleName}") } }
    }

    fun refresh() {
        _lock.value = node.lockState()
        _hasIdentity.value = node.hasIdentity()
        _accountDid.value = currentDid()
        profiles.select(_accountDid.value.takeIf { _lock.value == LockState.UNLOCKED })
        rememberName()
        _contacts.value = node.contacts()
        refreshRequests()
        refreshBlocks()
        _status.value = node.status()
        scope.launch { groups.refresh() }
        if (_lock.value == LockState.UNLOCKED) { refreshHistory(); refreshAvailability(); chat.refresh() }
        else { _history.value = emptyList(); _availability.value = Availability(true, null) }
        // The account to come back to if the app dies while a new one is being added.
        if (_hasIdentity.value && node.identityCommitted()) currentDid()?.let { lastPrefs.edit().putString("did", it).apply() }
    }

    /** DID -> name of the accounts seen, so "this phone was unlinked" can still say whose account it was. */
    private val names = java.util.concurrent.ConcurrentHashMap<String, String>()
    private fun rememberName() {
        try { node.profile()?.let { names[it.did] = it.name } } catch (_: Exception) {}
    }

    private val _unlinked = MutableStateFlow<String?>(null)
    /** Set (to the account's name) when another device removed the account from this phone; the screen clears it. */
    val unlinked: StateFlow<String?> = _unlinked
    fun dismissUnlinked() { _unlinked.value = null }

    /**
     * The core removed account [did] from this phone (we unlinked it here, or another device did):
     * forget its remembered key, select another account if one is left (and bring it online).
     * [notify] shows the "This phone was unlinked" screen. Blocking.
     */
    fun accountGone(did: String, notify: Boolean) {
        val name = names.remove(did) ?: ""
        UnlockStore.clear(this, did)
        if (lastPrefs.getString("did", null) == did) lastPrefs.edit().remove("did").apply()
        if (notify) _unlinked.value = name
        try { lifecycle.submit { selectLastAccountIfNone() }.get() } catch (e: Exception) { Log.w(TAG, "accountGone: ${e.javaClass.simpleName}") }
        refresh()
        if (node.hasIdentity()) { Notifications.cancelLocked(this); CoreService.ensureRunning(this); startNode() }
    }

    private val lastPrefs by lazy { getSharedPreferences("accounts", Context.MODE_PRIVATE) }
    private val callPrefs by lazy { getSharedPreferences("calls", Context.MODE_PRIVATE) }

    /** Offer and take video in calls (Settings); the core's default is on and it isn't stored there. */
    var videoEnabled: Boolean
        get() = callPrefs.getBoolean("video", true)
        set(on) { callPrefs.edit().putBoolean("video", on).apply(); node.setVideoEnabled(on) }

    /** Device-local capture preference; changing it takes effect at the next camera bind. */
    internal var videoQualityPreference: VideoQualityPreference
        get() = VideoQualityPreference.fromStored(callPrefs.getString("video_quality", null))
        set(value) { callPrefs.edit().putString("video_quality", value.name).apply() }

    internal val preferHdCapture: Boolean
        get() = if (callPrefs.contains("video_quality")) videoQualityPreference == VideoQualityPreference.PREFER_HD
            else BuildConfig.DEBUG && BuildConfig.VIDEO_HD_CAPTURE

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
    data class Adding(val from: String?, val restore: Boolean, val link: Boolean = false)
    private val _adding = MutableStateFlow<Adding?>(null)
    val adding: StateFlow<Adding?> = _adding

    /** Leaves the current account (offline until we come back) and lets onboarding add another. Blocking; throws InCall. */
    fun beginAdding(restore: Boolean, link: Boolean = false) {
        try {
            lifecycle.submit {
                val from = currentDid()
                profiles.invalidate()
                node.beginNewAccount()
                _adding.value = Adding(from, restore, link)
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
        profiles.invalidate()
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
     * "I forgot it" (forgot passphrase): restore the locked current account from its phrase. The core restores in place
     * (34f): contacts, history and chats are kept, the passphrase and device key are new. A phrase of a
     * different identity is refused before anything changes. Blocking.
     */
    fun restoreOverLocked(phrase: String, name: String, passphrase: String) {
        try { lifecycle.submit { restoreOverLockedNow(phrase, name, passphrase) }.get() }
        catch (e: java.util.concurrent.ExecutionException) { throw e.cause ?: e }
    }

    private fun restoreOverLockedNow(phrase: String, name: String, passphrase: String) {
        val oldDid = currentDid() ?: throw IllegalStateException("no account")
        if (uniffi.p2pcore.didOfPhrase(phrase) != oldDid) {
            throw IllegalArgumentException("That recovery phrase belongs to a different identity")
        }
        profiles.invalidate()
        node.beginNewAccount()
        try {
            node.restoreIdentity(phrase, name, passphrase)
            // The old remembered key opens nothing now (new data key); persistIdentity saves the new one.
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

    /** Profile reads/writes serialize with identity swaps. Called only on a worker, never the UI or a core callback. */
    fun <T> profileWork(ticket: ProfileSession.Ticket, work: (Node) -> T): T {
        try {
            return lifecycle.submit<T> {
                check(profiles.session.accepts(ticket) && node.lockState() == LockState.UNLOCKED && currentDid() == ticket.did) { "Account changed" }
                work(node)
            }.get()
        } catch (e: java.util.concurrent.ExecutionException) { throw e.cause ?: e }
    }

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
                try { Startup.trace("node.start") { node.start() } } catch (e: Exception) { Log.e(TAG, "node.start failed", e) }
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
        refreshRequests()
        refreshBlocks()
        refreshHistory()
    }

    fun refreshRequests() {
        try { _requestsIn.value = node.contactRequests(); _requestsOut.value = node.sentContactRequests() }
        catch (e: Exception) { Log.w(TAG, "requests: ${e.javaClass.simpleName}") }
    }

    /** Accept (the sheet or a Contacts card): they become a contact who may message and call. The `line` cue plays here. */
    suspend fun acceptRequest(did: String): Boolean = kotlinx.coroutines.withContext(Dispatchers.IO) {
        val ok = try { node.acceptContactRequest(did); true } catch (e: Exception) { Log.w(TAG, "acceptContactRequest: ${e.javaClass.simpleName}"); false }
        refreshRequests()
        if (ok) Cues.line()
        ok
    }

    // ---- ContactRequestEvents (core threads) ----
    private val requestEvents = object : ContactRequestEvents {
        override fun onContactRequest(request: ContactRequest) {
            testLog("request name=${request.name} did=${request.did}")
            refreshRequests()
            // Open: the knock, and the sheet opens by itself. Closed or locked away: a system notification.
            if (AppVisible.resumed) { Cues.knock(); incoming.value = IncomingSheet(request.did, request.name, joined = false, theirs = false) }
            else Notifications.request(this@P2pApp, request.did, request.name)
        }
        override fun onRequestAccepted(contact: Contact) {
            testLog("accepted name=${contact.name} did=${contact.did}")
            if (!AppVisible.resumed) Notifications.accepted(this@P2pApp, contact.did, contact.name)
            _contacts.value = node.contacts()
            refreshRequests()
            refreshBlocks()
            if (AppVisible.resumed) Cues.line()
            accepted.value = contact
            if (!addSheetOpen) incoming.value = IncomingSheet(contact.did, contact.name, joined = true, theirs = true)
        }
    }

    /** Re-reads block states for every contact and conversation, and the blocked list. Cheap core reads. */
    fun refreshBlocks() {
        try {
            val me = currentDid()
            val dids = (_contacts.value.map { it.did } + realChat.chats.value.map { it.peerDid }).filter { it != me }.toSet()
            val views = dids.associateWith { did ->
                val st = node.blockState(did)
                BlockView(st.blockedByMe, st.blockedAt?.toLong(), st.blockedMe?.signedAt?.toLong())
            }
            val list = node.blockedContacts().map { BlockedRow(it.did, it.name, it.blockedAt.toLong()) }
            val allowed = dids.filter { node.callsAllowed(it) }.toSet()
            val tokens = dids.filter { node.canCall(it) }.toSet()
            _blocks.value = Blocks.from(views, list, allowed, tokens)
        } catch (e: Exception) { Log.w(TAG, "blocks: $e") }
    }

    /** U19: the contact action. Keeps the contact and the chat; they are told. */
    fun block(did: String) = scope.launch {
        try { node.block(did) } catch (e: Exception) { Log.w(TAG, "block: $e") }
        refreshBlocks()
    }

    /** "Allow calls" (U13, P6): whether [did] may ring us. They keep messaging either way. */
    fun allowCalls(did: String, allowed: Boolean) {
        val ticket = profiles.session.ticket() ?: return
        scope.launch {
            try {
                profileWork(ticket) { it.allowCalls(did, allowed); refreshBlocks() }
            } catch (e: Exception) { Log.w(TAG, "allowCalls: $e") }
        }
    }

    fun unblock(did: String) = scope.launch {
        try { node.unblock(did) } catch (e: Exception) { Log.w(TAG, "unblock: $e") }
        refreshBlocks()
    }

    /** "blocked" while they block us and message [messageId] has not reached them. Blocking (store read). */
    fun notDeliveredReason(peerDid: String, messageId: String): String? =
        try { node.notDeliveredReason(peerDid, messageId) } catch (e: Exception) { null }
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
        /** The app once created (Cues reads the Quiet setting through it). */
        fun instanceOrNull(): P2pApp? = if (::instance.isInitialized) instance else null
    }
}

/** `pp.incoming`: whose line, and where the handshake is. `joined`: the line is open (taut), else asking (ringing).
 *  `theirs`: they accepted the request I sent (chip "Accepted your request"). */
data class IncomingSheet(val did: String, val name: String, val joined: Boolean, val theirs: Boolean)

/** Whether an activity of ours is resumed (MainActivity sets it): cues play live, else the system notifies. */
object AppVisible {
    @Volatile var resumed = false
    /** The home app bar is on screen, so a ringing call shows there (RingHead). Only the top route is composed. */
    @Volatile var ringHeadUp = false
}

fun testLog(msg: String) {
    if (BuildConfig.DEBUG) Log.i("P2PTEST", msg)
}
