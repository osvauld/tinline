import PhotosUI
import QuickLook
import SwiftUI
import UniformTypeIdentifiers

/// Things the chat area needs from the rest of the app and cannot know itself. The Shell sets them once at start.
enum ChatHooks {
    /// A call is active (or ringing): the mic is taken, so voice messages cannot be recorded.
    static var callActive: () -> Bool = { false }
}

/// One conversation: header, the day's messages (older days load at the top), and the composer (android
/// `ConversationScreen`). The one public root of the chat area; the Shell wires the navigation closures.
///
/// `chatId` is the contact's DID (the core's `Chat.peerDid`; our own DID for the "You" conversation).
struct ConversationView: View {
    let chatId: String
    /// The contact's name; the core's chat list is asked when nil.
    var name: String?
    /// This phone has a network connection at all.
    var online = true
    /// The "You" conversation: notes and files for our own devices, no contact on the other side.
    var isSelf = false
    /// Blocking on either side (People area); nil for "You" and previews. Chats are never deleted (U24): a block only closes the composer.
    var block: ConvBlock?
    /// Why our message is not delivered ("blocked"), from the core (`Node.notDeliveredReason`). Called off the main thread.
    var notDelivered: @Sendable (String) -> String? = { _ in nil }
    /// Calls with this person, shown in the conversation as pills.
    var calls: [CallRecord] = []
    var addedAtSecs: Int64?
    /// Global-search integration: the real hit, including its UTC day.
    var searchHit: MessageHit?
    /// My call with them is running: the phone button turns green and returns to it.
    var callLive = false
    /// Test and preview seam; the real app leaves this nil and gets the core source.
    var source: ChatSource?
    var onBack: () -> Void = {}
    var onCall: () -> Void = {}
    /// The header's video button: place the call and turn the camera on. Nil hides the button.
    var onVideo: (() -> Void)?
    var onReturnToCall: () -> Void = {}
    var onInfo: (() -> Void)?
    var onUnblock: () -> Void = {}
    var onAddContact: () -> Void = {}

    @EnvironmentObject private var app: AppEnv
    @Environment(\.tin) private var tin

    var body: some View {
        if let src = source ?? ChatBackend.current(node: app.node) {
            ConversationScreen(chatId: chatId, name: name ?? src.chats.first { $0.peerDid == chatId }?.peerName ?? "Chat", online: online, isSelf: isSelf,
                               block: block, notDelivered: notDelivered, calls: calls, addedAtSecs: addedAtSecs, searchHit: searchHit, callLive: callLive,
                               source: src, onBack: onBack, onCall: onCall, onVideo: onVideo, onReturnToCall: onReturnToCall, onInfo: onInfo,
                               onUnblock: onUnblock, onAddContact: onAddContact)
                .id(chatId)
        } else {
            // The vault is locked or the node is not up yet: nothing to show, and nothing to claim.
            ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity).background(tin.sf)
        }
    }
}

private struct ChatViewerItem: Identifiable { let id: String }

private struct ConversationScreen: View {
    let chatId: String
    let name: String
    let online: Bool
    let isSelf: Bool
    let block: ConvBlock?
    let notDelivered: @Sendable (String) -> String?
    let calls: [CallRecord]
    let addedAtSecs: Int64?
    let searchHit: MessageHit?
    let callLive: Bool
    let source: ChatSource
    let onBack: () -> Void
    let onCall: () -> Void
    let onVideo: (() -> Void)?
    let onReturnToCall: () -> Void
    let onInfo: (() -> Void)?
    let onUnblock: () -> Void
    let onAddContact: () -> Void

    @StateObject private var model: ConversationModel
    @StateObject private var find = FindState()
    @StateObject private var voice = VoiceRecState(canRecord: { !ChatHooks.callActive() })
    @ObservedObject private var draft: ConversationDraft
    @Environment(\.tin) private var tin
    @Environment(\.scenePhase) private var phase

    @State private var actionsFor: String?
    @State private var attachOpen = false
    @State private var photoViewer: ChatViewerItem?
    @State private var docViewer: ChatViewerItem?
    @State private var quickLook: URL?
    @State private var photoPicker = false
    @State private var pickedPhoto: PhotosPickerItem?
    @State private var filePicker = false
    @State private var cameraOpen = false
    @State private var findTask: Task<Void, Never>?
    @State private var keepId: String?
    @FocusState private var composerFocus: Bool

    init(chatId: String, name: String, online: Bool, isSelf: Bool, block: ConvBlock?, notDelivered: @escaping @Sendable (String) -> String?, calls: [CallRecord],
         addedAtSecs: Int64?, searchHit: MessageHit?, callLive: Bool, source: ChatSource, onBack: @escaping () -> Void, onCall: @escaping () -> Void,
         onVideo: (() -> Void)?, onReturnToCall: @escaping () -> Void, onInfo: (() -> Void)?, onUnblock: @escaping () -> Void, onAddContact: @escaping () -> Void) {
        self.chatId = chatId; self.name = name; self.online = online; self.isSelf = isSelf; self.block = block; self.notDelivered = notDelivered
        self.calls = calls; self.addedAtSecs = addedAtSecs; self.searchHit = searchHit; self.callLive = callLive; self.source = source
        self.onBack = onBack; self.onCall = onCall; self.onVideo = onVideo; self.onReturnToCall = onReturnToCall; self.onInfo = onInfo
        self.onUnblock = onUnblock; self.onAddContact = onAddContact
        _model = StateObject(wrappedValue: ConversationModel(peerDid: chatId, source: source))
        _draft = ObservedObject(wrappedValue: ConversationDrafts.forPeer(chatId))
    }

    private var first: String { ChatFmt.firstName(name) }
    private var composerState: ComposerState { block?.composer ?? .open }
    private var callGate: CallGate { block?.callGate ?? .ok }
    private var blockedMeAt: Int64? { block?.view.blockedMeAtSecs }

    var body: some View {
        let p = ChatPalette(tin)
        VStack(spacing: 0) {
            ZStack {
                if find.open {
                    ChatFindBar(state: find, onQuery: { _ in }, onOlder: olderMatch, onNewer: { find.newer() }, onClose: { find.close() })
                } else {
                    ConversationHeader(name: name, did: chatId, link: model.link, isSelf: isSelf, blockState: composerState, callGate: callGate, online: online,
                                       callLive: callLive, hasVideo: onVideo != nil, onBack: onBack, onCall: onCall, onVideo: { (onVideo ?? onCall)() },
                                       onReturnToCall: onReturnToCall, onFind: { find.show() }, onInfo: onInfo)
                }
            }
            .frame(height: 56).frame(maxWidth: .infinity).background(tin.sf)
            ZStack(alignment: .top) {
                thread(p)
                if (!online || model.link == .offline) && composerState == .open {
                    ChatStrip(text: online ? "\(first) is offline. Messages send when you’re both online." : "You’re offline. Messages send when you’re back online.", glyph: .off)
                }
            }
            .frame(maxHeight: .infinity).background(p.canvas)
            if composerState != .open {
                ChatBlockedFooter(name: name, state: composerState, onUnblock: onUnblock, onAddContact: onAddContact).background(tin.sf)
            } else { composer.background(tin.sf) }
        }
        .background(tin.sf)
        .navigationBarBackButtonHidden(true).toolbar(.hidden, for: .navigationBar)
        .overlay(alignment: .bottom) { toastView }
        .sheet(isPresented: Binding(get: { actionsFor != nil }, set: { if !$0 { actionsFor = nil } })) { actionsSheet }
        .sheet(isPresented: $attachOpen) {
            ChatAttachSheet(first: first, isSelf: isSelf,
                            onPhotos: { attachOpen = false; photoPicker = true },
                            onCamera: { attachOpen = false; openCamera() },
                            onFile: { attachOpen = false; filePicker = true })
        }
        .photosPicker(isPresented: $photoPicker, selection: $pickedPhoto, matching: .any(of: [.images, .videos]))
        .onChange(of: pickedPhoto) { _, item in
            guard let item else { return }
            pickedPhoto = nil
            Task {
                if let f = try? await item.loadTransferable(type: ChatPickedFile.self) { model.sendFile(f.url, mime: f.mime) } else { model.toast = "Couldn’t read that file." }
            }
        }
        .fileImporter(isPresented: $filePicker, allowedContentTypes: [.item]) { result in
            if case .success(let url) = result { model.sendPicked(url) }
        }
        .fullScreenCover(isPresented: $cameraOpen) {
            ChatCameraPicker { data in
                cameraOpen = false
                guard let data else { return }
                let dir = ChatMedia.outDir.appendingPathComponent(UUID().uuidString, isDirectory: true)
                try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
                let f = dir.appendingPathComponent("IMG_\(Int(Date().timeIntervalSince1970 * 1000)).jpg")
                if (try? data.write(to: f)) != nil { model.sendFile(f, mime: "image/jpeg") }
            }.ignoresSafeArea()
        }
        .fullScreenCover(item: $docViewer) { item in
            if let dm = model.message(item.id) {
                ChatFileViewer(message: dm, plan: ChatFileOpen.plan(name: dm.attachment?.name ?? "", mime: dm.attachment?.mime ?? ""), source: source,
                               onClose: { docViewer = nil }, onSave: { model.saveToPhone(dm) }, onShare: { model.share(dm) })
            }
        }
        .fullScreenCover(item: $photoViewer) { item in
            if let vm = model.message(item.id) {
                ChatPhotoViewer(message: vm, name: name, source: source, onClose: { photoViewer = nil }, onSave: { model.saveToPhone(vm) }, onShare: { model.share(vm) })
            }
        }
        .quickLookPreview($quickLook)
        .task {
            voice.onSend = { model.sendVoice($0) }
            model.setCalls(calls)
            await model.start()
            if let hit = searchHit { await model.showSearchHit(hit) }
        }
        .task { while !Task.isCancelled { try? await Task.sleep(nanoseconds: 15_000_000_000); model.tickClock() } }
        .task(id: calls.map(\.callId)) { model.setCalls(calls) }
        .task(id: searchHit?.id) { if let hit = searchHit, model.loaded { await model.showSearchHit(hit) } }
        .task(id: model.messages.filter { $0.outgoing && $0.delivery == .pending }.map(\.id) + [blockedMeAt.map { String($0) } ?? ""]) {
            await model.refreshUndelivered(blockedMe: blockedMeAt != nil, reason: notDelivered)
        }
        .onChange(of: find.q) { _, _ in restartFind() }
        .onChange(of: find.open) { _, _ in restartFind() }
        .onChange(of: model.messages) { _, _ in if find.open && !model.findUsesCore { restartFind() } }
        .onChange(of: find.current) { _, hit in
            guard find.open, let hit else { return }
            Task { await model.jump(to: hit) }
        }
        .onChange(of: draft.text) { old, new in
            // Typing is the wake point (consent-based links, 2026-10-17): the core dials them ahead of the send.
            if !new.isEmpty && new != old && !isSelf { source.typing(peerDid: chatId) }
            ConversationDrafts.publish(chatId, draft)
        }
        .onChange(of: draft.edit) { _, _ in ConversationDrafts.publish(chatId, draft) }
        .onChange(of: phase) { _, new in setReading(new == .active) }
        .onAppear { setReading(phase == .active) }
        .onDisappear { setReading(false); draft.cancelEdit(); ConversationDrafts.publish(chatId, draft); voice.cancel(); model.stop() }
    }

    // MARK: thread

    private func thread(_ p: ChatPalette) -> some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(spacing: 0) {
                    Color.clear.frame(height: 1).onAppear {
                        keepId = model.rows.first(where: { if case .msg = $0 { return true } else { return false } })?.id
                        Task { await model.loadOlder() }
                    }
                    // Only when this phone may lack earlier days: the contact was added before our oldest day and their phone has not said "nothing older".
                    if model.mayHaveOlder(addedAtSecs: addedAtSecs) { olderBanner }
                    ForEach(model.rows) { row in rowView(row).id(row.id) }
                    if let block, block.view.blockedByMe || blockedMeAt != nil { ChatBlockNote(name: name, view: block.view) }
                    Color.clear.frame(height: 1).id("bottom")
                }
                .padding(.horizontal, 10).padding(.vertical, 8)
            }
            .defaultScrollAnchor(.bottom)
            .scrollDismissesKeyboard(.interactively)
            .onChange(of: model.prependTick) { _, _ in if let k = keepId { proxy.scrollTo(k, anchor: .top) } }
            .onChange(of: model.scroll) { _, req in
                withAnimation(ChatMotion.animation(ChatMotion.enter)) {
                    if let id = req.id { proxy.scrollTo(id, anchor: .center) } else { proxy.scrollTo("bottom", anchor: .bottom) }
                }
            }
            .onChange(of: model.messages.last?.id) { _, _ in
                if !find.open && searchHit == nil { proxy.scrollTo("bottom", anchor: .bottom) }
            }
        }
    }

    private var olderBanner: some View {
        HStack(alignment: .top, spacing: 12) {
            ChatIcon(glyph: .clock, size: 24, color: tin.ink2)
            VStack(alignment: .leading, spacing: 4) {
                Text("Older messages are on \(first)’s phone").font(TinType.bodyM.weight(.semibold)).foregroundStyle(tin.ink)
                Text("This phone keeps them from when you started using it. Earlier days load when you’re both online.").font(TinType.bodyM).foregroundStyle(tin.ink2)
            }
        }
        .padding(16).overlay(SmoothRoundedRectangle(cornerRadius: 14).stroke(tin.ln2, lineWidth: 1)).padding(.vertical, 8)
    }

    @ViewBuilder private func rowView(_ row: ConvRow) -> some View {
        switch row {
        case .day(let label, _): ChatDayPill(label: label).padding(.vertical, 10)
        case .unread(let n): ChatUnreadDivider(n: n)
        case .call(let r): ChatCallPill(r: r, enabled: callGate == .ok, onCall: onCall)
        case .msg(let m, let pos): bubble(m, pos)
        }
    }

    private func bubble(_ m: Message, _ pos: BubblePos) -> some View {
        let hit = find.open && find.hits.contains { $0.id == m.id }
        let env = ChatBubbleEnv(
            name: name, link: model.link, online: online, nowMs: model.nowMs, progress: m.attachment.flatMap { model.progress[$0.hash] },
            requestedDownload: model.requested.contains(m.id), quoted: model.message(m.replyTo), receipt: model.receipts[m.id],
            notDelivered: model.undelivered.contains(m.id) ? notDeliveredText(name: name) : nil,
            selected: actionsFor == m.id || model.foundId == m.id, findQuery: hit ? find.q : "", findCurrent: find.current?.id == m.id,
            broken: model.brokenImages.contains(m.id))
        return ConversationBubble(
            m: m, pos: pos, env: env, source: source,
            onLong: { if !m.deleted { actionsFor = m.id } },
            onReply: { draft.beginReply(id: m.id); composerFocus = true },
            onDownload: { model.download(m) }, onCancel: { model.cancelDownload(m) }, onOpenPhoto: { photoViewer = ChatViewerItem(id: m.id) },
            onSaveFile: { model.saveToPhone(m) }, onOpenFile: { openFile(m) }, onBroken: { model.brokenImages.insert(m.id) })
    }

    // MARK: composer

    private var composer: some View {
        let banner: ChatComposerCtx? = {
            if let e = model.message(draft.edit) { return ChatComposerCtx(title: "Editing message", text: e.text, cancelLabel: "Cancel edit") }
            if let r = model.message(draft.reply) {
                return ChatComposerCtx(title: "Replying to \(r.outgoing ? "yourself" : first)", text: chatPreviewOf(r), cancelLabel: "Cancel reply")
            }
            return nil
        }()
        return ChatComposer(draft: $draft.text, placeholder: "Message", ctx: banner, editing: draft.edit != nil, recording: voice.recording,
                            onCancelCtx: { draft.cancel() }, onSend: { model.send(draft) }, onAttach: { attachOpen = true }, focus: $composerFocus,
                            recordingBar: { VoiceRecordingBar(state: voice) }, trailing: { VoiceMicButton(state: voice) })
    }

    // MARK: sheets and actions

    @ViewBuilder private var actionsSheet: some View {
        if let am = model.message(actionsFor) {
            ChatActionsSheet(
                m: am, first: first, isSelf: isSelf, receipt: model.receipts[am.id], name: name,
                onReply: { actionsFor = nil; draft.beginReply(id: am.id); composerFocus = true },
                onCopy: { actionsFor = nil; UIPasteboard.general.string = am.text; model.toast = "Copied" },
                onEdit: { actionsFor = nil; draft.beginEdit(id: am.id, body: am.text); composerFocus = true },
                onDelete: { actionsFor = nil; model.delete(am.id) },
                onSave: { actionsFor = nil; model.saveToPhone(am) })
        }
    }

    private func openFile(_ m: Message) {
        guard let a = m.attachment else { return }
        let plan = ChatFileOpen.plan(name: a.name, mime: a.mime)
        switch plan.kind {
        case .unsupported: model.saveToPhone(m)
        case .pdf, .text: docViewer = ChatViewerItem(id: m.id)
        case .external:
            // Android hands the file to another app; QuickLook shows what iOS can, with a share button for the rest.
            Task { if let f = try? await ChatMedia.decrypt(source: source, message: m) { quickLook = f } else { model.toast = "Couldn’t open that file." } }
        }
    }

    private func openCamera() {
        if UIImagePickerController.isSourceTypeAvailable(.camera) { cameraOpen = true } else { model.toast = "No camera available." }
    }

    private func olderMatch() {
        if find.olderOrNeedsPage() { Task { await model.findOlderPage(find) } }
    }

    private func restartFind() {
        findTask?.cancel()
        findTask = Task { await model.runFind(find) }
    }

    /// Reading: the open conversation never raises a notification, and its unread count is cleared.
    private func setReading(_ on: Bool) {
        if on { OpenChat.peer = chatId; ChatNotifier.shared.cancel(peer: chatId) }
        else if OpenChat.peer == chatId { OpenChat.peer = nil }
    }

    @ViewBuilder private var toastView: some View {
        if let t = model.toast {
            Text(t).font(TinType.bodyM).foregroundStyle(tin.bg).padding(.horizontal, 16).padding(.vertical, 10)
                .background(tin.ink, in: Capsule()).padding(.bottom, 90).transition(.opacity)
                .task(id: t) { try? await Task.sleep(nanoseconds: 2_500_000_000); if model.toast == t { model.toast = nil } }
                .accessibilityAddTraits(.updatesFrequently)
        }
    }
}

// MARK: header

/// Conversation.svelte `.bar`: back, the head (avatar 36, name 17/22, status 13/18 muted; tap = details), call, video, find,
/// details, each a 44 round button; call and video dim (with the reason) when calling isn't possible.
struct ConversationHeader: View {
    let name: String
    let did: String
    let link: ChatLink
    let isSelf: Bool
    var blockState: ComposerState = .open
    var callGate: CallGate = .ok
    var online = true
    var callLive = false
    var hasVideo = true
    let onBack: () -> Void
    let onCall: () -> Void
    let onVideo: () -> Void
    var onReturnToCall: () -> Void = {}
    var onFind: () -> Void = {}
    var onInfo: (() -> Void)?
    @Environment(\.tin) private var tin

    /// The line under the name. Presence is shown only while they're active with me (consent-based, 2026-10-17): otherwise it's
    /// unknown, so no word rather than a false "Offline". The line keeps its height (no layout shift).
    var status: String {
        if isSelf { return chatSelfSub }
        switch blockState {
        case .blockedByMe: return "Blocked"
        case .blockedMe: return "Blocked you"
        case .notContact: return "Not in your contacts"
        case .open: break
        }
        if !online { return "Waiting for connection" }
        if link == .connected || link == .relayed { return "Online" + (callGate == .notAllowed ? " · messages only" : "") }
        if callGate == .notAllowed { return "Messages only" }
        return ""
    }

    var body: some View {
        let p = ChatPalette(tin)
        let blocked = blockState == .blockedByMe || blockState == .blockedMe
        let first = ChatFmt.firstName(name)
        HStack(spacing: 2) {
            ChatIconButton(glyph: .back, label: "Back to chats", action: onBack)
            Button { onInfo?() } label: {
                HStack(spacing: 10) {
                    TinAvatar(did: did, size: 36, name: name)
                    VStack(alignment: .leading, spacing: 0) {
                        Text(name).font(ChatText.font(17, weight: .semibold)).foregroundStyle(tin.ink).lineLimit(1)
                        Text(status.isEmpty ? " " : status).font(ChatText.font(13)).foregroundStyle(blocked ? tin.er : tin.ink2).lineLimit(1)
                    }
                    Spacer(minLength: 0)
                }
                .padding(.leading, 6).padding(.trailing, 10).padding(.vertical, 4).frame(minHeight: 44).contentShape(Rectangle())
            }
            .buttonStyle(.plain).disabled(onInfo == nil)
            .accessibilityLabel("\(name). \(status.isEmpty ? "" : status + ". ")Chat details")
            if !isSelf {
                let ok = blockState == .open && callGate == .ok
                // Dimmed buttons still answer a tap: the call path says why it can't call.
                if callLive {
                    ChatIconButton(glyph: .phone, label: "Return to call", tint: p.accentInk, action: onReturnToCall)
                        .background(p.accentSoft, in: Circle())
                } else {
                    ChatIconButton(glyph: .phone, label: ok ? "Call" : "Can’t call \(first)", enabled: true, action: onCall).opacity(ok ? 1 : 0.45)
                }
                if hasVideo { ChatIconButton(glyph: .video, label: ok ? "Video call" : "Can’t call \(first)", action: ok ? onVideo : onCall).opacity(ok ? 1 : 0.45) }
            }
            ChatIconButton(glyph: .search, label: "Find in this chat", action: onFind)
            if let onInfo { ChatIconButton(glyph: .info, label: "Chat details", action: onInfo) }
        }
        .padding(.horizontal, 6).padding(.vertical, 4)
    }
}

// MARK: pickers

/// A picked photo or video, copied into the app's scratch folder under a sensible name.
struct ChatPickedFile: Transferable {
    let url: URL
    let mime: String

    static var transferRepresentation: some TransferRepresentation {
        FileRepresentation(importedContentType: .item) { received in
            let dir = ChatMedia.outDir.appendingPathComponent(UUID().uuidString, isDirectory: true)
            try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
            let dest = dir.appendingPathComponent(ChatMedia.safe(received.file.lastPathComponent))
            try FileManager.default.copyItem(at: received.file, to: dest)
            return ChatPickedFile(url: dest, mime: ChatMedia.mime(for: dest))
        }
    }
}

/// The system camera, for "Camera" in the attach sheet. Needs `NSCameraUsageDescription` in Info.plist.
struct ChatCameraPicker: UIViewControllerRepresentable {
    /// JPEG data of the photo, or nil when cancelled.
    let done: (Data?) -> Void

    func makeCoordinator() -> Coordinator { Coordinator(done) }

    func makeUIViewController(context: Context) -> UIImagePickerController {
        let p = UIImagePickerController()
        p.sourceType = .camera
        p.delegate = context.coordinator
        return p
    }

    func updateUIViewController(_ vc: UIImagePickerController, context: Context) {}

    final class Coordinator: NSObject, UIImagePickerControllerDelegate, UINavigationControllerDelegate {
        let done: (Data?) -> Void
        init(_ done: @escaping (Data?) -> Void) { self.done = done }
        func imagePickerController(_ picker: UIImagePickerController, didFinishPickingMediaWithInfo info: [UIImagePickerController.InfoKey: Any]) {
            done((info[.originalImage] as? UIImage)?.jpegData(compressionQuality: 0.9))
        }
        func imagePickerControllerDidCancel(_ picker: UIImagePickerController) { done(nil) }
    }
}
