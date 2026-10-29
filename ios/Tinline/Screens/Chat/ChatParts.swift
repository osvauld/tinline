import SwiftUI
import UIKit

/*
 * The conversation's building blocks, ported from the playground (screens/chat/MessageRow.svelte, Composer.svelte,
 * Layer.svelte, Conversation.svelte) through android ChatParts.kt. Shared by the 1:1 conversation and, through these
 * public parts, by group chats. Values are the playground's computed ones; the comments name the CSS they come from.
 */

/// --fs-body / --lh-body.
enum ChatText {
    static func font(_ size: CGFloat = 15, weight: Font.Weight = .regular, relativeTo style: Font.TextStyle = .body) -> Font {
        .custom("Figtree", size: size, relativeTo: style).weight(weight)
    }
    static let body = font(15)
    static let meta = font(11).monospacedDigit()

    /// `text` with every emoji run a little larger than the letters around it. `present.emojiRuns` decides where the runs are.
    static func styled(_ text: String, size: CGFloat = 15) -> AttributedString {
        var out = AttributedString()
        let scale = CGFloat(emojiInlineScale())
        for run in emojiRuns(text: text) {
            var piece = AttributedString(run.text)
            if run.isEmoji { piece.font = font(size * scale) }
            out += piece
        }
        return out
    }

    /// The same text as UIKit sees it, for measuring where the last line ends (SwiftUI cannot say).
    static func measuring(_ text: String, size: CGFloat = 15) -> NSAttributedString {
        let base = uiFont(size)
        let out = NSMutableAttributedString()
        let scale = CGFloat(emojiInlineScale())
        for run in emojiRuns(text: text) {
            out.append(NSAttributedString(string: run.text, attributes: [.font: run.isEmoji ? uiFont(size * scale) : base]))
        }
        return out
    }

    static func uiFont(_ size: CGFloat) -> UIFont {
        let f = UIFont(name: "Figtree", size: size) ?? UIFont.systemFont(ofSize: size)
        return UIFontMetrics(forTextStyle: .body).scaledFont(for: f)
    }
}

struct ChatTextMetrics { let lineCount: Int; let width: CGFloat; let lastRight: CGFloat }

enum ChatTextMeasure {
    /// Lines, widest line and the right edge of the last line of `s` laid out in `maxWidth` (TextKit, no padding).
    /// A layout pass asks for the same text at the same width many times (and again on every re-render), each answer a
    /// whole TextKit layout: keep the answers. Keyed by content, width and the content size category (fonts scale).
    private final class Box { let m: ChatTextMetrics; init(_ m: ChatTextMetrics) { self.m = m } }
    private static let cache: NSCache<NSString, Box> = { let c = NSCache<NSString, Box>(); c.countLimit = 2000; return c }()

    static func measure(_ s: NSAttributedString, maxWidth: CGFloat) -> ChatTextMetrics {
        let key = "\(s.hash)|\(s.length)|\(maxWidth.isFinite ? Int(maxWidth * 4) : -1)|\(UIApplication.shared.preferredContentSizeCategory.rawValue)" as NSString
        if let hit = cache.object(forKey: key) { return hit.m }
        let m = layout(s, maxWidth: maxWidth)
        cache.setObject(Box(m), forKey: key)
        return m
    }

    private static func layout(_ s: NSAttributedString, maxWidth: CGFloat) -> ChatTextMetrics {
        let storage = NSTextStorage(attributedString: s)
        let lm = NSLayoutManager()
        let tc = NSTextContainer(size: CGSize(width: maxWidth.isFinite ? maxWidth : 100_000, height: .greatestFiniteMagnitude))
        tc.lineFragmentPadding = 0
        lm.addTextContainer(tc)
        storage.addLayoutManager(lm)
        lm.ensureLayout(for: tc)
        var lines = 0
        var widest: CGFloat = 0, lastRight: CGFloat = 0
        lm.enumerateLineFragments(forGlyphRange: lm.glyphRange(for: tc)) { _, used, _, _, _ in
            lines += 1; widest = max(widest, used.maxX); lastRight = used.maxX
        }
        return ChatTextMetrics(lineCount: max(1, lines), width: widest, lastRight: lastRight)
    }
}

/// My message's receipt mark: the core's tick, the message it belongs to (the glyph animates only when that message's
/// state changes) and what a screen reader hears. `reserve` with no `tick` keeps the slot while it loads.
struct ChatMetaReceipt {
    var tick: ReceiptTick?
    var key: String
    var label: String
    var reserve = true
    /// A block stopped it: the same mark in the danger ink (`ReceiptTickView(notDelivered:)`).
    var notDelivered = false
}

/// The inline time and status of a bubble: `edited` time tick.
struct ChatBubbleMeta {
    let time: String
    var edited = false
    var receipt: ChatMetaReceipt?
}

/// The receipt mark in its fixed slot (two ticks wide, so a status change never narrows the bubble).
struct ChatReceiptMark: View {
    let mark: ChatMetaReceipt?
    let color: Color
    var body: some View {
        if let m = mark, (m.tick != nil || m.reserve || m.notDelivered) {
            if m.tick == nil && !m.notDelivered {
                Color.clear.frame(width: tickSlotWidth(12), height: 12)
            } else {
                ReceiptTickView(tick: m.tick ?? ReceiptTick.none, size: 12, key: m.key, notDelivered: m.notDelivered, meta: color, label: m.label)
            }
        }
    }
}

struct ChatMeta: View {
    let meta: ChatBubbleMeta
    let color: Color
    var body: some View {
        HStack(spacing: 3) {
            if meta.edited { Text(tr("chat_edited")).font(ChatText.meta).foregroundStyle(color).lineLimit(1) }
            Text(meta.time).font(ChatText.meta).foregroundStyle(color).lineLimit(1)
            ChatReceiptMark(mark: meta.receipt, color: color)
        }
    }
}

/// Text with the meta floated at the end of its last line (`.meta { float: right; margin: 10px -2px -2px 10px }`):
/// inline when it fits beside the last line, else on a row of its own that adds 19. A bubble whose text wraps, or whose
/// one line plus meta does not fit, takes the full width, as CSS shrink-to-fit does. The first subview is the body text,
/// the second the meta.
struct ChatInlineMetaLayout: Layout {
    let measure: (CGFloat) -> ChatTextMetrics

    private struct Resolved { var width: CGFloat; var inline: Bool; var body: CGSize; var meta: CGSize }

    private func resolve(width given: CGFloat?, minWidth: CGFloat = 0, _ subviews: Subviews) -> Resolved {
        let maxW = given ?? .infinity
        let meta = subviews[1].sizeThatFits(.unspecified)
        let box = meta.width + 8   // 10 left margin, -2 right margin
        let m = measure(maxW)
        let textW = ceil(m.width)
        let natural: CGFloat = (m.lineCount == 1 && textW + box <= maxW) || maxW == .infinity ? textW + box : maxW
        let w = given == nil ? max(natural, minWidth) : max(min(natural, maxW), minWidth)
        let inline = ceil(m.lastRight) + box <= w
        let body = subviews[0].sizeThatFits(ProposedViewSize(width: w, height: nil))
        return Resolved(width: w, inline: inline, body: body, meta: meta)
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let r = resolve(width: proposal.width, subviews)
        return CGSize(width: r.width, height: r.inline ? r.body.height : r.body.height + 19)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        // Placed at the width the parent settled on (a quote may have widened the bubble).
        var r = resolve(width: bounds.width, subviews)
        r.width = bounds.width
        let inline = ceil(measure(bounds.width).lastRight) + r.meta.width + 8 <= bounds.width
        let my = inline ? r.body.height - r.meta.height : r.body.height + 21 - r.meta.height
        subviews[0].place(at: bounds.origin, proposal: ProposedViewSize(width: bounds.width, height: nil))
        subviews[1].place(at: CGPoint(x: bounds.minX + bounds.width - r.meta.width + 2, y: bounds.minY + my), proposal: .unspecified)
    }
}

/// A column whose width is its widest child (CSS shrink-to-fit with block children), every child then laid out at that
/// width: a quote spans the bubble, and the meta floats to the bubble's end.
struct ChatShrinkStack: Layout {
    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let w = width(proposal, subviews)
        let h = subviews.reduce(CGFloat(0)) { $0 + $1.sizeThatFits(ProposedViewSize(width: w, height: nil)).height }
        return CGSize(width: w, height: h)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var y = bounds.minY
        for s in subviews {
            let size = s.sizeThatFits(ProposedViewSize(width: bounds.width, height: nil))
            s.place(at: CGPoint(x: bounds.minX, y: y), proposal: ProposedViewSize(width: bounds.width, height: nil))
            y += size.height
        }
    }

    private func width(_ proposal: ProposedViewSize, _ subviews: Subviews) -> CGFloat {
        let widest = subviews.map { $0.sizeThatFits(.unspecified).width }.max() ?? 0
        return min(widest, proposal.width ?? .infinity)
    }
}

/// The text of a bubble with its meta floated to the end of the last line.
struct ChatTextWithMeta<Meta: View>: View {
    let text: AttributedString
    let measuring: NSAttributedString
    let color: Color
    var font: Font = ChatText.body
    @ViewBuilder let meta: () -> Meta

    var body: some View {
        ChatInlineMetaLayout(measure: { ChatTextMeasure.measure(measuring, maxWidth: $0) }) {
            Text(text).font(font).foregroundStyle(color).fixedSize(horizontal: false, vertical: true)
            meta()
        }
    }
}

/// The bubble surface (MessageRow.svelte `.bub` + `.skin`): outline by run position, sent = --bub-out, received =
/// --bub-in with a 1 pt --bub-in-line stroke, deleted = a hairline outline only. 7/12 padding plus the 1 px border = 8/13.
/// `held` scales to .97 (the long-press sheet is open for it).
struct ChatBubbleSurface<Content: View>: View {
    let pos: BubblePos
    let mine: Bool
    var gone = false
    var held = false
    var padding = EdgeInsets(top: 8, leading: 13, bottom: 8, trailing: 13)
    @ViewBuilder let content: () -> Content
    @Environment(\.tin) private var tin
    @Environment(\.accessibilityReduceMotion) private var reduce

    var body: some View {
        let p = ChatPalette(tin)
        let shape = chatBubbleShape(pos, mine: mine)
        content().padding(padding)
            .background(shape.fill(gone ? Color.clear : (mine ? tin.prc : tin.bubIn)))
            .overlay(shape.strokeBorder(gone ? p.hairline : (mine ? Color.clear : tin.bubInLine), lineWidth: 1))
            .clipShape(shape)
            .scaleEffect(held && !reduce ? 0.97 : 1)
            .animation(ChatMotion.animation(ChatMotion.release), value: held)
    }
}

/// Bubble ink and meta colours.
func chatBubbleInk(_ tin: TinColors, mine: Bool) -> Color { mine ? tin.onPrc : tin.ink }
func chatBubbleMetaColor(_ tin: TinColors, mine: Bool) -> Color { mine ? ChatPalette(tin).bubMetaOut : ChatPalette(tin).bubMetaIn }

/// A text message: the bubble with its text and inline meta, or, for a few emojis only, big emoji with no surface.
struct ChatTextBubble: View {
    let text: String
    var display: AttributedString?
    let pos: BubblePos
    let mine: Bool
    let meta: ChatBubbleMeta
    var emoji = false
    var held = false
    var sender: String?
    var senderColor: Color?
    var quote: AnyView?
    @Environment(\.tin) private var tin

    var body: some View {
        let metaColor = chatBubbleMetaColor(tin, mine: mine)
        let ink = chatBubbleInk(tin, mine: mine)
        if emoji {
            // .bub.emoji: no surface, 34/42, the meta below at the end, muted.
            VStack(alignment: .trailing, spacing: 0) {
                Text(text.trimmingCharacters(in: .whitespacesAndNewlines)).font(ChatText.font(34)).foregroundStyle(ink)
                ChatMeta(meta: meta, color: tin.ink2)
            }.padding(.horizontal, 2)
        } else {
            let line = ChatTextWithMeta(text: display ?? ChatText.styled(text), measuring: ChatText.measuring(text), color: ink) {
                ChatMeta(meta: meta, color: metaColor)
            }
            ChatBubbleSurface(pos: pos, mine: mine, held: held) {
                if sender == nil && quote == nil { line }
                else {
                    ChatShrinkStack {
                        if let sender {
                            Text(sender).font(ChatText.font(13, weight: .bold)).foregroundStyle(senderColor ?? ChatPalette(tin).accentInk)
                                .lineLimit(1).padding(.bottom, 1).frame(maxWidth: .infinity, alignment: .leading)
                        }
                        if let quote { quote }
                        line
                    }
                }
            }
        }
    }
}

/// The quote of the message replied to (`.quote`): surface-2, 3 pt accent bar at the start, smooth 12 corners.
struct ChatQuote: View {
    let who: String?
    let whoColor: Color
    let text: String
    var onTap: (() -> Void)?
    @Environment(\.tin) private var tin

    var body: some View {
        let p = ChatPalette(tin)
        let content = VStack(alignment: .leading, spacing: 0) {
            if let who { Text(who).font(ChatText.font(12, weight: .bold)).foregroundStyle(whoColor).lineLimit(1) }
            Text(text).font(ChatText.font(13)).foregroundStyle(tin.ink2).lineLimit(2).truncationMode(.tail)
        }
        .padding(.leading, 13).padding(.trailing, 10).padding(.top, 5).padding(.bottom, 6)
        .frame(maxWidth: .infinity, minHeight: 44, alignment: .leading)
        .background(tin.sf2)
        .overlay(alignment: .leading) { Rectangle().fill(p.accentFill).frame(width: 3) }
        .clipShape(SmoothRoundedRectangle(cornerRadius: 12))
        .padding(.top, 2).padding(.bottom, 6)
        if let onTap { Button(action: onTap) { content }.buttonStyle(.plain).accessibilityHint(tr("chat_jump_hint")) } else { content }
    }
}

/// Day separator (`.day`): a centred pill, surface with an inset hairline, 11 semibold muted.
struct ChatDayPill: View {
    let label: String
    @Environment(\.tin) private var tin
    var body: some View {
        Text(label).font(ChatText.font(11, weight: .semibold)).foregroundStyle(tin.ink2).lineLimit(1)
            .padding(.horizontal, 10).padding(.vertical, 3)
            .background(Capsule().fill(tin.sf)).overlay(Capsule().strokeBorder(ChatPalette(tin).hairline, lineWidth: 1))
            .frame(maxWidth: .infinity)
    }
}

/// "2 new messages" (`.unread`): accent-ink label between two thin lines.
struct ChatUnreadDivider: View {
    let n: Int
    @Environment(\.tin) private var tin
    var body: some View {
        let ink = ChatPalette(tin).accentInk
        let label = trn("chat_unread_divider", n)
        HStack(spacing: 10) {
            Rectangle().fill(ink.opacity(0.35)).frame(height: 1)
            Text(label).font(ChatText.font(11, weight: .bold)).foregroundStyle(ink)
            Rectangle().fill(ink.opacity(0.35)).frame(height: 1)
        }
        .padding(.top, 12).padding(.bottom, 6).accessibilityElement(children: .ignore).accessibilityLabel(label)
    }
}

/// The status strip over the top of the list (`.strip`); `warn` is the gold "waiting for a host" look.
struct ChatStrip: View {
    let text: String
    let glyph: ChatGlyph
    var warn = false
    @Environment(\.tin) private var tin
    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                ChatIcon(glyph: glyph, size: 16, color: warn ? tin.onWarn : tin.ink2)
                Text(text).font(ChatText.font(13)).foregroundStyle(warn ? tin.onWarn : tin.ink2)
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 16).padding(.vertical, 7).background(warn ? tin.warn : tin.sf2)
            Rectangle().fill(ChatPalette(tin).hairline).frame(height: 1)
        }
    }
}

// MARK: composer

/// The context chip over the field (reply / edit).
struct ChatComposerCtx { let title: String; let text: String; let cancelLabel: String }

/// Composer (Composer.svelte): [attach] [field] [send], 6 apart, bottom-aligned; the field is a smooth 22 pt pill on the
/// surface with a hairline that turns accent with a soft glow on focus; it grows with the text up to 140. The send button
/// is the accent circle, dimmed to .38 while there is nothing to send; `trailing` replaces it (the voice button) then.
/// While `recording` the field is replaced by `recordingBar`.
struct ChatComposer<Recording: View, Trailing: View>: View {
    @Binding var draft: String
    let placeholder: String
    let ctx: ChatComposerCtx?
    let editing: Bool
    var recording = false
    var showTrailing = true
    let onCancelCtx: () -> Void
    let onSend: () -> Void
    var onAttach: (() -> Void)?
    var focus: FocusState<Bool>.Binding
    @ViewBuilder let recordingBar: () -> Recording
    @ViewBuilder let trailing: () -> Trailing
    @Environment(\.tin) private var tin

    var body: some View {
        let empty = draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        VStack(spacing: 0) {
            if let ctx {
                HStack(spacing: 0) {
                    if onAttach != nil { Spacer().frame(width: 50) }
                    ChatCtxChip(ctx: ctx, onCancel: onCancelCtx).padding(.bottom, 6)
                    Spacer().frame(width: 50)
                }
            }
            HStack(alignment: .bottom, spacing: 6) {
                if let onAttach { ChatIconButton(glyph: .clip, label: tr("chat_attach"), tint: tin.ink2, enabled: !editing, dimAlpha: 0.38, action: onAttach) }
                if recording { recordingBar().frame(maxWidth: .infinity) }
                else { ChatField(draft: $draft, placeholder: placeholder, focus: focus, ctxActive: ctx != nil, onSend: { if !empty { onSend() } }, onEscape: onCancelCtx) }
                if empty && !editing && showTrailing { trailing() } else { ChatSendButton(editing: editing, empty: empty, onSend: onSend) }
            }
        }
        .padding(.leading, 10).padding(.trailing, 10).padding(.top, 8).padding(.bottom, 10)
    }
}

struct ChatSendButton: View {
    let editing: Bool
    let empty: Bool
    let onSend: () -> Void
    @Environment(\.tin) private var tin
    var body: some View {
        let p = ChatPalette(tin)
        // The same 48 slot as the mic button it replaces, so the composer row keeps its height on the first letter.
        Button(action: onSend) {
            ChatIcon(glyph: editing ? .check : .send, size: 20, color: p.onAccentFill).padding(.trailing, editing ? 0 : 2)
                .frame(width: 44, height: 44).background(p.accentFill, in: Circle())
        }
        .buttonStyle(ChatPressStyle()).disabled(empty).opacity(empty ? 0.38 : 1)
        .frame(width: 48, height: 48).accessibilityLabel(editing ? tr("chat_save_edit") : tr("chat_send"))
    }
}

/// The message field: transparent text on a smooth-cornered surface, focus = accent hairline + 3 pt glow outside.
struct ChatField: View {
    @Binding var draft: String
    let placeholder: String
    var focus: FocusState<Bool>.Binding
    var ctxActive = false
    var onSend: () -> Void = {}
    var onEscape: () -> Void = {}
    @Environment(\.tin) private var tin

    var body: some View {
        let p = ChatPalette(tin)
        let focused = focus.wrappedValue
        let shape = SmoothRoundedRectangle(cornerRadius: 22)
        TextField(placeholder, text: $draft, axis: .vertical)
            .focused(focus).lineLimit(1...6).font(ChatText.body).foregroundStyle(tin.ink).tint(tin.pr)
            .textInputAutocapitalization(.sentences).accessibilityLabel(placeholder)
            .padding(.horizontal, 16).padding(.vertical, 11).frame(minHeight: 44)
            .background(shape.fill(tin.sf))
            .overlay(shape.stroke(focused ? p.accentFill : p.hairline, lineWidth: 1))
            .background(shape.stroke(p.accentInk.opacity(focused ? 0.14 : 0), lineWidth: 3).padding(-1.5).allowsHitTesting(false))
            .animation(ChatMotion.animation(ChatMotion.switch), value: focused)
            // A hardware keyboard: Cmd+Return sends, Escape drops the reply/edit chip.
            .onKeyPress(.return, phases: .down) { press in
                if press.modifiers.contains(.command) { onSend(); return .handled }
                return .ignored
            }
            .onKeyPress(.escape, phases: .down) { _ in
                if ctxActive { onEscape(); return .handled }
                return .ignored
            }
    }
}

/// Reply / edit chip (`.ctx`): surface-2, accent start bar, title in accent-ink, the text muted, a cancel button.
struct ChatCtxChip: View {
    let ctx: ChatComposerCtx
    let onCancel: () -> Void
    @Environment(\.tin) private var tin
    var body: some View {
        let p = ChatPalette(tin)
        HStack(spacing: 8) {
            VStack(alignment: .leading, spacing: 0) {
                Text(ctx.title).font(ChatText.font(13, weight: .semibold)).foregroundStyle(p.accentInk).lineLimit(1)
                Text(ctx.text).font(ChatText.font(13)).foregroundStyle(tin.ink2).lineLimit(1)
            }
            Spacer(minLength: 0)
            // CSS `margin-block: -4px`: the chip stays 4 + 36 + 4 tall.
            ChatIconButton(glyph: .x, label: ctx.cancelLabel, tint: tin.ink2, iconSize: 18, action: onCancel).padding(.vertical, -4)
        }
        .padding(.leading, 13).padding(.trailing, 2).padding(.vertical, 4)
        .background(tin.sf2)
        .overlay(alignment: .leading) { Rectangle().fill(p.accentFill).frame(width: 3) }
        .clipShape(SmoothRoundedRectangle(cornerRadius: 12))
    }
}

// MARK: sheets

/// The dialog body inside a sheet (`.dlg`): 20/20/16 padding, a 17/24 title.
struct ChatDialogBody<Content: View>: View {
    let title: String
    @ViewBuilder let content: () -> Content
    @Environment(\.tin) private var tin
    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(title).font(ChatText.font(17, weight: .semibold)).foregroundStyle(tin.ink).padding(.bottom, 4)
            content()
        }
        .padding(.leading, 20).padding(.trailing, 20).padding(.top, 20).padding(.bottom, 16).frame(maxWidth: .infinity, alignment: .leading)
    }
}

extension View {
    /// A bottom sheet as the playground's Layer: raised surface, 24 pt top corners, a drag notch (the system scrim).
    func chatSheetStyle(_ tin: TinColors) -> some View {
        self.presentationDetents([.medium, .large]).presentationDragIndicator(.visible)
            .presentationCornerRadius(24).presentationBackground(tin.sfRaised)
    }
}
