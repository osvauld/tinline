import SwiftUI
import UIKit
import CoreImage
import CoreImage.CIFilterBuiltins

/*
 * The group flows that are whole screens (android GroupsLive.kt NewGroupLive, AddMembersLive, JoinGroupLive, GroupTokenLive):
 * new group, add members (G3), join with a link (paste, scan or a tapped tinline://g/ link) and the invite link with a large QR.
 */

// ------------------------------------------------------------------ new group

struct NewGroupView: View {
    var body: some View { GroupsSessionGate { NewGroupScreen(session: $0) } }
}

private struct NewGroupScreen: View {
    @ObservedObject var session: GroupsSession
    @Environment(\.dismiss) private var dismiss
    @Environment(\.groupsNavigate) private var navigate
    @State private var name = ""
    @State private var busy = false
    @State private var err: String?

    var body: some View {
        GroupsPage(title: tr("group_new_title")) {
            ScrollView {
                VStack(spacing: 16) {
                    GroupsField(label: tr("group_name_label"), text: $name, error: err, look: .form, limit: 100)
                    GroupsInfoCard(text: tr("group_new_info"),
                                   icon: GroupsIcons.server, kind: .warn)
                    GroupsHint(tr("group_new_owner_hint"))
                }.padding(.horizontal, 20)
            }
            GroupsFlowButton(text: busy ? tr("group_making") : tr("group_create_button"), enabled: !name.groupsIsBlank && !busy) {
                busy = true; err = nil
                let n = name
                session.exec({ session.ctl.create(n) }) { r in
                    busy = false
                    switch r {
                    case .success(let id): dismiss(); DispatchQueue.main.asyncAfter(deadline: .now() + 0.35) { navigate(.group(id)) }
                    case .failure(let e): err = e.message
                    }
                }
            }.padding(20)
        }
        .groupsToasts(session)
    }
}

// ------------------------------------------------------------------ add members (G3)

/// Pick contacts to invite: blocked contacts (either direction) are not offered; members and people with a pending invitation
/// show why they can't be picked. Each invitation goes over the 1:1 chat.
struct AddMembersView: View {
    let groupId: String
    var body: some View { GroupsSessionGate { AddMembersScreen(session: $0, id: groupId) } }
}

private struct AddMembersScreen: View {
    @ObservedObject var session: GroupsSession
    let id: String
    @Environment(\.tin) private var tin
    @Environment(\.dismiss) private var dismiss
    @State private var picked: [String] = []
    @State private var busy = false

    var body: some View {
        let ctl = session.ctl
        let dir = session.dir
        if let d = ctl.opened.flatMap({ $0.id == id ? $0.detail : nil }) {
            let rows = GroupText.pickRows(dir.contacts.map { InviteCandidate(did: $0.did, name: $0.name) }, d, ctl.sent,
                                          blockedByMe: { dir.blockedByMe($0) }, blocksMe: { dir.blocksMe($0) })
            GroupsPage(title: tr("group_add_to_title", d.name)) {
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 4) {
                        GroupsSectionLabel(tr("group_contacts_selected", picked.count))
                        if rows.isEmpty { GroupsHint(tr("group_add_no_contacts")) }
                        ForEach(rows, id: \.did) { r in row(r) }
                        GroupsHint(tr("group_add_hint")).padding(.top, 8)
                    }.padding(.horizontal, 20).padding(.vertical, 8)
                }
                GroupsFlowButton(text: busy ? tr("group_invite_sending") : (picked.count > 1 ? trn("group_invite_people", picked.count) : tr("group_invite_button")), enabled: !picked.isEmpty && !busy) {
                    busy = true
                    let dids = picked
                    session.exec({ session.ctl.inviteContacts(id, dids: dids) }) { r in
                        busy = false
                        switch r {
                        case .success(let s): session.say(s); dismiss()
                        case .failure(let e): session.say(e.message.groupsIfBlank(tr("group_invite_failed")))
                        }
                    }
                }.padding(20)
            }
            .groupsToasts(session)
            .task(id: id) { session.background { if session.ctl.opened?.id != id { session.ctl.openGroup(id) } } }
        } else {
            GroupsPage(title: tr("group_add_members_title")) { Spacer() }
                .task(id: id) { session.background { if session.ctl.opened?.id != id { session.ctl.openGroup(id) } } }
        }
    }

    @ViewBuilder private func row(_ r: PickRow) -> some View {
        let on = picked.contains(r.did)
        let can = r.why == nil
        let sub = r.why ?? r.invited.map { GroupText.sentInvitationLine($0).0 }
        let content = HStack(spacing: 14) {
            TinAvatar(did: r.did, size: 40)
            VStack(alignment: .leading, spacing: 0) {
                Text(r.name).font(.custom("Figtree", size: 15, relativeTo: .body)).foregroundStyle(can ? tin.ink : tin.ink2).lineLimit(1)
                if let sub { Text(sub).font(TinType.bodyM).foregroundStyle(tin.ink2) }
            }
            Spacer(minLength: 0)
            if can { Image(systemName: on ? "checkmark.square.fill" : "square").foregroundStyle(on ? tin.pr : tin.ln2).imageScale(.large) }
        }
        .frame(minHeight: 56).clipShape(SmoothRoundedRectangle(cornerRadius: 12)).contentShape(Rectangle())
        if can {
            Button { if on { picked.removeAll { $0 == r.did } } else { picked.append(r.did) } } label: { content }
                .buttonStyle(.plain)
                .accessibilityElement(children: .combine).accessibilityAddTraits(on ? [.isButton, .isSelected] : .isButton)
        } else { content }
    }
}

// ------------------------------------------------------------------ join

struct JoinGroupView: View {
    var initial: String?
    var body: some View { GroupsSessionGate { JoinGroupScreen(session: $0, initial: initial) } }
}

private struct JoinGroupScreen: View {
    @ObservedObject var session: GroupsSession
    @Environment(\.tin) private var tin
    @Environment(\.dismiss) private var dismiss
    @Environment(\.groupsNavigate) private var navigate
    @Environment(\.groupsQRScanner) private var scanner
    @State private var token: String
    @State private var scanning = false
    @State private var busy = false
    @State private var err: String?
    @State private var peek: GroupInvitePeek?
    /// The details come from the group's host over the network: wait for typing to settle, show progress.
    @State private var checking = false

    init(session: GroupsSession, initial: String?) {
        self.session = session
        _token = State(initialValue: initial ?? "")
    }

    var body: some View {
        if scanning {
            GroupsPage(title: tr("group_scan_title"), onBack: { scanning = false }) {
                scanner.scannerView(prompt: tr("group_scan_prompt")) { t in token = t.trimmingCharacters(in: .whitespacesAndNewlines); scanning = false }
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        } else { form }
    }

    private var form: some View {
        let bad = !token.groupsIsBlank && !checking && peek == nil
        return GroupsPage(title: tr("group_join_title")) {
            ScrollView {
                VStack(spacing: 14) {
                    GroupsField(label: tr("group_join_field"), text: $token, placeholder: "tinline://g/…", error: nil, look: .form, mono: true, multiline: true, minHeight: 72,
                                hint: checking ? tr("group_join_checking") : (bad ? tr("group_join_bad") : nil))
                    HStack(spacing: 10) {
                        GroupsFlowButton(text: tr("group_paste"), style: .outlined, icon: GroupsIcons.paste) {
                            if let s = UIPasteboard.general.string { token = s.trimmingCharacters(in: .whitespacesAndNewlines) }
                        }
                        GroupsFlowButton(text: tr("group_scan"), style: .outlined, icon: GroupsIcons.scan) { scanning = true }
                    }
                    if let p = peek { peekCard(p) }
                    if let err { Text(err).font(TinType.bodyM).foregroundStyle(tin.er).frame(maxWidth: .infinity, alignment: .leading) }
                }.padding(.horizontal, 20)
            }
            GroupsFlowButton(text: busy ? tr("group_joining") : tr("group_join"), enabled: peek != nil && !busy) {
                busy = true; err = nil
                let t = token
                session.exec({ session.ctl.join(t) }) { r in
                    busy = false
                    switch r {
                    case .success(let id): dismiss(); DispatchQueue.main.asyncAfter(deadline: .now() + 0.35) { navigate(.group(id)) }
                    case .failure(let e): err = e.message
                    }
                }
            }.padding(20)
        }
        .groupsToasts(session)
        .task(id: token) {
            peek = nil; err = nil
            if token.groupsIsBlank { checking = false; return }
            checking = true
            try? await Task.sleep(nanoseconds: 400_000_000)
            if Task.isCancelled { return }
            let t = token, ctl = session.ctl
            let found = await Task.detached(priority: .userInitiated) { ctl.peek(t) }.value
            if Task.isCancelled { return }
            peek = found
            checking = false
        }
    }

    /// A line about someone on the join preview: one whole string for me, for a contact and for someone outside my contacts.
    private func peekLine(_ did: String, named: String, unknown: String, you: String) -> String {
        if did == session.me { return tr(you) }
        if let c = session.dir.contacts.first(where: { $0.did == did }), !c.name.groupsIsBlank { return tr(named, c.name) }
        return tr(unknown)
    }

    private func peekCard(_ p: GroupInvitePeek) -> some View {
        return VStack(spacing: 14) {
            GroupsCardBox(radius: 20) {
                VStack(alignment: .leading, spacing: 6) {
                    HStack(spacing: 12) {
                        GroupsAvatar(size: 44)
                        Text(p.name).font(TinType.titleM).foregroundStyle(tin.ink)
                    }
                    Text(p.issuerDid.map { peekLine($0, named: "group_join_invited_by", unknown: "group_join_invited_by_unknown", you: "group_join_invited_by_you") } ?? tr("group_join_own_link")).font(TinType.bodyM).foregroundStyle(tin.ink2)
                    Text(peekLine(p.ownerDid, named: "group_join_owner", unknown: "group_join_owner_unknown", you: "group_join_owner_you")).font(TinType.bodyM).foregroundStyle(tin.ink2)
                    if let e = p.expires { Text(tr("group_valid_until", GroupsTime.whenFull(e))).font(TinType.bodyM).foregroundStyle(tin.ink2) }
                }.padding(16)
            }
            GroupsHint(tr("group_join_hint"))
        }
    }
}

// ------------------------------------------------------------------ invite link (G3: short, large QR)

/// A QR code as an image: CoreImage's generator, scaled without smoothing so the modules stay sharp.
func groupsQRImage(_ text: String, scale: CGFloat = 12) -> UIImage? {
    let f = CIFilter.qrCodeGenerator()
    f.message = Data(text.utf8)
    f.correctionLevel = "M"
    guard let out = f.outputImage else { return nil }
    let big = out.transformed(by: CGAffineTransform(scaleX: scale, y: scale))
    guard let cg = CIContext().createCGImage(big, from: big.extent) else { return nil }
    return UIImage(cgImage: cg)
}

struct GroupTokenView: View {
    let groupId: String
    var body: some View { GroupsSessionGate { GroupTokenScreen(session: $0, id: groupId) } }
}

private struct GroupTokenScreen: View {
    @ObservedObject var session: GroupsSession
    let id: String
    @Environment(\.tin) private var tin
    @State private var token: String?
    @State private var err: String?
    @State private var full = false
    @State private var copied = false

    var body: some View {
        let name = session.ctl.opened.flatMap { $0.id == id ? $0.detail.name : nil }
        GroupsPage(title: tr("group_link_title")) {
            ScrollView {
                VStack(spacing: 14) {
                    GroupsHint(name.map { tr("group_link_hint", $0) } ?? tr("group_link_hint_unnamed"))
                    // Large, high contrast, the full width: easy to scan from another phone. Tap for full screen.
                    ZStack {
                        RoundedRectangle(cornerRadius: 16).fill(Color.white)
                        if let t = token, let img = groupsQRImage(t) {
                            Image(uiImage: img).interpolation(.none).resizable().scaledToFit().padding(12).accessibilityLabel(tr("group_link_qr_desc"))
                        } else if let err { Text(err).font(TinType.bodyM).foregroundStyle(tin.er).multilineTextAlignment(.center).padding(16) }
                        else { Text(tr("group_link_making")).font(TinType.bodyM).foregroundStyle(Color(hex: 0x4D5853)) }
                    }
                    .aspectRatio(1, contentMode: .fit)
                    .onTapGesture { if token != nil { full = true } }
                    .accessibilityAddTraits(token != nil ? .isButton : []).accessibilityHint(token != nil ? tr("group_link_show_full") : "")
                    if let t = token { Text(t).font(TinType.bodyM.monospaced()).foregroundStyle(tin.ink2).multilineTextAlignment(.center).textSelection(.enabled) }
                    HStack(spacing: 10) {
                        if let t = token {
                            ShareLink(item: name.map { tr("group_share_text", $0, t) } ?? tr("group_share_text_unnamed", t), subject: Text(tr("group_share_subject"))) {
                                Text(tr("group_share")).font(TinType.button).foregroundStyle(tin.onPrc).frame(maxWidth: .infinity, minHeight: 52).background(tin.prc, in: Capsule())
                            }
                        } else {
                            GroupsFlowButton(text: tr("group_share"), style: .tonal, icon: GroupsIcons.share, enabled: false) {}
                        }
                        GroupsFlowButton(text: copied ? tr("group_copied") : tr("group_copy"), style: .outlined, icon: GroupsIcons.copy, enabled: token != nil) {
                            if let t = token { UIPasteboard.general.string = t; copied = true }
                        }
                    }
                }.padding(.horizontal, 20)
            }
        }
        .groupsToasts(session)
        .task(id: id) {
            let ctl = session.ctl, gid = id
            let r = await Task.detached(priority: .userInitiated) { ctl.invite(gid) }.value
            switch r { case .success(let t): token = t; case .failure(let e): err = e.message }
        }
        .fullScreenCover(isPresented: $full) {
            if let t = token, let img = groupsQRImage(t) {
                ZStack {
                    Color.white.ignoresSafeArea()
                    Image(uiImage: img).interpolation(.none).resizable().scaledToFit().padding(16).accessibilityLabel(tr("group_link_qr_full_desc"))
                }.onTapGesture { full = false }.accessibilityAddTraits(.isButton).accessibilityHint(tr("group_close"))
            }
        }
    }
}
