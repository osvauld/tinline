package com.osvauld.p2p

import kotlinx.coroutines.flow.StateFlow

/*
 * PLACEHOLDER API for groups, chat requests, blocking, per-contact permissions and address changes.
 *
 * None of this exists in the core yet. These interfaces are shaped like the API the core is expected
 * to grow (one flow per kind of state, one call per user action), so the screens in GroupsScreens.kt
 * and PrivacyScreens.kt can be reviewed on a phone now against [FakeGroupsPrivacy] and later be pointed
 * at a core-backed implementation without changing. Nothing here is wired to the Node, and the whole
 * area is gated by [PreviewFlags] (debug builds only).
 *
 * Times are epoch milliseconds. "did" is the contact's stable DID; group members are identified by DID
 * too. All calls are cheap and synchronous in the fake; a real implementation would do IO off the main
 * thread and update the flows when the core answers.
 */

/** What an action did: done, or refused with a sentence the UI shows as-is. */
sealed interface Outcome {
    data object Ok : Outcome
    data class Refused(val reason: String) : Outcome
}

// ------------------------------------------------------------------ groups

/** Role pills. A member can hold several (Owner + Host). Host means one of the member's desktops hosts. */
enum class GroupRole { Owner, Admin, Host }

/** One admin's signed flag against a member. The same admin flagging again counts again. */
data class GroupFlag(val byDid: String, val byName: String, val byRole: GroupRole, val atMs: Long)

data class GroupMember(
    val did: String,
    val name: String,
    val roles: Set<GroupRole> = emptySet(),
    val memberSinceMs: Long = 0,
    val invitedBy: String? = null,
    val flags: List<GroupFlag> = emptyList(),
    val removed: Boolean = false,
    val isMe: Boolean = false,
) {
    val isAdmin get() = GroupRole.Owner in roles || GroupRole.Admin in roles
}

/** A member's desktop that stores the group and relays messages. Phones never host. */
data class GroupHost(
    val id: String,
    /** "Arjun’s desktop" */
    val label: String,
    val ownerDid: String,
    val online: Boolean,
    /** Name of who appointed it; null for the owner's own host. */
    val appointedBy: String?,
    /** When it went offline (for "Offline for 2 h"); null while online. */
    val offlineSinceMs: Long? = null,
    /** Asked to host, not accepted yet: not counted as a host for delivery. */
    val pending: Boolean = false,
)

enum class GroupDelivery {
    /** Accepted by a host. */
    Sent,
    /** No host reachable: saved, sent when one comes back. */
    WaitingForHost,
    /** The removed person's view: written offline before the removal, still delivered until the hosts' addresses refresh. */
    WrittenBeforeRemoval,
}

sealed interface GroupItem {
    val id: String
    val atMs: Long
    data class Msg(
        override val id: String, override val atMs: Long,
        val authorDid: String, val authorName: String, val text: String, val outgoing: Boolean,
        val delivery: GroupDelivery = GroupDelivery.Sent,
        /** The group's view: a late message from someone removed after they wrote it. */
        val writtenBeforeAuthorRemoved: Boolean = false,
    ) : GroupItem
    /** Small grey line: joins, removals, host changes. Plain words; never addresses or keys. */
    data class Sys(override val id: String, override val atMs: Long, val text: String) : GroupItem
}

/** Set when I was removed from the group. */
data class Removal(
    val atMs: Long,
    val flags: List<GroupFlag>,
    /** When the hosts refresh their addresses, after which my phone can't reach them. */
    val hostsRefreshAtMs: Long,
    /** "Keep as archive" was chosen: the delete/keep choice is not offered again. */
    val archived: Boolean = false,
)

data class Group(
    val id: String,
    val name: String,
    val members: List<GroupMember>,
    val hosts: List<GroupHost>,
    val items: List<GroupItem>,
    val unread: Int = 0,
    val muted: Boolean = false,
    /** Non-null when I was removed: read-only, greyed in the list. */
    val removal: Removal? = null,
    /** Picks the avatar tint. */
    val tint: Int = 0,
) {
    val me get() = members.firstOrNull { it.isMe }
    val activeMembers get() = members.filter { !it.removed }
    val iAmAdmin get() = me?.isAdmin == true && removal == null
    val activeHosts get() = hosts.filter { !it.pending }
    /** False means "Waiting for a host": messages queue. */
    val hostReachable get() = activeHosts.any { it.online }
    val lastActivityMs get() = items.maxOfOrNull { it.atMs } ?: 0
}

enum class InviteState { Pending, Joined, Declined, Expired }

/** A group invite, shown as a card in the inviter's 1:1 chat. */
data class GroupInvite(
    val id: String,
    val groupName: String,
    val inviterDid: String,
    val inviterName: String,
    val ownerName: String,
    val hostLabels: List<String>,
    val memberCount: Int,
    val expiresAtMs: Long,
    val state: InviteState = InviteState.Pending,
    /** Set once joined. */
    val groupId: String? = null,
)

interface GroupsSource {
    val groups: StateFlow<List<Group>>
    val invites: StateFlow<List<GroupInvite>>
    /** I become the owner. With [useMyDesktop] my linked desktop is the first host; otherwise the group waits for one. */
    fun createGroup(name: String, memberDids: List<String>, useMyDesktop: Boolean): String
    fun sendGroupText(groupId: String, text: String): Outcome
    /** Admins only. At 3 flags the member is removed at once. */
    fun flagMember(groupId: String, memberDid: String): Outcome
    fun makeAdmin(groupId: String, memberDid: String): Outcome
    /** Asks the member to host from one of their desktops; it shows as pending until they accept. */
    fun askToHost(groupId: String, memberDid: String): Outcome
    /** The group moves to new addresses so the removed host is cut off. */
    fun removeHost(groupId: String, hostId: String): Outcome
    fun setMuted(groupId: String, muted: Boolean)
    fun leaveGroup(groupId: String)
    /** Deletes a group I was removed from (or left) from this phone. */
    fun deleteGroup(groupId: String)
    fun keepAsArchive(groupId: String)
    fun markGroupRead(groupId: String)
    fun joinInvite(inviteId: String): Outcome
    fun declineInvite(inviteId: String)
}

// ------------------------------------------------------------------ people (1:1)

enum class ContactOrigin { Qr, GroupRequest }

enum class DmTick { Sent, Delivered, Read, WaitingToReach, NotDeliveredBlocked }

sealed interface DmItem {
    val id: String
    val atMs: Long
    data class Text(override val id: String, override val atMs: Long, val outgoing: Boolean, val text: String, val tick: DmTick = DmTick.Delivered) : DmItem
    data class Sys(override val id: String, override val atMs: Long, val text: String) : DmItem
    /** A group invite card (see [GroupInvite]). */
    data class Invite(override val id: String, override val atMs: Long, val inviteId: String) : DmItem
    /** "<name> would like to be able to call you", with Allow for the receiver. */
    data class CallAsk(override val id: String, override val atMs: Long, val outgoing: Boolean, val handled: Boolean = false) : DmItem
}

/** My block on someone (contact or not). */
enum class BlockVia { Contact, ChatRequest }
data class BlockedPerson(val did: String, val name: String, val atMs: Long, val via: BlockVia)

/** Their signed notice that they blocked me. */
data class BlockNotice(val atMs: Long)

data class Person(
    val did: String,
    val name: String,
    val origin: ContactOrigin = ContactOrigin.Qr,
    /** For [ContactOrigin.GroupRequest]: the group the request came through. */
    val originGroup: String? = null,
    val verifiedAtMs: Long? = null,
    /** My switches: what this person may do to me. */
    val canMessage: Boolean = true,
    val canCall: Boolean = true,
    /** Their switch for me: whether I may ring them. */
    val theyAllowMyCalls: Boolean = true,
    val blockedMe: BlockNotice? = null,
    /** Both sides changed address while apart and no shared group host could pass it on. */
    val lost: Boolean = false,
    /** While I block them: how many times they tried to message me since (never shown as messages). */
    val attemptsSinceBlock: Int = 0,
    /** Whether they have my current address. */
    val hasMyCurrentAddress: Boolean = true,
    val theirAddressChangedAtMs: Long? = null,
    /** The one-time "Allow calls" chip after accepting a request was dismissed or used. */
    val callsChipDone: Boolean = true,
    val items: List<DmItem> = emptyList(),
    val unread: Int = 0,
) {
    val lastActivityMs get() = items.maxOfOrNull { it.atMs } ?: 0
}

/** Why a call button does or doesn't dial. */
enum class CallCheck { Allowed, TheyHaveNotAllowed, IBlocked, BlockedMe, Lost }

interface PeopleSource {
    val people: StateFlow<List<Person>>
    fun sendText(did: String, text: String): Outcome
    fun setCanMessage(did: String, on: Boolean)
    fun setCanCall(did: String, on: Boolean)
    fun dismissCallsChip(did: String)
    /** Sends a normal message asking them to turn on "Call you" for me. */
    fun askToAllowCalls(did: String): Outcome
    /** Receiver side of [askToAllowCalls]: Allow turns my "Call you" switch on for them. */
    fun answerCallAsk(did: String, itemId: String, allow: Boolean)
    fun callCheck(did: String): CallCheck
    /** After re-scanning a lost contact's QR: same contact, history and verification kept. */
    fun reconnect(did: String)
    fun markPersonRead(did: String)
}

// ------------------------------------------------------------------ chat requests

enum class SentRequestState { Waiting, Declined, Expired, Accepted }
enum class IncomingRequestState { Pending, Accepted, Declined, Blocked }

data class SentRequest(
    val id: String, val toDid: String, val toName: String, val viaGroup: String, val note: String,
    val sentAtMs: Long, val state: SentRequestState = SentRequestState.Waiting, val endedAtMs: Long? = null,
)

data class IncomingRequest(
    val id: String, val fromDid: String, val fromName: String, val viaGroup: String, val sharedGroups: Int,
    val note: String, val atMs: Long, val state: IncomingRequestState = IncomingRequestState.Pending,
)

data class Requests(val sent: List<SentRequest> = emptyList(), val incoming: List<IncomingRequest> = emptyList())

interface ChatRequestsSource {
    val requests: StateFlow<Requests>
    /** One request per person per day; refused if they are already a contact (open the chat instead) or blocked me. */
    fun sendRequest(toDid: String, toName: String, viaGroup: String, note: String): Outcome
    fun cancelRequest(id: String)
    /** Removes a declined / expired row from the list. */
    fun dismissRequest(id: String)
    /** Adds them as a contact for messages only (calls off both ways) and opens that chat; returns their did. */
    fun acceptRequest(id: String): String?
    fun declineRequest(id: String)
    /** Third choice: they are told they're blocked and can't send more requests. */
    fun blockRequester(id: String)
    /** Ends waiting requests older than 7 days. The fake calls it on every read of time; a core would on a timer. */
    fun expireDue()
}

// ------------------------------------------------------------------ blocking

interface BlockingSource {
    /** One list across all my devices. */
    val blocked: StateFlow<List<BlockedPerson>>
    /** Personal: never changes group membership or device state. They are told, and skip my next address change. */
    fun block(did: String, name: String, via: BlockVia = BlockVia.Contact): Outcome
    /** They can message me again and get my current address; calls only if they were allowed before. */
    fun unblock(did: String)
    fun isBlocked(did: String): Boolean
}

// ------------------------------------------------------------------ address changes

enum class AddressInterval(val label: String, val days: Int) { Day("Day", 1), ThreeDays("3 days", 3), Week("Week", 7) }

data class AddressChanges(
    val interval: AddressInterval,
    val lastChangeMs: Long,
    val nextChangeMs: Long,
    /** Contacts that have my current address, of those that should (blocked people are excluded). */
    val contactsTold: Int,
    val contactsTotal: Int,
)

interface AddressChangeSource {
    val address: StateFlow<AddressChanges>
    fun setInterval(interval: AddressInterval)
}

/** Everything the preview screens use, in one object. */
interface GroupsPrivacySource : GroupsSource, PeopleSource, ChatRequestsSource, BlockingSource, AddressChangeSource {
    val myDid: String
    val myName: String
}
