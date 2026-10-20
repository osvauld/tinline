package com.osvauld.p2p

import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.unit.em

/** Emoji in message text. Which messages are "big" and where the emoji runs are is `uniffi.present`
 *  (emojiIsBig, emojiRuns); this only draws them. */
object Emoji {
    /** `text` with every emoji run a little larger than the letters around it. */
    fun styled(text: String): AnnotatedString = buildAnnotatedString {
        val scale = uniffi.present.emojiInlineScale().em
        for (run in uniffi.present.emojiRuns(text)) {
            if (run.isEmoji) pushStyle(SpanStyle(fontSize = scale))
            append(run.text)
            if (run.isEmoji) pop()
        }
    }
}
