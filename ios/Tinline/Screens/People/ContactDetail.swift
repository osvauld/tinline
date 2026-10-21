import SwiftUI

/// One call in a contact's history, already worded (the Calls area fills it; android `RecentCall`).
struct PeopleCallRecord: Identifiable, Equatable {
    enum Kind { case incoming, missed, unreached, outgoing }
    let id: String
    var kind: Kind
    var title: String
    var whenLong: String
    /// true = direct, false = relayed, nil = unknown.
    var direct: Bool?
}

/// Contact page: name (alias first), Call / Rename / Block, and this contact's call history.
/// U19: a contact's action is Block (kept contact, kept chat, Unblock available); there is no Remove.
/// Public root: `ContactDetailView(did:)`. Everything else is wired by the Shell through the optional actions; the page reads
/// contacts and block state from `PeopleModel`.
struct ContactDetailView: View {
    @EnvironmentObject private var people: PeopleModel
    @Environment(\.tin) private var c
    let did: String
    var history: [PeopleCallRecord] = []
    var onBack: () -> Void = {}
    var onCall: () -> Void = {}
    var onMessage: (() -> Void)? = nil

    @State private var confirmBlock = false
    @State private var renaming = false

    var body: some View {
        let contact = people.contacts.first { $0.did == did }
        let name = contact?.name ?? ""
        let alias = contact?.alias.flatMap { $0.trimmingCharacters(in: .whitespaces).isEmpty ? nil : $0 }
        let block = people.blocks.of(did)
        let blockedMe = block.blockedMeAtSecs != nil
        let gate = people.blocks.callGate(did, contact: contact != nil)
        let closed = block.blockedByMe || blockedMe || gate == .closed
        let canCall = !closed && gate == .ok
        let shown = displayName(alias: alias, name: name)
        let callsAllowed: Bool? = contact == nil ? nil : people.blocks.callsAllowed(did)

        PeoplePage {
            PeopleTopBar(onBack: onBack)
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    VStack(spacing: 8) {
                        TinAvatar(did: did, size: 96)
                        Text(shown).font(TinType.h1).foregroundStyle(c.ink).multilineTextAlignment(.center).padding(.top, 6)
                        if (alias != nil && alias != name) || block.blockedByMe || blockedMe {
                            HStack(spacing: 8) {
                                if alias != nil && alias != name { Text("Calls themselves \u{201C}\(name)\u{201D}").font(TinType.bodyM).foregroundStyle(c.ink2) }
                                if block.blockedByMe || blockedMe {
                                    Text(block.blockedByMe ? "Blocked" : "Blocked you").font(TinType.caption).fontWeight(.semibold).foregroundStyle(c.er)
                                }
                            }
                        }
                        PeopleButton("Call", variant: .primary, icon: canCall ? .phone : .callend, enabled: !closed, minWidth: 140, height: 48, action: onCall).padding(.top, 14)
                        HStack(spacing: 12) {
                            if let onMessage { PeopleActionChip(icon: .chat, label: "Message", tint: c.pr, action: onMessage) }
                            PeopleActionChip(icon: .edit, label: "Rename", tint: c.pr) { renaming = true }
                            if block.blockedByMe { PeopleActionChip(icon: .off, label: "Unblock", tint: c.pr) { people.unblock(did) } }
                            else { PeopleActionChip(icon: .off, label: "Block", tint: c.er) { confirmBlock = true } }
                        }
                        .padding(.top, 10)
                    }
                    .padding(.horizontal, 24).frame(maxWidth: .infinity)

                    if let callsAllowed {
                        let f = peopleFirstName(shown)
                        PeopleSectionLabel("WHAT \(f.uppercased()) CAN DO").padding(.top, 8)
                        PeopleListItem(title: "Allow calls", icon: .phone,
                                       sub: block.blockedByMe ? "Blocked: nothing gets through until you unblock."
                                           : callsAllowed ? "\(f) can ring this phone and your other devices."
                                           : "Off. \(f) can message you; their calls don\u{2019}t ring.") {
                            PeopleSwitch(checked: callsAllowed, label: "Allow calls") { on in if !block.blockedByMe { people.allowCalls(did, on) } }
                        }
                    }
                    if !history.isEmpty {
                        PeopleSectionLabel("CALLS WITH \(shown.uppercased())").padding(.top, 8)
                        ForEach(history) { PeopleHistoryRow(record: $0) }
                        PeopleHint("History stays on this phone only.").padding(.horizontal, 20).padding(.vertical, 8)
                    }
                }
            }
        }
        .peopleSheet(isPresented: $confirmBlock) { BlockDialog(name: shown, onDismiss: { confirmBlock = false }, onBlock: { people.block(did) }) }
        .peopleSheet(isPresented: $renaming) {
            PeopleRenameSheet(current: alias ?? name, theirName: name, hasAlias: alias != nil, onDismiss: { renaming = false }) {
                people.rename(did, alias: $0); renaming = false
            }
        }
    }
}

private struct PeopleActionChip: View {
    @Environment(\.tin) private var c
    let icon: PeopleIcon
    let label: String
    let tint: Color
    let action: () -> Void
    var body: some View {
        Button(action: action) {
            VStack(spacing: 6) {
                Circle().fill(c.sf2).frame(width: 52, height: 52).overlay(PeopleIconView(icon: icon, size: 24, color: tint))
                Text(label).font(TinType.caption).foregroundStyle(c.ink)
            }
            .frame(width: 88).contentShape(SmoothRoundedRectangle(cornerRadius: 12))
        }
        .buttonStyle(PeoplePressStyle())
    }
}

/// Components.kt SectionLabel: the small uppercase heading above a group of rows.
private struct PeopleSectionLabel: View {
    @Environment(\.tin) private var c
    let text: String
    init(_ text: String) { self.text = text }
    var body: some View {
        Text(text).peopleStyle(.secHead).foregroundStyle(c.ink2).accessibilityAddTraits(.isHeader)
            .frame(maxWidth: .infinity, alignment: .leading).padding(.horizontal, 20).padding(.top, 16).padding(.bottom, 4)
    }
}

/// Components.kt ListItem: icon, title over sub, a trailing control.
private struct PeopleListItem<Trailing: View>: View {
    @Environment(\.tin) private var c
    let title: String
    let icon: PeopleIcon
    let sub: String
    @ViewBuilder let trailing: Trailing
    var body: some View {
        HStack(spacing: 14) {
            PeopleIconView(icon: icon, size: 22, color: c.ink2)
            VStack(alignment: .leading, spacing: 1) {
                Text(title).peopleStyle(.body).foregroundStyle(c.ink)
                Text(sub).peopleStyle(.sub19).foregroundStyle(c.ink2)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            trailing
        }
        .padding(.horizontal, 20).padding(.vertical, 8).frame(minHeight: 64)
    }
}

private struct PeopleHistoryRow: View {
    @Environment(\.tin) private var c
    let record: PeopleCallRecord
    var body: some View {
        let red = record.kind == .missed || record.kind == .unreached
        HStack(spacing: 14) {
            PeopleIconView(icon: record.kind == .incoming || record.kind == .missed ? .callIn : .callOut, size: 20, color: red ? c.er : c.ink2)
            VStack(alignment: .leading, spacing: 0) {
                Text(record.title.trimmingCharacters(in: .whitespaces).isEmpty ? "Call" : record.title).peopleStyle(.body).foregroundStyle(red ? c.er : c.ink)
                Text(record.whenLong).peopleStyle(.sub).foregroundStyle(c.ink2)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            switch record.direct {
            case true?: badge("Direct", bg: c.prc, fg: c.onPrc)
            case false?: badge("Relayed", bg: c.relayC, fg: c.onRelayC)
            case nil: Text("\u{2014}").font(TinType.bodyM).foregroundStyle(c.ink2)
            }
        }
        .padding(.horizontal, 20).frame(minHeight: 56)
    }
    private func badge(_ text: String, bg: Color, fg: Color) -> some View {
        Text(text).peopleStyle(.sub, weight: .semibold).foregroundStyle(fg).padding(.horizontal, 12).frame(minHeight: 30).background(Capsule().fill(bg))
    }
}

struct PeopleRenameSheet: View {
    @Environment(\.tin) private var c
    let current: String
    let theirName: String
    let hasAlias: Bool
    let onDismiss: () -> Void
    /// nil = go back to their own name.
    let onSave: (String?) -> Void
    @State private var text: String

    init(current: String, theirName: String, hasAlias: Bool, onDismiss: @escaping () -> Void, onSave: @escaping (String?) -> Void) {
        self.current = current; self.theirName = theirName; self.hasAlias = hasAlias; self.onDismiss = onDismiss; self.onSave = onSave
        _text = State(initialValue: current)
    }

    var body: some View {
        let t = text.trimmingCharacters(in: .whitespacesAndNewlines)
        VStack(alignment: .leading, spacing: 0) {
            Text("Rename").peopleStyle(.title, size: 22, lineHeight: 28).foregroundStyle(c.ink).padding(.top, 6).padding(.bottom, 14)
            PeopleField(text: $text, label: "Name on this phone", secret: false, msgLines: 0)
            PeopleHint("Only you see this. They still call themselves \u{201C}\(theirName)\u{201D}.").padding(.top, 14)
            PeopleActs {
                if hasAlias { PeopleButton("Use their name", variant: .quiet) { onSave(nil) } }
                PeopleButton("Cancel", variant: .quiet, action: onDismiss)
                PeopleButton("Save", variant: .primary, enabled: !t.isEmpty) { onSave(t == theirName ? nil : t) }
            }
        }
        .padding(.horizontal, 20).padding(.top, 20).padding(.bottom, 16)
    }
}
