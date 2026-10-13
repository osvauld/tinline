package com.osvauld.p2p

import androidx.compose.runtime.*
import kotlinx.coroutines.flow.MutableStateFlow

/** In-memory only, wiped by the secure gate. Reply survives navigation; edit never replaces the saved draft. */
class ConversationDraft {
    var text by mutableStateOf("")
    var reply by mutableStateOf<String?>(null)
    var edit by mutableStateOf<String?>(null)
    private var stash = ""
    fun beginEdit(id: String, body: String) {
        if (edit == null) stash = text
        edit = id; reply = null; text = body
    }
    fun cancel() {
        if (edit != null) text = stash
        stash = ""; edit = null; reply = null
    }
    fun beginReply(id: String) { cancelEdit(); reply = id }
    fun cancelEdit() { if (edit != null) { text = stash; stash = ""; edit = null } }
    fun sent() { if (edit != null) cancelEdit() else text = ""; reply = null }
}

object ConversationDrafts {
    private val entries = mutableMapOf<String, ConversationDraft>()
    /** Integration hook for the shell's draft previews (current account only). */
    val previews = MutableStateFlow<Map<String, String>>(emptyMap())
    fun forPeer(did: String) = entries.getOrPut(did) { ConversationDraft() }
    fun publish(did: String, draft: ConversationDraft) {
        // A disposed screen must not republish private data after account/secure-gate clearing.
        if (entries[did] !== draft) return
        if (draft.edit == null) previews.value = if (draft.text.isBlank()) previews.value - did else previews.value + (did to draft.text)
    }
    fun clear() { entries.clear(); previews.value = emptyMap() }
}
