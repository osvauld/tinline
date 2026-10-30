package com.osvauld.p2p

import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.dp

/**
 * When a chat thread follows its newest message. Both threads (1:1 and group) are reversed lists: the newest row is item 0
 * and sits at the bottom, so a list that gets shorter (the keyboard rises, the composer grows a line, a reply chip appears)
 * keeps its bottom row where it is. These rules cover the rest.
 */
object ChatFollow {
    /** The reader is at the newest message: the bottom row is the newest, at most [slackPx] scrolled past its end. */
    fun atNewest(firstIndex: Int, firstOffset: Int, slackPx: Int): Boolean = firstIndex == 0 && firstOffset <= slackPx

    /**
     * The list just got shorter (from [before] to [after] px) while the reader was at the newest message, and the newest
     * is no longer exactly at the bottom: hold it there. A reader up in the history is never moved.
     */
    fun holdOnShrink(before: Int, after: Int, wasAtNewest: Boolean, firstIndex: Int, firstOffset: Int): Boolean =
        after < before && wasAtNewest && (firstIndex != 0 || firstOffset != 0)

    /** A new newest message arrived: follow my own; follow theirs only when the reader is reading the last few rows. */
    fun followNew(mine: Boolean, firstIndex: Int, theirs: Boolean): Boolean = mine || (theirs && firstIndex <= 2)
}

/**
 * The newest message stays above the composer for a reader who is there. [state] belongs to a reversed thread list.
 * [newestKey] is the newest row's key: when it changes the list follows it if [ChatFollow.followNew] says so. [enabled]
 * false (find open) turns both off; [followTheirs] false (a search jump in progress) only stops following their messages.
 */
@Composable
fun FollowNewest(state: LazyListState, newestKey: Any?, mine: Boolean, enabled: Boolean = true, followTheirs: Boolean = true) {
    val slack = with(LocalDensity.current) { 48.dp.roundToPx() }
    val on = rememberUpdatedState(enabled)
    LaunchedEffect(state, slack) {
        var height = -1
        var wasAtNewest = true
        snapshotFlow { Triple(state.layoutInfo.viewportSize.height, state.firstVisibleItemIndex, state.firstVisibleItemScrollOffset) }
            .collect { (h, index, offset) ->
                if (height > 0 && on.value && ChatFollow.holdOnShrink(height, h, wasAtNewest, index, offset)) state.requestScrollToItem(0)
                else wasAtNewest = ChatFollow.atNewest(index, offset, slack)
                if (h > 0) height = h
            }
    }
    LaunchedEffect(newestKey) {
        if (newestKey != null && enabled && ChatFollow.followNew(mine, state.firstVisibleItemIndex, followTheirs)) state.requestScrollToItem(0)
    }
}
