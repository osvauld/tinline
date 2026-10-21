import SwiftUI
import PhotosUI

/*
 * Add contact (people/AddContact.svelte, decided 2026-10-13; SIGNUP-HANDOFF.md 6b and 6f; android AddContactSheet.kt), as a bottom
 * sheet over Chats. Two ways only, By code (first, the default) and QR. Copies the bare code. "Paste their code" reads the
 * clipboard and looks it up at once (core peek_code). Hints are muted and polite, never red. The body is a fixed 360 high, so the
 * sheet never resizes; the found state (me - the mark - them) replaces it in place. Adding dials them (core add_contact_by_code):
 * their device holds it as a request ("Request sent...") until they accept. The same sheet is the owner's side too: "Contact
 * request" (Not now / Accept) when someone used my code, "New contact" once the line is open (`PeopleModel.incoming`).
 * Present it with `.peopleSheet(isPresented:) { AddContactSheet(...) }`.
 */

/// Why an add didn't work, by the core's AddError kind (never its text).
private enum Reach { case timeout, refused, blocked, offline }
private enum ContactCodeSource { case code, camera, image }

/// "Mon 19 Oct" (codeUntil in people.svelte.ts), local time.
private func codeUntil(_ exp: UInt64) -> String {
    let f = DateFormatter()
    f.locale = .current
    f.setLocalizedDateFormatFromTemplate("EEE d MMM")
    return f.string(from: Date(timeIntervalSince1970: TimeInterval(exp)))
}

struct AddContactSheet: View {
    @EnvironmentObject private var people: PeopleModel
    @Environment(\.tin) private var c
    var startOnScan = false
    var ticket: String? = nil
    var pasteOnOpen = false
    let onClose: () -> Void
    let onMessage: (String) -> Void
    let onProfile: (String) -> Void

    @State private var qrTab = false
    @State private var scanning = false
    @State private var hint = ""
    @State private var found: CardPeek?
    @State private var source = ContactCodeSource.code
    @State private var inputGeneration = 0
    @State private var adding = false
    @State private var slow = false
    @State private var fail: Reach?
    @State private var added = false
    @State private var requested = false
    @State private var copied = false
    @State private var myCode: String?
    @State private var myCodeExp: UInt64?
    @State private var codeVersion = 0
    @State private var answering = false
    @State private var readingImage = false
    @State private var photo: PhotosPickerItem?
    @State private var photoOpen = false
    @State private var sharing: String?

    private struct Reload: Equatable { var online: Bool?; var version: Int; var contacts: Int }

    init(startOnScan: Bool = false, ticket: String? = nil, pasteOnOpen: Bool = false, onClose: @escaping () -> Void,
         onMessage: @escaping (String) -> Void, onProfile: @escaping (String) -> Void) {
        self.startOnScan = startOnScan; self.ticket = ticket; self.pasteOnOpen = pasteOnOpen
        self.onClose = onClose; self.onMessage = onMessage; self.onProfile = onProfile
        _qrTab = State(initialValue: startOnScan); _scanning = State(initialValue: startOnScan)
    }

    private var meDid: String { people.node?.profile()?.did ?? "" }

    var body: some View {
        let inc = people.incoming
        PeopleDialogBody(title: inc == nil ? "Add contact" : inc!.joined ? "New contact" : "Contact request") {
            // `.tabs.gone`: hidden but keeping its height, so the sheet doesn't jump when it becomes a request.
            HStack(spacing: 8) {
                PeopleChip(text: "Code", pressed: !qrTab) { if inc == nil { qrTab = false; scanning = false; reset() } }
                PeopleChip(text: "QR", pressed: qrTab) { if inc == nil { qrTab = true; reset() } }
            }
            .padding(.top, 8).opacity(inc != nil ? 0 : 1)

            // 360: room for a 260 pt QR. The real code is long and dense; at 176 pt its modules were too small to scan.
            Group {
                if let inc { PeopleIncoming(meDid: meDid, incoming: inc) }
                else if let p = found {
                    PeopleFound(meDid: meDid, peek: p, source: source, added: added, requested: requested, adding: adding, slow: slow, fail: fail,
                                known: p.known, blocked: p.known && people.blocks.blockedByMe(p.did))
                }
                else if qrTab { qrBody }
                else { codeBody }
            }
            .frame(maxWidth: .infinity).frame(height: 360).padding(.top, 12)

            footer(inc)
        }
        .task(id: Reload(online: people.status?.online, version: codeVersion, contacts: people.contacts.count)) {
            // A code can be replaced (a rename, a block): read it again whenever contacts change (don't cache it).
            guard let node = people.node else { return }
            if let r = await Task.detached(operation: { try? node.myCode() }).value { myCode = r.code; myCodeExp = r.expiresAt }
        }
        .onChange(of: people.accepted) { _, a in
            // They accepted while the sheet is still on them: it turns taut by itself.
            if let a, found?.did == a.did, !added { added = true; requested = false }
        }
        .onAppear { people.addSheetOpen = true }
        .onDisappear { people.addSheetOpen = false }
        .task(id: copied) { if copied { try? await Task.sleep(nanoseconds: 1_600_000_000); copied = false } }
        .task {
            if let ticket {
                if let p = await peek(ticket) { found = p; add() }
            } else if pasteOnOpen { look(PeopleClipboard.text, .code) }
        }
        .photosPicker(isPresented: $photoOpen, selection: $photo, matching: .images)
        .onChange(of: photo) { _, item in if let item { photo = nil; readImage(item) } }
        .sheet(item: Binding(get: { sharing.map { PeopleShareItem(text: $0) } }, set: { if $0 == nil { sharing = nil } })) { PeopleShareSheet(text: $0.text) }
    }

    private struct PeopleShareItem: Identifiable { let text: String; var id: String { text } }

    // MARK: bodies

    private var qrBody: some View {
        VStack(spacing: 8) {
            ZStack {
                if scanning {
                    PeopleQRCameraView(label: "Point at their Tinline code.") { t in
                        // A final camera callback can arrive after the photo picker replaces it.
                        if scanning && qrTab && !readingImage && people.incoming == nil { scanning = false; look(t, .camera) }
                    }
                    .frame(width: 260, height: 260).clipShape(SmoothRoundedRectangle(cornerRadius: 20))
                } else {
                    ZStack {
                        SmoothRoundedRectangle(cornerRadius: 12).fill(Color.white)
                        if let t = myCode, let img = PeopleQR.image(t) {
                            Image(uiImage: img).interpolation(.none).resizable().scaledToFit().padding(6).accessibilityLabel("Your QR code")
                        } else { Text("Preparing your code\u{2026}").peopleStyle(.sub).foregroundStyle(Color(hex: 0x4D5853)) }
                    }
                    .frame(width: 260, height: 260)
                }
            }
            .frame(width: 260, height: 260)
            Text(readingImage ? "Reading image\u{2026}" : !hint.isEmpty ? hint : scanning ? "Point at their Tinline code." : "Let them scan this with Tinline.")
                .peopleStyle(.sub19).foregroundStyle(c.ink2).multilineTextAlignment(.center).lineLimit(2)
                .frame(maxWidth: .infinity, minHeight: 38, maxHeight: 38).accessibilityAddTraits(.updatesFrequently)
            HStack(spacing: 8) {
                PeopleButton(scanning ? "Show my code" : "Scan their code", enabled: !readingImage, padH: 12, fill: true) { reset(); scanning.toggle() }
                PeopleButton("Choose image", enabled: !readingImage, padH: 12, fill: true) { reset(); scanning = false; photoOpen = true }
            }
        }
    }

    private var codeBody: some View {
        VStack(alignment: .leading, spacing: 12) {
            VStack(alignment: .leading, spacing: 8) {
                // "New code" sits on the label's line (a text button, same height), so it never moves anything.
                HStack {
                    Text("Your code").peopleStyle(.sub, weight: .bold, lineHeight: 21).foregroundStyle(c.ink)
                    Spacer(minLength: 0)
                    Button {
                        guard let node = people.node else { return }
                        Task { _ = await Task.detached { try? node.resetTicket() }.value; myCode = nil; codeVersion += 1 }
                    } label: { Text("New code").peopleStyle(.sub, weight: .semibold, lineHeight: 21).foregroundStyle(c.peopleAccentInk) }
                }
                // Reusable for 7 days (core my_code); the date is the code's own exp.
                Text(myCodeExp.map { "Send it to anyone you\u{2019}d like to talk to, in any app. It works until \(codeUntil($0))." }
                     ?? "Send it to anyone you\u{2019}d like to talk to, in any app.")
                    .peopleStyle(.sub19).foregroundStyle(c.ink2)
                Text(myCode ?? "Preparing your code\u{2026}").peopleStyle(.mono, lineHeight: 18, size: 12).foregroundStyle(c.ink2).lineLimit(1).truncationMode(.tail)
                    .padding(.horizontal, 12).padding(.vertical, 8).frame(maxWidth: .infinity, alignment: .leading)
                    .background(RoundedRectangle(cornerRadius: 12, style: .circular).fill(c.sf2))
                // the bare code only (OSVC2:...)
                HStack(spacing: 8) {
                    PeopleButton(copied ? "Copied" : "Copy my code", variant: .primary, enabled: myCode != nil, padH: 12, fill: true) {
                        if let m = myCode { PeopleClipboard.copy(m); copied = true }
                    }
                    PeopleButton("Share", icon: .share, enabled: myCode != nil, padH: 12, fill: true, action: share)
                }
            }
            HStack(spacing: 10) {
                Rectangle().fill(c.ln).frame(height: 1)
                Text("or").peopleStyle(.sub).foregroundStyle(c.ink2)
                Rectangle().fill(c.ln).frame(height: 1)
            }
            VStack(alignment: .leading, spacing: 8) {
                Text("Their code").peopleStyle(.sub, weight: .bold, lineHeight: 21).foregroundStyle(c.ink)
                PeopleButton("Paste their code", fill: true) { look(PeopleClipboard.text, .code) }
                // Hints are not errors: muted, polite (never red, never an alert).
                Text(hint).peopleStyle(.sub19).foregroundStyle(c.ink2).frame(maxWidth: .infinity, minHeight: 38, alignment: .topLeading)
                    .accessibilityAddTraits(.updatesFrequently)
            }
        }
    }

    @ViewBuilder private func footer(_ i: IncomingSheet?) -> some View {
        let p = found
        PeopleActs(top: 2) {
            if let i, !i.joined {
                PeopleButton("Not now", variant: .quiet, enabled: !answering, action: onClose)
                PeopleButton("Accept", variant: .primary, enabled: !answering, minWidth: 132) { accept(i.did) }
            } else if let i {
                PeopleButton("Done", variant: .quiet, action: onClose)
                PeopleButton("Message", variant: .primary, minWidth: 132) { onMessage(i.did) }
            } else if let p {
                if requested && !added { PeopleButton("Done", variant: .primary, minWidth: 132, action: onClose) }
                else if added {
                    PeopleButton("Done", variant: .quiet, action: onClose)
                    PeopleButton("Message", variant: .primary) { onMessage(p.did) }
                } else if p.known {
                    PeopleButton("Close", variant: .quiet, action: onClose)
                    PeopleButton("Open profile", variant: .primary) { onProfile(p.did) }
                } else if fail == .refused {
                    PeopleButton("Back", variant: .quiet) { reset() }
                    PeopleButton("Paste a new code", variant: .primary) { reset() }
                } else if fail == .blocked {
                    PeopleButton("Back", variant: .quiet) { reset() }
                    PeopleButton("Close", variant: .primary, minWidth: 132, action: onClose)
                } else {
                    PeopleButton("Back", variant: .quiet, enabled: !adding) { reset() }
                    // Keep the action short; the complete username is shown in the preview.
                    PeopleButton(adding ? "Adding\u{2026}" : fail != nil ? "Try again" : "Add contact", variant: .primary, enabled: !adding, minWidth: 132, action: add)
                }
            } else {
                if qrTab && !scanning { PeopleButton("Share", icon: .share, enabled: myCode != nil && !readingImage, action: share) }
                PeopleButton("Close", variant: .quiet, action: onClose)
            }
        }
    }

    // MARK: actions

    private func reset() { inputGeneration += 1; found = nil; hint = ""; fail = nil; added = false; requested = false; source = .code }

    private func peek(_ text: String) async -> CardPeek? {
        guard let node = people.node else { return nil }
        let r = await Task.detached { node.peekCode(text: text) }.value
        if case .found(let card) = r { return card }
        return nil
    }

    private func look(_ text: String?, _ how: ContactCodeSource) {
        reset()
        let generation = inputGeneration
        guard let text, !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            hint = "Your clipboard is empty right now. When their code arrives, copy it and paste it here."; return
        }
        guard let node = people.node else { return }
        Task {
            // core peek_code: says why a code can't be used, without dialling.
            let p = await Task.detached { node.peekCode(text: text) }.value
            if generation != inputGeneration || people.incoming != nil { return }
            switch p {
            case .found(let card): found = card; source = how
            case .own: hint = "That\u{2019}s your own code. Send it to them, and paste theirs here."
            case .expired(let name, _, _): hint = "That code from \(name) has expired. Ask them for a fresh one."
            default: hint = "That doesn\u{2019}t look like a Tinline code. Could they copy it again for you?"
            }
        }
    }

    private func readImage(_ item: PhotosPickerItem) {
        let generation = inputGeneration
        readingImage = true
        Task {
            let data = try? await item.loadTransferable(type: Data.self)
            let result = data.map { PeopleQRImageDecoder.decode($0) } ?? .unreadable
            readingImage = false
            guard generation == inputGeneration, qrTab, people.incoming == nil else { return }
            switch result {
            case .decoded(let t): look(t, .image)
            case .noQr: hint = "No QR found. Try a clearer image."
            case .multiple: hint = "More than one QR. Crop to the one you want."
            case .unreadable: hint = "Couldn\u{2019}t read that image. Choose another."
            }
        }
    }

    private func share() { if let m = myCode { sharing = m } }

    private func add() {
        guard let p = found, !adding, let node = people.node else { return }
        fail = nil
        if people.status?.started == true && people.status?.online == false { fail = .offline; return }
        adding = true; slow = false
        let timer = Task { try? await Task.sleep(nanoseconds: 2_500_000_000); if !Task.isCancelled { slow = true } }
        Task {
            let r: Result<AddOutcome, Error> = await Task.detached { Result { try node.addContactByCode(text: p.ticket) } }.value
            timer.cancel(); adding = false; slow = false
            switch r {
            case .success(let outcome):
                people.refresh()
                switch outcome {
                case .added: added = true; people.playCue(.line)
                // Delivered: the line keeps its connecting sway while they decide.
                case .requested: if !added { requested = true }
                }
            case .failure(let e):
                // By the core's AddError kind, never its display text.
                switch e as? AddError {
                case .Refused?: fail = .refused
                case .Blocked?: fail = .blocked
                case .Offline?: fail = .offline
                default: fail = .timeout
                }
            }
        }
    }

    private func accept(_ did: String) {
        if answering { return }
        answering = true
        Task {
            let ok = await people.acceptRequest(did)
            answering = false
            if ok, var i = people.incoming, i.did == did { i.joined = true; people.incoming = i }
        }
    }
}

// MARK: - LineJoin

private enum PeopleLineState { case slack, ringing, taut }

/// LineJoin.svelte: me - the mark - them, fixed 64 high; slack -> ringing (the call "connecting" sway, 1.2 s) -> taut.
// TODO(ios-device): the line between the faces is a stand-in for `TinMark` (Components area) in its line state; swap it in once its
// initialiser is known. The sway and the settle to taut are drawn here with the same timings.
private struct PeopleLineJoin: View {
    @Environment(\.tin) private var c
    @Environment(\.accessibilityReduceMotion) private var reduced
    let meDid: String
    let did: String
    let state: PeopleLineState

    var body: some View {
        HStack(spacing: 6) {
            TinAvatar(did: meDid, size: 56)
            PeopleLineMark(state: state, reduced: reduced).frame(width: 64, height: 64)
                .accessibilityLabel(state == .taut ? "Connected" : state == .ringing ? "Connecting" : "Not connected yet")
            TinAvatar(did: did, size: 56)
        }
        .frame(height: 64)
    }
}

private struct PeopleLineMark: View {
    @Environment(\.tin) private var c
    let state: PeopleLineState
    let reduced: Bool
    var body: some View {
        TimelineView(.animation(minimumInterval: 1.0 / 30, paused: reduced || state != .ringing)) { ctx in
            let t = ctx.date.timeIntervalSinceReferenceDate
            // slack hangs low; ringing sways 6 degrees over 1.2 s; taut is straight.
            let sway = state == .ringing && !reduced ? sin(t * 2 * .pi / 1.2) * 6 : 0
            let sag: CGFloat = state == .taut ? 0 : 14
            Path { p in
                p.move(to: CGPoint(x: 0, y: 32))
                p.addQuadCurve(to: CGPoint(x: 64, y: 32), control: CGPoint(x: 32, y: 32 + sag + CGFloat(sway)))
            }
            .stroke(state == .taut ? c.thread : c.ln2, style: StrokeStyle(lineWidth: 3, lineCap: .round))
            .animation(reduced ? nil : PeopleMotion.easeOut(0.4), value: state)
        }
    }
}

/// Source describes how a code was read, never proof of the person's identity.
private struct PeopleStateChip: View {
    @Environment(\.tin) private var c
    let icon: PeopleIcon
    let text: String
    var body: some View {
        HStack(spacing: 6) {
            PeopleIconView(icon: icon, size: 14, color: c.peopleAccentInk)
            Text(text).peopleStyle(.sub, weight: .semibold, lineHeight: 20).foregroundStyle(c.peopleAccentInk)
        }
        .padding(.horizontal, 10).padding(.vertical, 3).background(Capsule().fill(c.peopleAccentSoft))
    }
}

/// The reason line, in a fixed three-line slot (the face never moves); muted, announced politely.
private struct PeopleWhy: View {
    @Environment(\.tin) private var c
    let text: String
    var body: some View {
        Text(text).peopleStyle(.sub19).foregroundStyle(c.ink2).multilineTextAlignment(.center)
            .frame(maxWidth: 300, minHeight: 57, alignment: .top).accessibilityAddTraits(.updatesFrequently)
    }
}

/// The found card: LineJoin, the name, and the reason line.
private struct PeopleFound: View {
    @Environment(\.tin) private var c
    let meDid: String
    let peek: CardPeek
    let source: ContactCodeSource
    let added: Bool
    let requested: Bool
    let adding: Bool
    let slow: Bool
    let fail: Reach?
    let known: Bool
    let blocked: Bool

    var body: some View {
        // The whole user-chosen username, not a first/last-name split.
        let first = peek.name
        // slack (recognised) -> ringing while the dial runs and while the request waits -> taut.
        let state: PeopleLineState = added || known ? .taut : adding || requested ? .ringing : .slack
        let why: String = {
            if blocked { return "You blocked \(first). Unblock them in Privacy before adding them again." }
            if known { return "\(first) is already in your contacts." }
            if added { return "Your line to \(first) is open. Say hello." }
            if requested { return "Request sent. Your line opens when \(first) accepts." }
            if adding { return slow ? "Still trying. \(first)\u{2019}s Tinline needs to be open and online for this." : "Reaching \(first)\u{2019}s Tinline\u{2026}" }
            switch fail {
            case .timeout?: return "Couldn\u{2019}t reach \(first)\u{2019}s Tinline. It needs to be open and online, so try again when \(first)\u{2019}s around."
            case .refused?: return "\(first)\u{2019}s code isn\u{2019}t valid any more. They may have made a new one, so ask \(first) for their newest code."
            case .blocked?: return "You can\u{2019}t add \(first) right now. They\u{2019}ve chosen not to be in touch here."
            case .offline?: return "You\u{2019}re offline. Once you\u{2019}re connected, try again."
            case nil: return "Is this \(first)? If you\u{2019}re not sure, check with them first."
            }
        }()
        VStack(spacing: 8) {
            PeopleLineJoin(meDid: meDid, did: peek.did, state: state)
            Text(peek.name).peopleStyle(.title, size: 20, lineHeight: 26).foregroundStyle(c.ink)
            switch source {
            case .camera: PeopleStateChip(icon: .scan, text: "Read with camera")
            case .image: PeopleStateChip(icon: .image, text: "Read from image")
            case .code: EmptyView()
            }
            PeopleWhy(text: why)
        }
    }
}

/// The owner's side (or mine, when they accepted while the sheet was closed): AddContact.svelte `inc`.
private struct PeopleIncoming: View {
    @Environment(\.tin) private var c
    let meDid: String
    let incoming: IncomingSheet
    var body: some View {
        let f = incoming.name
        VStack(spacing: 8) {
            PeopleLineJoin(meDid: meDid, did: incoming.did, state: incoming.joined ? .taut : .ringing)
            Text(incoming.name).peopleStyle(.title, size: 20, lineHeight: 26).foregroundStyle(c.ink)
            PeopleStateChip(icon: .userPlus, text: incoming.theirs ? "Accepted your request" : "Has your code")
            PeopleWhy(text: !incoming.joined ? "\(f) wants to add you as a contact. Accepting lets you message and call each other."
                      : incoming.theirs ? "\(f) accepted. Your line to \(f) is open. Say hello."
                      : "Your line to \(f) is open. Say hello.")
        }
    }
}
