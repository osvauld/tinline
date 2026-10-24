import SwiftUI
import Combine
import UIKit

/// App-wide fx bus: a sent message makes every visible live mark pluck, quietly (playground: fx 'sent').
enum TinFx {
    static let sent = PassthroughSubject<Void, Never>()
    static func messageSent() { sent.send() }
}

/// Runs a `MarkModel` against the display's frames. Main thread only; the frame link exists only while the model is moving.
/// This is Mark.kt's LaunchedEffects in one object (state -> springs, tug reporting, the pluck bus, the frame loop).
final class MarkController: NSObject, ObservableObject {
    @Published private(set) var frame = 0
    /// The string went tight since the last slack (drives the line colour's warmth).
    @Published private(set) var tight = false
    private(set) var model: MarkModel
    private(set) var tier: MarkTier
    private var tierSize: Int
    var onTug: ((Int) -> Void)?
    private var told = 0
    private var applied: MarkState?
    private var link: CADisplayLink?
    private var last: CFTimeInterval = 0
    private var fx: AnyCancellable?

    init(px: Double, spring: MarkSpring) {
        tierSize = markPickTier(px)
        tier = markTierFor(px)
        model = MarkModel(tier, spring: spring)
        super.init()
    }

    /// A different optical tier means a fresh model (Compose: `remember(size)`).
    func resize(px: Double) {
        guard markPickTier(px) != tierSize else { return }
        let spring = model.spring
        stop()
        tierSize = markPickTier(px)
        tier = markTierFor(px)
        model = MarkModel(tier, spring: spring)
        applied = nil; told = 0
        frame += 1
    }

    /// Report tugs and the line's warmth after every model change.
    private func report() {
        if model.tugCount > told { let n = model.tugCount; told = n; onTug?(n) }
        if tight != model.tight { tight = model.tight }
    }

    /// state -> springs. The first call sets the pose; later changes retarget mid-flight (Mark.kt `LaunchedEffect(model, state, live, reduced)`).
    func apply(state: MarkState, live: Bool, reduced: Bool) {
        listen(live && !reduced)
        if !live || reduced {
            let first = applied == nil
            if first { model.set(state) } else { model.snap(toTaut: state == .taut) }
            applied = state
            if first { told = model.tugCount }
            report(); frame += 1
            return
        }
        if let prev = applied {
            if prev != state {
                switch state {
                case .taut: model.connect()
                case .ringing: model.setRing(true)
                case .slack: model.setRing(false); model.end()
                }
            }
        } else {
            model.set(state == .taut ? .taut : .slack)
            if state == .ringing { model.setRing(true) }
            told = model.tugCount
            if tight != model.tight { tight = model.tight }
        }
        applied = state
        kick()
    }

    private func listen(_ on: Bool) {
        if !on { fx = nil; return }
        if fx != nil { return }
        fx = TinFx.sent.receive(on: DispatchQueue.main).sink { [weak self] in self?.model.pluck(); self?.kick() }
    }

    func kick() {
        guard link == nil, model.active else { return }
        last = CACurrentMediaTime()
        let l = CADisplayLink(target: self, selector: #selector(tick(_:)))
        l.add(to: .main, forMode: .common)
        link = l
    }

    @objc private func tick(_ l: CADisplayLink) {
        let now = l.timestamp
        let more = model.advance(now - last)
        last = now
        report()
        frame += 1
        if !more { stop() }
    }

    func stop() { link?.invalidate(); link = nil }
}

/// The canvas as an animatable view: `warm` (0 slack-line colour .. 1 gold) interpolates under `withAnimation`.
private struct MarkCanvas: View, Animatable {
    let model: MarkModel
    let tier: MarkTier
    let frame: Int
    let can: Color, hole: Color, string: Color, slackString: Color, dimString: Color
    let lineColor: Bool, dim: Bool
    var warm: Double

    var animatableData: Double {
        get { warm }
        set { warm = newValue }
    }

    var body: some View {
        let str = dim ? dimString : (lineColor ? Color.tinMix(slackString, string, by: warm) : string)
        Canvas { ctx, size in
            _ = frame // redraw on every model frame
            let k = Double(size.width) / tier.vbSize
            let cx = tier.c - model.delta
            drawCan(&ctx, tier, cx, k, can, hole, mirrored: false)
            drawCan(&ctx, tier, cx, k, can, hole, mirrored: true)
            let x1 = cx + tier.ko, x2 = 24 - x1, sg = model.sagNow, phi = model.phi
            func X(_ v: Double) -> CGFloat { CGFloat((v - tier.vb) * k) }
            var path = Path()
            path.move(to: CGPoint(x: X(x1), y: X(12)))
            path.addQuadCurve(to: CGPoint(x: X(x2), y: X(12)), control: CGPoint(x: X(12 + 2 * sg * sin(phi)), y: X(12 + 2 * sg * cos(phi))))
            ctx.stroke(path, with: .color(str), style: StrokeStyle(lineWidth: CGFloat(tier.sw * k), lineCap: .round, lineJoin: .round))
        }
    }
}

/// Draws one can in device points. `mirrored` mirrors about x = 12.
private func drawCan(_ ctx: inout GraphicsContext, _ p: MarkTier, _ cx: Double, _ k: Double, _ can: Color, _ mouth: Color, mirrored m: Bool) {
    func X(_ v: Double) -> CGFloat { CGFloat(((m ? 24 - v : v) - p.vb) * k) }
    func Y(_ v: Double) -> CGFloat { CGFloat((v - p.vb) * k) }
    let ym = 12 - p.b, yp = 12 + p.b
    let ax = CGFloat(p.a * k), bx = CGFloat(p.b * k)
    // body: flat top and bottom behind the rim, half-ellipse back (left in the unmirrored frame)
    let back = cx - p.len
    let rx = X(back), ry = Y(12)
    let sgn: CGFloat = m ? 1 : -1
    let kappa: CGFloat = 0.5522847498
    var body = Path()
    body.move(to: CGPoint(x: X(cx), y: Y(ym)))
    body.addLine(to: CGPoint(x: rx, y: Y(ym)))
    // Two quarter-ellipse Beziers (error < 0.03 %) for Android's arcTo(270, +-180).
    body.addCurve(to: CGPoint(x: rx + sgn * ax, y: ry), control1: CGPoint(x: rx + sgn * ax * kappa, y: ry - bx), control2: CGPoint(x: rx + sgn * ax, y: ry - bx * kappa))
    body.addCurve(to: CGPoint(x: rx, y: ry + bx), control1: CGPoint(x: rx + sgn * ax, y: ry + bx * kappa), control2: CGPoint(x: rx + sgn * ax * kappa, y: ry + bx))
    body.addLine(to: CGPoint(x: X(cx), y: Y(yp)))
    body.closeSubpath()
    ctx.fill(body, with: .color(can))
    ctx.fill(Path(ellipseIn: CGRect(x: X(cx) - ax, y: Y(12) - bx, width: 2 * ax, height: 2 * bx)), with: .color(can))
    if p.hasMouth {
        let mu = CGFloat(p.mu)
        ctx.fill(Path(ellipseIn: CGRect(x: X(cx) - mu * ax, y: Y(12) - mu * bx, width: 2 * mu * ax, height: 2 * mu * bx)), with: .color(mouth))
    }
}

/// The brand mark: two cans, one string. `can`/`string`/`mouth` are the palette (mouth defaults to the playground's hole colour for
/// the can colour). With `live` the mark animates: `state` changes retarget the springs, and a sent message plucks it (`TinFx`).
/// `size` picks the optical tier; call sites that pass only a size get the static slack pose.
struct TinMark: View {
    var size: CGFloat
    var can: Color?
    var string: Color?
    var mouth: Color?
    var state: MarkState
    var live: Bool
    /// Ringing sway: period (s) and amplitude (deg); lock the period to the cue that is playing.
    var sway: Sway?
    var dim: Bool
    var description: String?
    /// Spring parameters for this use (Done's two tugs); calls keep the default.
    var spring: MarkSpring
    /// Sign-up and first-screen marks: a slack line is faded gold (gold 55 % into --line) and warms to gold on the first tug.
    var lineColor: Bool
    /// Each tug of the string going tight (1, 2, ...): sign-up Done plays the `line` cue on the first.
    var onTug: ((Int) -> Void)?
    // No glow (decided 2026-10-13): dark-mode depth is the rim only, sign-up included.

    @Environment(\.tin) private var tin
    @Environment(\.accessibilityReduceMotion) private var reduced
    @StateObject private var controller: MarkController
    @State private var warm = 0.0

    init(size: CGFloat, can: Color? = nil, string: Color? = nil, mouth: Color? = nil, state: MarkState = .slack, live: Bool = false,
         sway: Sway? = nil, dim: Bool = false, description: String? = nil, spring: MarkSpring = MarkSpring(),
         lineColor: Bool = false, onTug: ((Int) -> Void)? = nil) {
        self.size = size; self.can = can; self.string = string; self.mouth = mouth; self.state = state; self.live = live
        self.sway = sway; self.dim = dim; self.description = description; self.spring = spring; self.lineColor = lineColor; self.onTug = onTug
        _controller = StateObject(wrappedValue: MarkController(px: Double(size), spring: spring))
    }

    private struct Key: Hashable { var state: MarkState; var live: Bool; var reduced: Bool }

    var body: some View {
        let stringColor = string ?? tin.thread
        MarkCanvas(
            model: controller.model, tier: controller.tier, frame: controller.frame,
            can: can ?? (tin.dark ? Color(hex: MarkColors.canDark) : tin.pr), hole: mouth ?? Color(hex: MarkColors.mouth),
            string: stringColor, slackString: Color.tinMix(stringColor, tin.ln, by: 0.45), dimString: tin.ink2,
            lineColor: lineColor, dim: dim, warm: warm)
            .frame(width: size, height: size)
            .opacity(dim ? 0.45 : 1)
            .accessibilityElement()
            .accessibilityLabel(description ?? "")
            .accessibilityHidden(description == nil)
            .onChange(of: size) { _, new in controller.resize(px: Double(new)) }
            .onChange(of: sway, initial: true) { _, s in if let s { controller.model.setSway(period: s.period, amp: s.amp) } }
            .onChange(of: spring, initial: true) { _, s in controller.model.spring = s }
            .onChange(of: Key(state: state, live: live, reduced: reduced), initial: true) { _, k in
                controller.onTug = onTug
                controller.apply(state: k.state, live: k.live, reduced: k.reduced)
            }
            .onChange(of: controller.tight, initial: true) { _, tight in
                // The line warms on the first tug (SWITCH, ease-out); instantly under reduced motion.
                if tight && !reduced { withAnimation(TinMotion.easeOut(TinMotion.switchTime)) { warm = 1 } } else { warm = tight ? 1 : 0 }
            }
            .onAppear { controller.kick() }
            .onDisappear { controller.stop() }
    }
}
