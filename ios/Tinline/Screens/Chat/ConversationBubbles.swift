import SwiftUI
import UIKit

// The rows of the thread: the message bubble with its attachments, call pills, the block note and footer, the long-press
// and attach sheets (android Conversation.kt, the parts below `ConversationScreen`).

/// Gives its one child up to `fraction` of the row (capped at `cap`), pinned to the start or the end: "up to 82 % of the
/// screen (capped for tablets), so text is not wrapped into a narrow column".
struct ChatRowLayout: Layout {
    var trailing: Bool
    var fraction: CGFloat = 0.78
    var cap: CGFloat = 560

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let w = proposal.width ?? 320
        let h = subviews.first?.sizeThatFits(ProposedViewSize(width: min(w * fraction, cap), height: nil)).height ?? 0
        return CGSize(width: w, height: h)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        guard let s = subviews.first else { return }
        let p = ProposedViewSize(width: min(bounds.width * fraction, cap), height: nil)
        let size = s.sizeThatFits(p)
        s.place(at: CGPoint(x: trailing ? bounds.maxX - size.width : bounds.minX, y: bounds.minY), proposal: p)
    }
}

/// What a bubble needs besides the message (kept together so the call site stays readable).
struct ChatBubbleEnv {
    var name: String
    var link: ChatLink
    var online: Bool
    var nowMs: Int64
    var progress: (done: UInt64, total: UInt64)?
    var requestedDownload: Bool
    var quoted: Message?
    var receipt: MessageReceiptStatus?
    var notDelivered: String?
    var selected: Bool
    var findQuery: String
    var findCurrent: Bool
    var broken: Bool
}

struct ConversationBubble: View {
    let m: Message
    let pos: BubblePos
    let env: ChatBubbleEnv
    let source: ChatSource
    var onLong: () -> Void
    var onReply: () -> Void
    var onDownload: () -> Void
    var onCancel: () -> Void
    var onOpenPhoto: () -> Void
    var onSaveFile: () -> Void
    var onOpenFile: () -> Void
    var onBroken: () -> Void

    @Environment(\.tin) private var tin
    @State private var drag: CGFloat = 0
    private let threshold: CGFloat = 64

    var body: some View {
        let out = m.outgoing
        ChatRowLayout(trailing: out) { content }
            .padding(.top, chatGapAbove(pos))
            .overlay(alignment: .leading) {
                if drag > 0 { ChatIcon(glyph: .reply, size: 24, color: tin.pr).accessibilityLabel(tr("chat_reply")) }
            }
            .simultaneousGesture(swipe)
    }

    /// Swipe right to reply: only a mostly horizontal drag counts, so scrolling is not stolen.
    private var swipe: some Gesture {
        DragGesture(minimumDistance: 12)
            .onChanged { v in
                guard !m.deleted, drag > 0 || (v.translation.width > 0 && abs(v.translation.width) > abs(v.translation.height)) else { return }
                drag = min(max(0, v.translation.width), threshold * 1.4)
            }
            .onEnded { _ in
                if drag >= threshold { onReply() }
                withAnimation(ChatMotion.animation(ChatMotion.release)) { drag = 0 }
            }
    }

    private var first: String { ChatFmt.firstName(env.name) }

    @ViewBuilder private var content: some View {
        let out = m.outgoing
        let p = ChatPalette(tin)
        if m.deleted {
            VStack(alignment: .trailing, spacing: 0) {
                Text(out ? tr("chat_deleted_by_me") : tr("chat_deleted_by_them", first)).font(ChatText.font(15).italic()).foregroundStyle(tin.ink2)
                ChatMetaLine(text: ChatFmt.clock(Int64(m.at)), mark: nil, color: tin.ink2)
            }
            .padding(.leading, 12).padding(.trailing, 12).padding(.top, 8).padding(.bottom, 6)
            .overlay { if out { chatBubbleShape(pos, mine: out).strokeBorder(tin.ln2, lineWidth: 1) } }
        } else if m.attachment == nil && env.notDelivered == nil {
            textBubble.offset(x: drag)
        } else {
            attachmentBubble(p).offset(x: drag)
        }
    }

    // MARK: text only

    @ViewBuilder private var textBubble: some View {
        let p = ChatPalette(tin)
        let out = m.outgoing
        let shape = chatBubbleShape(pos, mine: out)
        let meta = ChatBubbleMeta(time: ChatFmt.clock(Int64(m.at)), edited: m.editedAt != nil, receipt: receiptMark(false))
        let quote: AnyView? = m.replyTo == nil ? nil : AnyView(ChatQuote(
            who: env.quoted.map { $0.outgoing ? tr("chat_you") : first }, whoColor: p.accentInk,
            text: env.quoted.map(chatPreviewOf) ?? tr("chat_quote_unavailable"), onTap: nil))
        ChatTextBubble(
            text: m.text,
            display: ChatFind.marked(ChatText.styled(m.text), q: env.findQuery, current: env.findCurrent, gold: tin.thread),
            pos: pos, mine: out, meta: meta, emoji: m.replyTo == nil && emojiIsBig(text: m.text), held: env.selected, quote: quote)
            .overlay { if env.selected { shape.strokeBorder(p.accentInk, lineWidth: 2) } }
            .onLongPressGesture(perform: onLong)
            .accessibilityAction(named: tr("chat_message_actions"), onLong)
    }

    private func receiptMark(_ notDelivered: Bool) -> ChatMetaReceipt? {
        if !m.outgoing || env.receipt?.tick == ReceiptTick.none { return nil }
        let vis = tickVisOf(env.receipt?.tick, notDelivered: notDelivered)
        return ChatMetaReceipt(tick: env.receipt?.tick, key: m.id, label: vis.map { tickLabel($0, first: first) } ?? "", reserve: true, notDelivered: notDelivered)
    }

    // MARK: attachments and not-delivered

    @ViewBuilder private func attachmentBubble(_ p: ChatPalette) -> some View {
        let out = m.outgoing
        let shape = chatBubbleShape(pos, mine: out)
        let a = m.attachment
        let photo = a.map { ChatFmt.isImage($0) } == true && !env.broken
        let tick: ChatTick? = out && env.receipt != nil && env.receipt!.tick != ReceiptTick.none
            ? chatTickOf(m, receipt: env.receipt!.tick, link: env.link, online: env.online, nowMs: env.nowMs) : nil
        let fg = out ? tin.onPrc : tin.ink
        let at = ChatFmt.clock(Int64(m.at))
        let sendingPct: Int? = (out && env.progress != nil && env.progress!.total > 0) ? Int(env.progress!.done * 100 / env.progress!.total) : nil
        let label: String = {
            if let nd = env.notDelivered { return "\(nd) · \(at)" }
            if let s = sendingPct, !photo { return tr("chat_meta_sending", String(s), at) }
            if tick == .clock { return tr("chat_meta_waiting", at) }
            if m.editedAt != nil { return tr("chat_meta_edited", at) }
            return at
        }()
        let metaColor: Color = env.notDelivered != nil ? p.tickBadText : (tick == .clock ? tin.threadText : (out ? tin.onPrc : tin.ink2))
        let mark = receiptMark(env.notDelivered != nil)
        let hasText = !m.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        VStack(alignment: .leading, spacing: 0) {
            if m.replyTo != nil {
                VStack(alignment: .leading, spacing: 0) {
                    Text(env.quoted == nil ? tr("chat_quote_earlier") : (env.quoted!.outgoing ? tr("chat_you") : first)).font(TinType.caption.weight(.bold)).foregroundStyle(tin.pr)
                    Text(env.quoted.map(chatPreviewOf) ?? tr("chat_quote_gone")).font(ChatText.font(13)).foregroundStyle(fg.opacity(0.8)).lineLimit(1)
                }
                .padding(.leading, 12).padding(.trailing, 10).padding(.vertical, 7).frame(maxWidth: .infinity, alignment: .leading)
                .background(out ? Color.white.opacity(0.22) : tin.bg)
                .overlay(alignment: .leading) { Rectangle().fill(tin.pr).frame(width: 3) }
                .clipShape(SmoothRoundedRectangle(cornerRadius: 8)).padding(.bottom, 8)
            }
            if let a {
                if a.kind == .voice {
                    ChatVoiceBubble(m: m, a: a, ready: a.state == .ready || out, out: out, source: source, onDownload: onDownload)
                } else if photo {
                    ChatPhotoBox(m: m, a: a, out: out, pos: pos, first: first, waiting: out && tick == .clock, progress: env.progress, source: source,
                                 onDownload: onDownload, onCancel: onCancel, onOpen: onOpenPhoto, onBroken: onBroken)
                } else {
                    ChatFileRow(a: a, out: out, progress: env.progress, link: env.link, first: first, asked: env.requestedDownload,
                                onDownload: onDownload, onCancel: onCancel, onSave: onSaveFile, onOpen: onOpenFile, onLong: onLong)
                }
            }
            if a == nil && hasText {
                // Only a few emojis: big, like the other chat apps; emojis in text: a bit larger.
                let big = emojiIsBig(text: m.text)
                let size: CGFloat = big ? 38 : 15
                ChatTextWithMeta(text: big ? AttributedString(m.text.trimmingCharacters(in: .whitespacesAndNewlines)) : ChatText.styled(m.text),
                                 measuring: ChatText.measuring(m.text, size: size), color: fg, font: ChatText.font(size)) {
                    ChatMetaLine(text: label, mark: mark, color: metaColor, padded: false)
                }
            } else {
                // A photo's caption and time keep to the photo's width (the bubble is never wider than the photo).
                let w: CGFloat? = photo ? a.map { ChatFmt.photoBox(width: $0.width, height: $0.height).width } : nil
                if hasText {
                    Text(ChatText.styled(m.text)).font(ChatText.body).foregroundStyle(fg)
                        .frame(width: w, alignment: .leading).padding(.horizontal, photo ? 9 : 0).padding(.top, photo ? 5 : 0)
                }
                ChatMetaLine(text: label, mark: mark, color: metaColor, padded: true).frame(width: w).padding(.horizontal, photo ? 7 : 0)
            }
        }
        .padding(photo ? EdgeInsets(top: 3, leading: 3, bottom: 6, trailing: 3) : EdgeInsets(top: 8, leading: 12, bottom: 6, trailing: 12))
        .background(shape.fill(out ? tin.prc : tin.bubIn))
        .overlay { if !out { shape.strokeBorder(tin.bubInLine, lineWidth: 1) } }
        .overlay { if env.selected { shape.strokeBorder(tin.pr, lineWidth: 3) } }
        .clipShape(shape)
        .onLongPressGesture(perform: onLong)
    }
}

/// The time and status under an attachment or at the end of a caption (`Meta`).
struct ChatMetaLine: View {
    let text: String
    let mark: ChatMetaReceipt?
    let color: Color
    var padded = true
    var body: some View {
        HStack(spacing: 4) {
            if padded { Spacer(minLength: 0) }
            Text(text).font(ChatText.font(11)).foregroundStyle(color)
            ChatReceiptMark(mark: mark, color: color)
        }
        .padding(.top, padded ? 4 : 0)
    }
}

// MARK: photo

private func previewImage(_ px: [(UInt8, UInt8, UInt8)]) -> UIImage? {
    var bytes = [UInt8]()
    for p in px { bytes += [p.0, p.1, p.2, 255] }
    guard let provider = CGDataProvider(data: Data(bytes) as CFData),
          let cg = CGImage(width: 4, height: 3, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: 16, space: CGColorSpaceCreateDeviceRGB(),
                           bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue), provider: provider, decode: nil,
                           shouldInterpolate: true, intent: .defaultIntent) else { return nil }
    return UIImage(cgImage: cg)
}

/// PhotoBody.svelte (Framed, decided 2026-10-14): the photo inside its bubble's 3 pt rim, its corners following the
/// bubble's (so they tighten in a run). Until it is here the core's colour preview fills the box, soft; sending shows a
/// ring with cancel, waiting a chip, not downloaded the size and Download. The time sits under it (the bubble's meta).
struct ChatPhotoBox: View {
    let m: Message
    let a: Attachment
    let out: Bool
    let pos: BubblePos
    let first: String
    let waiting: Bool
    let progress: (done: UInt64, total: UInt64)?
    let source: ChatSource
    let onDownload: () -> Void
    let onCancel: () -> Void
    let onOpen: () -> Void
    let onBroken: () -> Void
    @Environment(\.tin) private var tin
    @State private var image: UIImage?

    var body: some View {
        let p = ChatPalette(tin)
        let ready = a.state == .ready || out
        let box = ChatFmt.photoBox(width: a.width, height: a.height)
        let frac: CGFloat? = progress.map { $0.total > 0 ? CGFloat($0.done) / CGFloat($0.total) : 0 }
            ?? (a.state == .downloading && a.size > 0 ? CGFloat(a.transferred) / CGFloat(a.size) : nil)
        let busy = frac != nil && !(ready && progress == nil)
        let pv = ChatFmt.previewPixels(a.preview).flatMap(previewImage)
        ZStack {
            tin.sf3
            if let pv {
                Image(uiImage: pv).resizable().interpolation(.high).scaledToFill().frame(width: box.width, height: box.height)
                    .scaleEffect(1.24).blur(radius: 14).clipped()
            }
            if let image { Image(uiImage: image).resizable().scaledToFill().frame(width: box.width, height: box.height).clipped()
                .opacity(a.state == .failed ? 0.55 : 1).accessibilityLabel(a.name) }
            if busy {
                Color.black.opacity(0.18)
                ChatRing {
                    ZStack {
                        Circle().stroke(Color.white.opacity(0.28), lineWidth: 3)
                        Circle().trim(from: 0, to: min(1, max(0, frac ?? 0))).stroke(Color.white, style: StrokeStyle(lineWidth: 3, lineCap: .round)).rotationEffect(.degrees(-90))
                    }.frame(width: 48, height: 48)
                    ChatIcon(glyph: .x, size: 18, color: .white)
                }
                .accessibilityLabel(out ? tr("chat_cancel_sending") : tr("chat_cancel_download"))
            } else if a.state == .remote && !out {
                VStack(spacing: 8) {
                    ChatPhotoChip(text: tr("chat_photo_chip", ChatFmt.size(Int64(a.size))))
                    Button(action: onDownload) {
                        HStack(spacing: 6) {
                            ChatIcon(glyph: .chevRight, size: 16, color: p.onAccentFill)
                            Text(tr("chat_download")).font(TinType.caption.weight(.semibold)).foregroundStyle(p.onAccentFill)
                        }.padding(.horizontal, 16).padding(.vertical, 7).background(p.accentFill, in: Capsule())
                    }
                }
            } else if a.state == .failed {
                ChatRing { ChatIcon(glyph: .retry, size: 24, color: .white) }.accessibilityLabel(tr("chat_retry_download"))
            } else if image == nil && ready && pv == nil {
                ChatIcon(glyph: .image, size: 24, color: tin.ink2)
            }
            if waiting && !busy { ChatPhotoChip(text: tr("chat_sends_when_online", first)).padding(8).frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottomLeading) }
        }
        .frame(width: box.width, height: box.height).clipShape(chatBubbleShape(pos, mine: out, inset: 3))
        .contentShape(Rectangle())
        .onTapGesture { if ready && image != nil { onOpen() } else if busy { onCancel() } else if !ready { onDownload() } }
        .accessibilityAddTraits(.isButton)
        .task(id: "\(m.id)\(ready)") {
            guard ready else { image = nil; return }
            let source = source, m = m
            if let f = try? await ChatMedia.decrypt(source: source, message: m) {
                image = await Task.detached { ChatMedia.image(f, maxPx: 720) }.value
            }
            // Ready but undecodable (odd GIF/WebP/HEIC): fall back to a file bubble with Open/Save.
            if image == nil { onBroken() }
        }
    }
}

private struct ChatPhotoChip: View {
    let text: String
    var body: some View {
        Text(text).font(ChatText.font(11)).foregroundStyle(.white).padding(.horizontal, 8).padding(.vertical, 2)
            .background(Color.black.opacity(0.42), in: RoundedRectangle(cornerRadius: 10))
    }
}

private struct ChatRing<Content: View>: View {
    @ViewBuilder let content: () -> Content
    var body: some View { ZStack { content() }.frame(width: 56, height: 56).background(Color(red: 0x0F / 255, green: 0x15 / 255, blue: 0x13 / 255, opacity: 0x8C / 255), in: Circle()) }
}

// MARK: file

struct ChatFileRow: View {
    let a: Attachment
    let out: Bool
    let progress: (done: UInt64, total: UInt64)?
    let link: ChatLink
    let first: String
    let asked: Bool
    let onDownload: () -> Void
    let onCancel: () -> Void
    let onSave: () -> Void
    let onOpen: () -> Void
    let onLong: () -> Void
    @Environment(\.tin) private var tin

    var body: some View {
        // "starting…" only until the core reports Downloading; never hides a failure (tap to retry).
        let askedNow = asked && a.state == .remote
        let ready = a.state == .ready || out
        let size = ChatFmt.size(Int64(a.size))
        let frac: CGFloat? = progress.map { $0.total > 0 ? CGFloat($0.done) / CGFloat($0.total) : 0 }
            ?? (a.state == .downloading && a.size > 0 ? CGFloat(a.transferred) / CGFloat(a.size) : nil)
        let paused = link == .offline
        let canOpen = ChatFileOpen.plan(name: a.name, mime: a.mime).kind != .unsupported
        let sub: String = {
            if ready && canOpen { return tr("chat_file_open", size, ChatFmt.typeLabel(a)) }
            if ready { return tr("chat_file_save", size, ChatFmt.typeLabel(a)) }
            if a.state == .downloading && !paused { return tr("chat_file_downloading", ChatFmt.size(Int64((frac ?? 0) * CGFloat(a.size))), size) }
            if (a.state == .downloading || askedNow) && paused { return tr("chat_file_paused", size, first) }
            if askedNow { return tr("chat_file_starting", size) }
            if a.state == .failed { return tr("chat_file_failed", size) }
            return tr("chat_file_remote", size)
        }()
        let canDownload = !ready && (a.state == .remote || a.state == .failed) && !askedNow
        HStack(spacing: 12) {
            ChatIcon(glyph: ChatFmt.isVideo(a) ? .video : .file, size: 24, color: tin.pr).frame(width: 44, height: 44)
                .background(out ? Color.white.opacity(0.55) : tin.sf2, in: SmoothRoundedRectangle(cornerRadius: 12))
            VStack(alignment: .leading, spacing: 0) {
                Text(a.name).font(ChatText.font(15, weight: .semibold)).foregroundStyle(out ? tin.onPrc : tin.ink).lineLimit(1).truncationMode(.tail)
                Text(sub).font(TinType.caption.weight(.regular)).foregroundStyle(out ? tin.onPrc : tin.ink2)
                if let frac, !ready, !paused {
                    ZStack(alignment: .leading) {
                        Capsule().fill(Color.black.opacity(0.1))
                        GeometryReader { g in Rectangle().fill(tin.pr).frame(width: g.size.width * min(1, max(0, frac))) }.clipShape(Capsule())
                    }.frame(height: 4).padding(.top, 6)
                }
            }
            Spacer(minLength: 0)
            if canDownload && a.state == .remote { ChatIconButton(glyph: .download, label: tr("chat_file_download_a11y", a.name), tint: tin.pr, action: onDownload) }
            if !ready && (a.state == .downloading || askedNow) { ChatIconButton(glyph: .x, label: tr("chat_file_cancel_a11y", a.name), tint: tin.ink2, action: onCancel) }
            if ready && canOpen { ChatIconButton(glyph: .save, label: tr("chat_file_save_a11y", a.name), tint: out ? tin.onPrc : tin.ink2, action: onSave) }
        }
        .frame(minWidth: 240).contentShape(Rectangle())
        .onTapGesture { if ready { if canOpen { onOpen() } else { onSave() } } else if canDownload { onDownload() } }
        .onLongPressGesture(perform: onLong)
        .accessibilityElement(children: .contain)
    }
}

// MARK: voice

/// Voice message body: decrypts the attachment to the cache once, then hands the file to the player.
struct ChatVoiceBubble: View {
    let m: Message
    let a: Attachment
    let ready: Bool
    let out: Bool
    let source: ChatSource
    let onDownload: () -> Void
    @Environment(\.tin) private var tin
    @State private var path: String?

    var body: some View {
        Group {
            if let path {
                VoiceBubbleContent(id: m.id, path: path, durationMs: Int(a.durationMs), waveform: a.waveform, outgoing: out)
            } else {
                let remote = !ready && a.state != .downloading
                HStack(spacing: 10) {
                    Button(action: onDownload) {
                        ChatIcon(glyph: remote ? .download : .play, size: 22, color: tin.onPr).frame(width: 40, height: 40).background(tin.pr, in: Circle())
                    }
                    .disabled(!remote).accessibilityLabel(remote ? tr("chat_voice_download") : tr("chat_voice_loading"))
                    Text(clockMs(ms: a.durationMs)).font(.custom("IBMPlexMono-Medium", size: 12, relativeTo: .caption))
                        .foregroundStyle(out ? tin.onPrc : tin.ink2)
                    Spacer(minLength: 0)
                }.frame(minWidth: 230)
            }
        }
        .task(id: "\(m.id)\(ready)") {
            guard ready else { return }
            let f = ChatMedia.voiceDir.appendingPathComponent("\(m.id).ogg")
            if let n = (try? FileManager.default.attributesOfItem(atPath: f.path)[.size] as? Int), n > 0 { path = f.path; return }
            try? FileManager.default.createDirectory(at: ChatMedia.voiceDir, withIntermediateDirectories: true)
            if (try? await source.save(peerDid: m.peerDid, messageId: m.id, destPath: f.path)) != nil { path = f.path }
        }
    }
}

// MARK: call pill, block note, footer

/// MessageRow `.callrec`: a one-line pill (32 tall, radius 16, surface, hairline) on the caller's side; tap = call back.
struct ChatCallPill: View {
    let r: CallRecord
    let enabled: Bool
    let onCall: () -> Void
    @Environment(\.tin) private var tin

    var body: some View {
        let text = ChatFmt.callPillText(r)
        let answered = r.durationSecs > 0
        let bad = !answered && r.incoming && r.reason != "declined_local"
        let time = ChatFmt.clock(Int64(r.startedAt) * 1000)
        HStack {
            if !r.incoming { Spacer(minLength: 0) }
            Button(action: onCall) {
                HStack(spacing: 6) {
                    ChatIcon(glyph: r.video ? .video : .phone, size: 16, color: bad ? tin.er : tin.ink2)
                    Text(text).font(ChatText.font(13, weight: .semibold)).foregroundStyle(bad ? tin.er : tin.ink).lineLimit(1)
                    Text(time).font(ChatText.font(11)).monospacedDigit().foregroundStyle(tin.ink2)
                }
                .padding(.horizontal, 12).frame(minHeight: 32)
                .background(tin.sf, in: Capsule()).overlay(Capsule().strokeBorder(tin.ln, lineWidth: 1))
            }
            .buttonStyle(ChatPressStyle()).disabled(!enabled)
            .accessibilityLabel(tr("chat_call_back_a11y", text, time))
            if r.incoming { Spacer(minLength: 0) }
        }
        .padding(.top, 10)
    }
}

/// Under the last message: a quiet line when we block them, their signed notice when they block us.
struct ChatBlockNote: View {
    let name: String
    let view: BlockView
    @Environment(\.tin) private var tin
    var body: some View {
        let f = ChatFmt.firstName(name)
        if view.blockedByMe {
            Text(view.blockedAtSecs.map { tr("chat_blocked_note_date", f, BlockCopy.date($0)) } ?? tr("chat_blocked_note", f))
                .font(TinType.caption.weight(.semibold)).foregroundStyle(tin.ink2)
                .padding(.horizontal, 12).padding(.vertical, 4).background(tin.sf2, in: Capsule())
                .frame(maxWidth: .infinity).padding(.vertical, 8)
        } else if let at = view.blockedMeAtSecs {
            VStack(alignment: .leading, spacing: 4) {
                Text(blockedYouTitle(name: name)).font(TinType.bodyL.weight(.bold)).foregroundStyle(tin.onErc)
                Text(blockedYouBody(name: name)).font(TinType.bodyM).foregroundStyle(tin.onErc)
                Text(tr("chat_blocked_signed", f, BlockCopy.date(at))).font(TinType.caption).foregroundStyle(tin.onErc.opacity(0.8))
            }
            .padding(16).frame(maxWidth: .infinity, alignment: .leading).background(tin.erc, in: SmoothRoundedRectangle(cornerRadius: 16)).padding(.vertical, 8)
        }
    }
}

/// In place of the composer while a block is on, or for a kept chat with someone no longer a contact.
struct ChatBlockedFooter: View {
    let name: String
    let state: ComposerState
    let onUnblock: () -> Void
    let onAddContact: () -> Void
    @Environment(\.tin) private var tin
    var body: some View {
        VStack(spacing: 0) {
            Rectangle().fill(tin.ln).frame(height: 1)
            HStack(spacing: 12) {
                Text(line).font(TinType.bodyM).foregroundStyle(tin.ink2).frame(maxWidth: .infinity, alignment: .leading)
                switch state {
                case .blockedByMe: ChatCardButton(title: tr("chat_unblock"), height: 44, fill: false, action: onUnblock)
                case .notContact: ChatCardButton(title: tr("chat_add_contact"), style: .tonal, height: 44, fill: false, action: onAddContact)
                default: EmptyView()
                }
            }
            .padding(.leading, 20).padding(.trailing, 12).padding(.top, 12).padding(.bottom, 16)
        }
    }
    private var line: String {
        switch state {
        case .blockedByMe: return blockedByMeLine(name: name)
        case .blockedMe: return cantSendText(name: name)
        default: return notContactLine(name: name)
        }
    }
}

// MARK: sheets

private struct ChatHint: View {
    let text: String
    @Environment(\.tin) private var tin
    var body: some View { Text(text).font(TinType.bodyM).foregroundStyle(tin.ink2).frame(maxWidth: .infinity, alignment: .leading) }
}

private struct ChatSheetAction: View {
    let glyph: ChatGlyph
    let label: String
    var color: Color?
    let action: () -> Void
    @Environment(\.tin) private var tin
    var body: some View {
        Button(action: action) {
            HStack(spacing: 16) {
                ChatIcon(glyph: glyph, size: 24, color: color ?? tin.ink2)
                Text(label).font(TinType.bodyL).foregroundStyle(color ?? tin.ink)
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 8).frame(minHeight: 56).contentShape(Rectangle())
        }.buttonStyle(.plain)
    }
}

struct ChatActionsSheet: View {
    let m: Message
    let first: String
    let isSelf: Bool
    let receipt: MessageReceiptStatus?
    let name: String
    let onReply: () -> Void
    let onCopy: () -> Void
    let onEdit: () -> Void
    let onDelete: () -> Void
    let onSave: () -> Void
    @Environment(\.tin) private var tin

    var body: some View {
        let at = ChatFmt.clock(Int64(m.at))
        let proofs = m.outgoing && !isSelf && receipt?.recipients.contains(where: { !$0.proofs.isEmpty }) == true
        VStack(alignment: .leading, spacing: 0) {
            // Only the core's signed receipts say "on" or "read on" their device; a signature is the device's claim, not a person's attention.
            ChatHint(text: headline(at)).padding(.leading, 8).padding(.bottom, proofs ? 2 : 8)
            if m.outgoing && !isSelf {
                ForEach(Array((receipt?.recipients ?? []).enumerated()), id: \.offset) { _, r in
                    ForEach(Array(r.proofs.enumerated()), id: \.offset) { _, p in
                        ChatHint(text: tr(p.kind == .read ? "chat_proof_read" : "chat_proof_delivered", ChatFmt.firstName(name), String(p.signerDevice.prefix(8))))
                            .padding(.leading, 8).padding(.bottom, 2)
                    }
                }
            }
            if proofs { Spacer().frame(height: 6) }
            ChatSheetAction(glyph: .reply, label: tr("chat_reply"), action: onReply)
            if !m.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { ChatSheetAction(glyph: .copy, label: tr("chat_copy_text"), action: onCopy) }
            if let a = m.attachment, a.state == .ready || m.outgoing { ChatSheetAction(glyph: .save, label: tr("chat_save_to_phone"), action: onSave) }
            if m.outgoing && m.attachment == nil { ChatSheetAction(glyph: .edit, label: tr("chat_edit"), action: onEdit) }
            if m.outgoing { ChatSheetAction(glyph: .trash, label: isSelf ? tr("chat_delete_self") : tr("chat_delete_both"), color: tin.er, action: onDelete) }
            if m.outgoing {
                ChatHint(text: isSelf ? tr("chat_sheet_hint_self") : tr("chat_sheet_hint", first))
                    .padding(.leading, 8).padding(.top, 8)
            }
        }
        .padding(.horizontal, 16).padding(.top, 16).padding(.bottom, 8).frame(maxWidth: .infinity, alignment: .leading)
        .chatSheetStyle(tin)
    }

    private func headline(_ at: String) -> String {
        if isSelf { return tr("chat_head_saved", at) }
        if !m.outgoing { return tr("chat_head_received", at) }
        switch receipt?.tick {
        case .read: return tr("chat_head_read", at, first)
        case .delivered: return tr("chat_head_delivered", at, first)
        case .onlyHere: return tr("chat_head_only_here", at)
        default: return tr("chat_head_sent", at, first)
        }
    }
}

struct ChatAttachSheet: View {
    let first: String
    let isSelf: Bool
    let onPhotos: () -> Void
    let onCamera: () -> Void
    let onFile: () -> Void
    @Environment(\.tin) private var tin

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(isSelf ? tr("chat_attach_title_self") : tr("chat_attach_title", first)).font(TinType.titleM).foregroundStyle(tin.ink).padding(.leading, 4).padding(.bottom, 8)
            HStack(spacing: 12) {
                ForEach([(ChatGlyph.image, tr("chat_attach_photos"), onPhotos), (ChatGlyph.camera, tr("chat_attach_camera"), onCamera), (ChatGlyph.file, tr("chat_attach_file"), onFile)], id: \.1) { item in
                    Button(action: item.2) {
                        VStack(spacing: 8) {
                            ChatIcon(glyph: item.0, size: 24, color: tin.onPrc).frame(width: 60, height: 60).background(tin.prc, in: SmoothRoundedRectangle(cornerRadius: 18))
                            Text(item.1).font(TinType.bodyM.weight(.medium)).foregroundStyle(tin.ink)
                        }
                        .frame(maxWidth: .infinity).padding(.vertical, 8).contentShape(Rectangle())
                    }.buttonStyle(.plain)
                }
            }
            HStack(alignment: .top, spacing: 8) {
                ChatIcon(glyph: .lock, size: 18, color: tin.ink2)
                ChatHint(text: isSelf ? tr("chat_attach_hint_self")
                                       : tr("chat_attach_hint"))
            }.padding(.leading, 4).padding(.top, 12)
        }
        .padding(.horizontal, 16).padding(.top, 16).padding(.bottom, 8).frame(maxWidth: .infinity, alignment: .leading)
        .chatSheetStyle(tin)
    }
}
