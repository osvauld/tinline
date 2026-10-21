import SwiftUI

/*
 * Contact blocking on the core's real API (Node.block / unblock / blockState / blockedContacts / notDeliveredReason;
 * crates/core/src/block.rs). Decisions (rotation-open-decisions.md section 0): blocking is visible to the blocked person (U5
 * revised); a contact's action is Block, there is no "Remove contact" (U19); every block asks the other app to delete its
 * contact entry for us, so after an unblock they must add us again (U20); chats are never deleted (U24). Groups are not touched.
 *
 * The model below is plain Swift (no uniffi calls) so unit tests can run it; `PeopleModel` fills it from the Node. The shared
 * words (`ComposerState`, `CallGate` and the block copy) are the generated `present` module's, as on Android.
 */

/// One conversation's block state. Times are unix seconds.
struct BlockView: Equatable {
    var blockedByMe = false
    var blockedAtSecs: Int64? = nil
    var blockedMeAtSecs: Int64? = nil
}

/// A row of Settings > Blocked people.
struct BlockedRow: Equatable {
    let did: String
    let name: String
    let blockedAtSecs: Int64
}

struct Blocks: Equatable {
    /// Only conversations with a block on either side.
    var states: [String: BlockView] = [:]
    /// Newest first; one list across all our devices.
    var list: [BlockedRow] = []
    /// Contacts we let call us ("Allow calls" on; P6 Node.callsAllowed).
    var allowed: Set<String> = []
    /// Contacts whose call token we hold: they let us call them (P6 Node.canCall).
    var tokens: Set<String> = []

    func of(_ did: String) -> BlockView {
        if let s = states[did] { return s }
        if let row = list.first(where: { $0.did == did }) { return BlockView(blockedByMe: true, blockedAtSecs: row.blockedAtSecs) }
        return BlockView()
    }
    func blockedByMe(_ did: String) -> Bool { of(did).blockedByMe }
    func blockedMeAt(_ did: String) -> Int64? { of(did).blockedMeAtSecs }

    /// `contact`: they are in our contacts (or it is the "You" conversation).
    func composer(_ did: String, contact: Bool) -> ComposerState {
        if blockedByMe(did) { return .blockedByMe }
        if blockedMeAt(did) != nil { return .blockedMe }
        if !contact { return .notContact }
        return .open
    }

    /// Closed while either side blocks or they are not a contact; notAllowed when they let us message, not call.
    func callGate(_ did: String, contact: Bool) -> CallGate {
        if composer(did, contact: contact) != .open { return .closed }
        if !tokens.contains(did) { return .notAllowed }
        return .ok
    }

    func canCall(_ did: String, contact: Bool) -> Bool { callGate(did, contact: contact) == .ok }

    /// The "Allow calls" switch: whether we let them call us.
    func callsAllowed(_ did: String) -> Bool { allowed.contains(did) }

    /// The red line under the name in lists and headers.
    func status(_ did: String) -> String? {
        switch composer(did, contact: true) {
        case .blockedByMe: return "Blocked"
        case .blockedMe: return "Blocked you"
        default: return nil
        }
    }

    /// Builds the state from per-DID reads; DIDs with no block on either side are dropped.
    static func from(_ views: [String: BlockView], _ list: [BlockedRow], allowed: Set<String> = [], tokens: Set<String> = []) -> Blocks {
        Blocks(states: views.filter { $0.value.blockedByMe || $0.value.blockedMeAtSecs != nil },
               list: list.sorted { $0.blockedAtSecs > $1.blockedAtSecs }, allowed: allowed, tokens: tokens)
    }
}

/// What a conversation needs to know about blocking.
struct ConvBlock: Equatable {
    var view: BlockView
    var composer: ComposerState
    var callGate: CallGate = .ok
}

/// The words of blocking are the `present` module's (block*, cantSendText, notContactLine, unblockNote...).
enum BlockCopy {
    /// Why the call button is closed (`ComposerState` mapped to the shared words).
    static func cantCall(name: String, state: ComposerState) -> String { cantCallText(name: name, composer: state) }

    static func date(_ secs: Int64, zone: TimeZone = .current) -> String {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.timeZone = zone
        f.dateFormat = "MMM d, HH:mm"
        return f.string(from: Date(timeIntervalSince1970: TimeInterval(secs)))
    }
}

/// "Block X?" with every consequence listed before the tap.
struct BlockDialog: View {
    @Environment(\.tin) private var c
    let name: String
    let onDismiss: () -> Void
    let onBlock: () -> Void

    var body: some View {
        PeopleDialogBody(title: blockDialogTitle(name: name)) {
            VStack(alignment: .leading, spacing: 0) {
                ForEach(blockDialogLines(name: name), id: \.self) { PeopleDialogText("\u{2022} \($0)") }
            }
            PeopleActs {
                PeopleButton("Cancel", variant: .quiet, action: onDismiss)
                PeopleButton("Block", variant: .danger) { onDismiss(); onBlock() }
            }
        }
    }
}

/// Settings > Blocked people: everyone we block (real data), with Unblock.
struct BlockedPeopleScreen: View {
    @EnvironmentObject private var people: PeopleModel
    @Environment(\.tin) private var c
    let onBack: () -> Void
    let say: (String) -> Void

    var body: some View {
        PeoplePage {
            PeopleTopBar(title: "Blocked people", onBack: onBack)
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    PeopleHint("One list across all your devices: a block made on another device shows here too.")
                        .padding(.horizontal, 20).padding(.vertical, 8)
                    if people.blocks.list.isEmpty {
                        PeopleHint("Nobody is blocked. To block someone, open their contact page and tap Block.")
                            .padding(.horizontal, 20).padding(.vertical, 12)
                    }
                    ForEach(people.blocks.list, id: \.did) { b in
                        let name = b.name.trimmingCharacters(in: .whitespaces).isEmpty ? "Someone" : b.name
                        HStack(spacing: 14) {
                            TinAvatar(did: b.did, size: 44)
                            VStack(alignment: .leading, spacing: 0) {
                                Text(name).peopleStyle(.bodyB, lineHeight: 22).foregroundStyle(c.ink).lineLimit(1)
                                PeopleHint("Blocked \(BlockCopy.date(b.blockedAtSecs))")
                            }
                            Spacer(minLength: 0)
                            Button {
                                people.unblock(b.did)
                                say("\(peopleFirstName(name)) is told you unblocked them")
                            } label: {
                                Text("Unblock").peopleStyle(.btn).foregroundStyle(c.pr)
                                    .padding(.horizontal, 12).frame(minHeight: 48)
                            }
                            .buttonStyle(PeoplePressStyle())
                            .accessibilityLabel("Unblock \(name)")
                        }
                        .frame(minHeight: 72).padding(.leading, 20).padding(.trailing, 8)
                    }
                    VStack(alignment: .leading, spacing: 8) {
                        Text("Unblocking").peopleStyle(.bodyB).foregroundStyle(c.ink)
                        Text(unblockNote()).peopleStyle(.sub19).foregroundStyle(c.ink2)
                    }
                    .padding(20).frame(maxWidth: .infinity, alignment: .leading)
                }
            }
        }
    }
}
