package com.osvauld.p2p

import android.util.Log
import androidx.annotation.StringRes
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import uniffi.p2pcore.ChatRequestState
import uniffi.p2pcore.GroupChatRequest
import uniffi.p2pcore.GroupDetail
import uniffi.p2pcore.GroupRequestEvents
import uniffi.p2pcore.GroupEvents
import uniffi.p2pcore.GroupInvitation
import uniffi.p2pcore.GroupInvitationEvents
import uniffi.p2pcore.GroupInvitationState
import uniffi.p2pcore.GroupInvitePeek
import uniffi.p2pcore.GroupItemKind
import uniffi.p2pcore.GroupMessage
import uniffi.p2pcore.GroupPage
import uniffi.p2pcore.GroupReceiptStatus
import uniffi.p2pcore.GroupSummary
import uniffi.p2pcore.Node
import uniffi.p2pcore.OnlineMember
import uniffi.p2pcore.GroupDelivery as CoreDelivery
import uniffi.p2pcore.GroupMember as CoreMember
import uniffi.p2pcore.GroupRole as CoreRole

/*
 * Group chat on the core's real API (G1: crates/core/src/group/api.rs, docs/group-core.md).
 * [GroupsController] is what the screens in GroupsLive.kt read; every core call goes through [GroupApi]
 * so the flows are tested on the JVM with a stand-in (GroupsCoreTest).
 *
 * Decisions (rotation-open-decisions.md §0): 3 admin flags remove a member (U9/U15) and the removed member
 * sees the signed flags; a post written before a removal still shows, marked (U11); only hosts know member
 * endpoints (U7), so our posts honestly say "Waiting for a host" until a host acknowledges them.
 * G3: admins invite contacts directly (the invitation travels over the 1:1 channel; blocked contacts are
 * never offered); invite links are short (tinline://g/…) and the core fetches the invite from a host.
 * There is no "add my other device" step: own-device sync (G2) brings groups to linked devices.
 */

/** The core's group calls. Blocking ones (create, invite, join, accept host) are for IO threads. */
interface GroupApi {
    fun groups(): List<GroupSummary>
    fun detail(id: String): GroupDetail
    fun messages(id: String, before: String?, limit: Int): GroupPage
    fun markRead(id: String)
    /** Signed member receipts of my posts with `at` in [fromMs, toMs] (the group tick and n/N). */
    fun receipts(id: String, fromMs: ULong, toMs: ULong): List<GroupReceiptStatus> = emptyList()
    fun create(name: String): String
    fun invite(id: String): String
    /** Fetches from the host the link names: network, IO threads only. */
    fun peek(token: String): GroupInvitePeek?
    fun join(token: String): String
    fun post(id: String, text: String, replyTo: String?): GroupMessage
    fun flag(id: String, did: String)
    fun setAdmin(id: String, did: String, admin: Boolean)
    fun leave(id: String)
    fun appointHost(id: String, did: String, deviceId: String)
    fun revokeHost(id: String, did: String, deviceId: String)
    fun acceptHost(id: String, accept: Boolean)
    fun deviceId(): String
    // Chat requests through the group's hosts (G2, U3).
    fun chatRequest(id: String, did: String): GroupChatRequest
    fun requestsIncoming(): List<GroupChatRequest>
    fun requestsOutgoing(): List<GroupChatRequest>
    fun chatAccept(requestId: String): GroupChatRequest
    fun chatDecline(requestId: String): GroupChatRequest
    /** The personal block (P5), for "Block" on an incoming request. */
    fun block(did: String)
    // Direct invitations to contacts (G3).
    fun inviteContacts(id: String, dids: List<String>): List<GroupInvitation>
    fun invitationsIncoming(): List<GroupInvitation>
    fun invitationsOutgoing(id: String): List<GroupInvitation>
    fun invitationAccept(invitationId: String): GroupInvitation
    fun invitationDecline(invitationId: String): GroupInvitation
    // Call hosts (a separate permit from chat hosting; the device accepts it itself). Defaults keep JVM stand-ins compiling.
    fun appointCallHost(id: String, did: String, deviceId: String): Unit = throw UnsupportedOperationException()
    fun revokeCallHost(id: String, did: String, deviceId: String): Unit = throw UnsupportedOperationException()
    fun acceptCallHost(id: String, accept: Boolean): Unit = throw UnsupportedOperationException()
    // Public groups (docs/design/public-group.md §2–§3). Defaults keep JVM stand-ins compiling.
    /** Network (registers the code at the hosts): IO threads only. Opens the group if it is closed. */
    fun openLink(id: String): String = throw UnsupportedOperationException()
    fun setOpen(id: String, open: Boolean): Unit = throw UnsupportedOperationException()
    fun setAdminsOnly(id: String, adminsOnly: Boolean): Unit = throw UnsupportedOperationException()
    /** In memory in the core: empty for a private group. */
    fun online(id: String): List<OnlineMember> = emptyList()
    fun presenceVisible(): Boolean = true
    // Image posts (public group §5). Defaults keep JVM stand-ins compiling.
    /** Blocking (the core scales and re-encodes the picture): IO threads only. */
    fun postImage(id: String, bytes: ByteArray, caption: String): GroupMessage = throw UnsupportedOperationException()
    fun fetchImage(id: String, messageId: String) {}
    /** Blocking the first time (the core opens the picture into a file): IO threads only. */
    fun imagePath(id: String, messageId: String): String? = null
    // Voice posts: the 1:1 recording (Ogg Opus), sealed and fetched as a picture is.
    /** Blocking (the core checks and seals the file): IO threads only. */
    fun postVoice(id: String, path: String, waveform: ByteArray): GroupMessage = throw UnsupportedOperationException()
    fun fetchVoice(id: String, messageId: String) {}
    /** Blocking the first time (the core opens the recording into a file): IO threads only. */
    fun voicePath(id: String, messageId: String): String? = null
    // File posts (docs/design/group-files.md): any file up to 2 GB, streamed from disk by the core.
    /** Blocking (the core seals the whole file): IO threads only. */
    fun postFile(id: String, path: String, name: String, mime: String, caption: String): GroupMessage = throw UnsupportedOperationException()
    fun fetchFile(id: String, messageId: String) {}
    fun cancelFile(id: String, messageId: String) {}
    /** Blocking the first time (the core decrypts the file once): IO threads only. */
    fun filePath(id: String, messageId: String): String? = null
    fun deleteFileCopy(id: String, messageId: String) {}
}

class NodeGroupApi(private val node: () -> Node) : GroupApi {
    override fun groups() = node().groups()
    override fun detail(id: String) = node().groupDetail(id)
    override fun messages(id: String, before: String?, limit: Int) = node().groupMessages(id, before, limit.toUInt())
    override fun markRead(id: String) = node().groupMarkRead(id)
    override fun receipts(id: String, fromMs: ULong, toMs: ULong) = node().groupReceipts(id, fromMs, toMs)
    override fun create(name: String) = node().groupCreate(name)
    override fun invite(id: String) = node().groupInvite(id)
    override fun peek(token: String) = node().groupPeekInvite(token)
    override fun join(token: String) = node().groupJoin(token)
    override fun post(id: String, text: String, replyTo: String?) = node().groupPost(id, text, replyTo)
    override fun flag(id: String, did: String) = node().groupFlag(id, did)
    override fun setAdmin(id: String, did: String, admin: Boolean) = node().groupSetAdmin(id, did, admin)
    override fun leave(id: String) = node().groupLeave(id)
    override fun appointHost(id: String, did: String, deviceId: String) = node().groupAppointHost(id, did, deviceId)
    override fun revokeHost(id: String, did: String, deviceId: String) = node().groupRevokeHost(id, did, deviceId)
    override fun acceptHost(id: String, accept: Boolean) = node().groupAcceptHost(id, accept)
    override fun deviceId() = node().groupDeviceId()
    override fun chatRequest(id: String, did: String) = node().groupChatRequest(id, did)
    override fun requestsIncoming() = node().groupChatRequestsIncoming()
    override fun requestsOutgoing() = node().groupChatRequestsOutgoing()
    override fun chatAccept(requestId: String) = node().groupChatAccept(requestId)
    override fun chatDecline(requestId: String) = node().groupChatDecline(requestId)
    override fun block(did: String) = node().block(did)
    override fun inviteContacts(id: String, dids: List<String>) = node().groupInviteContacts(id, dids)
    override fun invitationsIncoming() = node().groupInvitationsIncoming()
    override fun invitationsOutgoing(id: String) = node().groupInvitationsOutgoing(id)
    override fun invitationAccept(invitationId: String) = node().groupInvitationAccept(invitationId)
    override fun invitationDecline(invitationId: String) = node().groupInvitationDecline(invitationId)
    override fun appointCallHost(id: String, did: String, deviceId: String) = node().groupAppointCallHost(id, did, deviceId)
    override fun revokeCallHost(id: String, did: String, deviceId: String) = node().groupRevokeCallHost(id, did, deviceId)
    override fun acceptCallHost(id: String, accept: Boolean) = node().groupAcceptCallHost(id, accept)
    override fun openLink(id: String) = node().groupOpenLink(id)
    override fun setOpen(id: String, open: Boolean) = node().groupSetOpen(id, open)
    override fun setAdminsOnly(id: String, adminsOnly: Boolean) = node().groupSetAdminsOnly(id, adminsOnly)
    override fun online(id: String) = node().groupOnline(id)
    override fun presenceVisible() = node().groupPresenceVisible()
    override fun postImage(id: String, bytes: ByteArray, caption: String) = node().groupPostImageBytes(id, bytes, caption)
    override fun fetchImage(id: String, messageId: String) = node().groupFetchImage(id, messageId)
    override fun imagePath(id: String, messageId: String) = node().groupImagePath(id, messageId)
    override fun postVoice(id: String, path: String, waveform: ByteArray) = node().groupPostVoice(id, path, waveform)
    override fun fetchVoice(id: String, messageId: String) = node().groupFetchVoice(id, messageId)
    override fun voicePath(id: String, messageId: String) = node().groupVoicePath(id, messageId)
    override fun postFile(id: String, path: String, name: String, mime: String, caption: String) = node().groupPostFile(id, path, name, mime, caption.ifEmpty { null })
    override fun fetchFile(id: String, messageId: String) = node().groupFetchFile(id, messageId)
    override fun cancelFile(id: String, messageId: String) = node().groupCancelFile(id, messageId)
    override fun filePath(id: String, messageId: String) = node().groupFilePath(id, messageId)
    override fun deleteFileCopy(id: String, messageId: String) = node().groupDeleteFileCopy(id, messageId)
}

/** A contact as the "Add members" picker sees it. */
data class InviteCandidate(val did: String, val name: String)

/** One row of the picker: who, and why they can't be picked (null: pickable). */
data class PickRow(val did: String, val name: String, val why: String?, val invited: GroupInvitation?)

/** The group on screen: its detail and the loaded window of messages (oldest first). */
data class OpenGroup(val id: String, val detail: GroupDetail, val messages: List<GroupMessage>, val older: String?,
                     /** Signed member receipts of my loaded posts, by message id. */
                     val receipts: Map<String, GroupReceiptStatus> = emptyMap(),
                     /** Who is online in an open (public) group; empty for a private one (public group §3). */
                     val online: List<OnlineMember> = emptyList())

/** What a member's sheet offers us, in order. */
sealed interface MemberAction {
    /** Message privately: their chat if a contact, else a chat request (U3). */
    data object Message : MemberAction
    data object MakeAdmin : MemberAction
    data object RevokeAdmin : MemberAction
    data class Hosting(val deviceId: String) : MemberAction
    data object Flag : MemberAction
}

object GroupText {
    const val FLAGS_TO_REMOVE = 3
    const val PAGE = 100
    /** A host's presence frame lists at most this many DIDs (public group §3): at or above it the count may be cut short. */
    const val ONLINE_CAP = 500

    fun active(r: CoreRole) = r == CoreRole.OWNER || r == CoreRole.ADMIN || r == CoreRole.MEMBER
    fun isAdmin(r: CoreRole) = r == CoreRole.OWNER || r == CoreRole.ADMIN

    /** The name if there is one; blank means "someone" and the caller picks a whole-sentence variant for it. */
    private fun named(name: String): String? = name.trim().ifBlank { null }
    fun shortDev(dev: String) = dev.take(8)

    /** [named] or the unnamed text: whole-sentence variants, never a translated "someone" slotted into a name. */
    internal fun withName(name: String, @StringRes named: Int, @StringRes anon: Int, vararg more: Any): String =
        named(name)?.let { appString(named, it, *more) } ?: appString(anon, *more)

    /** A person in a system line: me, a known name, or nobody we know. */
    private sealed interface Party { data object Me : Party; data class Named(val name: String) : Party; data object Anon : Party }

    private fun party(did: String, name: String, me: String): Party = if (did == me) Party.Me else named(name)?.let { Party.Named(it) } ?: Party.Anon

    /** A system line in plain words. [me] is our DID. */
    fun sysText(m: GroupMessage, me: String): String {
        val by = party(m.authorDid, m.authorName, me)
        val sub = party(m.subjectDid, m.subjectName, me)
        fun pick(me: Int, named: Int, anon: Int, vararg more: Any) = when (sub) {
            Party.Me -> appString(me, *more)
            is Party.Named -> appString(named, sub.name, *more)
            Party.Anon -> appString(anon, *more)
        }
        /** Admin granted/revoked: the actor times the subject, one whole sentence each. */
        fun adminLine(youByNamed: Int, youByAnon: Int, byMe: Int, byMeAnon: Int, other: Int, otherAnon: Int, anonSub: Int, anonAnon: Int): String = when (sub) {
            Party.Me -> if (by is Party.Named) appString(youByNamed, by.name) else appString(youByAnon)
            else -> when (by) {
                Party.Me -> if (sub is Party.Named) appString(byMe, sub.name) else appString(byMeAnon)
                is Party.Named -> if (sub is Party.Named) appString(other, by.name, sub.name) else appString(otherAnon, by.name)
                Party.Anon -> if (sub is Party.Named) appString(anonSub, sub.name) else appString(anonAnon)
            }
        }
        return when (m.kind) {
            GroupItemKind.POST -> m.text
            GroupItemKind.JOINED -> pick(R.string.groupsc_sys_joined_you, R.string.groupsc_sys_joined_other, R.string.groupsc_sys_joined_someone)
            GroupItemKind.LEFT -> pick(R.string.groupsc_sys_left_you, R.string.groupsc_sys_left_other, R.string.groupsc_sys_left_someone)
            GroupItemKind.REMOVED -> when (sub) {
                Party.Me -> appPlural(R.plurals.groupsc_removed_you_flags, FLAGS_TO_REMOVE, FLAGS_TO_REMOVE)
                is Party.Named -> appPlural(R.plurals.groupsc_removed_other_flags, FLAGS_TO_REMOVE, sub.name, FLAGS_TO_REMOVE)
                Party.Anon -> appPlural(R.plurals.groupsc_removed_someone_flags, FLAGS_TO_REMOVE, FLAGS_TO_REMOVE)
            }
            GroupItemKind.ADMIN_GRANTED -> adminLine(R.string.groupsc_admin_granted_you, R.string.groupsc_admin_granted_you_by_someone,
                R.string.groupsc_admin_granted_by_you, R.string.groupsc_admin_granted_by_you_someone, R.string.groupsc_admin_granted_other, R.string.groupsc_admin_granted_other_someone,
                R.string.groupsc_admin_granted_by_someone, R.string.groupsc_admin_granted_by_someone_someone)
            GroupItemKind.ADMIN_REVOKED -> adminLine(R.string.groupsc_admin_revoked_you, R.string.groupsc_admin_revoked_you_by_someone,
                R.string.groupsc_admin_revoked_by_you, R.string.groupsc_admin_revoked_by_you_someone, R.string.groupsc_admin_revoked_other, R.string.groupsc_admin_revoked_other_someone,
                R.string.groupsc_admin_revoked_by_someone, R.string.groupsc_admin_revoked_by_someone_someone)
            GroupItemKind.HOST_ADDED -> pick(R.string.groupsc_host_added_you, R.string.groupsc_host_added_other, R.string.groupsc_host_added_someone)
            GroupItemKind.HOST_REMOVED -> pick(R.string.groupsc_host_removed_you, R.string.groupsc_host_removed_other, R.string.groupsc_host_removed_someone)
        }
    }

    /** The list row's second line. */
    fun preview(s: GroupSummary, me: String): String = when (s.myRole) {
        CoreRole.REMOVED -> appString(R.string.groupsc_removed_you_group)
        CoreRole.LEFT -> appString(R.string.groupsc_left_you)
        else -> when (val m = s.last) {
            null -> ""
            else -> when {
                m.kind != GroupItemKind.POST -> sysText(m, me)
                m.outgoing -> appString(R.string.groupsc_preview_you, postLine(m))
                else -> withName(firstName(m.authorName), R.string.groupsc_preview_other, R.string.groupsc_preview_someone, postLine(m))
            }
        }
    }

    /** A post in one line: its text, "Voice message", "Photo" for a picture alone, "Photo · caption" with one, "File: name". Same words as desktop. */
    fun postLine(m: GroupMessage): String = when {
        m.voice != null -> appString(R.string.chat_preview_voice)
        m.file != null -> appString(R.string.groupsc_file_line, m.file!!.name)
        m.image == null -> m.text
        m.text.isEmpty() -> appString(R.string.groups_photo_desc)
        else -> appString(R.string.groupsc_photo_caption_line, m.text)
    }

    /**
     * Under a file card: who else has the file ("Anoop is also hosting this file", "Anoop and 2 others have this file"), from
     * the core's holders (the author first). I am never named; nobody else: null. Same words as desktop.
     */
    fun holdersLine(f: uniffi.p2pcore.GroupFile, me: String, name: (uniffi.p2pcore.GroupFileHolder) -> String): String? {
        val others = f.holders.filter { it.did != me }
        val first = others.firstOrNull() ?: return null
        val who = firstName(name(first)).ifBlank { appString(R.string.common_someone) }
        return if (others.size == 1) appString(R.string.groupsc_file_also_hosting, who)
        else appPlural(R.plurals.groupsc_file_holders, others.size - 1, who, others.size - 1)
    }

    /** Largest picture file the composer reads in (the core refuses anything larger). */
    const val MAX_PICTURE_BYTES = 64 * 1024 * 1024

    /** The header's sub-line. */
    fun status(d: GroupDetail): String {
        when (d.myRole) {
            CoreRole.REMOVED -> return appString(R.string.groupsc_removed_you_group)
            CoreRole.LEFT -> return appString(R.string.groupsc_left_you)
            else -> {}
        }
        val members = appPlural(R.plurals.groupsc_members, d.members.size, d.members.size)
        val state = when {
            d.amHost -> appString(R.string.groupsc_status_this_device)
            d.hosts.isEmpty() -> appString(R.string.groupsc_status_no_host)
            d.hosts.none { it.online } -> appString(R.string.groupsc_status_waiting)
            else -> appPlural(R.plurals.groupsc_hosts, d.hosts.size, d.hosts.size)
        }
        return appString(R.string.groupsc_status_join, members, state)
    }

    fun waitingForHost(d: GroupDetail) = active(d.myRole) && !d.amHost && d.hosts.none { it.online }

    // ---- public groups (docs/design/public-group.md §8; same words as desktop)

    /** "12 online"; at the presence cap the count may be cut short: "500+ online". */
    fun onlineLabel(n: Int) = if (n >= ONLINE_CAP) appString(R.string.groupsc_online_cap, ONLINE_CAP) else appPlural(R.plurals.groupsc_online_count, n, n)

    /** An open group we are still in shows "N online" in its top bar. */
    fun showsOnline(d: GroupDetail) = d.open && active(d.myRole)

    /** The online list's order: admins first, then by name (case-insensitive), then DID. */
    fun onlineRows(list: List<OnlineMember>): List<OnlineMember> =
        list.sortedWith(compareBy<OnlineMember>({ !it.isAdmin }, { it.name.lowercase() }, { it.did }))

    /** An admins-only group (a channel) and we are a member who may not post: no composer, a calm line. */
    fun adminsOnlyLocked(d: GroupDetail) = d.adminsOnly && d.myRole == CoreRole.MEMBER

    val ADMINS_ONLY: String get() = appString(R.string.groupsc_admins_only_post)
    val ADMINS_ONLY_ERROR: String get() = appString(R.string.groupsc_admins_only_error)
    val OPEN_LINK_COPIED: String get() = appString(R.string.groupsc_open_link_copied)

    /** The small line under one of our posts, or null when delivered. A refused post says why ([isFailed]); it never reads as delivered. */
    fun deliveryWord(m: GroupMessage): String? = when {
        !m.outgoing || m.kind != GroupItemKind.POST -> null
        m.delivery == CoreDelivery.FAILED -> m.failure ?: appString(R.string.groupsc_not_posted)
        m.delivery == CoreDelivery.WAITING -> appString(R.string.groups_waiting_host)
        else -> null
    }

    /** The hosts refuse this post of ours and the core stopped sending it: its line is the error one, in the waiting slot. */
    fun isFailed(m: GroupMessage) = m.outgoing && m.kind == GroupItemKind.POST && m.delivery == CoreDelivery.FAILED

    /** A sent request's state line and whether it is bad news. A block is visible (U5 revised, P5). */
    fun sentLine(r: GroupChatRequest): Pair<String, Boolean> {
        val f = firstName(r.peerName)
        return when (r.state) {
            ChatRequestState.PENDING -> withName(f, R.string.groupsc_req_pending_named, R.string.groupsc_req_pending_anon, r.groupName) to false
            ChatRequestState.ACCEPTED -> (if (r.contactAdded) withName(f, R.string.groupsc_req_accepted_messages, R.string.groupsc_req_accepted_messages_anon)
                else withName(f, R.string.groupsc_req_accepted_connecting, R.string.groupsc_req_accepted_connecting_anon)) to false
            ChatRequestState.DECLINED -> withName(f, R.string.groupsc_req_declined_named, R.string.groupsc_req_declined_anon) to true
            ChatRequestState.BLOCKED -> withName(f, R.string.groupsc_req_blocked_named, R.string.groupsc_req_blocked_anon) to true
        }
    }

    /** Who asked this device to host [d], or null when nobody did. */
    fun hostAsker(d: GroupDetail): String? {
        if (!(d.hostRequestedHere || d.callHostRequestedHere) || !active(d.myRole)) return null
        val by = (d.hostRequests + d.callHostRequests).firstOrNull { it.isMe }?.appointedBy
        return by?.let { b -> d.members.firstOrNull { it.did == b }?.name }?.ifBlank { null } ?: appString(R.string.groupsc_an_admin)
    }

    /** Unread posts in groups we are still in, plus invitations waiting for an answer: the Groups tab's badge. */
    fun badge(list: List<GroupSummary>, invitations: List<GroupInvitation> = emptyList()) =
        list.filter { active(it.myRole) }.sumOf { it.unread.toInt() } + pendingInvitations(invitations).size

    /** Incoming invitations still to answer (Join / Decline). */
    fun pendingInvitations(invitations: List<GroupInvitation>) = invitations.filter { !it.outgoing && it.state == GroupInvitationState.PENDING }

    /**
     * The "Add members" picker: blocked contacts (either direction, P5) are not offered at all; members show
     * as already in; someone with a live invitation shows its state and can't be picked again.
     */
    fun pickRows(contacts: List<InviteCandidate>, d: GroupDetail, sent: List<GroupInvitation>, blockedByMe: (String) -> Boolean, blocksMe: (String) -> Boolean): List<PickRow> =
        contacts.filter { !blockedByMe(it.did) && !blocksMe(it.did) }.map { c ->
            val inv = sent.filter { it.inviteeDid == c.did }.maxByOrNull { it.at }
            val why = when {
                d.members.any { it.did == c.did } -> appString(R.string.groupsc_pick_member)
                inv != null && inv.state == GroupInvitationState.PENDING -> appString(R.string.groupsc_pick_invited_waiting)
                else -> null
            }
            PickRow(c.did, c.name, why, inv)
        }.sortedWith(compareBy({ it.why != null }, { it.name.lowercase() }))

    /** An invitation we sent, in words, and whether it is bad news. */
    fun sentInvitationLine(i: GroupInvitation): Pair<String, Boolean> = when (i.state) {
        GroupInvitationState.PENDING -> (if (i.delivered) appString(R.string.groupsc_invited_waiting_answer) else appString(R.string.groupsc_invited_not_delivered)) to false
        GroupInvitationState.JOINED -> appString(R.string.groupsc_invite_joined) to false
        GroupInvitationState.DECLINED -> appString(R.string.groupsc_invite_declined) to true
        GroupInvitationState.EXPIRED -> appString(R.string.groupsc_invite_expired) to true
        GroupInvitationState.BLOCKED -> appString(R.string.groupsc_invite_blocked_you) to true
    }

    fun actions(d: GroupDetail, m: CoreMember): List<MemberAction> {
        if (m.isMe || !active(d.myRole)) return emptyList()
        val out = mutableListOf<MemberAction>(MemberAction.Message)
        if (d.myRole == CoreRole.OWNER && !m.isOwner) out += if (m.isAdmin) MemberAction.RevokeAdmin else MemberAction.MakeAdmin
        if (isAdmin(d.myRole)) {
            m.deviceIds.forEach { dev ->
                out += MemberAction.Hosting(dev)
            }
            if (!m.isOwner) out += MemberAction.Flag
        }
        return out
    }

    /** Which flag ours would be, and the dialog's sentence. */
    fun flagCopy(name: String, flagsSoFar: Int): Pair<Int, String> {
        val f = firstName(name)
        val yours = flagsSoFar + 1
        return yours to appString(if (yours >= FLAGS_TO_REMOVE) R.string.groupsc_flag_final else R.string.groupsc_flag_more, yours, FLAGS_TO_REMOVE, f)
    }

    /** Words for a failed group call (the core's flat errors carry "they refused: <reason>"). */
    fun error(e: Throwable): String {
        val msg = e.message.orEmpty()
        return when (e) {
            is uniffi.p2pcore.Exception.Rejected -> {
                val r = msg.removePrefix("they refused: ").trim()
                when (r) {
                    "removed" -> appString(R.string.groupsc_err_removed)
                    "not a member" -> appString(R.string.groupsc_err_not_member)
                    "already a contact" -> appString(R.string.groupsc_err_already_contact)
                    "you block them" -> appString(R.string.groupsc_err_you_block)
                    "blocked" -> appString(R.string.groupsc_err_they_block)
                    "unknown or expired invite code" -> appString(R.string.groupsc_err_invite_invalid)
                    "admins only" -> ADMINS_ONLY_ERROR
                    "" -> appString(R.string.groupsc_err_refused)
                    else -> r.replaceFirstChar { it.uppercase() }.trimEnd('.') + "."
                }
            }
            is uniffi.p2pcore.Exception.Timeout -> appString(R.string.groupsc_err_timeout)
            is uniffi.p2pcore.Exception.Locked -> appString(R.string.groupsc_err_locked)
            is uniffi.p2pcore.Exception.NotFound -> appString(R.string.groupsc_err_not_found)
            else -> msg.ifBlank { appString(R.string.groupsc_err_generic) }
        }
    }
}

/** Inserts or replaces by id, keeping the core's (at, id) order. */
fun upsertGroupMessage(list: List<GroupMessage>, m: GroupMessage): List<GroupMessage> {
    val i = list.indexOfFirst { it.id == m.id }
    if (i >= 0) return list.toMutableList().also { it[i] = m }
    val at = list.indexOfFirst { it.at > m.at || (it.at == m.at && it.id > m.id) }.let { if (it < 0) list.size else it }
    return list.toMutableList().also { it.add(at, m) }
}

/**
 * The groups state the screens read, refreshed from the core on its [GroupEvents]. Reads are in-memory in
 * the core; actions return Result with what to tell the user, and must be called off the main thread.
 */
class GroupsController(private val api: GroupApi, private val me: () -> String?) : GroupEvents, GroupRequestEvents, GroupInvitationEvents {
    private val _incoming = MutableStateFlow<List<GroupChatRequest>>(emptyList())
    /** Chat requests to us (blocked senders are never listed), newest first. */
    val incoming: StateFlow<List<GroupChatRequest>> = _incoming
    private val _outgoing = MutableStateFlow<List<GroupChatRequest>>(emptyList())
    val outgoing: StateFlow<List<GroupChatRequest>> = _outgoing
    private val _notice = MutableStateFlow<String?>(null)
    /** Something to tell the user once ("Leo accepted your request"); the UI clears it. */
    val notice: StateFlow<String?> = _notice
    fun clearNotice() { _notice.value = null }
    private val _list = MutableStateFlow<List<GroupSummary>>(emptyList())
    val list: StateFlow<List<GroupSummary>> = _list
    private val _open = MutableStateFlow<OpenGroup?>(null)
    /** Bumped on every group change (receipts included): what the chat list's marks re-read on. */
    private val _changes = MutableStateFlow(0L)
    val changes: StateFlow<Long> = _changes
    val open: StateFlow<OpenGroup?> = _open
    @Volatile var myDevice: String = ""
        private set

    /** Read during composition: [me] must be cheap (a cached value, not a core call). */
    val myDid get() = me().orEmpty()

    @Synchronized fun refresh() {
        offMain("GroupsController.refresh")
        // A node without groups (or locked) answers with an error: an empty list.
        _list.value = try { api.groups() } catch (e: Exception) { emptyList() }
        if (myDevice.isEmpty()) myDevice = try { api.deviceId() } catch (_: Exception) { "" }
        reloadRequests()
        reloadOpen()
        reloadInvitations()
    }

    /** The group a screen shows (set by [open], cleared by [close]); read without the monitor. */
    @Volatile private var shown: String? = null

    /**
     * Blocking, off the main thread only: marks read (the core signs read receipts) and loads the page with
     * its receipt statuses (verifies every receipt signature of the days in range).
     */
    @Synchronized fun open(id: String) {
        offMain("GroupsController.open")
        shown = id
        _open.value = null
        try { api.markRead(id) } catch (_: Exception) {}
        load(id, GroupText.PAGE)
        _list.value = try { api.groups() } catch (e: Exception) { _list.value }
        reloadInvitations()
    }

    /**
     * The chat screen went away. Called on the main thread (onDispose), so it must never take the monitor:
     * [open], [refresh] and the core's group events hold it across markRead and receipt verification
     * (most of a second on a phone), and the UI thread would freeze waiting. Lock-free instead: [load]
     * re-checks [shown] after publishing, so a page that finishes after this is dropped.
     */
    fun close(id: String) {
        if (shown == id) shown = null
        _open.update { if (it?.id == id) null else it }
    }

    private fun load(id: String, want: Int) {
        _open.value = try {
            val d = api.detail(id)
            val p = api.messages(id, null, want)
            // Closing drops the link in the core; a link we kept for this group goes with it.
            if (!d.open) _openLink.update { if (it?.first == id) null else it }
            OpenGroup(id, d, p.messages, p.older, receiptsOf(id, p.messages), onlineOf(id, d))
        } catch (e: Exception) { null }
        // A close() that ran meanwhile wins (it clears [shown] before it clears [_open]).
        if (shown != id) _open.update { if (it?.id == id) null else it }
    }

    /** One core read per day doc in the range of my loaded posts; none if it fails (ticks stay one). */
    private fun receiptsOf(id: String, messages: List<GroupMessage>): Map<String, GroupReceiptStatus> {
        val mine = messages.filter { it.outgoing }
        if (mine.isEmpty()) return emptyMap()
        return try { api.receipts(id, mine.minOf { it.at }, mine.maxOf { it.at }).associateBy { it.messageId } } catch (e: Exception) { emptyMap() }
    }

    /** Who is online in an open group we are in (a cheap in-memory read; none for a private group). */
    private fun onlineOf(id: String, d: GroupDetail): List<OnlineMember> =
        if (GroupText.showsOnline(d)) try { api.online(id) } catch (_: Exception) { emptyList() } else emptyList()

    private fun reloadOpen() { _open.value?.let { load(it.id, maxOf(it.messages.size, GroupText.PAGE)) } }

    @Synchronized fun older() {
        val o = _open.value ?: return
        val before = o.older ?: return
        val p = try { api.messages(o.id, before, GroupText.PAGE) } catch (e: Exception) { return }
        _open.update { cur ->
            if (cur?.id != o.id) cur
            else cur.messages.map { it.id }.toSet().let { have -> cur.copy(messages = p.messages.filter { it.id !in have } + cur.messages, older = p.older) }
        }
    }

    fun peek(token: String): GroupInvitePeek? = if (token.isBlank()) null else try { api.peek(token.trim()) } catch (_: Exception) { null }

    fun post(id: String, text: String, replyTo: String?): Result<GroupMessage> {
        val t = text.trim()
        if (t.isEmpty()) return Result.failure(IllegalArgumentException(appString(R.string.groupsc_type_first)))
        if (_open.value?.takeIf { it.id == id }?.detail?.let(GroupText::adminsOnlyLocked) == true) return Result.failure(GroupFailure(GroupText.ADMINS_ONLY_ERROR))
        return runCatching { api.post(id, t, replyTo) }.onSuccess { m -> synchronized(this) {
            _open.update { o -> if (o?.id == id) o.copy(messages = upsertGroupMessage(o.messages, m)) else o }
            _list.value = try { api.groups() } catch (e: Exception) { _list.value }
        } }.mapError()
    }

    /** Posts a picture (bytes of a PNG, JPEG or WebP) with [caption]; the core scales and re-encodes it. IO threads only. */
    fun postImage(id: String, bytes: ByteArray, caption: String): Result<GroupMessage> {
        if (_open.value?.takeIf { it.id == id }?.detail?.let(GroupText::adminsOnlyLocked) == true) return Result.failure(GroupFailure(GroupText.ADMINS_ONLY_ERROR))
        if (bytes.isEmpty() || bytes.size > GroupText.MAX_PICTURE_BYTES) return Result.failure(GroupFailure(appString(R.string.groupsc_picture_too_large)))
        return runCatching { api.postImage(id, bytes, caption.trim()) }.onSuccess { m -> synchronized(this) {
            _open.update { o -> if (o?.id == id) o.copy(messages = upsertGroupMessage(o.messages, m)) else o }
            _list.value = try { api.groups() } catch (e: Exception) { _list.value }
        } }.mapError()
    }

    /** Posts the finished recording at [path] (the 1:1 voice format) with its [waveform]. IO threads only. */
    fun postVoice(id: String, path: String, waveform: ByteArray): Result<GroupMessage> {
        if (_open.value?.takeIf { it.id == id }?.detail?.let(GroupText::adminsOnlyLocked) == true) return Result.failure(GroupFailure(GroupText.ADMINS_ONLY_ERROR))
        return runCatching { api.postVoice(id, path, waveform) }.onSuccess { m -> synchronized(this) {
            _open.update { o -> if (o?.id == id) o.copy(messages = upsertGroupMessage(o.messages, m)) else o }
            _list.value = try { api.groups() } catch (e: Exception) { _list.value }
        } }.mapError()
    }

    /** Loads a voice post's recording now (tap, or try again); `onGroupImage` reports it, as for pictures. IO threads only. */
    fun fetchVoice(id: String, messageId: String): Result<Unit> = runCatching { api.fetchVoice(id, messageId) }.mapError()

    /** The voice post's recording as a file once it is here (null before). IO threads only. */
    fun voicePath(id: String, messageId: String): String? = try { api.voicePath(id, messageId) } catch (_: Exception) { null }

    /** Posts the file at [path] (a temporary copy the caller deletes after) as [name] with [caption]. IO threads only. */
    fun postFile(id: String, path: String, name: String, mime: String, caption: String): Result<GroupMessage> {
        if (_open.value?.takeIf { it.id == id }?.detail?.let(GroupText::adminsOnlyLocked) == true) return Result.failure(GroupFailure(GroupText.ADMINS_ONLY_ERROR))
        return runCatching { api.postFile(id, path, name, mime, caption.trim()) }.onSuccess { m -> synchronized(this) {
            _open.update { o -> if (o?.id == id) o.copy(messages = upsertGroupMessage(o.messages, m)) else o }
            _list.value = try { api.groups() } catch (e: Exception) { _list.value }
        } }.mapError()
    }

    /** Downloads a file post's file now (tap, or try again); `onGroupImage` reports progress and the end. IO threads only. */
    fun fetchFile(id: String, messageId: String): Result<Unit> = runCatching { api.fetchFile(id, messageId) }.mapError()

    /** Stops a download; the file goes back to "not downloaded". IO threads only. */
    fun cancelFile(id: String, messageId: String): Result<Unit> = runCatching { api.cancelFile(id, messageId) }.mapError()

    /** The file of a file post once it is here (null before). IO threads only. */
    fun filePath(id: String, messageId: String): String? = try { api.filePath(id, messageId) } catch (_: Exception) { null }

    /** Deletes this phone's copy (the group learns I no longer have it). IO threads only. */
    fun deleteFileCopy(id: String, messageId: String): Result<Unit> = runCatching { api.deleteFileCopy(id, messageId) }.mapError()

    /** Loads a post's picture now (tap to load, or try again). IO threads only. */
    fun fetchImage(id: String, messageId: String): Result<Unit> = runCatching { api.fetchImage(id, messageId) }.mapError()

    /** The post's picture as a file once it is here (null before). IO threads only. */
    fun imagePath(id: String, messageId: String): String? = try { api.imagePath(id, messageId) } catch (_: Exception) { null }

    fun create(name: String): Result<String> {
        if (name.isBlank()) return Result.failure(IllegalArgumentException(appString(R.string.groupsc_name_required)))
        return runCatching { api.create(name.trim()) }.also { refresh() }.mapError()
    }

    /** Blocks until joined (about a minute at most). */
    fun join(token: String): Result<String> = runCatching { api.join(token.trim()) }.also { refresh() }.mapError()

    fun invite(id: String): Result<String> = runCatching { api.invite(id) }.mapError()

    // ---- direct invitations (G3) ----

    private val _invitations = MutableStateFlow<List<GroupInvitation>>(emptyList())
    /** Invitations to us, newest first (blocked inviters are never listed). */
    val invitations: StateFlow<List<GroupInvitation>> = _invitations
    private val _sent = MutableStateFlow<List<GroupInvitation>>(emptyList())
    /** Invitations we sent for the open group. */
    val sent: StateFlow<List<GroupInvitation>> = _sent

    private fun reloadInvitations() {
        _invitations.value = try { api.invitationsIncoming() } catch (_: Exception) { emptyList() }
        _sent.value = _open.value?.id?.let { id -> try { api.invitationsOutgoing(id) } catch (_: Exception) { emptyList() } } ?: emptyList()
    }

    fun inviteContacts(id: String, dids: List<String>): Result<String> {
        if (dids.isEmpty()) return Result.failure(GroupFailure(appString(R.string.groupsc_pick_first)))
        return runCatching { api.inviteContacts(id, dids) }.map {
            if (it.size == 1) GroupText.withName(firstName(it[0].inviteeName), R.string.groupsc_invited_one_named, R.string.groupsc_invited_one_anon)
            else appPlural(R.plurals.groupsc_invited_many, it.size, it.size)
        }.also { synchronized(this) { reloadInvitations() } }.mapError()
    }

    /** Joins (blocks until joined, about a minute at most); the group id. */
    fun acceptInvitation(invitationId: String): Result<String> =
        runCatching { api.invitationAccept(invitationId).groupId }.also { refresh() }.mapError()

    fun declineInvitation(invitationId: String): Result<String> = runCatching { api.invitationDecline(invitationId) }
        .map { GroupText.withName(firstName(it.inviterName), R.string.groupsc_decline_inv_named, R.string.groupsc_decline_inv_anon) }.also { synchronized(this) { reloadInvitations() } }.mapError()

    /** Set by the app: an invitation arrived or changed (notifications; called on a core thread). */
    @Volatile var onInvitation: ((GroupInvitation) -> Unit)? = null

    override fun onGroupInvitation(invitation: GroupInvitation) { synchronized(this) { reloadInvitations() }; onInvitation?.invoke(invitation) }

    fun flag(id: String, did: String) = act({ appString(R.string.groupsc_flag_recorded) }) { api.flag(id, did) }
    fun setAdmin(id: String, did: String, admin: Boolean) = act({ if (admin) appString(R.string.groupsc_made_admin) else appString(R.string.groupsc_no_longer_admin) }) { api.setAdmin(id, did, admin) }
    fun leave(id: String) = act({ appString(R.string.groupsc_left_group) }) { api.leave(id) }
    /** One setup: admins grant capabilities; the local device accepts exactly the displayed selection. */
    fun configureHosting(id: String, did: String, dev: String, local: Boolean, messages: Boolean, calls: Boolean) = act({ appString(R.string.group_hosting_saved) }) {
        val d = api.detail(id)
        fun has(hosts: List<uniffi.p2pcore.GroupHost>) = hosts.any { it.did == did && it.deviceId == dev }
        if (GroupText.isAdmin(d.myRole)) {
            if (messages != (has(d.hosts) || has(d.hostRequests))) {
                if (messages) api.appointHost(id, did, dev) else api.revokeHost(id, did, dev)
            }
            if (calls != (has(d.callHosts) || has(d.callHostRequests))) {
                if (calls) api.appointCallHost(id, did, dev) else api.revokeCallHost(id, did, dev)
            }
        }
        if (local) {
            if (messages != (d.amHost || has(d.hostOffers)) || (d.hostRequestedHere && !messages)) api.acceptHost(id, messages)
            if (calls != (d.amCallHost || has(d.callHostOffers)) || (d.callHostRequestedHere && !calls)) api.acceptCallHost(id, calls)
        }
    }

    fun hostingDeviceId(): String = api.deviceId()

    // ---- public groups (docs/design/public-group.md §2–§3, §8)

    private val _openLink = MutableStateFlow<Pair<String, String>?>(null)
    /** The open link as the core last gave it: (group, link). Dropped when the group closes. */
    val openLink: StateFlow<Pair<String, String>?> = _openLink

    /** The open link of [id], opening the group if it is closed (network: IO threads only). */
    fun openLink(id: String): Result<String> = runCatching { api.openLink(id) }
        .onSuccess { _openLink.value = id to it }.also { refresh() }.mapError()

    fun closeLink(id: String) = act({ appString(R.string.groupsc_link_closed) }) { api.setOpen(id, false) }
        .also { r -> if (r.isSuccess) _openLink.update { if (it?.first == id) null else it } }

    fun setAdminsOnly(id: String, on: Boolean) = act({ if (on) appString(R.string.groupsc_admins_only_on) else appString(R.string.groupsc_admins_only_off) }) { api.setAdminsOnly(id, on) }

    /** This account shows in the online lists of public groups (a setting of this install; default on). */
    fun presenceVisible(): Boolean = try { api.presenceVisible() } catch (_: Exception) { true }

    private fun reloadRequests() {
        _incoming.value = try { api.requestsIncoming() } catch (_: Exception) { emptyList() }
        _outgoing.value = try { api.requestsOutgoing() } catch (_: Exception) { emptyList() }
    }

    /** "Message privately" for a member who is not a contact. */
    fun request(groupId: String, did: String): Result<String> =
        runCatching { api.chatRequest(groupId, did) }.map { GroupText.withName(firstName(it.peerName), R.string.groupsc_request_sent_named, R.string.groupsc_request_sent_anon) }
            .also { reloadRequests() }.mapError()

    fun accept(requestId: String): Result<String> = runCatching { api.chatAccept(requestId) }
        .map { GroupText.withName(firstName(it.peerName), R.string.groupsc_req_accept_named, R.string.groupsc_req_accept_anon) }
        .also { reloadRequests() }.mapError()

    fun decline(requestId: String): Result<String> = runCatching { api.chatDecline(requestId) }
        .map { GroupText.withName(firstName(it.peerName), R.string.groupsc_req_declined, R.string.groupsc_req_declined_told_anon) }.also { reloadRequests() }.mapError()

    /** Block (P5: they are told) and decline. */
    fun blockRequester(requestId: String, did: String): Result<String> = runCatching { api.block(did); api.chatDecline(requestId) }
        .map { GroupText.withName(firstName(it.peerName), R.string.groupsc_req_block_named, R.string.groupsc_req_block_anon) }.also { reloadRequests() }.mapError()

    override fun onChatRequest(request: GroupChatRequest) {
        reloadRequests()
        if (request.outgoing && request.state == ChatRequestState.ACCEPTED && request.contactAdded)
            _notice.value = GroupText.withName(firstName(request.peerName), R.string.groupsc_notice_accepted_named, R.string.groupsc_notice_accepted_anon)
    }

    private fun act(ok: () -> String, f: () -> Unit): Result<String> = runCatching { f(); ok() }.also { refresh() }.mapError()

    private fun <T> Result<T>.mapError(): Result<T> = fold({ Result.success(it) }, { Result.failure(GroupFailure(GroupText.error(it))) })

    // ---- core events (core threads: never block) ----

    /** Set by the app: a post arrived (notifications; called on a core thread). */
    @Volatile var onPost: ((GroupMessage) -> Unit)? = null

    override fun onGroupMessage(message: GroupMessage) {
        onPost?.invoke(message)
        val gid = message.groupId
        _open.update { o -> if (o?.id == gid) o.copy(messages = upsertGroupMessage(o.messages, message)) else o }
        // On screen: read now. Outside the monitor: markRead is a blocking core call (it signs read receipts).
        if (shown == gid && _open.value?.id == gid) try { api.markRead(gid) } catch (_: Exception) {}
        synchronized(this) { _list.value = try { api.groups() } catch (e: Exception) { _list.value } }
    }

    override fun onGroupChanged(groupId: String) {
        _changes.value = _changes.value + 1
        synchronized(this) {
            _list.value = try { api.groups() } catch (e: Exception) { _list.value }
            if (_open.value?.id == groupId) { reloadOpen(); reloadInvitations() }
        }
    }

    /**
     * An open group's online list changed (public group §3): re-read it for the group on screen. An in-memory core read, and
     * without the monitor (it can be held across markRead for most of a second; these come every 15 s or so).
     */
    /** A post's picture arrived or changed state (public group §5): the group on screen re-reads its window. */
    override fun onGroupImage(groupId: String, messageId: String) {
        if (shown != groupId) return
        synchronized(this) { if (_open.value?.id == groupId) reloadOpen() }
    }

    override fun onGroupPresence(groupId: String) {
        val o = _open.value?.takeIf { it.id == groupId } ?: return
        val now = onlineOf(groupId, o.detail)
        _open.update { cur -> if (cur?.id == groupId) cur.copy(online = now) else cur }
    }

    fun attach(node: Node) {
        try { node.setGroupEvents(this); node.setGroupRequestEvents(this); node.setGroupInvitationEvents(this) } catch (e: Exception) { Log.w(P2pApp.TAG, "setGroupEvents: $e") }
    }
}

/**
 * StrictMode-style guard for the blocking group calls: logs (with the stack) when one runs on the main
 * thread, where it froze the group screen for most of a second. Silent off Android (unit tests).
 */
internal fun offMain(what: String) {
    val main = runCatching { android.os.Looper.getMainLooper()?.isCurrentThread == true }.getOrDefault(false)
    if (main) runCatching { Log.e(P2pApp.TAG, "$what on the main thread: move it to Dispatchers.IO", Throwable(what)) }
}

/** Group invite links (G3): `tinline://g/<code>`; the bare code is accepted too. */
object GroupLinks {
    const val PREFIX = "tinline://g/"
    /** Whether [text] looks like a group invite link (any case of the scheme). */
    fun isLink(text: String?): Boolean = text?.trim()?.let { it.length > PREFIX.length && it.startsWith(PREFIX, ignoreCase = true) } == true
}

/** A tapped tinline://g/ link waits here until the app is unlocked, then opens Join. */
object GroupLinkInbox {
    val link = MutableStateFlow<String?>(null)

    fun take(intent: android.content.Intent?) {
        if (intent?.action != android.content.Intent.ACTION_VIEW) return
        val d = intent.dataString ?: return
        if (!GroupLinks.isLink(d)) return
        intent.action = null // not again after a recreate
        link.value = d
    }
}

/** A failed group action: [message] is already in the user's words. */
class GroupFailure(message: String) : Exception(message)
