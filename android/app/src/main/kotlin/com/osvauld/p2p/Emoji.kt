package com.osvauld.p2p

import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.unit.em

/** Emoji in message text: a message of only 1-3 emojis shows big, emojis inside text a little
 *  larger than the letters (same rules as the desktop's `emoji.rs`). */
object Emoji {
    const val BIG_MAX = 3
    private const val INLINE_SCALE = 1.25f

    fun isEmoji(cp: Int) = cp in 0x1F000..0x1FAFF || cp in 0x2600..0x27BF || cp in 0x2300..0x23FF || cp in 0x2B00..0x2BFF

    /** Parts of an emoji that never stand alone: joiner, variation selectors, keycap, skin tones, tags. */
    private fun isPart(cp: Int) = cp == 0x200D || cp == 0xFE0E || cp == 0xFE0F || cp == 0x20E3 || cp in 0x1F3FB..0x1F3FF || cp in 0xE0020..0xE007F

    private fun isFlagHalf(cp: Int) = cp in 0x1F1E6..0x1F1FF

    /** How many emojis `text` holds if it is nothing but emojis (and spaces); null otherwise. */
    fun onlyCount(text: String): Int? {
        var n = 0
        var afterJoiner = false
        var openFlag = false
        var i = 0
        while (i < text.length) {
            val cp = text.codePointAt(i)
            i += Character.charCount(cp)
            when {
                Character.isWhitespace(cp) -> { afterJoiner = false; openFlag = false }
                isPart(cp) -> afterJoiner = cp == 0x200D
                isFlagHalf(cp) -> { if (!openFlag) n++; openFlag = !openFlag; afterJoiner = false }
                isEmoji(cp) -> { if (!afterJoiner) n++; afterJoiner = false; openFlag = false }
                else -> return null
            }
        }
        return n.takeIf { it > 0 }
    }

    fun isBig(text: String) = onlyCount(text)?.let { it <= BIG_MAX } == true

    /** `text` with every emoji run a little larger than the letters around it. */
    fun styled(text: String): AnnotatedString = buildAnnotatedString {
        var i = 0
        while (i < text.length) {
            val start = i
            val emoji = text.codePointAt(i).let { isEmoji(it) || isPart(it) }
            while (i < text.length) {
                val cp = text.codePointAt(i)
                if ((isEmoji(cp) || isPart(cp)) != emoji) break
                i += Character.charCount(cp)
            }
            if (emoji) pushStyle(SpanStyle(fontSize = INLINE_SCALE.em))
            append(text, start, i)
            if (emoji) pop()
        }
    }
}
