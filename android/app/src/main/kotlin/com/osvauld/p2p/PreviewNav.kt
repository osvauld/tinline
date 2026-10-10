package com.osvauld.p2p

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch

/** Screens of the PLACEHOLDER groups & privacy preview. */
sealed interface PRoute {
    data object Chats : PRoute
    data class GroupChat(val id: String) : PRoute
    data object NewGroup : PRoute
    data class GroupInfo(val id: String) : PRoute
    data class Hosts(val id: String) : PRoute
    data class RemovalDetails(val id: String) : PRoute
    data class Dm(val did: String) : PRoute
    data class Contact(val did: String) : PRoute
    data object Blocked : PRoute
    data object Address : PRoute

    companion object {
        /** Start screens by name, for Settings and the debug gallery (`--es pv hike`). */
        fun parse(key: String?): PRoute = when (key) {
            "hike", "group" -> GroupChat(FakeGroupsPrivacy.HIKE)
            "family", "waiting" -> GroupChat(FakeGroupsPrivacy.FAMILY)
            "book", "removed" -> GroupChat(FakeGroupsPrivacy.BOOK)
            "removal" -> RemovalDetails(FakeGroupsPrivacy.BOOK)
            "newgroup" -> NewGroup
            "info" -> GroupInfo(FakeGroupsPrivacy.HIKE)
            "hosts" -> Hosts(FakeGroupsPrivacy.HIKE)
            "arjun" -> Dm(FakeGroupsPrivacy.ARJUN)
            "meera", "blockedme" -> Dm(FakeGroupsPrivacy.MEERA)
            "sam", "lost", "invite" -> Dm(FakeGroupsPrivacy.SAM)
            "contact" -> Contact(FakeGroupsPrivacy.ARJUN)
            "blocked" -> Blocked
            "address" -> Address
            else -> Chats
        }
    }
}

/** What every preview screen can do besides reading the source. */
class PNav(
    val go: (PRoute) -> Unit,
    val back: () -> Unit,
    /** Replaces the top screen. */
    val replace: (PRoute) -> Unit,
    val say: (String) -> Unit,
    /** Leaves the preview to the real "My code" screen. */
    val showMyQr: () -> Unit,
    /** Leaves the preview to the real Add contact (scan) screen. */
    val addContact: () -> Unit,
    val exit: () -> Unit,
)

/**
 * The preview's own back stack, on top of the app's. Back pops inside the preview first, then [onExit].
 * Everything below renders [FakeGroupsPrivacy]; nothing reaches the Node.
 */
@Composable
fun PreviewApp(src: FakeGroupsPrivacy, start: PRoute, onExit: () -> Unit, onShowMyQr: () -> Unit, onAddContact: () -> Unit = onShowMyQr) {
    val stack = remember { mutableStateListOf(start) }
    val snack = remember { SnackbarHostState() }
    val scope = rememberCoroutineScope()
    val nav = remember {
        PNav(
            go = { stack.add(it) },
            back = { if (stack.size > 1) stack.removeAt(stack.lastIndex) else onExit() },
            replace = { r -> stack[stack.lastIndex] = r },
            say = { m -> scope.launch { snack.currentSnackbarData?.dismiss(); snack.showSnackbar(m) } },
            showMyQr = onShowMyQr,
            addContact = onAddContact,
            exit = onExit,
        )
    }
    BackHandler { nav.back() }
    Box(Modifier.fillMaxSize()) {
        when (val r = stack.last()) {
            PRoute.Chats -> PChatsScreen(src, nav)
            is PRoute.GroupChat -> PGroupChatScreen(src, nav, r.id)
            PRoute.NewGroup -> PNewGroupScreen(src, nav)
            is PRoute.GroupInfo -> PGroupInfoScreen(src, nav, r.id)
            is PRoute.Hosts -> PHostsScreen(src, nav, r.id)
            is PRoute.RemovalDetails -> PRemovalDetailsScreen(src, nav, r.id)
            is PRoute.Dm -> PDmScreen(src, nav, r.did)
            is PRoute.Contact -> PContactScreen(src, nav, r.did)
            PRoute.Blocked -> PBlockedScreen(src, nav)
            PRoute.Address -> PAddressScreen(src, nav)
        }
        SnackbarHost(snack, Modifier.align(Alignment.BottomCenter).navigationBarsPadding().padding(bottom = 80.dp))
    }
}
