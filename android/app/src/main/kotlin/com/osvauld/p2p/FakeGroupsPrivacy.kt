package com.osvauld.p2p

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import java.time.Instant
import java.time.ZoneId

/**
 * PLACEHOLDER: in-memory [GroupsPrivacySource] seeded with the people and groups of the design boards
 * (docs/design/groups/). Deterministic for a given [clock]: no threads, no timers, no randomness.
 * Actions mutate the state so the flows can be clicked through; the `simulate*` calls stand in for
 * what other people would do (accept a request, block me, take hosts offline).
 *
 * Pure Kotlin (no Android types) so the state machine is covered by JVM unit tests.
 */
class FakeGroupsPrivacy(
    private val clock: () -> Long = System::currentTimeMillis,
    override val myName: String = "You",
    private val zone: ZoneId = ZoneId.systemDefault(),
) : GroupsPrivacySource {

    companion object {
        const val ME = "did:key:fake-me"
        const val HIKE = "g-hike"
        const val FAMILY = "g-family"
        const val BOOK = "g-book"
        const val MINUTE = 60_000L
        const val HOUR = 60 * MINUTE
        const val DAY = 24 * HOUR
        /** Removal happens at this many flags. */
        const val FLAGS_TO_REMOVE = 3
        const val REQUEST_EXPIRY_MS = 7 * DAY
        const val REQUEST_RATE_MS = DAY
        /** Hosts refresh their addresses this long after a removal. */
        const val HOST_REFRESH_MS = 2 * DAY
        /** Contacts outside the named sample people, all already told (the board's "11 of 12"). */
        const val OTHER_CONTACTS = 9
        fun did(name: String) = "did:key:fake-" + name.substringBefore(' ').lowercase()
        val ARJUN = did("Arjun"); val MEERA = did("Meera"); val SAM = did("Sam"); val PRIYA = did("Priya")
        val LEO = did("Leo"); val DEV = did("Dev"); val RAVI = did("Ravi")
    }

    override val myDid = ME

    private val _groups = MutableStateFlow<List<Group>>(emptyList())
    override val groups: StateFlow<List<Group>> = _groups
    private val _invites = MutableStateFlow<List<GroupInvite>>(emptyList())
    override val invites: StateFlow<List<GroupInvite>> = _invites
    private val _people = MutableStateFlow<List<Person>>(emptyList())
    override val people: StateFlow<List<Person>> = _people
    private val _requests = MutableStateFlow(Requests())
    override val requests: StateFlow<Requests> = _requests
    private val _blocked = MutableStateFlow<List<BlockedPerson>>(emptyList())
    override val blocked: StateFlow<List<BlockedPerson>> = _blocked
    private val _address = MutableStateFlow(AddressChanges(AddressInterval.ThreeDays, 0, 0, 0, 0))
    override val address: StateFlow<AddressChanges> = _address

    /** Messages a member wrote offline that arrive just after their removal (the group's "written before" view). */
    private val lateMessages = HashMap<String, String>()
    private var seq = 0
    private fun nid(p: String = "x") = "$p${++seq}"
    private fun now() = clock()

    init { reset() }

    // ------------------------------------------------------------------ seed

    /** Back to the boards' sample data. */
    fun reset() {
        seq = 0
        val n = now()
        lateMessages.clear()
        lateMessages[LEO] = "Running late, start without me"
        val meMember = GroupMember(ME, myName, setOf(GroupRole.Admin), n - 60 * DAY, "Meera K", isMe = true)
        val hike = Group(
            HIKE, "Weekend hike",
            members = listOf(
                GroupMember(MEERA, "Meera K", setOf(GroupRole.Owner, GroupRole.Host), n - 90 * DAY),
                GroupMember(ARJUN, "Arjun Oommen", setOf(GroupRole.Host), n - 80 * DAY, "Meera K"),
                GroupMember(SAM, "Sam Rao", setOf(GroupRole.Admin), n - 80 * DAY, "Meera K"),
                meMember,
                GroupMember(LEO, "Leo B", emptySet(), n - 38 * DAY, "Sam Rao",
                    flags = listOf(GroupFlag(SAM, "Sam Rao", GroupRole.Admin, n - 20 * HOUR))),
                GroupMember(PRIYA, "Priya N", emptySet(), n - 30 * DAY, "Sam Rao"),
                GroupMember(did("Nisha"), "Nisha R", emptySet(), n - 30 * DAY, "Meera K"),
                GroupMember(did("Tom"), "Tom W", emptySet(), n - 20 * DAY, "Arjun Oommen"),
            ),
            hosts = listOf(
                GroupHost("h-arjun", "Arjun’s desktop", ARJUN, online = true, appointedBy = "Meera"),
                GroupHost("h-meera", "Meera’s desktop", MEERA, online = false, appointedBy = null, offlineSinceMs = n - 2 * HOUR),
            ),
            items = listOf(
                GroupItem.Sys(nid(), n - 3 * HOUR, "Leo joined, invited by Sam"),
                gm(SAM, "Sam", "Saturday 7am at the trailhead. Bring rain jackets.", n - 2 * HOUR - 50 * MINUTE),
                gm(PRIYA, "Priya", "Can someone give me a lift from the station?", n - 2 * HOUR - 40 * MINUTE),
                gm(ME, myName, "I can, I’ll message you.", n - 2 * HOUR - 35 * MINUTE, out = true),
                GroupItem.Sys(nid(), n - 2 * HOUR, "Messages are going through Arjun’s desktop. Meera’s desktop is offline."),
                gm(LEO, "Leo", "Sent this on the train yesterday, just got signal.", n - 70 * MINUTE),
                gm(SAM, "Sam", "Map attached: trail-loop.gpx", n - 50 * MINUTE),
            ),
            unread = 4, tint = 0,
        )
        val family = Group(
            FAMILY, "Family",
            members = listOf(
                GroupMember(ME, myName, setOf(GroupRole.Owner), n - 200 * DAY, isMe = true),
                GroupMember(did("Dad"), "Dad", setOf(GroupRole.Host), n - 200 * DAY, myName),
                GroupMember(did("Anu"), "Anu", setOf(GroupRole.Admin, GroupRole.Host), n - 200 * DAY, myName),
                GroupMember(did("Mum"), "Mum", emptySet(), n - 200 * DAY, myName),
            ),
            hosts = listOf(
                GroupHost("h-dad", "Dad’s desktop", did("Dad"), online = false, appointedBy = myName, offlineSinceMs = n - 26 * HOUR),
                GroupHost("h-anu", "Anu’s desktop", did("Anu"), online = false, appointedBy = myName, offlineSinceMs = n - 20 * HOUR),
            ),
            items = listOf(
                gm(did("Dad"), "Dad", "Who has the tent?", n - 27 * HOUR),
                gm(ME, myName, "I do", n - 25 * HOUR, out = true, d = GroupDelivery.WaitingForHost),
                gm(ME, myName, "Also two sleeping mats", n - 25 * HOUR + MINUTE, out = true, d = GroupDelivery.WaitingForHost),
                GroupItem.Sys(nid(), n - 24 * HOUR, "Mum’s desktop is no longer a host. The group is moving to new addresses; nothing to do on your side."),
                gm(ME, myName, "Photos from Sunday", n - 23 * HOUR, out = true, d = GroupDelivery.WaitingForHost),
            ),
            tint = 1,
        )
        val removedAt = n - 12 * DAY
        val bookFlags = listOf(
            GroupFlag(SAM, "Sam Rao", GroupRole.Admin, removedAt - 17 * HOUR),
            GroupFlag(MEERA, "Meera K", GroupRole.Owner, removedAt - 4 * HOUR - 20 * MINUTE),
            GroupFlag(PRIYA, "Priya N", GroupRole.Admin, removedAt),
        )
        val book = Group(
            BOOK, "Book club",
            members = listOf(
                GroupMember(MEERA, "Meera K", setOf(GroupRole.Owner), n - 100 * DAY),
                GroupMember(SAM, "Sam Rao", setOf(GroupRole.Admin, GroupRole.Host), n - 100 * DAY),
                GroupMember(PRIYA, "Priya N", setOf(GroupRole.Admin), n - 100 * DAY),
                GroupMember(ME, myName, emptySet(), n - 60 * DAY, "Sam Rao", flags = bookFlags, removed = true, isMe = true),
            ),
            hosts = listOf(GroupHost("h-sam", "Sam’s desktop", SAM, online = true, appointedBy = "Meera")),
            items = listOf(
                gm(SAM, "Sam", "Next one is on the 12th.", removedAt - 2 * DAY),
                gm(ME, myName, "I’ll bring the second book", removedAt - 2 * DAY + 10 * MINUTE, out = true),
                GroupItem.Sys(nid(), removedAt, "You were removed from the group"),
                gm(ME, myName, "Running late, start without me", removedAt + MINUTE, out = true, d = GroupDelivery.WrittenBeforeRemoval),
            ),
            removal = Removal(removedAt, bookFlags, removedAt + HOST_REFRESH_MS),
            tint = 2,
        )
        _groups.value = listOf(hike, family, book)

        _invites.value = listOf(
            GroupInvite("inv-trek", "Monsoon trek", SAM, "Sam Rao", "Meera K", listOf("Arjun’s desktop", "Meera’s desktop"), 7, n + 6 * DAY),
        )

        val yesterday0312 = Instant.ofEpochMilli(n).atZone(zone).toLocalDate().minusDays(1).atTime(3, 12).atZone(zone).toInstant().toEpochMilli()
        _people.value = listOf(
            Person(ARJUN, "Arjun Oommen", verifiedAtMs = n - 40 * DAY, theirAddressChangedAtMs = n - 2 * DAY, items = listOf(
                DmItem.Text(nid(), n - 6 * HOUR, false, "Are you coming Saturday?"),
                DmItem.Text(nid(), n - 5 * HOUR, true, "Not sure yet", DmTick.Read),
                DmItem.Text(nid(), n - 4 * HOUR - 28 * MINUTE, false, "See you at 6"),
            )),
            Person(MEERA, "Meera K", verifiedAtMs = n - 50 * DAY, blockedMe = BlockNotice(n - 15 * MINUTE), theirAddressChangedAtMs = n - DAY, items = listOf(
                DmItem.Text(nid(), n - 4 * DAY, true, "Are you coming Saturday?", DmTick.Read),
                DmItem.Text(nid(), n - 4 * DAY + HOUR, false, "Not sure yet"),
                DmItem.Text(nid(), n - 10 * MINUTE, true, "Hello?", DmTick.NotDeliveredBlocked),
            )),
            Person(SAM, "Sam Rao", verifiedAtMs = n - 70 * DAY, lost = true, hasMyCurrentAddress = false, theirAddressChangedAtMs = n - 9 * DAY, items = listOf(
                DmItem.Text(nid(), n - 14 * DAY, false, "Adding you to the hike group"),
                DmItem.Invite(nid(), n - 14 * DAY + MINUTE, "inv-trek"),
                DmItem.Text(nid(), n - 12 * DAY, false, "Back from the trek on the 20th"),
                DmItem.Text(nid(), n - 12 * DAY + HOUR, true, "Have fun", DmTick.Delivered),
                DmItem.Text(nid(), n - 2 * DAY, true, "Are you back?", DmTick.WaitingToReach),
            )),
        )
        _requests.value = Requests(
            sent = listOf(
                SentRequest("req-leo", LEO, "Leo B", "Weekend hike", "Hi Leo, can I ask about the lift?", n - 2 * HOUR),
                SentRequest("req-dev", DEV, "Dev T", "Weekend hike", "Hi Dev", n - 8 * DAY, SentRequestState.Declined, n - 7 * DAY),
                SentRequest("req-ravi", RAVI, "Ravi J", "Book club", "Hi Ravi", n - 17 * DAY, SentRequestState.Expired, n - 10 * DAY),
            ),
            incoming = listOf(
                IncomingRequest("in-priya", PRIYA, "Priya N", "Weekend hike", 1, "Hi, it’s Priya. Can I ask about the lift?", n - 30 * MINUTE),
            ),
        )
        _blocked.value = listOf(BlockedPerson(DEV, "Dev T", n - 7 * DAY, BlockVia.ChatRequest))
        _address.value = AddressChanges(AddressInterval.ThreeDays, yesterday0312, yesterday0312 + 3 * DAY, 0, 0)
        recount()
    }

    private fun gm(did: String, name: String, text: String, at: Long, out: Boolean = false, d: GroupDelivery = GroupDelivery.Sent) =
        GroupItem.Msg(nid("m"), at, did, name, text, out, d)

    // ------------------------------------------------------------------ helpers

    private fun group(id: String) = _groups.value.firstOrNull { it.id == id }
    private fun person(did: String) = _people.value.firstOrNull { it.did == did }
    private fun updGroup(id: String, f: (Group) -> Group) = _groups.update { l -> l.map { if (it.id == id) f(it) else it } }
    private fun updPerson(did: String, f: (Person) -> Person) { _people.update { l -> l.map { if (it.did == did) f(it) else it } }; recount() }
    private fun Group.addItem(i: GroupItem) = copy(items = items + i)
    private fun Person.addItem(i: DmItem) = copy(items = items + i)
    private fun first(name: String) = name.substringBefore(' ')

    /** Recomputes "contacts told N of M": blocked people are excluded from address updates. */
    private fun recount() {
        val eligible = _people.value.filter { p -> _blocked.value.none { it.did == p.did } }
        _address.update { it.copy(contactsTold = OTHER_CONTACTS + eligible.count { p -> p.hasMyCurrentAddress }, contactsTotal = OTHER_CONTACTS + eligible.size) }
    }

    // ------------------------------------------------------------------ groups

    override fun createGroup(name: String, memberDids: List<String>, useMyDesktop: Boolean): String {
        val n = now()
        val id = nid("g")
        val hosts = if (useMyDesktop) listOf(GroupHost(nid("h"), "Your desktop", ME, online = true, appointedBy = null)) else emptyList()
        val invited = memberDids.mapNotNull { person(it) }
        _groups.update { it + Group(
            id, name.trim().ifBlank { "New group" },
            members = listOf(GroupMember(ME, myName, if (useMyDesktop) setOf(GroupRole.Owner, GroupRole.Host) else setOf(GroupRole.Owner), n, isMe = true)),
            hosts = hosts,
            items = listOf(GroupItem.Sys(nid(), n, "You made the group" + if (invited.isEmpty()) "" else ". Invited " + invited.joinToString { p -> first(p.name) })),
            tint = (_groups.value.size % 3),
        ) }
        // Adding someone sends them an invite in my 1:1 chat with them.
        invited.forEach { p ->
            val inv = GroupInvite(nid("inv"), name, ME, myName, myName, hosts.map { it.label }, 1, n + 7 * DAY, groupId = id)
            _invites.update { it + inv }
            updPerson(p.did) { it.addItem(DmItem.Invite(nid(), n, inv.id)) }
        }
        return id
    }

    override fun sendGroupText(groupId: String, text: String): Outcome {
        val g = group(groupId) ?: return Outcome.Refused("No such group")
        if (g.removal != null || g.me?.removed != false) return Outcome.Refused("You can’t send messages to this group.")
        if (text.isBlank()) return Outcome.Refused("Empty message")
        updGroup(groupId) { it.addItem(gm(ME, myName, text.trim(), now(), out = true, d = if (it.hostReachable) GroupDelivery.Sent else GroupDelivery.WaitingForHost)) }
        return Outcome.Ok
    }

    override fun flagMember(groupId: String, memberDid: String): Outcome {
        val g = group(groupId) ?: return Outcome.Refused("No such group")
        val me = g.me
        if (!g.iAmAdmin || me == null) return Outcome.Refused("Only the owner and admins can flag.")
        if (memberDid == ME) return Outcome.Refused("You can’t flag yourself.")
        val target = g.members.firstOrNull { it.did == memberDid } ?: return Outcome.Refused("Not a member")
        if (target.removed) return Outcome.Refused("${first(target.name)} was already removed.")
        val n = now()
        val flag = GroupFlag(ME, myName, if (GroupRole.Owner in me.roles) GroupRole.Owner else GroupRole.Admin, n)
        val flags = target.flags + flag
        val removed = flags.size >= FLAGS_TO_REMOVE
        updGroup(groupId) { gr ->
            var out = gr.copy(members = gr.members.map { if (it.did == memberDid) it.copy(flags = flags, removed = removed) else it })
            if (removed) {
                out = out.addItem(GroupItem.Sys(nid(), n, "${first(target.name)} was removed after $FLAGS_TO_REMOVE admin flags"))
                // A message they wrote offline before the removal still arrives, labelled as such.
                lateMessages.remove(memberDid)?.let { t ->
                    out = out.addItem(GroupItem.Msg(nid("m"), n + 1, memberDid, first(target.name), t, false, writtenBeforeAuthorRemoved = true))
                }
            }
            out
        }
        return Outcome.Ok
    }

    override fun makeAdmin(groupId: String, memberDid: String): Outcome {
        val g = group(groupId) ?: return Outcome.Refused("No such group")
        if (!g.iAmAdmin) return Outcome.Refused("Only the owner and admins can make admins.")
        val t = g.members.firstOrNull { it.did == memberDid && !it.removed } ?: return Outcome.Refused("Not a member")
        if (t.isAdmin) return Outcome.Refused("${first(t.name)} is already an admin.")
        updGroup(groupId) { gr ->
            gr.copy(members = gr.members.map { if (it.did == memberDid) it.copy(roles = it.roles + GroupRole.Admin) else it })
                .addItem(GroupItem.Sys(nid(), now(), "${first(t.name)} is now an admin"))
        }
        return Outcome.Ok
    }

    override fun askToHost(groupId: String, memberDid: String): Outcome {
        val g = group(groupId) ?: return Outcome.Refused("No such group")
        if (!g.iAmAdmin) return Outcome.Refused("Only the owner and admins can choose hosts.")
        val t = g.members.firstOrNull { it.did == memberDid && !it.removed } ?: return Outcome.Refused("Not a member")
        if (g.hosts.any { it.ownerDid == memberDid }) return Outcome.Refused("${first(t.name)}’s desktop is already a host.")
        updGroup(groupId) { it.copy(hosts = it.hosts + GroupHost(nid("h"), "${first(t.name)}’s desktop", memberDid, online = false, appointedBy = first(myName), pending = true)) }
        return Outcome.Ok
    }

    override fun removeHost(groupId: String, hostId: String): Outcome {
        val g = group(groupId) ?: return Outcome.Refused("No such group")
        if (!g.iAmAdmin) return Outcome.Refused("Only the owner and admins can remove hosts.")
        val h = g.hosts.firstOrNull { it.id == hostId } ?: return Outcome.Refused("Not a host")
        updGroup(groupId) { gr ->
            val hosts = gr.hosts.filter { it.id != hostId }
            gr.copy(
                hosts = hosts,
                members = gr.members.map { m -> if (m.did == h.ownerDid && hosts.none { it.ownerDid == m.did }) m.copy(roles = m.roles - GroupRole.Host) else m },
            ).let { if (h.pending) it else it.addItem(GroupItem.Sys(nid(), now(), "${h.label} is no longer a host. The group is moving to new addresses; nothing to do on your side.")) }
        }
        return Outcome.Ok
    }

    override fun setMuted(groupId: String, muted: Boolean) = updGroup(groupId) { it.copy(muted = muted) }
    override fun leaveGroup(groupId: String) = _groups.update { l -> l.filter { it.id != groupId } }
    override fun deleteGroup(groupId: String) = _groups.update { l -> l.filter { it.id != groupId } }
    override fun keepAsArchive(groupId: String) = updGroup(groupId) { g -> g.copy(removal = g.removal?.copy(archived = true)) }
    override fun markGroupRead(groupId: String) = updGroup(groupId) { it.copy(unread = 0) }

    override fun joinInvite(inviteId: String): Outcome {
        val inv = _invites.value.firstOrNull { it.id == inviteId } ?: return Outcome.Refused("No such invite")
        if (inv.state != InviteState.Pending) return Outcome.Refused("This invite was already answered.")
        val n = now()
        if (n > inv.expiresAtMs) { setInvite(inviteId) { it.copy(state = InviteState.Expired) }; return Outcome.Refused("Invite expired") }
        if (inv.inviterDid == ME) return Outcome.Refused("This is your own invite.")
        val id = nid("g")
        val others = (3 until inv.memberCount).map { GroupMember(did("member$it"), "Member ${it - 1}", emptySet(), n - DAY) }
        _groups.update { it + Group(
            id, inv.groupName,
            members = listOf(
                GroupMember(did(inv.ownerName), inv.ownerName, setOf(GroupRole.Owner), n - 10 * DAY),
                GroupMember(inv.inviterDid, inv.inviterName, setOf(GroupRole.Admin), n - 10 * DAY),
            ) + others + GroupMember(ME, myName, emptySet(), n, first(inv.inviterName), isMe = true),
            hosts = inv.hostLabels.mapIndexed { i, l -> GroupHost(nid("h"), l, did(l), online = i == 0, appointedBy = first(inv.ownerName)) },
            items = listOf(GroupItem.Sys(nid(), n, "You joined, invited by ${first(inv.inviterName)}")),
            tint = 0,
        ) }
        setInvite(inviteId) { it.copy(state = InviteState.Joined, groupId = id) }
        return Outcome.Ok
    }

    override fun declineInvite(inviteId: String) = setInvite(inviteId) { if (it.state == InviteState.Pending) it.copy(state = InviteState.Declined) else it }
    private fun setInvite(id: String, f: (GroupInvite) -> GroupInvite) = _invites.update { l -> l.map { if (it.id == id) f(it) else it } }

    // ------------------------------------------------------------------ people

    override fun sendText(did: String, text: String): Outcome {
        val p = person(did) ?: return Outcome.Refused("Not a contact")
        if (text.isBlank()) return Outcome.Refused("Empty message")
        if (isBlocked(did)) return Outcome.Refused("Unblock ${first(p.name)} to message them.")
        val n = now()
        if (p.blockedMe != null) {
            updPerson(did) { it.addItem(DmItem.Text(nid(), n, true, text.trim(), DmTick.NotDeliveredBlocked)) }
            return Outcome.Refused("Not delivered: ${first(p.name)} blocked you")
        }
        updPerson(did) { it.addItem(DmItem.Text(nid(), n, true, text.trim(), if (p.lost) DmTick.WaitingToReach else DmTick.Delivered)) }
        return Outcome.Ok
    }

    override fun setCanMessage(did: String, on: Boolean) = updPerson(did) { it.copy(canMessage = on) }
    override fun setCanCall(did: String, on: Boolean) = updPerson(did) { it.copy(canCall = on, callsChipDone = true) }
    override fun dismissCallsChip(did: String) = updPerson(did) { it.copy(callsChipDone = true) }

    override fun askToAllowCalls(did: String): Outcome {
        val p = person(did) ?: return Outcome.Refused("Not a contact")
        if (p.theyAllowMyCalls) return Outcome.Refused("${first(p.name)} already allows your calls.")
        if (p.blockedMe != null || isBlocked(did)) return Outcome.Refused("You can’t ask while blocked.")
        updPerson(did) { it.addItem(DmItem.CallAsk(nid(), now(), outgoing = true)) }
        return Outcome.Ok
    }

    override fun answerCallAsk(did: String, itemId: String, allow: Boolean) = updPerson(did) { p ->
        p.copy(
            items = p.items.map { if (it.id == itemId && it is DmItem.CallAsk) it.copy(handled = true) else it },
            canCall = if (allow) true else p.canCall,
        )
    }

    override fun callCheck(did: String): CallCheck {
        val p = person(did) ?: return CallCheck.TheyHaveNotAllowed
        return when {
            isBlocked(did) -> CallCheck.IBlocked
            p.blockedMe != null -> CallCheck.BlockedMe
            p.lost -> CallCheck.Lost
            !p.theyAllowMyCalls -> CallCheck.TheyHaveNotAllowed
            else -> CallCheck.Allowed
        }
    }

    override fun reconnect(did: String) = updPerson(did) { p ->
        p.copy(lost = false, hasMyCurrentAddress = true, theirAddressChangedAtMs = now(),
            items = p.items.map { if (it is DmItem.Text && it.tick == DmTick.WaitingToReach) it.copy(tick = DmTick.Delivered) else it } +
                DmItem.Sys(nid(), now(), "Reconnected with ${first(p.name)}. History and verification kept."))
    }

    override fun markPersonRead(did: String) = updPerson(did) { it.copy(unread = 0) }

    // ------------------------------------------------------------------ requests

    override fun sendRequest(toDid: String, toName: String, viaGroup: String, note: String): Outcome {
        val n = now()
        expireDue()
        if (person(toDid) != null) return Outcome.Refused("${first(toName)} is already a contact.")
        if (isBlocked(toDid)) return Outcome.Refused("Unblock ${first(toName)} first.")
        val recent = _requests.value.sent.firstOrNull { it.toDid == toDid && n - it.sentAtMs < REQUEST_RATE_MS }
        if (recent != null) return Outcome.Refused("You already asked ${first(toName)} today. You can send one request per person per day.")
        _requests.update { r -> r.copy(sent = listOf(SentRequest(nid("req"), toDid, toName, viaGroup, note.trim(), n)) + r.sent.filter { it.toDid != toDid }) }
        return Outcome.Ok
    }

    override fun cancelRequest(id: String) = _requests.update { r -> r.copy(sent = r.sent.filter { it.id != id }) }
    override fun dismissRequest(id: String) = _requests.update { r -> r.copy(sent = r.sent.filter { it.id != id }) }

    override fun acceptRequest(id: String): String? {
        val req = _requests.value.incoming.firstOrNull { it.id == id && it.state == IncomingRequestState.Pending } ?: return null
        val n = now()
        setIncoming(id, IncomingRequestState.Accepted)
        if (person(req.fromDid) == null) _people.update { it + Person(
            req.fromDid, req.fromName, ContactOrigin.GroupRequest, req.viaGroup,
            canMessage = true, canCall = false, theyAllowMyCalls = false, callsChipDone = false,
            theirAddressChangedAtMs = n,
            items = listOfNotNull(
                DmItem.Sys(nid(), n, "You accepted ${first(req.fromName)}’s request from ${req.viaGroup}"),
                req.note.takeIf { it.isNotBlank() }?.let { DmItem.Text(nid(), req.atMs.coerceAtMost(n), false, it) },
            ).sortedBy { it.atMs },
        ) }
        recount()
        return req.fromDid
    }

    override fun declineRequest(id: String) = setIncoming(id, IncomingRequestState.Declined)

    override fun blockRequester(id: String) {
        val req = _requests.value.incoming.firstOrNull { it.id == id } ?: return
        setIncoming(id, IncomingRequestState.Blocked)
        block(req.fromDid, req.fromName, BlockVia.ChatRequest)
    }

    private fun setIncoming(id: String, s: IncomingRequestState) =
        _requests.update { r -> r.copy(incoming = r.incoming.map { if (it.id == id) it.copy(state = s) else it }) }

    override fun expireDue() {
        val n = now()
        _requests.update { r -> r.copy(sent = r.sent.map {
            if (it.state == SentRequestState.Waiting && n - it.sentAtMs >= REQUEST_EXPIRY_MS) it.copy(state = SentRequestState.Expired, endedAtMs = it.sentAtMs + REQUEST_EXPIRY_MS) else it
        }) }
    }

    // ------------------------------------------------------------------ blocking

    override fun block(did: String, name: String, via: BlockVia): Outcome {
        if (did == ME) return Outcome.Refused("You can’t block yourself.")
        if (isBlocked(did)) return Outcome.Refused("${first(name)} is already blocked.")
        val n = now()
        _blocked.update { it + BlockedPerson(did, name, n, via) }
        // Personal only: group membership, roles and flags are untouched.
        if (person(did) != null) updPerson(did) { it.copy(attemptsSinceBlock = 0).addItem(DmItem.Sys(nid(), n, "You blocked ${first(name)}")) }
        recount()
        return Outcome.Ok
    }

    override fun unblock(did: String) {
        val b = _blocked.value.firstOrNull { it.did == did } ?: return
        _blocked.update { l -> l.filter { it.did != did } }
        // They get my current address again; calls come back only if they were allowed before (canCall is untouched).
        if (person(did) != null) updPerson(did) { it.copy(hasMyCurrentAddress = true, attemptsSinceBlock = 0).addItem(DmItem.Sys(nid(), now(), "You unblocked ${first(b.name)}")) }
        recount()
    }

    override fun isBlocked(did: String) = _blocked.value.any { it.did == did }

    // ------------------------------------------------------------------ address changes

    override fun setInterval(interval: AddressInterval) =
        _address.update { it.copy(interval = interval, nextChangeMs = it.lastChangeMs + interval.days * DAY) }

    // ------------------------------------------------------------------ simulation (what other people do)

    /** Leo (or whoever) accepts my waiting request: a messages-only contact, calls off both ways. */
    fun simulateRequestAnswered(requestId: String, accept: Boolean) {
        val r = _requests.value.sent.firstOrNull { it.id == requestId && it.state == SentRequestState.Waiting } ?: return
        val n = now()
        _requests.update { rq -> rq.copy(sent = rq.sent.map { if (it.id == requestId) it.copy(state = if (accept) SentRequestState.Accepted else SentRequestState.Declined, endedAtMs = n) else it }) }
        if (accept && person(r.toDid) == null) {
            _people.update { it + Person(r.toDid, r.toName, ContactOrigin.GroupRequest, r.viaGroup, canMessage = true, canCall = false,
                theyAllowMyCalls = false, callsChipDone = false, theirAddressChangedAtMs = n, items = listOfNotNull(
                    r.note.takeIf { it.isNotBlank() }?.let { DmItem.Text(nid(), r.sentAtMs, true, it, DmTick.Read) },
                    DmItem.Sys(nid(), n, "${first(r.toName)} accepted your request from ${r.viaGroup}"),
                )) }
            // An accepted request leaves the "sent" list: the chat takes over.
            _requests.update { rq -> rq.copy(sent = rq.sent.filter { it.id != requestId }) }
            recount()
        }
    }

    /** They block me (signed notice) or lift it. */
    fun simulateBlockedMe(did: String, blocked: Boolean) = updPerson(did) {
        it.copy(blockedMe = if (blocked) BlockNotice(now()) else null)
    }

    /** Someone I blocked tries to message me: counted quietly, never shown as a message. */
    fun simulateBlockedAttempt(did: String) { if (isBlocked(did)) updPerson(did) { it.copy(attemptsSinceBlock = it.attemptsSinceBlock + 1) } }

    fun simulateHostsOnline(groupId: String, online: Boolean) = updGroup(groupId) { g ->
        val n = now()
        g.copy(
            hosts = g.hosts.map { if (it.pending) it else it.copy(online = online, offlineSinceMs = if (online) null else n) },
            items = if (online) g.items.map { if (it is GroupItem.Msg && it.delivery == GroupDelivery.WaitingForHost) it.copy(delivery = GroupDelivery.Sent) else it } else g.items,
        )
    }

    /** A contact asks me to allow their calls. */
    fun simulateCallAsk(did: String) = updPerson(did) { it.addItem(DmItem.CallAsk(nid(), now(), outgoing = false)) }

    fun simulateLost(did: String) = updPerson(did) { it.copy(lost = true, hasMyCurrentAddress = false) }

    /** A new incoming request (Priya's again, if hers was answered). */
    fun simulateIncomingRequest() {
        if (_requests.value.incoming.any { it.state == IncomingRequestState.Pending }) return
        if (person(PRIYA) != null || isBlocked(PRIYA)) return
        _requests.update { r -> r.copy(incoming = r.incoming.filter { it.fromDid != PRIYA } +
            IncomingRequest(nid("in"), PRIYA, "Priya N", "Weekend hike", 1, "Hi, it’s Priya. Can I ask about the lift?", now())) }
    }
}
