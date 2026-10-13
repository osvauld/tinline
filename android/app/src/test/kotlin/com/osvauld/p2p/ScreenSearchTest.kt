package com.osvauld.p2p

import androidx.compose.ui.graphics.Color
import org.junit.Assert.*
import org.junit.Test
import uniffi.p2pcore.HitRange

class ScreenSearchTest {
    @Test fun codePointRangesBecomeUtf16() {
        val text = "😀😀 hello zebra"
        val ranges = hitRanges(text, listOf(HitRange(9u, 14u)))
        assertEquals(listOf(11 until 16), ranges)
        val shown = marked(text, ranges, Color.Black)
        assertEquals("zebra", shown.text.substring(shown.spanStyles.single().start, shown.spanStyles.single().end))
    }

    @Test fun longSnippetKeepsMatchInView() {
        val text = "a long introductory sentence with many words before needle and after"
        val start = text.indexOf("needle")
        val shown = marked(text, listOf(start until start + 6), Color.Black, lead = 18)
        assertTrue(shown.text.startsWith("…"))
        assertTrue(shown.spanStyles.single().start <= 19)
        assertEquals("needle", shown.text.substring(shown.spanStyles.single().start, shown.spanStyles.single().end))
    }

    @Test fun malformedRangesAreClampedAndPlainTextSurvives() {
        assertEquals("hello", marked("hello", emptyList(), Color.Black).text)
        assertEquals("hello", marked("hello", listOf(-2..99), Color.Black).text)
    }
}
