package com.osvauld.p2p

import org.junit.Assert.*
import org.junit.Test
import androidx.compose.ui.unit.dp

class ScreenConversationTest {
    @Test fun spacingUsesRunPosition() {
        assertEquals(10.dp, gapAbove(BubblePos.Solo))
        assertEquals(10.dp, gapAbove(BubblePos.First))
        assertEquals(2.dp, gapAbove(BubblePos.Mid))
        assertEquals(2.dp, gapAbove(BubblePos.Last))
    }
    @Test fun editCancelAndSaveRestoreDraft() {
        val d = ConversationDraft(); d.text = "unsent"
        d.beginEdit("one", "original"); d.text = "changed"; d.cancel()
        assertEquals("unsent", d.text); assertNull(d.edit)
        d.beginEdit("two", "second"); d.sent()
        assertEquals("unsent", d.text); assertNull(d.edit)
    }
    @Test fun replySurvivesLeavingButEditDoesNot() {
        val d = ConversationDraft(); d.text = "unsent"; d.beginReply("reply")
        d.cancelEdit(); assertEquals("reply", d.reply); assertEquals("unsent", d.text)
        d.sent(); assertNull(d.reply); assertEquals("", d.text)
    }
    @Test fun changingEditTargetDoesNotOverwriteStash() {
        val d = ConversationDraft(); d.text = "unsent"
        d.beginEdit("one", "first"); d.beginEdit("two", "second"); d.cancelEdit()
        assertEquals("unsent", d.text)
    }
    @Test fun clearedDraftCannotBeRepublishedByDisposedScreen() {
        ConversationDrafts.clear()
        val stale = ConversationDrafts.forPeer("peer")
        stale.text = "private draft"
        ConversationDrafts.publish("peer", stale)
        assertEquals("private draft", ConversationDrafts.previews.value["peer"])
        ConversationDrafts.clear()
        ConversationDrafts.publish("peer", stale)
        assertTrue(ConversationDrafts.previews.value.isEmpty())
        assertEquals("", ConversationDrafts.forPeer("peer").text)
        ConversationDrafts.clear()
    }
    @Test fun highlightsRespectUnicodeAndPrefixes() {
        assertEquals(listOf(5..8), ChatFind.ranges("😀😀 café", "cafe"))
        assertTrue(ChatFind.ranges("landlord", "andlord").isEmpty())
        assertFalse(ChatFind.ranges("ഊണ്", "ഊണ്").isEmpty())
        assertTrue(ChatFind.ranges("ഊണ്", "ഊണി").isEmpty())
        assertEquals(listOf(1..2), ChatFind.ranges("会議は", "議は"))
    }
}
