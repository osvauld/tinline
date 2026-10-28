package com.osvauld.p2p

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Test

/** How a ring and a notice name a background account (docs/multi-online.md). Synthetic names and DIDs. */
class AccountLabelsTest {
    @Test fun theRingNamesTheAccountOnlyWhenItIsNotTheOpenOne() {
        assertEquals("Asha → Work", AccountLabels.ringName("Asha", "Work"))
        assertEquals("Asha", AccountLabels.ringName("Asha", null))
        assertEquals("Asha", AccountLabels.ringName("Asha", "  "))
        assertEquals("Asha → Work", AccountLabels.ringName("Asha", " Work "))
    }

    @Test fun aBackgroundMessageIsTitledWithTheSenderAndTheAccount() {
        assertEquals("Asha · Work", AccountLabels.messageTitle("Asha", "Work"))
        assertEquals("Asha", AccountLabels.messageTitle("Asha", ""))
    }

    @Test fun eachAccountHasItsOwnNoticeTag() {
        assertEquals("bg:did:key:zWork", AccountLabels.noticeTag("did:key:zWork"))
        assertNotEquals(AccountLabels.noticeTag("did:key:zWork"), AccountLabels.noticeTag("did:key:zHome"))
    }
}
