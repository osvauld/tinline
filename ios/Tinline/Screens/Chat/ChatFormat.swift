import SwiftUI

// Chat formatting, tokens and small pure helpers. Ports of android ChatUi.kt, the conversation tokens of Theme.kt, and
// the shape maths of Geometry.kt. Everything here is plain Swift so ChatFormatTests can run it without a UI.

/// How we reach a contact right now (android ChatSource.kt `Link`). The core has no per-peer "connected" call yet, so
/// the real source only knows `.unknown` until traffic from that peer proves a link; the UI never claims "online"
/// without proof.
enum ChatLink { case connected, relayed, offline, unknown }

/// Position in a run of same-sender messages.
enum BubblePos { case solo, first, mid, last }

/// Honest status of a message we sent, for words ("Waiting"). The bubble draws the receipt tick itself
/// (`ReceiptTickView`). `.two` and `.read` come only from the contact's signed receipts (core `ReceiptTick`); `.one` =
/// sent or saved here, not confirmed yet; `.clock` = it cannot go out now (waiting until you are both online).
enum ChatTick { case one, two, read, clock }

/// The tick for a message we sent. `receipt` is the core's decision; the link only says whether a message still on this
/// phone can go out now (a clock when the link is offline, or with no signal at all and this phone offline or the
/// message has sat a while).
func chatTickOf(_ m: Message, receipt: ReceiptTick?, link: ChatLink, online: Bool, nowMs: Int64) -> ChatTick {
    if receipt == .read { return .read }
    if receipt == .delivered { return .two }
    if m.delivery == .pending && link == .offline { return .clock }
    if m.delivery == .pending && link == .unknown && (!online || nowMs - Int64(m.at) > 30_000) { return .clock }
    return .one
}

/// Subtitle of the "You" conversation (saved items) in the list and its header.
let chatSelfSub = "Saved items · only on your devices"

/// Time and date wording for the conversation (android ChatUi.kt).
enum ChatFmt {
    static var calendar: Calendar = .current

    private static func date(_ ms: Int64) -> Date { Date(timeIntervalSince1970: Double(ms) / 1000) }

    /// "16:02", 24-hour as on Android (`HH:mm`).
    static func clock(_ ms: Int64) -> String {
        let c = calendar.dateComponents([.hour, .minute], from: date(ms))
        return String(format: "%02d:%02d", c.hour ?? 0, c.minute ?? 0)
    }

    static func sameDay(_ a: Int64, _ b: Int64) -> Bool { calendar.isDate(date(a), inSameDayAs: date(b)) }

    private static func daysAgo(_ ms: Int64, _ nowMs: Int64) -> Int {
        let a = calendar.startOfDay(for: date(ms)), b = calendar.startOfDay(for: date(nowMs))
        return calendar.dateComponents([.day], from: a, to: b).day ?? 0
    }

    /// Day pill in a conversation (Conversation.svelte `dayLabel`): "Today", "Yesterday", else "Thursday 8 Oct" (with the
    /// year when it isn't this year).
    static func dayLabel(_ ms: Int64, now nowMs: Int64) -> String {
        let n = daysAgo(ms, nowMs)
        if n == 0 { return "Today" }
        if n == 1 { return "Yesterday" }
        if n < 0 { return "Today" }
        let sameYear = calendar.component(.year, from: date(ms)) == calendar.component(.year, from: date(nowMs))
        let f = DateFormatter()
        f.calendar = calendar
        f.locale = .current
        f.dateFormat = sameYear ? "EEEE d MMM" : "EEEE d MMM yyyy"
        return f.string(from: date(ms))
    }

    /// The viewer's top line: "Today, 16:02".
    static func viewerWhen(_ ms: Int64, now nowMs: Int64) -> String { "\(dayLabel(ms, now: nowMs)), \(clock(ms))" }

    /// "2.4 MB", "34 MB", "812 KB" (`present.bytes`).
    static func size(_ count: Int64) -> String { bytes(n: UInt64(max(0, count))) }

    /// "PDF", "MP4", ... from the name's extension, else the MIME subtype.
    static func typeLabel(_ a: Attachment) -> String {
        let ext = (a.name as NSString).pathExtension
        if (1...5).contains(ext.count) { return ext.uppercased() }
        let sub = a.mime.split(separator: "/", maxSplits: 1).dropFirst().first.map(String.init) ?? ""
        return String(sub.uppercased().prefix(5))
    }

    /// First word of a name, for "Arjun deleted this message".
    static func firstName(_ name: String) -> String {
        let t = name.trimmingCharacters(in: .whitespaces)
        let w = t.split(separator: " ", maxSplits: 1).first.map(String.init) ?? ""
        return w.isEmpty ? name : w
    }

    static func isImage(_ a: Attachment) -> Bool { a.kind == .file && a.mime.hasPrefix("image/") }
    static func isVideo(_ a: Attachment) -> Bool { a.kind == .file && a.mime.hasPrefix("video/") }

    /// Call pill wording (Conversation.kt `CallPill`).
    static func callPillText(_ r: CallRecord) -> String {
        let answered = r.durationSecs > 0
        if answered { return "\(r.video ? "Video" : "Voice") call · \(Int(r.durationSecs) / 60):\(String(format: "%02d", Int(r.durationSecs) % 60))" }
        if r.incoming { return "\(r.reason == "declined_local" ? "Declined" : "Missed") \(r.video ? "video" : "voice") call" }
        switch r.reason {
        case "no_answer": return "No answer"
        case "declined": return "Declined"
        case "cancelled", "hangup_local": return "Cancelled call"
        default: return "Call failed"
        }
    }

    /// A photo's box in its bubble: the true shape from the size the core sends (chat/5), at most 248 x 330, at least 120
    /// wide; 4:3 when unknown (photos from older apps).
    static func photoBox(width w: UInt32, height h: UInt32) -> CGSize {
        let (pw, ph): (CGFloat, CGFloat) = (w > 0 && h > 0) ? (CGFloat(w), CGFloat(h)) : (4, 3)
        let s = min(248 / pw, 330 / ph)
        return CGSize(width: max(120, pw * s), height: ph * s)
    }

    /// The core's 4x3 colour preview as 12 RGB pixels, or nil when it is not exactly 36 bytes.
    static func previewPixels(_ p: Data) -> [(UInt8, UInt8, UInt8)]? {
        guard p.count == 36 else { return nil }
        let b = [UInt8](p)
        return (0..<12).map { (b[$0 * 3], b[$0 * 3 + 1], b[$0 * 3 + 2]) }
    }

    /// Peaks resampled to `n` bars (max of each slice), 0...1.
    static func bars(_ wave: Data, _ n: Int) -> [Float] {
        let w = [UInt8](wave)
        return (0..<n).map { i in
            if w.isEmpty { return 0 }
            let lo = i * w.count / n
            let hi = min(w.count, max(lo + 1, (i + 1) * w.count / n))
            var m: UInt8 = 0
            for k in lo..<hi { m = max(m, w[k]) }
            return Float(m) / 255
        }
    }
}

/// The conversation tokens tokens.css has beyond `TinColors` (android Theme.kt extensions). A value type built from the
/// environment's `tin`, so no `TinColors` extension is added that another area might also add.
struct ChatPalette {
    let t: TinColors
    init(_ t: TinColors) { self.t = t }
    private var dark: Bool { t.dark }

    /// --accent: filled primary surfaces. Dark is #117e6b, not `pr` (which is --accent-ink there).
    var accentFill: Color { dark ? Color(hex: 0x117E6B) : Color(hex: 0x0B6B5B) }
    var onAccentFill: Color { dark ? Color(hex: 0xF2FFFB) : .white }
    /// --accent-ink: teal as text and icons.
    var accentInk: Color { dark ? Color(hex: 0x6FD3BD) : Color(hex: 0x0A5D50) }
    var accentSoft: Color { dark ? Color(hex: 0x14433A) : Color(hex: 0xD7ECE6) }
    var hairline: Color { dark ? Color(hex: 0x2A3531) : Color(hex: 0xE2E7E1) }
    /// --canvas: the conversation's scroll area behind the bubbles.
    var canvas: Color { dark ? Color(hex: 0x0B100E) : Color(hex: 0xEDEFE9) }
    /// --bub-meta-in / --bub-meta-out: the inline time and status in a received / sent bubble.
    var bubMetaIn: Color { dark ? Color(hex: 0x95A19B) : Color(hex: 0x68746E) }
    var bubMetaOut: Color { dark ? Color(hex: 0x8FC9B9) : Color(hex: 0x33655A) }
    /// The small "Not delivered" label in the bubble meta (11 pt text needs 4.5 : 1).
    var tickBadText: Color { dark ? Color(hex: 0xFF9A9A) : t.er }
    /// The viewers are always dark, like the playground's.
    static let viewerDim = Color(hex: 0xA6B2AD)
}

/// Motion tokens (playground src/lib/fx/motion.ts), in seconds.
enum ChatMotion {
    static let press = 0.100, release = 0.140, `switch` = 0.120, enter = 0.200
    /// cubic-bezier(.2, .8, .2, 1).
    static let out = Animation.timingCurve(0.2, 0.8, 0.2, 1)
    static func animation(_ d: Double) -> Animation { .timingCurve(0.2, 0.8, 0.2, 1, duration: d) }
}

/// Bubble outline for a message at `pos` in its run. `mine`: stack against the end edge, else the start edge (mirrors in
/// RTL, as UnevenRoundedRectangle's leading/trailing do). `inset`: the same outline that far in (a photo inside its 3 pt
/// rim), each corner at least 3 pt. Continuous corners stand in for the Figma-style smoothing of `SmoothCornerShape`.
func chatBubbleShape(_ pos: BubblePos, mine: Bool, inset: CGFloat = 0) -> UnevenRoundedRectangle {
    let o = max(18 - inset, 3), t = max(6 - inset, 3)
    let top = (pos == .mid || pos == .last) ? t : o
    let bot = (pos == .mid || pos == .first) ? t : o
    return mine
        ? UnevenRoundedRectangle(topLeadingRadius: o, bottomLeadingRadius: o, bottomTrailingRadius: bot, topTrailingRadius: top, style: .continuous)
        : UnevenRoundedRectangle(topLeadingRadius: top, bottomLeadingRadius: bot, bottomTrailingRadius: o, topTrailingRadius: o, style: .continuous)
}

/// Spacing between rows (MessageRow.svelte `.m`): 10 above a run, 2 inside one.
func chatGapAbove(_ pos: BubblePos) -> CGFloat { (pos == .mid || pos == .last) ? 2 : 10 }

/// Icons for the conversation: the generated "Tinline line" set (`TinIcons`, crates/tin-icons), by the names the Android
/// chat screens use (`Pg.*`).
enum ChatGlyph {
    case back, search, send, clip, chev, chevUp, chevRight, info, x, reply, edit, trash, copy, check, clock, file, image
    case off, phone, mic, warning, video, lock, download, save, share, retry, play, pause, camera

    var icon: TinIconData {
        switch self {
        case .back: return TinIcons.back
        case .search: return TinIcons.search
        case .send: return TinIcons.send
        case .clip: return TinIcons.clip
        case .chev: return TinIcons.chev
        case .chevUp: return TinIcons.chevup
        case .chevRight: return TinIcons.chevr
        case .info: return TinIcons.info
        case .x: return TinIcons.x
        case .reply: return TinIcons.reply
        case .edit: return TinIcons.edit
        case .trash: return TinIcons.trash
        case .copy: return TinIcons.copy
        case .check: return TinIcons.check
        case .clock: return TinIcons.clock
        case .file: return TinIcons.file
        case .image: return TinIcons.image
        case .off: return TinIcons.off
        case .phone: return TinIcons.phone
        case .mic: return TinIcons.mic
        case .warning: return TinIcons.warning
        case .video: return TinIcons.video
        case .lock: return TinIcons.lock
        case .download: return TinIcons.download
        case .save: return TinIcons.save
        case .share: return TinIcons.share
        case .retry: return TinIcons.retry
        case .play: return TinIcons.play
        case .pause: return TinIcons.pause
        case .camera: return TinIcons.camera
        }
    }
}

/// A glyph at a size and colour (decorative: the button or row around it carries the label).
struct ChatIcon: View {
    let glyph: ChatGlyph
    var size: CGFloat = 22
    var color: Color?
    var body: some View { TinIcon(glyph.icon, size: size, tint: color).frame(width: size, height: size).accessibilityHidden(true) }
}

/// The playground's `.ib`: a 44 round icon button, 22 icon, dimmed (.4) when disabled.
struct ChatIconButton: View {
    let glyph: ChatGlyph
    let label: String
    var tint: Color?
    var enabled = true
    var iconSize: CGFloat = 22
    var dimAlpha = 0.4
    var clickableWhenDim = false
    let action: () -> Void
    var body: some View {
        Button(action: action) {
            ChatIcon(glyph: glyph, size: iconSize, color: tint).frame(width: 44, height: 44).contentShape(Circle())
        }
        .buttonStyle(ChatPressStyle())
        .opacity(enabled ? 1 : dimAlpha)
        .allowsHitTesting(enabled || clickableWhenDim)
        .accessibilityLabel(label)
    }
}

/// The playground's `.press` feel: a quick scale to .97 while pressed, back on release.
struct ChatPressStyle: ButtonStyle {
    @Environment(\.accessibilityReduceMotion) private var reduce
    func makeBody(configuration: Configuration) -> some View {
        configuration.label.scaleEffect(configuration.isPressed && !reduce ? 0.97 : 1)
            .animation(ChatMotion.animation(configuration.isPressed ? ChatMotion.press : ChatMotion.release), value: configuration.isPressed)
    }
}
