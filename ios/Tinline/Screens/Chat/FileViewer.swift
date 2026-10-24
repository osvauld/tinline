import PDFKit
import SwiftUI

/// What the document viewer found in a file (android FileViewer.kt `Loaded`).
enum ChatLoadedFile {
    case loading
    case pdf(PDFDocument)
    case text([String])
    case error(String)
}

enum ChatFileLoader {
    /// Reads `url` for the viewer. PDFKit replaces Android's PdfRenderer: a locked or unreadable PDF gets the same friendly
    /// messages and keeps Save.
    static func load(_ url: URL, kind: ChatOpenKind) -> ChatLoadedFile {
        if kind == .pdf {
            guard let doc = PDFDocument(url: url) else { return .error("Tinline can’t read this PDF. It may be damaged. You can still save it.") }
            if doc.isLocked { return .error("This PDF is password-protected, so Tinline can’t show it. Save it to open it in another app.") }
            return .pdf(doc)
        }
        return loadText(url)
    }

    static func loadText(_ url: URL) -> ChatLoadedFile {
        let size = (try? FileManager.default.attributesOfItem(atPath: url.path)[.size] as? Int64) ?? 0
        if size > ChatFileOpen.textLimit { return .error("This file is too big to show here. Save it to open it in another app.") }
        guard let data = try? Data(contentsOf: url) else { return .error("Couldn’t open that file.") }
        if data.prefix(4096).contains(0) { return .error("This doesn’t look like a text file. Save it to open it in another app.") }
        var s = String(decoding: data, as: UTF8.self)
        if s.hasPrefix("\u{FEFF}") { s.removeFirst() }
        return .text(s.components(separatedBy: .newlines))
    }
}

/// Full-screen document viewer (PDF or plain text), always dark like the photo viewer. Unreadable files get a friendly
/// message and Save. Presented with `.fullScreenCover`.
struct ChatFileViewer: View {
    let message: Message
    let plan: ChatOpenPlan
    let source: ChatSource
    let onClose: () -> Void
    let onSave: () -> Void
    let onShare: () -> Void

    @State private var state: ChatLoadedFile = .loading
    @State private var page = 1
    @State private var now = Int64(Date().timeIntervalSince1970 * 1000)

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 6) {
                ChatIconButton(glyph: .x, label: "Close", tint: .white, action: onClose)
                VStack(alignment: .leading, spacing: 0) {
                    Text(message.attachment?.name ?? "File").font(TinType.bodyL.weight(.semibold)).foregroundStyle(.white).lineLimit(1).truncationMode(.tail)
                    Text(subtitle).font(TinType.caption).foregroundStyle(ChatPalette.viewerDim)
                }
                Spacer(minLength: 0)
                ChatIconButton(glyph: .share, label: "Share", tint: .white, action: onShare)
                ChatIconButton(glyph: .save, label: "Save to phone", tint: .white, action: onSave)
            }
            .frame(height: 64).padding(.horizontal, 4)
            ZStack {
                switch state {
                case .loading: Text("Opening…").font(TinType.bodyM).foregroundStyle(ChatPalette.viewerDim)
                case .error(let msg):
                    VStack(spacing: 16) {
                        ChatIcon(glyph: .warning, size: 24, color: ChatPalette.viewerDim)
                        Text(msg).font(TinType.bodyL).foregroundStyle(.white).multilineTextAlignment(.center)
                        Button(action: onSave) {
                            Text("Save to phone").font(TinType.bodyL.weight(.semibold)).foregroundStyle(.black)
                                .padding(.horizontal, 24).padding(.vertical, 12).background(.white, in: Capsule())
                        }
                    }.padding(32)
                case .pdf(let doc): ChatPDFPages(doc: doc, page: $page)
                case .text(let lines): ChatTextLines(lines: lines)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .background(Color.black.ignoresSafeArea())
        .task {
            let source = source, message = message, kind = plan.kind
            guard let f = try? await ChatMedia.decrypt(source: source, message: message) else { state = .error("Couldn’t open that file."); return }
            state = await Task.detached { ChatFileLoader.load(f, kind: kind) }.value
        }
    }

    private var subtitle: String {
        if case .pdf(let d) = state { return "Page \(page) of \(d.pageCount)" }
        return ChatFmt.viewerWhen(Int64(message.at), now: now)
    }
}

/// The pages stacked in one scrolling view, with the page number following the scroll (PDFView does both).
private struct ChatPDFPages: UIViewRepresentable {
    let doc: PDFDocument
    @Binding var page: Int

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeUIView(context: Context) -> PDFView {
        let v = PDFView()
        v.document = doc
        v.autoScales = true
        v.displayMode = .singlePageContinuous
        v.displayDirection = .vertical
        v.backgroundColor = UIColor(red: 0x1E / 255, green: 0x25 / 255, blue: 0x22 / 255, alpha: 1)
        NotificationCenter.default.addObserver(context.coordinator, selector: #selector(Coordinator.changed(_:)), name: .PDFViewPageChanged, object: v)
        return v
    }

    func updateUIView(_ v: PDFView, context: Context) {}

    final class Coordinator: NSObject {
        var parent: ChatPDFPages
        init(_ p: ChatPDFPages) { parent = p }
        @objc func changed(_ n: Notification) {
            guard let v = n.object as? PDFView, let cur = v.currentPage, let doc = v.document else { return }
            parent.page = doc.index(for: cur) + 1
        }
    }
}

private struct ChatTextLines: View {
    let lines: [String]
    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 0) {
                ForEach(Array(lines.enumerated()), id: \.offset) { _, l in
                    Text(l.isEmpty ? " " : l).font(.custom("IBMPlexMono-Medium", size: 13, relativeTo: .footnote))
                        .foregroundStyle(Color(hex: 0xE6ECE9)).frame(maxWidth: .infinity, alignment: .leading)
                }
            }.padding(16)
        }
    }
}

/// Full-screen photo, always dark. Share and Save decrypt a copy; the original stays encrypted in the core.
struct ChatPhotoViewer: View {
    let message: Message
    let name: String
    let source: ChatSource
    let onClose: () -> Void
    let onSave: () -> Void
    let onShare: () -> Void
    @State private var image: UIImage?

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 6) {
                ChatIconButton(glyph: .x, label: "Close", tint: .white, action: onClose)
                VStack(alignment: .leading, spacing: 0) {
                    Text(message.outgoing ? "You" : name).font(TinType.bodyL.weight(.semibold)).foregroundStyle(.white)
                    Text(ChatFmt.viewerWhen(Int64(message.at), now: Int64(Date().timeIntervalSince1970 * 1000)))
                        .font(TinType.caption).foregroundStyle(ChatPalette.viewerDim)
                }
                Spacer(minLength: 0)
                ChatIconButton(glyph: .share, label: "Share", tint: .white, action: onShare)
                ChatIconButton(glyph: .save, label: "Save to phone", tint: .white, action: onSave)
            }
            .frame(height: 64).padding(.horizontal, 4)
            ZStack {
                if let image {
                    Image(uiImage: image).resizable().scaledToFit().accessibilityLabel(message.attachment?.name ?? "Photo")
                }
            }.frame(maxWidth: .infinity, maxHeight: .infinity)
            HStack(alignment: .top, spacing: 8) {
                ChatIcon(glyph: .lock, size: 18, color: ChatPalette.viewerDim)
                Text("Kept encrypted inside Tinline. “Save to phone” copies it to your photos, where other apps can see it.")
                    .font(TinType.bodyM).foregroundStyle(ChatPalette.viewerDim)
            }
            .padding(.horizontal, 20).padding(.top, 16).padding(.bottom, 24)
        }
        .background(Color.black.ignoresSafeArea())
        .task {
            let source = source, message = message
            if let f = try? await ChatMedia.decrypt(source: source, message: message) {
                image = await Task.detached { ChatMedia.image(f, maxPx: 2048) }.value
            }
        }
    }
}
