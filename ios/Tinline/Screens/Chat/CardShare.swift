import Combine
import CoreImage.CIFilterBuiltins
import CryptoKit
import SwiftUI
import UIKit

// Contact card sharing (android CardShare.kt): my QR code with Copy and Share, and the "Add <name>?" prompt that appears
// when a card is on the clipboard or was shared into the app.

/// Text shared into Tinline from another app ("Share -> Tinline"); the UI picks it up once the app is unlocked.
@MainActor
enum CardInbox {
    static let shared = CurrentValueSubject<String?, Never>(nil)

    /// Called by the app for text handed to it (a share extension or an opened URL carries the card as text).
    static func take(_ text: String?) { shared.send(text ?? "") }
}

private func cardHash(_ ticket: String) -> String {
    SHA256.hash(data: Data(ticket.utf8)).prefix(8).map { String(format: "%02x", $0) }.joined()
}

func peekCardOrNil(node: Node?, text: String?) -> CardPeek? {
    guard let node, let text, !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return nil }
    return node.peekCard(text: text)
}

enum ChatQR {
    /// A QR code of `text`, scaled to `px` with hard edges (no interpolation), in `fg` on white.
    static func image(_ text: String, px: Int, fg: UIColor) -> UIImage? {
        let f = CIFilter.qrCodeGenerator()
        f.message = Data(text.utf8)
        f.correctionLevel = "M"
        guard let out = f.outputImage else { return nil }
        let color = CIFilter.falseColor()
        color.inputImage = out
        color.color0 = CIColor(color: fg)
        color.color1 = CIColor(color: .white)
        guard let tinted = color.outputImage else { return nil }
        let scale = CGFloat(px) / tinted.extent.width
        let scaled = tinted.transformed(by: CGAffineTransform(scaleX: scale, y: scale), highQualityDownsample: false)
        guard let cg = CIContext().createCGImage(scaled, from: scaled.extent) else { return nil }
        return UIImage(cgImage: cg, scale: 1, orientation: .up)
    }
}

enum ChatCardButtonStyle { case filled, tonal, outlined, text }

/// A pill button for the card screens (the Components area's `TinButton` is not assumed): 48 tall by default.
struct ChatCardButton: View {
    let title: String
    var glyph: ChatGlyph?
    var style: ChatCardButtonStyle = .filled
    var enabled = true
    var height: CGFloat = 48
    var fill = true
    let action: () -> Void
    @Environment(\.tin) private var tin

    var body: some View {
        let p = ChatPalette(tin)
        let fg: Color = style == .filled ? p.onAccentFill : (style == .tonal ? tin.onPrc : (style == .outlined ? tin.pr : tin.pr))
        Button(action: action) {
            HStack(spacing: 8) {
                if let glyph { ChatIcon(glyph: glyph, size: 18, color: fg) }
                Text(title).font(TinType.button).foregroundStyle(fg).lineLimit(1)
            }
            .padding(.horizontal, 20).frame(maxWidth: fill ? .infinity : nil).frame(height: height)
            .background(Capsule().fill(style == .filled ? p.accentFill : (style == .tonal ? tin.prc : Color.clear)))
            .overlay(Capsule().strokeBorder(style == .outlined ? tin.ln2 : Color.clear, lineWidth: 1))
            .contentShape(Capsule())
        }
        .buttonStyle(ChatPressStyle()).disabled(!enabled).opacity(enabled ? 1 : 0.38)
    }
}

/// Copy button that says "Copied" with a check for two seconds.
struct ChatCopyButton: View {
    let text: String?
    var label = tr("chat_card_copy")
    var style: ChatCardButtonStyle = .outlined
    @State private var copied = false

    var body: some View {
        ChatCardButton(title: copied ? tr("chat_card_copied") : label, glyph: copied ? .check : .copy, style: style, enabled: text != nil) {
            if let text { UIPasteboard.general.string = text; copied = true }
        }
        .task(id: copied) { if copied { try? await Task.sleep(nanoseconds: 2_000_000_000); copied = false } }
    }
}

/// My QR code with Copy and Share right under it. Shared by the first-run home and the Add contact screen.
struct ChatMyCodeBlock: View {
    let node: Node?
    let meName: String
    var qr: CGFloat = 248
    var showNewCode = true
    @Environment(\.tin) private var tin
    @State private var ticket: String?
    @State private var err: String?
    @State private var version = 0

    var body: some View {
        VStack(spacing: 14) {
            VStack(spacing: 14) {
                Text(meName).font(TinType.titleL.weight(.bold)).foregroundStyle(tin.ink)
                ZStack {
                    SmoothRoundedRectangle(cornerRadius: 14).fill(.white)
                    if let t = ticket, let img = ChatQR.image(t, px: 720, fg: UIColor(red: 0x17 / 255, green: 0x20 / 255, blue: 0x1D / 255, alpha: 1)) {
                        Image(uiImage: img).interpolation(.none).resizable().scaledToFit().padding(10)
                            .accessibilityLabel(tr("chat_card_qr_label"))
                        // The mark in the middle: a white tile, the accent square, the mark in its own colours.
                        ZStack {
                            SmoothRoundedRectangle(cornerRadius: 10).fill(.white).frame(width: 40, height: 40)
                            SmoothRoundedRectangle(cornerRadius: 8).fill(Color(hex: 0x0B6B5B)).frame(width: 34, height: 34)
                            TinMark(size: 26, can: .white, string: Color(hex: 0xE8B04A), mouth: Color(hex: 0x0B6B5B))
                        }
                    } else {
                        Text(err ?? tr("chat_card_preparing")).font(TinType.bodyM).foregroundStyle(Color(hex: 0x4D5853))
                            .multilineTextAlignment(.center).padding(16)
                    }
                }
                .frame(width: qr, height: qr)
                Text(tr("chat_card_once")).font(TinType.bodyM).foregroundStyle(tin.ink2)
                    .multilineTextAlignment(.center).frame(maxWidth: 300)
            }
            .padding(20).frame(maxWidth: .infinity)
            .background(tin.sf, in: SmoothRoundedRectangle(cornerRadius: 24))
            .overlay(SmoothRoundedRectangle(cornerRadius: 24).stroke(tin.ln, lineWidth: 1))
            HStack(spacing: 10) {
                if let t = ticket {
                    ShareLink(item: t, subject: Text(tr("chat_card_share_subject"))) {
                        HStack(spacing: 8) { ChatIcon(glyph: .share, size: 18, color: tin.onPrc); Text(tr("chat_card_share")).font(TinType.button).foregroundStyle(tin.onPrc) }
                            .frame(maxWidth: .infinity).frame(height: 48).background(tin.prc, in: Capsule())
                    }
                } else {
                    ChatCardButton(title: tr("chat_card_share"), glyph: .share, style: .tonal, enabled: false) {}
                }
                ChatCopyButton(text: ticket)
            }
            if showNewCode {
                ChatCardButton(title: tr("chat_card_new_code"), glyph: .retry, style: .text, fill: false) {
                    let node = node
                    Task {
                        _ = await Task.detached { try? node?.resetTicket() }.value
                        ticket = nil; version += 1
                    }
                }
            }
        }
        .task(id: version) {
            let node = node
            do { ticket = try await Task.detached { try node?.myTicket() }.value; err = nil }
            catch { err = ChatErrors.friendly(error) }
        }
    }
}

/// Looks for a card on the clipboard when the app comes forward, and for text shared into the app, and offers "Add
/// <name>?". Draws nothing when there is nothing to offer.
/// `clipboardOk`: this screen may prompt from the clipboard (Home, Add; not in a call, not locked, not onboarding).
/// `shareOk`: a shared card may be handled now (unlocked, past onboarding).
///
/// iOS differs from Android here: reading the pasteboard text makes the system ask "Allow Paste". `hasStrings` and
/// `changeCount` do not, so the text is read only when something new was copied, once per coming forward.
/// TODO(ios-device): check how the paste prompt feels; the alternative is a `PasteButton` in the Add screen only.
struct ChatCardPrompts: View {
    let node: Node?
    let clipboardOk: Bool
    let shareOk: Bool
    let showOnScreen: Bool
    let onChat: (CardPeek) -> Void
    let onAdd: (CardPeek) -> Void

    @Environment(\.tin) private var tin
    @Environment(\.scenePhase) private var phase
    @State private var prompt: CardPeek?
    @State private var armed = true
    @State private var dismissed: String? = UserDefaults.standard.string(forKey: "card_prompt.dismissed")
    @State private var lastChange = UserDefaults.standard.integer(forKey: "card_prompt.change")

    var body: some View {
        ZStack {
            if let p = prompt, showOnScreen {
                VStack {
                    HStack(spacing: 12) {
                        TinAvatar(did: p.did, size: 44)
                        VStack(alignment: .leading, spacing: 0) {
                            Text(p.known ? (p.name.isEmpty ? tr("chat_card_chat_with_unnamed") : tr("chat_card_chat_with", p.name)) : (p.name.isEmpty ? tr("chat_card_add_name_unnamed") : tr("chat_card_add_name", p.name)))
                                .font(TinType.bodyL.weight(.bold)).foregroundStyle(tin.ink).lineLimit(1)
                            Text(p.known ? tr("chat_card_known_sub") : tr("chat_card_found_sub"))
                                .font(TinType.bodyM).foregroundStyle(tin.ink2)
                        }
                        Spacer(minLength: 0)
                        ChatCardButton(title: tr("chat_card_not_now"), style: .text, height: 44, fill: false) {
                            dismissed = cardHash(p.ticket)
                            UserDefaults.standard.set(dismissed, forKey: "card_prompt.dismissed")
                            prompt = nil
                        }
                        ChatCardButton(title: p.known ? tr("chat_card_open") : tr("chat_card_add"), height: 44, fill: false) {
                            prompt = nil
                            if p.known { onChat(p) } else { onAdd(p) }
                        }
                    }
                    .padding(.leading, 14).padding(.trailing, 8).padding(.vertical, 12)
                    .background(tin.sf, in: SmoothRoundedRectangle(cornerRadius: 20))
                    .shadow(color: .black.opacity(0.18), radius: 14, y: 6)
                    .padding(12)
                    Spacer()
                }
                .transition(.move(edge: .top).combined(with: .opacity))
            }
        }
        .animation(ChatMotion.animation(ChatMotion.enter), value: prompt)
        .onChange(of: phase) { _, new in if new == .active { armed = true; checkClipboard() } }
        .onChange(of: clipboardOk) { _, ok in if ok { checkClipboard() } }
        .onReceive(CardInbox.shared) { text in
            guard let text, shareOk else { return }
            CardInbox.shared.send(nil)
            if let p = peekCardOrNil(node: node, text: text) { prompt = p }
        }
        .onAppear { if clipboardOk { checkClipboard() } }
    }

    private func checkClipboard() {
        guard armed, clipboardOk else { return }
        let pb = UIPasteboard.general
        armed = false
        guard pb.hasStrings, pb.changeCount != lastChange else { return }
        lastChange = pb.changeCount
        UserDefaults.standard.set(lastChange, forKey: "card_prompt.change")
        guard let p = peekCardOrNil(node: node, text: pb.string) else { return }
        if cardHash(p.ticket) != dismissed && prompt == nil { prompt = p }
    }
}
