package com.osvauld.p2p

import android.graphics.BitmapFactory
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject
import uniffi.p2pcore.LockState
import uniffi.p2pcore.Node
import uniffi.p2pcore.ProfileEvents
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicLong

/**
 * One person's account profile as the UI sees it (profile-sync.md: `account_profile(did)` + `profile_picture(did)`).
 * [fields] is null while no profile is accessible (not arrived yet, or none). [image] is the decoded current picture, present only
 * when [hash] is selected, ready, and its bytes decoded; a hash without an image is "awaiting" (Avatar.svelte's breathing ring).
 */
data class ProfileView(
    val fields: JSONObject? = null,
    val hash: String? = null,
    val ready: Boolean = false,
    val image: ImageBitmap? = null,
    val broken: Boolean = false,
) {
    val awaiting: Boolean get() = hash != null && !ready
    fun text(key: String): String? = fields?.opt(key) as? String
}

/** The core calls the profile screens make. Each patches one thing (profile.ts mirrors these one-to-one). */
sealed interface ProfileOp {
    data class SetName(val name: String) : ProfileOp
    /** `set_profile_field(key, json?)`: [json] is a JSON value; null removes the key. */
    data class SetField(val key: String, val json: String?) : ProfileOp
    data class SetAvailable(val available: Boolean, val untilSecs: Long?) : ProfileOp
    class ReplacePicture(val png: ByteArray) : ProfileOp
    data object RemovePicture : ProfileOp
}

/** The profile adapter the screens and every Avatar read. Production: [ProfilesController]; the debug gallery swaps in a fake. */
interface ProfileStore {
    val views: StateFlow<Map<String, ProfileView>>
    /** The selected account (null while locked / none). */
    val self: StateFlow<String?>
    /** Whether this device is connected (a save then says it syncs when connected). */
    val online: StateFlow<Boolean>
    fun watch(did: String)
    fun refresh(did: String)
    /** Runs one write on a worker. Null on success; otherwise the copy to show (what was typed stays). */
    suspend fun apply(op: ProfileOp): String?
}

object ProfileBackend {
    fun of(app: P2pApp): ProfileStore = app.profiles
}

/** UI work is bound to an account session, not just a DID (A -> B -> A is a new session). */
class ProfileSession {
    data class Ticket(val epoch: Long, val did: String)
    private var epoch = 0L
    private var did: String? = null
    @Synchronized fun select(next: String?): Boolean {
        if (did == next) return false
        epoch++; did = next; return true
    }
    @Synchronized fun invalidate() { epoch++; did = null }
    @Synchronized fun ticket(): Ticket? = did?.let { Ticket(epoch, it) }
    @Synchronized fun accepts(ticket: Ticket): Boolean = ticket.epoch == epoch && ticket.did == did
}

/**
 * Production adapter. The core callback only enqueues; Node reads, picture reads and decoding run on [P2pApp.scope] (IO) through
 * [P2pApp.profileWork], serialized with account swaps. Results are keyed by account session + owner + request and dropped when any
 * changed; a picture is re-checked against the selected hash before it is shown, so an old download never overwrites a new one.
 */
class ProfilesController(private val app: P2pApp) : ProfileEvents, ProfileStore {
    val session = ProfileSession()
    private val serial = AtomicLong()
    private val reads = ConcurrentHashMap<String, Long>()
    private val _views = MutableStateFlow<Map<String, ProfileView>>(emptyMap())
    override val views: StateFlow<Map<String, ProfileView>> = _views
    private val _self = MutableStateFlow<String?>(null)
    override val self: StateFlow<String?> = _self
    override val online: StateFlow<Boolean> by lazy {
        app.status.map { it?.online == true }.stateIn(app.scope, SharingStarted.Eagerly, app.status.value?.online == true)
    }

    fun invalidate() { session.invalidate(); _self.value = null; reads.clear(); _views.value = emptyMap() }
    fun select(did: String?) {
        if (session.select(did)) { _self.value = did; reads.clear(); _views.value = emptyMap(); did?.let { refresh(it) } }
    }
    override fun watch(did: String) { if (!_views.value.containsKey(did) && !reads.containsKey(did)) refresh(did) }

    override fun onProfileChanged(did: String) {
        // A core callback thread: enqueue only. Re-read, never infer from event order.
        refresh(did)
        val ticket = session.ticket() ?: return
        app.scope.launch {
            runCatching { app.profileWork(ticket) {
                if (did == ticket.did) app.refreshAvailability() else app.refreshContacts()
            } }
        }
    }

    override fun refresh(did: String) {
        val ticket = session.ticket() ?: return
        val request = serial.incrementAndGet()
        reads[did] = request
        app.scope.launch {
          try {
            val metaResult = runCatching { app.profileWork(ticket) { it.accountProfile(did) } }
            if (metaResult.isFailure) return@launch
            val meta = metaResult.getOrNull()
            val fields = meta?.let { runCatching { JSONObject(it.fieldsJson) }.getOrElse { return@launch } }
            val prior = _views.value[did]
            // Keep the decoded image only while the same hash stays selected (no flash back to initials on a field edit).
            val keep = prior?.image?.takeIf { prior.hash == meta?.pictureHash && meta?.pictureReady == true }
            publish(ticket, did, request, ProfileView(fields, meta?.pictureHash, meta?.pictureReady == true, keep))
            val hash = meta?.pictureHash
            if (meta?.pictureReady == true && hash != null && keep == null) {
                val image = runCatching {
                    app.profileWork(ticket) { it.profilePicture(did) }?.let { b -> BitmapFactory.decodeByteArray(b, 0, b.size)?.asImageBitmap() }
                }.getOrNull()
                val againResult = runCatching { app.profileWork(ticket) { it.accountProfile(did) } }
                if (againResult.isFailure) return@launch
                val again = againResult.getOrNull()
                if (again?.pictureHash != hash || again.pictureReady != true) { if (session.accepts(ticket)) refresh(did); return@launch }
                publish(ticket, did, request, ProfileView(fields, hash, true, image, broken = image == null))
            }
          } finally {
            reads.remove(did, request)
          }
        }
    }

    private suspend fun publish(t: ProfileSession.Ticket, did: String, request: Long, view: ProfileView) = withContext(Dispatchers.Main.immediate) {
        if (session.accepts(t) && reads[did] == request) _views.value = _views.value + (did to view)
    }

    override suspend fun apply(op: ProfileOp): String? {
        if (op is ProfileOp.SetAvailable) ProfilePolicy.availabilityProblem(op.available)?.let { return it }
        val ticket = session.ticket() ?: return SAVE_FAILED
        val result = withContext(Dispatchers.IO) {
            runCatching {
                app.profileWork(ticket) { node: Node ->
                    when (op) {
                        is ProfileOp.SetName -> node.setName(op.name)
                        is ProfileOp.SetField -> node.setProfileField(op.key, op.json)
                        is ProfileOp.SetAvailable -> node.setAvailable(op.available, op.untilSecs?.toULong())
                        is ProfileOp.ReplacePicture -> node.replaceProfilePicture(op.png)
                        ProfileOp.RemovePicture -> node.removeProfilePicture()
                    }
                }
            }
        }
        if (!session.accepts(ticket)) return ACCOUNT_CHANGED
        return result.fold(onSuccess = {
            refresh(ticket.did)
            if (op is ProfileOp.SetAvailable || op is ProfileOp.SetName) app.scope.launch {
                runCatching { app.profileWork(ticket) { app.refreshAvailability() } }
            }
            null
        }, onFailure = {
            when (op) {
                is ProfileOp.ReplacePicture -> appString(R.string.profile_err_picture_save)
                ProfileOp.RemovePicture -> appString(R.string.profile_err_picture_remove)
                else -> SAVE_FAILED
            }
        })
    }

    companion object {
        /** profile.ts SAVE_FAILED. */
        val SAVE_FAILED: String get() = appString(R.string.profile_err_save_failed)
        /** Native-only state (the playground has one account in front at a time): a write that raced an account switch. */
        val ACCOUNT_CHANGED: String get() = appString(R.string.profile_err_account_changed)
    }
}
