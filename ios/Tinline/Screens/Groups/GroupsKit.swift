import SwiftUI
import UIKit

/*
 * The groups area's building blocks (android GroupsKit.kt + GroupsUi.kt), ported from the playground
 * (docs/design/playground/src/screens/groups: Btn, Pill, Collapse, fold; lib/ui Badge, Icon). Values come from
 * tokens.css and lib/fx/motion.ts; the few colours TinColors doesn't name yet are the extension values below.
 * Everything is prefixed Groups so it cannot collide with another area's kit.
 */

// ------------------------------------------------------------------ tokens beyond TinColors (tokens.css)

extension TinColors {
    /// --accent: filled primary surfaces (badges, Answer). Dark is #117e6b, not `pr` (which is --accent-ink there).
    var grpAccent: Color { callButtonAnswer }
    var grpOnAccent: Color { dark ? Color(hex: 0xF2FFFB) : .white }
    /// --accent-ink: teal as text and icons.
    var grpAccentInk: Color { dark ? Color(hex: 0x6FD3BD) : Color(hex: 0x0A5D50) }
    /// --accent-soft: selected chips, the New group disc.
    var grpAccentSoft: Color { dark ? Color(hex: 0x14433A) : Color(hex: 0xD7ECE6) }
    var grpHairline: Color { dark ? Color(hex: 0x2A3531) : Color(hex: 0xE2E7E1) }
    /// --danger-btn: filled Decline / Leave / Flag.
    var grpDangerBtn: Color { callButtonEnd }
    var grpLav: Color { relayC }
    var grpLavInk: Color { dark ? Color(hex: 0xDFE3FF) : Color(hex: 0x374582) }
    var grpGoldInk: Color { dark ? Color(hex: 0xE8B04A) : Color(hex: 0x845A10) }
    /// --canvas: the conversation's scroll area behind the bubbles.
    var grpCanvas: Color { dark ? Color(hex: 0x0B100E) : Color(hex: 0xEDEFE9) }
    /// --bub-meta-in / --bub-meta-out.
    var grpBubMetaIn: Color { dark ? Color(hex: 0x95A19B) : Color(hex: 0x68746E) }
    var grpBubMetaOut: Color { dark ? Color(hex: 0x8FC9B9) : Color(hex: 0x33655A) }
}

// ------------------------------------------------------------------ motion (lib/fx/motion.ts)

enum GroupsMotion {
    static let press = 0.100, release = 0.140, switchT = 0.120, crossfade = 0.120, enter = 0.200, exit = 0.140
    static func easeOut(_ d: Double) -> Animation { .timingCurve(0.2, 0.8, 0.2, 1, duration: d) }
    static func easeIn(_ d: Double) -> Animation { .timingCurve(0.4, 0, 1, 0, duration: d) }
    /// svelte cubicOut / cubicIn, used by fold() for list inserts and removals.
    static func cubicOut(_ d: Double) -> Animation { .timingCurve(0.33, 1, 0.68, 1, duration: d) }
    static func cubicIn(_ d: Double) -> Animation { .timingCurve(0.32, 0, 0.67, 0, duration: d) }
    /// Enter animation that honours Reduce Motion: an instant jump to the end.
    static func enterAnim(_ reduced: Bool) -> Animation? { reduced ? nil : easeOut(enter) }
}

// ------------------------------------------------------------------ icons (lib/ui/icons.ts, 24 grid, stroke 1.8, round)

/// An icon is its playground path data; `GroupsIconShape` draws it scaled to a square, stroked 1.8 of 24 with round caps.
struct GroupsIcon: Equatable { let name: String; let d: String }

enum GroupsIcons {
    static let users = GroupsIcon(name: "users", d: "M5.5,8a3.5,3.5 0 1 0 7,0a3.5,3.5 0 1 0 -7,0M2.5 20c.5-3.5 3.2-5.5 6.5-5.5s6 2 6.5 5.5M14.5,9a2.5,2.5 0 1 0 5,0a2.5,2.5 0 1 0 -5,0M16.5 14.6c2.6.2 4.5 2 5 4.9")
    static let plus = GroupsIcon(name: "plus", d: "M12 5v14M5 12h14")
    static let chev = GroupsIcon(name: "chev", d: "M7 10l5 5 5-5")
    static let chevR = GroupsIcon(name: "chevr", d: "M10 7l5 5-5 5")
    static let back = GroupsIcon(name: "back", d: "M15 5l-7 7 7 7")
    static let userPlus = GroupsIcon(name: "userplus", d: "M5.5,8a3.5,3.5 0 1 0 7,0a3.5,3.5 0 1 0 -7,0M2.5 20c.5-3.5 3.2-5.5 6.5-5.5s6 2 6.5 5.5M19 8v6M16 11h6")
    static let link = GroupsIcon(name: "link", d: "M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1")
    static let server = GroupsIcon(name: "server", d: "M5,4h14a2,2 0 0 1 2,2v3a2,2 0 0 1 -2,2h-14a2,2 0 0 1 -2,-2v-3a2,2 0 0 1 2,-2zM5,13h14a2,2 0 0 1 2,2v3a2,2 0 0 1 -2,2h-14a2,2 0 0 1 -2,-2v-3a2,2 0 0 1 2,-2zM7 7.5v.01M7 16.5v.01")
    static let phone = GroupsIcon(name: "phone", d: "M5 4h3.5l1.5 4.5-2.2 1.4a11 11 0 0 0 6.3 6.3l1.4-2.2L20 15.5V19a1.5 1.5 0 0 1-1.6 1.5A16 16 0 0 1 3.5 5.6 1.5 1.5 0 0 1 5 4z")
    static let leave = GroupsIcon(name: "leave", d: "M10 4H5v16h5M15 8l4 4-4 4M19 12H9")
    static let flag = GroupsIcon(name: "flag", d: "M5 21V4M5 4h11l-2 4 2 4H5")
    static let chat = GroupsIcon(name: "chat", d: "M4 5h16v11H9l-5 4z")
    static let clock = GroupsIcon(name: "clock", d: "M3,12a9,9 0 1 0 18,0a9,9 0 1 0 -18,0M12 7v5l3 2")
    static let off = GroupsIcon(name: "off", d: "M3,12a9,9 0 1 0 18,0a9,9 0 1 0 -18,0M5.6 5.6l12.8 12.8")
    static let shield = GroupsIcon(name: "shield", d: "M12 3l8 3v6c0 5-3.5 8-8 9-4.5-1-8-4-8-9V6z")
    static let check = GroupsIcon(name: "check", d: "M5 12.5l4.5 4.5L19 7")
    static let x = GroupsIcon(name: "x", d: "M6 6l12 12M18 6L6 18")
    static let info = GroupsIcon(name: "info", d: "M3,12a9,9 0 1 0 18,0a9,9 0 1 0 -18,0M12 11v6M12 7.5v.01")
    static let send = GroupsIcon(name: "send", d: "M4 12l16-8-6 16-3-7z")
    static let paste = GroupsIcon(name: "paste", d: "M9 4h6v3H9zM7 5.5H5.5v15h13v-15H17")
    static let scan = GroupsIcon(name: "scan", d: "M4 8V5h3M17 5h3v3M20 16v3h-3M7 19H4v-3M4 12h16")
    static let share = GroupsIcon(name: "share", d: "M12 15V4M8 8l4-4 4 4M5 12v8h14v-8")
    static let copy = GroupsIcon(name: "copy", d: "M8 8h11v12H8zM5 16V4h11")
}

struct GroupsIconShape: Shape {
    let icon: GroupsIcon
    func path(in rect: CGRect) -> Path {
        let s = min(rect.width, rect.height) / 24
        return GroupsSVGPath.parse(icon.d).applying(CGAffineTransform(scaleX: s, y: s)).offsetBy(dx: rect.minX, dy: rect.minY)
    }
}

struct GroupsIconView: View {
    let icon: GroupsIcon
    var size: CGFloat = 20
    var color: Color
    var body: some View {
        GroupsIconShape(icon: icon)
            .stroke(color, style: StrokeStyle(lineWidth: 1.8 * size / 24, lineCap: .round, lineJoin: .round))
            .frame(width: size, height: size)
            .accessibilityHidden(true)
    }
}

/// Minimal SVG path reader for the icon data: M L H V C S A Z, absolute and relative, with implicit repeats.
enum GroupsSVGPath {
    static func parse(_ d: String) -> Path {
        var tokens: [Token] = []
        let chars = Array(d)
        var i = 0
        while i < chars.count {
            let ch = chars[i]
            if ch.isLetter { tokens.append(.cmd(ch)); i += 1 }
            else if ch == "-" || ch == "." || ch.isNumber {
                var j = i
                if chars[j] == "-" { j += 1 }
                var dot = false
                while j < chars.count, chars[j].isNumber || (chars[j] == "." && !dot) { if chars[j] == "." { dot = true }; j += 1 }
                tokens.append(.num(Double(String(chars[i..<j])) ?? 0)); i = j
            } else { i += 1 }
        }
        var p = Path()
        var cur = CGPoint.zero, start = CGPoint.zero, lastC2: CGPoint?
        var k = 0
        var cmd: Character = "M"
        func num() -> Double { if k < tokens.count, case .num(let v) = tokens[k] { k += 1; return v }; return 0 }
        while k < tokens.count {
            if case .cmd(let c) = tokens[k] { cmd = c; k += 1 }
            let rel = cmd.isLowercase
            let ox = rel ? cur.x : 0, oy = rel ? cur.y : 0
            switch cmd.uppercased().first! {
            case "M":
                cur = CGPoint(x: ox + num(), y: oy + num()); start = cur; p.move(to: cur); lastC2 = nil
                cmd = rel ? "l" : "L"
            case "L":
                cur = CGPoint(x: ox + num(), y: oy + num()); p.addLine(to: cur); lastC2 = nil
            case "H":
                cur = CGPoint(x: ox + num(), y: cur.y); p.addLine(to: cur); lastC2 = nil
            case "V":
                cur = CGPoint(x: cur.x, y: oy + num()); p.addLine(to: cur); lastC2 = nil
            case "C":
                let c1 = CGPoint(x: ox + num(), y: oy + num()), c2 = CGPoint(x: ox + num(), y: oy + num()), e = CGPoint(x: ox + num(), y: oy + num())
                p.addCurve(to: e, control1: c1, control2: c2); cur = e; lastC2 = c2
            case "S":
                let c1 = lastC2.map { CGPoint(x: 2 * cur.x - $0.x, y: 2 * cur.y - $0.y) } ?? cur
                let c2 = CGPoint(x: ox + num(), y: oy + num()), e = CGPoint(x: ox + num(), y: oy + num())
                p.addCurve(to: e, control1: c1, control2: c2); cur = e; lastC2 = c2
            case "A":
                let rx = num(), ry = num(), rot = num(), large = num() != 0, sweep = num() != 0
                let e = CGPoint(x: ox + num(), y: oy + num())
                addArc(&p, from: cur, to: e, rx: rx, ry: ry, rotation: rot, large: large, sweep: sweep); cur = e; lastC2 = nil
            case "Z":
                p.closeSubpath(); cur = start; lastC2 = nil
            default: k += 1
            }
        }
        return p
    }

    private enum Token { case cmd(Character), num(Double) }

    /// SVG arc (endpoint form, spec F.6.5) as cubic segments of at most a quarter turn.
    private static func addArc(_ p: inout Path, from a: CGPoint, to b: CGPoint, rx rx0: Double, ry ry0: Double, rotation: Double, large: Bool, sweep: Bool) {
        var rx = abs(rx0), ry = abs(ry0)
        if rx == 0 || ry == 0 || a == b { p.addLine(to: b); return }
        let phi = rotation * .pi / 180, cp = cos(phi), sp = sin(phi)
        let dx = (a.x - b.x) / 2, dy = (a.y - b.y) / 2
        let x1 = cp * dx + sp * dy, y1 = -sp * dx + cp * dy
        let lam = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry)
        if lam > 1 { rx *= lam.squareRoot(); ry *= lam.squareRoot() }
        let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1
        let den = rx * rx * y1 * y1 + ry * ry * x1 * x1
        var co = den == 0 ? 0 : (max(0, num / den)).squareRoot()
        if large == sweep { co = -co }
        let cxp = co * rx * y1 / ry, cyp = -co * ry * x1 / rx
        let cx = cp * cxp - sp * cyp + (a.x + b.x) / 2, cy = sp * cxp + cp * cyp + (a.y + b.y) / 2
        func ang(_ ux: Double, _ uy: Double, _ vx: Double, _ vy: Double) -> Double {
            let s = ux * vy - uy * vx < 0 ? -1.0 : 1.0
            let d = (ux * vx + uy * vy) / ((ux * ux + uy * uy).squareRoot() * (vx * vx + vy * vy).squareRoot())
            return s * acos(max(-1, min(1, d)))
        }
        let th1 = ang(1, 0, (x1 - cxp) / rx, (y1 - cyp) / ry)
        var dth = ang((x1 - cxp) / rx, (y1 - cyp) / ry, (-x1 - cxp) / rx, (-y1 - cyp) / ry)
        if !sweep && dth > 0 { dth -= 2 * .pi } else if sweep && dth < 0 { dth += 2 * .pi }
        let n = max(1, Int((abs(dth) / (.pi / 2)).rounded(.up)))
        let step = dth / Double(n)
        let t = 4.0 / 3.0 * tan(step / 4)
        func pt(_ th: Double) -> CGPoint {
            let x = rx * cos(th), y = ry * sin(th)
            return CGPoint(x: cp * x - sp * y + cx, y: sp * x + cp * y + cy)
        }
        func dv(_ th: Double) -> CGPoint {
            let x = -rx * sin(th), y = ry * cos(th)
            return CGPoint(x: cp * x - sp * y, y: sp * x + cp * y)
        }
        var th = th1
        for _ in 0..<n {
            let s = pt(th), e = pt(th + step), ds = dv(th), de = dv(th + step)
            p.addCurve(to: e, control1: CGPoint(x: s.x + t * ds.x, y: s.y + t * ds.y), control2: CGPoint(x: e.x - t * de.x, y: e.y - t * de.y))
            th += step
        }
    }
}

// ------------------------------------------------------------------ type (tokens.css --fs-*)

enum GroupsType {
    private static func f(_ size: CGFloat, _ weight: Font.Weight, _ style: Font.TextStyle) -> Font {
        .custom("Figtree", size: size, relativeTo: style).weight(weight)
    }
    static let title = f(17, .semibold, .headline)
    static let body = f(15, .regular, .body)
    static let bodyStrong = f(15, .semibold, .body)
    static let sub = f(13, .regular, .footnote)
    static let subStrong = f(13, .semibold, .footnote)
    static let note = f(13, .regular, .footnote)
    static let meta = f(11, .regular, .caption2).monospacedDigit()
    /// Section heads: 12/16, 650, +0.06em, uppercase.
    static let sec = f(12, .semibold, .caption)
    static let hero = f(22, .semibold, .title2)
}

extension View {
    /// Line height 21 over a 15 pt body and the like: the extra leading between wrapped lines.
    func groupsLeading(_ extra: CGFloat) -> some View { lineSpacing(extra) }
}

// ------------------------------------------------------------------ press, buttons, pills

/// `.press`: scale to .97 in 100 ms on press, back in 140 ms (ease-out); none under reduced motion.
struct GroupsPressStyle: ButtonStyle {
    @Environment(\.accessibilityReduceMotion) private var reduced
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .scaleEffect(!reduced && configuration.isPressed ? 0.97 : 1)
            .animation(reduced ? nil : GroupsMotion.easeOut(configuration.isPressed ? GroupsMotion.press : GroupsMotion.release), value: configuration.isPressed)
    }
}

extension View {
    /// A pressable area with the .97 press scale and the surface-2 wash while pressed.
    @ViewBuilder
    func groupsTap(enabled: Bool = true, label: String? = nil, action: @escaping () -> Void) -> some View {
        let b = Button(action: action) { self.contentShape(Rectangle()) }.buttonStyle(GroupsPressStyle()).disabled(!enabled)
        if let label { b.accessibilityLabel(label) } else { b }
    }
}

enum GroupsBtnKind { case primary, outline, quiet, danger, dangerQuiet }

/// Btn.svelte: 44 pt, 18 pt side padding, radius 22, 15 pt 600. Disabled at 45 %.
struct GroupsButton: View {
    @Environment(\.tin) private var tin
    let text: String
    var kind: GroupsBtnKind = .primary
    var enabled = true
    var fill = false
    var desc: String?
    let action: () -> Void

    var body: some View {
        let (bg, fg): (Color, Color) = {
            switch kind {
            case .primary: return (tin.grpAccent, tin.grpOnAccent)
            case .outline: return (tin.sf, tin.ink)
            case .quiet: return (.clear, tin.ink)
            case .danger: return (tin.grpDangerBtn, .white)
            case .dangerQuiet: return (.clear, tin.er)
            }
        }()
        Button(action: action) {
            Text(text).font(GroupsType.bodyStrong).foregroundStyle(fg).lineLimit(1)
                .padding(.horizontal, 18).frame(maxWidth: fill ? .infinity : nil, minHeight: 44)
                .background(bg, in: Capsule())
                .overlay { if kind == .outline { Capsule().strokeBorder(tin.ln, lineWidth: 1) } }
                .contentShape(Capsule())
        }
        .buttonStyle(GroupsPressStyle())
        .disabled(!enabled)
        .opacity(enabled ? 1 : 0.45)
        .accessibilityLabel(desc ?? text)
    }
}

enum GroupsTone { case owner, admin, host, call, warn, muted }

/// Pill.svelte: 20 pt, radius 10, 8 pt sides, 11 pt 650.
struct GroupsPill: View {
    @Environment(\.tin) private var tin
    let text: String
    let tone: GroupsTone
    var body: some View {
        let (bg, fg): (Color, Color) = {
            switch tone {
            case .owner: return (tin.grpAccentSoft, tin.grpAccentInk)
            case .admin: return (tin.grpLav, tin.grpLavInk)
            case .host: return (tin.warn, tin.onWarn)
            case .call: return (tin.sf2, tin.ink)
            case .warn: return (tin.erc, tin.er)
            case .muted: return (tin.sf2, tin.ink2)
            }
        }()
        Text(text).font(.custom("Figtree", size: 11, relativeTo: .caption2).weight(.semibold)).tracking(0.11).foregroundStyle(fg).lineLimit(1)
            .padding(.horizontal, 8).frame(height: 20)
            .background(bg, in: RoundedRectangle(cornerRadius: 10))
            .overlay { if tone == .call { RoundedRectangle(cornerRadius: 10).strokeBorder(tin.ln, lineWidth: 1) } }
    }
}

/// A member's host / call-host mark (`.mk`): a 22 pt disc with a 14 pt glyph.
struct GroupsMark: View {
    @Environment(\.tin) private var tin
    let icon: GroupsIcon
    let desc: String
    var call = false
    var body: some View {
        ZStack {
            Circle().fill(call ? tin.sf2 : tin.warn)
            if call { Circle().strokeBorder(tin.ln, lineWidth: 1) }
            GroupsIconView(icon: icon, size: 14, color: call ? tin.ink : tin.onWarn)
        }
        .frame(width: 22, height: 22)
        .accessibilityElement(children: .ignore).accessibilityLabel(desc)
    }
}

/// t-badge: 18 pt, radius 9, 11 pt 700 tabular; muted is an outline. Space is not reserved: callers keep it at the row's end.
struct GroupsBadge: View {
    @Environment(\.tin) private var tin
    let count: Int
    var muted = false
    var body: some View {
        if count > 0 {
            Text(count > 99 ? "99+" : "\(count)")
                .font(.custom("Figtree", size: 11, relativeTo: .caption2).weight(muted ? .semibold : .bold).monospacedDigit())
                .foregroundStyle(muted ? tin.ink2 : tin.grpOnAccent)
                .padding(.horizontal, 5).frame(minWidth: 18, minHeight: 18)
                .background { if !muted { RoundedRectangle(cornerRadius: 9).fill(tin.grpAccent) } }
                .overlay { if muted { RoundedRectangle(cornerRadius: 9).strokeBorder(tin.ln, lineWidth: 1) } }
                .accessibilityLabel("\(count) unread")
        }
    }
}

// ------------------------------------------------------------------ sections, cards

/// Section head (`.sec`): 12/16 uppercase muted, padding 18 16 6; an optional "· n" in tabular digits at 80 %.
struct GroupsSection<Trailing: View>: View {
    @Environment(\.tin) private var tin
    let text: String
    var count: Int?
    var countText: String?
    var countColor: Color?
    var padding = EdgeInsets(top: 18, leading: 16, bottom: 6, trailing: 16)
    @ViewBuilder var trailing: () -> Trailing

    init(_ text: String, count: Int? = nil, countText: String? = nil, countColor: Color? = nil,
         padding: EdgeInsets = EdgeInsets(top: 18, leading: 16, bottom: 6, trailing: 16), @ViewBuilder trailing: @escaping () -> Trailing) {
        self.text = text; self.count = count; self.countText = countText; self.countColor = countColor; self.padding = padding; self.trailing = trailing
    }

    var body: some View {
        let n = countText ?? count.map(String.init)
        HStack(spacing: 0) {
            Text(text.uppercased()).font(GroupsType.sec).tracking(0.72).foregroundStyle(tin.ink2).lineLimit(1)
            if let n {
                Text("·").font(GroupsType.sec).foregroundStyle(tin.ink2).opacity(0.8).padding(.leading, 6)
                Text(n).font(GroupsType.sec.monospacedDigit()).foregroundStyle(countColor ?? tin.ink2).opacity(countColor == nil ? 0.8 : 1).padding(.leading, 6)
            }
            Spacer(minLength: 0)
            trailing()
        }
        .padding(padding).frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityElement(children: .combine).accessibilityAddTraits(.isHeader)
    }
}

extension GroupsSection where Trailing == EmptyView {
    init(_ text: String, count: Int? = nil, countText: String? = nil, countColor: Color? = nil,
         padding: EdgeInsets = EdgeInsets(top: 18, leading: 16, bottom: 6, trailing: 16)) {
        self.init(text, count: count, countText: countText, countColor: countColor, padding: padding) { EmptyView() }
    }
}

/// `.sub2`: a quiet sub-head inside a section (no caps).
struct GroupsSub2: View {
    @Environment(\.tin) private var tin
    let text: String
    init(_ text: String) { self.text = text }
    var body: some View {
        Text(text).font(GroupsType.sec).foregroundStyle(tin.ink2).padding(.init(top: 14, leading: 16, bottom: 2, trailing: 16))
            .frame(maxWidth: .infinity, alignment: .leading).accessibilityAddTraits(.isHeader)
    }
}

/// Flat card: surface, 1 px hairline, smooth 14 corners.
struct GroupsCard<Content: View>: View {
    @Environment(\.tin) private var tin
    var bg: Color?
    @ViewBuilder var content: () -> Content
    var body: some View {
        VStack(spacing: 0, content: content)
            .background(bg ?? tin.sf, in: SmoothRoundedRectangle(cornerRadius: 14))
            .clipShape(SmoothRoundedRectangle(cornerRadius: 14))
            .overlay { SmoothRoundedRectangle(cornerRadius: 14).stroke(tin.grpHairline, lineWidth: 1) }
    }
}

/// A thin separator between rows of a card.
struct GroupsRule: View {
    @Environment(\.tin) private var tin
    var body: some View { Rectangle().fill(tin.grpHairline).frame(height: 1) }
}

/// Muted note under a section (13/19).
struct GroupsNote: View {
    @Environment(\.tin) private var tin
    let text: String
    init(_ text: String) { self.text = text }
    var body: some View { Text(text).font(GroupsType.note).lineSpacing(3).foregroundStyle(tin.ink2).frame(maxWidth: .infinity, alignment: .leading) }
}

/// Several texts sharing one cell: the tallest sets the height and only `current` is visible, so a status changing never
/// moves what is below (the playground's `.stk`). The swap cross-fades in 120 ms.
struct GroupsStack: View {
    let lines: [String]
    let current: Int
    let font: Font
    let color: Color
    @Environment(\.accessibilityReduceMotion) private var reduced
    var body: some View {
        ZStack(alignment: .topLeading) {
            ForEach(Array(lines.enumerated()), id: \.offset) { i, line in
                Text(line).font(font).lineSpacing(3).foregroundStyle(color).opacity(i == current ? 1 : 0)
                    .fixedSize(horizontal: false, vertical: true).accessibilityHidden(i != current)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .animation(reduced ? nil : GroupsMotion.easeOut(GroupsMotion.crossfade), value: current)
    }
}

/// A status dot (8 pt): accent when on, muted when off.
struct GroupsDot: View {
    @Environment(\.tin) private var tin
    @Environment(\.accessibilityReduceMotion) private var reduced
    let on: Bool
    var size: CGFloat = 8
    var body: some View {
        Circle().fill(on ? tin.grpAccent : tin.ink2).frame(width: size, height: size)
            .animation(GroupsMotion.enterAnim(reduced), value: on)
    }
}

// ------------------------------------------------------------------ fold / collapse

/// Collapse.svelte: height and opacity, open in 200 ms ease-out, closed in 140 ms ease-in; instant under reduced motion.
/// SwiftUI folds the height when the siblings below re-lay out inside the animation, so only the fade is the transition.
struct GroupsCollapse<Content: View>: View {
    @Environment(\.accessibilityReduceMotion) private var reduced
    let open: Bool
    @ViewBuilder var content: () -> Content
    var body: some View {
        VStack(spacing: 0) { if open { content().transition(.opacity) } }
            .clipped()
            .animation(reduced ? nil : (open ? GroupsMotion.easeOut(GroupsMotion.enter) : GroupsMotion.easeIn(GroupsMotion.exit)), value: open)
    }
}

/// fold(): a list item or section arriving or leaving folds its height with opacity (cubic out in, cubic in out).
struct GroupsFold<Content: View>: View {
    @Environment(\.accessibilityReduceMotion) private var reduced
    let visible: Bool
    @ViewBuilder var content: () -> Content
    var body: some View {
        VStack(spacing: 0) { if visible { content().transition(.opacity) } }
            .clipped()
            .animation(reduced ? nil : (visible ? GroupsMotion.cubicOut(GroupsMotion.enter) : GroupsMotion.cubicIn(GroupsMotion.exit)), value: visible)
    }
}

// ------------------------------------------------------------------ the sheet (chat/Layer 'dialog')

/// Bottom sheet as the playground draws it: surface-raised, radius 24 on top, a notch, the sheet shadow. SwiftUI's own sheet
/// supplies the scrim, rise, drag to dismiss and the exit; this styles it. Apply to the sheet's content.
struct GroupsSheetStyle: ViewModifier {
    @Environment(\.tin) private var tin
    func body(content: Content) -> some View {
        content
            .presentationDetents([.medium, .large])
            .presentationDragIndicator(.visible)
            .presentationBackground(sheetBg)
            .presentationCornerRadius(24)
            .tinTheme()
    }
    private var sheetBg: Color { tin.dark ? Color(hex: 0x242D2A) : .white }   // --surface-raised
}
extension View { func groupsSheetStyle() -> some View { modifier(GroupsSheetStyle()) } }

/// The sheet's dialog body (`.dlg`): padding 20 20 16, a 17/24 title.
struct GroupsDialogBody<Content: View>: View {
    @Environment(\.tin) private var tin
    let title: String
    @ViewBuilder var content: () -> Content
    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(title).font(GroupsType.title).foregroundStyle(tin.ink).accessibilityAddTraits(.isHeader)
            content()
        }
        .padding(.init(top: 20, leading: 20, bottom: 16, trailing: 20)).frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// A muted paragraph in a sheet (13/19, 8 pt above).
struct GroupsPara: View {
    @Environment(\.tin) private var tin
    let text: String
    var top: CGFloat = 8
    var color: Color?
    var font = GroupsType.note
    init(_ text: String, top: CGFloat = 8, color: Color? = nil, font: Font = GroupsType.note) { self.text = text; self.top = top; self.color = color; self.font = font }
    var body: some View {
        Text(text).font(font).lineSpacing(3).foregroundStyle(color ?? tin.ink2).padding(.top, top)
            .frame(maxWidth: .infinity, alignment: .leading).fixedSize(horizontal: false, vertical: true)
    }
}

/// End-aligned actions row (`.acts`), 8 pt apart, 18 pt above.
struct GroupsActs<Content: View>: View {
    var top: CGFloat = 18
    @ViewBuilder var content: () -> Content
    var body: some View { HStack(spacing: 8) { Spacer(minLength: 0); content() }.padding(.top, top) }
}

// ------------------------------------------------------------------ ongoing group call (groups/OngoingCall.svelte)

/// The list row's call line: three calm bars, then the text in accent-ink 600 (`.callpv` + `.eq`). Static under reduced motion.
struct GroupsCallLine: View {
    @Environment(\.tin) private var tin
    @Environment(\.accessibilityReduceMotion) private var reduced
    let text: String
    var body: some View {
        HStack(spacing: 7) {
            TimelineView(.animation(minimumInterval: 1.0 / 30, paused: reduced)) { ctx in
                HStack(alignment: .bottom, spacing: 2) {
                    ForEach([0.0, 0.5, 1.0], id: \.self) { delay in
                        RoundedRectangle(cornerRadius: 1).fill(tin.grpAccentInk).frame(width: 2.5, height: height(ctx.date, delay))
                    }
                }.frame(height: 12, alignment: .bottom)
            }
            .accessibilityHidden(true)
            Text(text).font(GroupsType.subStrong.monospacedDigit()).foregroundStyle(tin.grpAccentInk).lineLimit(1)
        }
    }
    /// 800 ms ease-in-out, reversing; each bar fast-forwarded into the cycle by its delay.
    private func height(_ date: Date, _ delay: Double) -> CGFloat {
        if reduced { return 8 }
        let t = (date.timeIntervalSinceReferenceDate + delay).truncatingRemainder(dividingBy: 1.6)
        let u = t < 0.8 ? t / 0.8 : 2 - t / 0.8
        let eased = u * u * (3 - 2 * u)
        return 4 + 8 * eased
    }
}

/// The list row's "Join" chip (`.join`): decorative, the row itself opens the group.
struct GroupsJoinChip: View {
    @Environment(\.tin) private var tin
    var body: some View {
        Text("Join").font(.custom("Figtree", size: 11, relativeTo: .caption2).weight(.bold)).foregroundStyle(tin.grpOnAccent)
            .padding(.horizontal, 11).padding(.vertical, 2).background(tin.grpAccent, in: RoundedRectangle(cornerRadius: 12))
            .accessibilityHidden(true)
    }
}

// ------------------------------------------------------------------ avatars, meter, conversation pieces (GroupsUi.kt)

/// Group avatar (playground Avatar `group`): a circle in surface-2 with the accent-ink people glyph at 46 %; `dim` (a group we
/// left or were removed from) fades it to 72 %.
// TODO: android draws the group's mandala (FingerprintCache.ofGroup) here; no Swift binding for it was found, so this is its fallback.
struct GroupsAvatar: View {
    @Environment(\.tin) private var tin
    let size: CGFloat
    var dim = false
    var body: some View {
        ZStack {
            Circle().fill(tin.sf2)
            GroupsIconView(icon: GroupsIcons.users, size: size * 0.46, color: tin.grpAccentInk)
        }
        .frame(width: size, height: size).opacity(dim ? 0.72 : 1).accessibilityHidden(true)
    }
}

/// Three-segment flag meter (playground `.meter`): 6 pt bars, radius 3, gap 6; `filled` in danger-btn, the next one (`yours`) at 45 %.
struct GroupsFlagMeter: View {
    @Environment(\.tin) private var tin
    @Environment(\.accessibilityReduceMotion) private var reduced
    let filled: Int
    var total = GroupText.flagsToRemove
    var yours = false
    var desc: String?
    var body: some View {
        HStack(spacing: 6) {
            ForEach(0..<total, id: \.self) { i in
                RoundedRectangle(cornerRadius: 3).fill(color(i)).frame(height: 6)
                    .animation(GroupsMotion.enterAnim(reduced), value: filled)
            }
        }
        .accessibilityElement(children: .ignore).accessibilityLabel(desc ?? "\(filled) of \(total) flags")
    }
    private func color(_ i: Int) -> Color {
        if i < filled { return tin.grpDangerBtn }
        if yours && i == filled { return GroupsBlend.mix(tin.ln, tin.grpDangerBtn, 0.45) }
        return tin.ln
    }
}

enum GroupsBlend {
    /// Linear mix of two colours (Compose `lerp`).
    static func mix(_ a: Color, _ b: Color, _ t: Double) -> Color {
        let ua = UIColor(a), ub = UIColor(b)
        var (r1, g1, b1, a1) = (CGFloat(0), CGFloat(0), CGFloat(0), CGFloat(0)), (r2, g2, b2, a2) = (CGFloat(0), CGFloat(0), CGFloat(0), CGFloat(0))
        ua.getRed(&r1, green: &g1, blue: &b1, alpha: &a1); ub.getRed(&r2, green: &g2, blue: &b2, alpha: &a2)
        let k = CGFloat(t)
        return Color(.sRGB, red: r1 + (r2 - r1) * k, green: g1 + (g2 - g1) * k, blue: b1 + (b2 - b1) * k, opacity: a1 + (a2 - a1) * k)
    }
}

/// Board `.sys`: centred grey pill line.
struct GroupsSysLine: View {
    @Environment(\.tin) private var tin
    let text: String
    var onTap: (() -> Void)?
    var body: some View {
        let pill = Text(text).font(TinType.caption.weight(.regular)).foregroundStyle(tin.ink2).multilineTextAlignment(.center)
            .padding(.horizontal, 12).padding(.vertical, 4).frame(maxWidth: 320).background(tin.sf2, in: Capsule())
        HStack { Spacer(minLength: 0)
            if let onTap { Button(action: onTap) { pill.frame(minHeight: 48) }.buttonStyle(.plain) } else { pill }
            Spacer(minLength: 0) }
    }
}

/// Replaces the composer: board's bordered footer with a centred hint and optional buttons.
struct GroupsBlockedFooter<Buttons: View>: View {
    @Environment(\.tin) private var tin
    let hint: String
    @ViewBuilder var buttons: () -> Buttons
    var body: some View {
        VStack(spacing: 8) {
            Text(hint).font(TinType.bodyM).foregroundStyle(tin.ink2).multilineTextAlignment(.center).frame(maxWidth: .infinity)
            buttons()
        }
        .padding(.init(top: 12, leading: 16, bottom: 16, trailing: 16))
        .overlay(alignment: .top) { Rectangle().fill(tin.ln).frame(height: 1) }
    }
}

/// Board's tinted notice card (erc for removal / blocked, warn for lost contact).
struct GroupsNoticeCard<Extra: View>: View {
    let bg: Color, fg: Color, title: String, text: String
    @ViewBuilder var extra: () -> Extra
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(title).font(TinType.bodyL.weight(.bold)).foregroundStyle(fg).accessibilityAddTraits(.isHeader)
            Text(text).font(TinType.bodyM).foregroundStyle(fg)
            extra()
        }
        .padding(16).frame(maxWidth: .infinity, alignment: .leading)
        .background(bg, in: SmoothRoundedRectangle(cornerRadius: 20)).padding(.top, 8)
    }
}

/// Per-author name colour in group chats (relay, amber text, teal, pink), stable by DID. `hashCode` is Java's String.hashCode
/// (UTF-16 units, 31x + c over Int32) so an author keeps the colour they have on Android.
func groupsAuthorColor(_ did: String, _ c: TinColors) -> Color {
    var h: Int32 = 0
    for u in did.utf16 { h = h &* 31 &+ Int32(u) }
    return [c.relay, c.threadText, c.pr, c.onPk][Int(h & 0x7fffffff) % 4]
}

// ------------------------------------------------------------------ time words

enum GroupsTime {
    private static var cal: Calendar { Calendar.current }
    private static func date(_ ms: UInt64) -> Date { Date(timeIntervalSince1970: Double(ms) / 1000) }

    /// "14:02"
    static func clock(_ ms: UInt64) -> String {
        let c = cal.dateComponents([.hour, .minute], from: date(ms))
        return String(format: "%02d:%02d", c.hour ?? 0, c.minute ?? 0)
    }

    static func daysAgo(_ ms: UInt64, _ nowMs: UInt64) -> Int {
        cal.dateComponents([.day], from: cal.startOfDay(for: date(ms)), to: cal.startOfDay(for: date(nowMs))).day ?? 0
    }

    static func sameDay(_ a: UInt64, _ b: UInt64) -> Bool { cal.isDate(date(a), inSameDayAs: date(b)) }

    /// Chat list time: "12:41", "Yesterday", "Monday", "12 Sep" (`present.listTime`).
    static func list(_ ms: UInt64, _ nowMs: UInt64) -> String {
        listTime(then: Int64(ms / 1000), now: Int64(nowMs / 1000), utcOffsetSecs: Int32(cal.timeZone.secondsFromGMT(for: date(nowMs))))
    }

    /// Day pill in a conversation: "Today", "Yesterday", else "Thursday 8 Oct" (with the year when it isn't this year).
    static func dayLabel(_ ms: UInt64, _ nowMs: UInt64) -> String {
        let n = daysAgo(ms, nowMs)
        if n <= 0 { return "Today" }
        if n == 1 { return "Yesterday" }
        let sameYear = cal.component(.year, from: date(ms)) == cal.component(.year, from: date(nowMs))
        return format(sameYear ? "EEEE d MMM" : "EEEE d MMM yyyy", ms)
    }

    /// "d MMM", the group screens' day-only words ("Valid until 12 Oct").
    static func dayOnly(_ ms: UInt64) -> String { format("d MMM", ms) }

    /// "Oct 10, 14:02"
    static func whenFull(_ ms: UInt64) -> String { format("MMM d, HH:mm", ms) }

    /// "Yesterday, 03:12" / "Today, 09:00" / "Oct 3, 10:00"
    static func dayAndTime(_ ms: UInt64, _ nowMs: UInt64) -> String {
        (0...1).contains(daysAgo(ms, nowMs)) ? "\(dayLabel(ms, nowMs)), \(clock(ms))" : whenFull(ms)
    }

    private static func format(_ pattern: String, _ ms: UInt64) -> String {
        let f = DateFormatter()
        f.locale = .current
        f.setLocalizedDateFormatFromTemplate(pattern)
        if pattern.hasSuffix("HH:mm") { f.dateFormat = pattern }
        return f.string(from: date(ms))
    }
}
