package com.osvauld.p2p

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.p2pcore.ChatRequestState
import uniffi.p2pcore.GroupChatRequest
import uniffi.p2pcore.GroupDetail
import uniffi.p2pcore.GroupFlag
import uniffi.p2pcore.GroupHost
import uniffi.p2pcore.GroupInvitation
import uniffi.p2pcore.GroupInvitationState
import uniffi.p2pcore.GroupInvitePeek
import uniffi.p2pcore.GroupItemKind
import uniffi.p2pcore.GroupMessage
import uniffi.p2pcore.GroupPage
import uniffi.p2pcore.GroupRemoval
import uniffi.p2pcore.GroupSummary
import uniffi.p2pcore.GroupDelivery as CoreDelivery
import uniffi.p2pcore.GroupMember as CoreMember
import uniffi.p2pcore.GroupRole as CoreRole

/** The group model and controller on a stand-in for the core (rules from docs/group-core.md). */
class GroupsCoreTest {
    private val me = "did:key:me"

    private fun msg(id: String, at: Long, author: String = "did:key:sam", name: String = "Sam Rao", kind: GroupItemKind = GroupItemKind.POST, text: String = "hi",
                    delivery: CoreDelivery = CoreDelivery.DELIVERED, gid: String = "g") =
        GroupMessage(id, gid, kind, author, name, author, name, at.toULong(), text, null, author == me, delivery, false)

    private fun member(did: String, owner: Boolean = false, admin: Boolean = owner, flags: Int = 0, devs: List<String> = listOf("dev-$did")) =
        CoreMember(did, did.removePrefix("did:key:"), did == me, owner, admin, false, flags.toUInt(), 1uL, null, devs)

    private fun detail(role: CoreRole, members: List<CoreMember>, hosts: List<GroupHost> = emptyList(), amHost: Boolean = false, removal: GroupRemoval? = null,
                       hostRequests: List<GroupHost> = emptyList()) =
        GroupDetail("g", "Hike", members.first { it.isOwner }.did, role, amHost, members, hosts, emptyList(), hostRequests, hostRequests.any { it.isMe }, removal)

    private fun summary(id: String, role: CoreRole, unread: Int, last: GroupMessage?) =
        GroupSummary(id, "G $id", role, 3u, 1u, false, true, last, unread.toUInt())

    private class FakeApi : GroupApi {
        var list = mutableListOf<GroupSummary>()
        val details = mutableMapOf<String, GroupDetail>()
        val msgs = mutableMapOf<String, MutableList<GroupMessage>>()
        val marked = mutableListOf<String>()
        var postError: Throwable? = null
        var locked = false
        override fun groups(): List<GroupSummary> { if (locked) throw uniffi.p2pcore.Exception.Locked("identity is locked"); return list.toList() }
        override fun detail(id: String) = details[id] ?: throw uniffi.p2pcore.Exception.NotFound("not found")
        override fun messages(id: String, before: String?, limit: Int): GroupPage {
            val all = msgs[id].orEmpty()
            val end = before?.let { b -> all.indexOfFirst { it.id == b } } ?: all.size
            val start = maxOf(0, end - limit)
            return GroupPage(all.subList(start, end).toList(), if (start > 0) all[start].id else null)
        }
        override fun markRead(id: String) { marked += id }
        override fun create(name: String) = "new"
        override fun invite(id: String) = "tinline://g/$id"
        override fun peek(token: String) = if (token.startsWith("tinline://g/")) GroupInvitePeek("g", "Hike", "did:key:sam", "did:key:sam", 1uL, 1u) else null
        override fun join(token: String): String = throw uniffi.p2pcore.Exception.Rejected("they refused: invite expired or invalid")
        override fun post(id: String, text: String, replyTo: String?): GroupMessage {
            postError?.let { throw it }
            val m = GroupMessage("p${msgs[id]!!.size}", id, GroupItemKind.POST, "did:key:me", "Me", "did:key:me", "Me", 9999uL, text, replyTo, true, CoreDelivery.WAITING, false)
            msgs[id]!!.add(m); return m
        }
        override fun flag(id: String, did: String) = throw uniffi.p2pcore.Exception.Rejected("they refused: only admins can do that")
        override fun setAdmin(id: String, did: String, admin: Boolean) {}
        override fun leave(id: String) {}
        override fun appointHost(id: String, did: String, deviceId: String) {}
        override fun revokeHost(id: String, did: String, deviceId: String) {}
        override fun acceptHost(id: String, accept: Boolean) {}
        override fun deviceId() = "aa".repeat(32)
        val reqs = mutableListOf<GroupChatRequest>()
        val blocked = mutableListOf<String>()
        override fun chatRequest(id: String, did: String): GroupChatRequest {
            if (did == "did:key:contact") throw uniffi.p2pcore.Exception.Rejected("they refused: already a contact")
            return GroupChatRequest("r${reqs.size}", id, "Hike", did, "Leo B", true, ChatRequestState.PENDING, 1uL, false).also { reqs += it }
        }
        override fun requestsIncoming() = reqs.filter { !it.outgoing }
        override fun requestsOutgoing() = reqs.filter { it.outgoing }
        private fun answer(id: String, st: ChatRequestState): GroupChatRequest {
            val i = reqs.indexOfFirst { it.id == id && !it.outgoing }
            if (i < 0) throw uniffi.p2pcore.Exception.NotFound("not found")
            return reqs[i].copy(state = st).also { reqs[i] = it }
        }
        override fun chatAccept(requestId: String) = answer(requestId, ChatRequestState.ACCEPTED)
        override fun chatDecline(requestId: String) = answer(requestId, ChatRequestState.DECLINED)
        override fun block(did: String) { blocked += did }
        val invs = mutableListOf<GroupInvitation>()
        override fun inviteContacts(id: String, dids: List<String>): List<GroupInvitation> {
            if (dids.any { it in blocked }) throw uniffi.p2pcore.Exception.Rejected("they refused: you block them")
            return dids.map { d -> GroupInvitation("i${invs.size}", id, "Hike", me, "Me", d, d.removePrefix("did:key:").replaceFirstChar { it.uppercase() } + " K", true,
                GroupInvitationState.PENDING, 1uL, 9uL, false).also { invs += it } }
        }
        override fun invitationsIncoming() = invs.filter { !it.outgoing }
        override fun invitationsOutgoing(id: String) = invs.filter { it.outgoing && it.groupId == id }
        private fun answerInv(id: String, st: GroupInvitationState): GroupInvitation {
            val i = invs.indexOfFirst { it.id == id }
            if (invs[i].state != GroupInvitationState.PENDING) throw uniffi.p2pcore.Exception.Rejected("they refused: already answered")
            return invs[i].copy(state = st).also { invs[i] = it }
        }
        override fun invitationAccept(invitationId: String) = answerInv(invitationId, GroupInvitationState.JOINED)
        override fun invitationDecline(invitationId: String) = answerInv(invitationId, GroupInvitationState.DECLINED)
        private val me = "did:key:me"
    }

    private fun inv(id: String, state: GroupInvitationState, outgoing: Boolean = false, invitee: String = me, delivered: Boolean = true) =
        GroupInvitation(id, "g", "Hike", if (outgoing) me else "did:key:sam", if (outgoing) "Me" else "Sam Rao", invitee, "Leo B", outgoing, state, 1uL, 9uL, delivered)

    @Test fun invitationsCountOnTheBadgeUntilAnswered() {
        val a = summary("a", CoreRole.MEMBER, 2, null)
        val list = listOf(inv("1", GroupInvitationState.PENDING), inv("2", GroupInvitationState.DECLINED), inv("3", GroupInvitationState.PENDING, outgoing = true))
        assertEquals(3, GroupText.badge(listOf(a), list))
        assertEquals(listOf("1"), GroupText.pendingInvitations(list).map { it.id })
    }

    @Test fun thePickerHidesBlockedContactsAndShowsMembersAndPendingInvites() {
        val d = detail(CoreRole.OWNER, listOf(member(me, owner = true), member("did:key:sam")))
        val contacts = listOf("sam", "leo", "priya", "ravi", "zoe").map { InviteCandidate("did:key:$it", it.replaceFirstChar { c -> c.uppercase() }) }
        val sent = listOf(inv("1", GroupInvitationState.PENDING, outgoing = true, invitee = "did:key:leo"), inv("2", GroupInvitationState.DECLINED, outgoing = true, invitee = "did:key:zoe"))
        val rows = GroupText.pickRows(contacts, d, sent, { it == "did:key:priya" }, { it == "did:key:ravi" })
        assertEquals(listOf("did:key:zoe", "did:key:leo", "did:key:sam"), rows.map { it.did })
        assertNull(rows[0].why) // declined before: can be invited again
        assertEquals("Invited · waiting", rows[1].why)
        assertEquals("Already a member", rows[2].why)
        assertEquals("Declined" to true, GroupText.sentInvitationLine(sent[1]))
        assertEquals("Invited · not delivered yet" to false, GroupText.sentInvitationLine(sent[0].copy(delivered = false)))
        assertEquals("Joined" to false, GroupText.sentInvitationLine(sent[0].copy(state = GroupInvitationState.JOINED)))
    }

    @Test fun invitingContactsAndAnsweringAnInvitation() {
        val api = FakeApi()
        val ctl = GroupsController(api) { me }
        assertEquals("Pick someone first.", ctl.inviteContacts("g", emptyList()).exceptionOrNull()!!.message)
        assertEquals("Invitation sent to Leo. They join when they accept.", ctl.inviteContacts("g", listOf("did:key:leo")).getOrThrow())
        assertEquals("2 invitations sent. Each person joins when they accept.", ctl.inviteContacts("g", listOf("did:key:a", "did:key:b")).getOrThrow())
        api.blocked += "did:key:x"
        assertTrue(ctl.inviteContacts("g", listOf("did:key:x")).isFailure)
        api.invs += inv("in1", GroupInvitationState.PENDING)
        api.invs += inv("in2", GroupInvitationState.PENDING)
        ctl.onGroupInvitation(api.invs.last())
        assertEquals(2, GroupText.pendingInvitations(ctl.invitations.value).size)
        assertEquals("g", ctl.acceptInvitation("in1").getOrThrow())
        assertTrue(ctl.declineInvitation("in2").getOrThrow().startsWith("Declined. Sam sees"))
        assertTrue(GroupText.pendingInvitations(ctl.invitations.value).isEmpty())
        assertEquals("Already answered.", ctl.declineInvitation("in2").exceptionOrNull()!!.message)
    }

    @Test fun groupLinksAreRecognised() {
        assertTrue(GroupLinks.isLink("tinline://g/3xK9abc"))
        assertTrue(GroupLinks.isLink(" TINLINE://G/3xK9abc "))
        assertFalse(GroupLinks.isLink("tinline://g/"))
        assertFalse(GroupLinks.isLink("tlgroup1:AAAA"))
        assertFalse(GroupLinks.isLink(null))
    }

    @Test fun previewsAndBadgeSkipGroupsWeLeftOrWereRemovedFrom() {
        val a = summary("a", CoreRole.MEMBER, 2, msg("1", 1, text = "anyone?"))
        val b = summary("b", CoreRole.REMOVED, 5, msg("2", 2))
        val c = summary("c", CoreRole.ADMIN, 1, msg("3", 3, author = me, text = "on my way"))
        assertEquals("Sam: anyone?", GroupText.preview(a, me))
        assertEquals("You were removed from this group", GroupText.preview(b, me))
        assertEquals("You: on my way", GroupText.preview(c, me))
        assertEquals(3, GroupText.badge(listOf(a, b, c)))
        val joined = msg("4", 4, kind = GroupItemKind.JOINED)
        assertEquals("Sam Rao joined", GroupText.preview(summary("d", CoreRole.MEMBER, 0, joined), me))
    }

    @Test fun systemLinesSayYouAndNeverShowKeys() {
        val m = GroupMessage("x", "g", GroupItemKind.ADMIN_GRANTED, "did:key:sam", "Sam", me, "Me", 1uL, "", null, false, CoreDelivery.DELIVERED, false)
        assertEquals("Sam made you an admin", GroupText.sysText(m, me))
        assertEquals("Priya was removed after 3 admin flags", GroupText.sysText(m.copy(kind = GroupItemKind.REMOVED, subjectDid = "did:key:p", subjectName = "Priya"), me))
        assertEquals("Someone left", GroupText.sysText(m.copy(kind = GroupItemKind.LEFT, subjectDid = "did:key:x", subjectName = ""), me))
    }

    @Test fun memberActionsFollowRoles() {
        val owner = member("did:key:sam", owner = true)
        val priya = member("did:key:priya")
        val mine = member(me)
        val asOwner = detail(CoreRole.OWNER, listOf(mine.copy(isOwner = true, isAdmin = true), priya.copy(isAdmin = false)).let { it + member("did:key:x") })
            .copy(ownerDid = me)
        val a = GroupText.actions(asOwner, priya)
        assertEquals(MemberAction.Message, a[0])
        assertEquals(MemberAction.MakeAdmin, a[1])
        assertTrue(MemberAction.Flag in a)
        assertTrue(a.any { it is MemberAction.Appoint })
        assertTrue(GroupText.actions(asOwner, asOwner.members.first { it.isMe }).isEmpty())

        val host = GroupHost("did:key:sam", "Sam", "dev-did:key:sam", null, false, true)
        val asAdmin = detail(CoreRole.ADMIN, listOf(owner, mine, priya), hosts = listOf(host))
        assertFalse(MemberAction.MakeAdmin in GroupText.actions(asAdmin, priya))
        assertFalse(MemberAction.Flag in GroupText.actions(asAdmin, owner))
        assertTrue(MemberAction.Revoke("dev-did:key:sam") in GroupText.actions(asAdmin, owner))

        assertEquals(listOf(MemberAction.Message), GroupText.actions(detail(CoreRole.MEMBER, listOf(owner, mine, priya)), priya))
        assertTrue(GroupText.actions(detail(CoreRole.REMOVED, listOf(owner, mine, priya)), priya).isEmpty())
    }

    @Test fun theFlagDialogSaysWhichFlagOursIs() {
        assertEquals(1, GroupText.flagCopy("Dev Patel", 0).first)
        val (n, text) = GroupText.flagCopy("Dev Patel", 2)
        assertEquals(3, n)
        assertTrue(text.contains("Dev is removed right away"))
    }

    @Test fun statusAndWaitingForAHost() {
        val ms = listOf(member("did:key:sam", owner = true), member(me))
        assertEquals("2 members · no host yet", GroupText.status(detail(CoreRole.MEMBER, ms)))
        val off = GroupHost("did:key:sam", "Sam", "d", null, false, false)
        val d = detail(CoreRole.MEMBER, ms, hosts = listOf(off))
        assertEquals("2 members · waiting for a host", GroupText.status(d))
        assertTrue(GroupText.waitingForHost(d))
        assertEquals("2 members · this device hosts the group", GroupText.status(d.copy(amHost = true)))
        val removed = d.copy(myRole = CoreRole.REMOVED, removal = GroupRemoval(5uL, listOf(GroupFlag("did:key:a", "A", 1uL))))
        assertEquals("You were removed from this group", GroupText.status(removed))
        assertFalse(GroupText.waitingForHost(removed))
        assertEquals("Waiting for a host", GroupText.deliveryWord(msg("x", 1, author = me, delivery = CoreDelivery.WAITING)))
        assertNull(GroupText.deliveryWord(msg("x", 1, author = me)))
    }

    @Test fun errorsReadPlainly() {
        assertEquals("You were removed from this group.", GroupText.error(uniffi.p2pcore.Exception.Rejected("they refused: removed")))
        assertEquals("The owner cannot leave.", GroupText.error(uniffi.p2pcore.Exception.Rejected("they refused: the owner cannot leave")))
        assertEquals("No host answered in time. Try again when a host is online.", GroupText.error(uniffi.p2pcore.Exception.Timeout("timed out")))
    }

    @Test fun upsertKeepsTimeOrderAndReplacesById() {
        var l = listOf(msg("b", 2), msg("d", 4))
        l = upsertGroupMessage(l, msg("c", 3))
        l = upsertGroupMessage(l, msg("a", 1))
        l = upsertGroupMessage(l, msg("e", 5))
        l = upsertGroupMessage(l, msg("c", 3, text = "edited delivery"))
        assertEquals(listOf("a", "b", "c", "d", "e"), l.map { it.id })
        assertEquals("edited delivery", l[2].text)
    }

    @Test fun theControllerOpensMarksReadAndTakesLiveMessages() {
        val api = FakeApi()
        api.list += summary("g", CoreRole.MEMBER, 1, msg("1", 1))
        api.details["g"] = detail(CoreRole.MEMBER, listOf(member("did:key:sam", owner = true), member(me)))
        api.msgs["g"] = mutableListOf(msg("1", 10))
        val ctl = GroupsController(api) { me }
        ctl.refresh()
        assertEquals(1, ctl.list.value.size)
        assertEquals("aa".repeat(32), ctl.myDevice)
        ctl.open("g")
        assertEquals(listOf("g"), api.marked)
        assertEquals(1, ctl.open.value!!.messages.size)
        ctl.onGroupMessage(msg("0", 5))
        ctl.onGroupMessage(msg("2", 20))
        ctl.onGroupMessage(msg("x", 30, gid = "other"))
        assertEquals(listOf("0", "1", "2"), ctl.open.value!!.messages.map { it.id })

        // Our post shows at once, waiting for a host; a refusal comes back in plain words.
        val r = ctl.post("g", "  on my way ", null)
        assertEquals("on my way", r.getOrThrow().text)
        assertEquals("Waiting for a host", GroupText.deliveryWord(ctl.open.value!!.messages.last()))
        api.postError = uniffi.p2pcore.Exception.Rejected("they refused: removed")
        assertEquals("You were removed from this group.", ctl.post("g", "still here?", null).exceptionOrNull()!!.message)
        assertTrue(ctl.post("g", "   ", null).isFailure)

        assertEquals("Only admins can do that.", ctl.flag("g", "did:key:sam").exceptionOrNull()!!.message)
        assertEquals("Invite expired or invalid.", ctl.join("tinline://g/x").exceptionOrNull()!!.message)
        assertEquals("tinline://g/g", ctl.invite("g").getOrThrow())
        assertNull(ctl.peek("garbage"))
        assertEquals("Hike", ctl.peek(" tinline://g/x ")!!.name)

        ctl.close("g")
        assertNull(ctl.open.value)
    }

    @Test fun olderPagesGoInFrontAndALockedNodeShowsNoGroups() {
        val api = FakeApi()
        api.list += summary("g", CoreRole.MEMBER, 0, null)
        api.details["g"] = detail(CoreRole.MEMBER, listOf(member("did:key:sam", owner = true), member(me)))
        api.msgs["g"] = (0 until GroupText.PAGE + 3).map { msg("m%03d".format(it), it.toLong()) }.toMutableList()
        val ctl = GroupsController(api) { me }
        ctl.open("g")
        assertEquals(GroupText.PAGE, ctl.open.value!!.messages.size)
        ctl.older()
        assertEquals(GroupText.PAGE + 3, ctl.open.value!!.messages.size)
        assertEquals("m000", ctl.open.value!!.messages.first().id)
        assertNull(ctl.open.value!!.older)
        api.locked = true
        ctl.refresh()
        assertTrue(ctl.list.value.isEmpty())
    }

    @Test fun chatRequestsFromAGroupMember() {
        val api = FakeApi()
        val ctl = GroupsController(api) { me }
        assertEquals("Request sent to Leo. It waits in Chats until they answer.", ctl.request("g", "did:key:leo").getOrThrow())
        assertEquals(1, ctl.outgoing.value.size)
        assertEquals("Waiting for Leo · via Hike" to false, GroupText.sentLine(ctl.outgoing.value[0]))
        assertEquals("Leo declined" to true, GroupText.sentLine(ctl.outgoing.value[0].copy(state = ChatRequestState.DECLINED)))
        assertEquals("Leo blocked you" to true, GroupText.sentLine(ctl.outgoing.value[0].copy(state = ChatRequestState.BLOCKED)))
        assertEquals("You already have them as a contact. Message them from Chats.", ctl.request("g", "did:key:contact").exceptionOrNull()!!.message)

        // Their answer arrives as an event: one notice when the contact is in place.
        ctl.onChatRequest(ctl.outgoing.value[0].copy(state = ChatRequestState.ACCEPTED, contactAdded = true))
        assertTrue(ctl.notice.value!!.startsWith("Leo accepted your request"))
        ctl.clearNotice()
        assertNull(ctl.notice.value)

        api.reqs += GroupChatRequest("in1", "g", "Hike", "did:key:priya", "Priya N", false, ChatRequestState.PENDING, 2uL, false)
        api.reqs += GroupChatRequest("in2", "g", "Hike", "did:key:ravi", "Ravi J", false, ChatRequestState.PENDING, 3uL, false)
        ctl.refresh()
        assertEquals(2, ctl.incoming.value.size)
        assertTrue(ctl.accept("in1").getOrThrow().contains("messages only"))
        assertTrue(ctl.blockRequester("in2", "did:key:ravi").isSuccess)
        assertEquals(listOf("did:key:ravi"), api.blocked)
        assertTrue(ctl.incoming.value.none { it.state == ChatRequestState.PENDING })
    }

    @Test fun anAdminAskingThisDeviceToHostShowsWhoAsked() {
        val ms = listOf(member("did:key:sam", owner = true), member(me))
        assertNull(GroupText.hostAsker(detail(CoreRole.MEMBER, ms)))
        val ask = GroupHost(me, "Me", "aa", "did:key:sam", true, true)
        assertEquals("sam", GroupText.hostAsker(detail(CoreRole.MEMBER, ms, hostRequests = listOf(ask))))
        assertNull(GroupText.hostAsker(detail(CoreRole.REMOVED, ms, hostRequests = listOf(ask))))
    }
}
