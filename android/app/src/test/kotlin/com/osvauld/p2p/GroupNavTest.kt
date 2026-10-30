package com.osvauld.p2p

import org.junit.Assert.assertEquals
import org.junit.Test

/** GroupNav.open: a group post notification opens that group's conversation, never the Group info left in front. */
class GroupNavTest {
    private fun open(stack: MutableList<String>, gid: String) = GroupNav.open(stack, { it == "group:$gid" }) { "group:$gid" }

    @Test fun groupInfoLeftInFrontGivesWayToTheConversation() {
        val s = mutableListOf("home", "group:a", "info:a")
        open(s, "a")
        assertEquals(listOf("home", "group:a"), s)
    }

    @Test fun aChatOpenedFromGroupInfoAlsoGivesWay() {
        val s = mutableListOf("home", "group:a", "info:a", "chat:sam")
        open(s, "a")
        assertEquals(listOf("home", "group:a"), s)
    }

    @Test fun anotherGroupGoesOnTop() {
        val s = mutableListOf("home", "group:a", "info:a")
        open(s, "b")
        assertEquals(listOf("home", "group:a", "info:a", "group:b"), s)
    }

    @Test fun theConversationAlreadyInFrontStays() {
        val s = mutableListOf("home", "group:a")
        open(s, "a")
        assertEquals(listOf("home", "group:a"), s)
    }
}
