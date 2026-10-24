import SwiftUI

/*
 * The call screen's moving colour field (Fog), drawn behind the voice call screens. The model is CallFieldModel.swift
 * (parity with the playground, desktop and Android). Drawing is gl.ts's cssRenderer, Android's below-API-33 path: the
 * five lobes as radial gradients over the theme background, plus the dark-mode calm patches behind text. The warped
 * noise of the AGSL/WGSL shader is not drawn yet (a Metal `[[stitchable]]` port of FOG_AGSL is the follow-up).
 * The loop sleeps when nothing animates (end/failed fade finished, reduced motion between notes).
 */

/** Dark mode: calm patches behind text (0 = none) and the chroma gain (CloudField.svelte SCRIM_DARK / GAIN_DARK). */
private let scrimDark = 0.45
private let gainDark = 1.55

/** A calm patch behind text or controls: centre and half size, in points, relative to the field. */
struct FieldCalm: Equatable { var cx: CGFloat; var cy: CGFloat; var hw: CGFloat; var hh: CGFloat }

/** Holds the model across redraws and decides when the timeline may sleep. */
@MainActor
final class CallFieldHolder: ObservableObject {
    var model: CallField?
    var frame: FieldFrame?
    let origin = Date()
    @Published var sleeping = false
    private var wake: Task<Void, Never>?

    func nowMs(_ d: Date) -> Double { d.timeIntervalSince(origin) * 1000 }

    /** Configures the model for the current inputs (each setter is a no-op when unchanged), then advances it. */
    func step(_ date: Date, state: CallFieldState, theme: FieldTheme, reduced: Bool, muted: Bool, size: CGSize, avatar: CGPoint) -> FieldFrame {
        let m = model ?? CallField(theme: theme, reduced: reduced)
        model = m
        m.setTheme(theme); m.setReduced(reduced); m.setMute(muted)
        m.setView(w: size.width, h: size.height, avx: avatar.x, avy: avatar.y)
        let t = nowMs(date)
        if m.state != state { m.setState(state, t: t, cueStart: t + state.cueDelayMs(reduced: reduced)) }
        let fr = m.update(t)
        frame = fr
        if !fr.animating {
            // Not from inside the render pass: publish on the next turn.
            let wakeAt = fr.wakeAt
            Task { @MainActor in self.sleep(until: wakeAt, now: t) }
        }
        return fr
    }

    private func sleep(until wakeAt: Double?, now: Double) {
        sleeping = true
        wake?.cancel()
        guard let wakeAt else { return }
        let ns = UInt64(max(0, wakeAt - now) + 1) * 1_000_000
        wake = Task { @MainActor [weak self] in
            try? await Task.sleep(nanoseconds: ns)
            if !Task.isCancelled { self?.sleeping = false }
        }
    }

    /** An input changed: run again (the model decides whether anything moves). */
    func poke() { wake?.cancel(); sleeping = false }
}

/** Fills its parent with the field. `avatar` and `calm` are in points inside this view. */
struct CallFieldBackground: View {
    let state: CallFieldState
    let muted: Bool
    let avatar: CGPoint
    let calm: [FieldCalm]
    @Environment(\.tin) var tin
    @Environment(\.accessibilityReduceMotion) var reduced
    @StateObject var holder = CallFieldHolder()

    var body: some View {
        GeometryReader { geo in
            TimelineView(.animation(minimumInterval: nil, paused: holder.sleeping)) { tl in
                Canvas { ctx, size in
                    guard size.width > 0, size.height > 0 else { return }
                    let fr = holder.step(tl.date, state: state, theme: tin.dark ? .dark : .light, reduced: reduced,
                                         muted: muted, size: size, avatar: avatar)
                    drawGradients(&ctx, fr, calm, size, k: tin.dark ? scrimDark : 0, gain: tin.dark ? gainDark : 1)
                }
            }
            .frame(width: geo.size.width, height: geo.size.height)
        }
        .clipped()
        .accessibilityHidden(true)
        .onChange(of: state) { holder.poke() }
        .onChange(of: tin.dark) { holder.poke() }
        .onChange(of: reduced) { holder.poke() }
        .onChange(of: muted) { holder.poke() }
        .onChange(of: avatar) { holder.poke() }
    }
}

private func rgbColor(_ c: [Double], _ a: Double) -> Color {
    func ch(_ v: Double) -> Double { min(max(v / 255, 0), 1) }
    return Color(.sRGB, red: ch(c[0]), green: ch(c[1]), blue: ch(c[2]), opacity: min(max(a, 0), 1))
}

/** gl.ts cssRenderer: one radial gradient per lobe over the theme background, calm patches on top. */
private func drawGradients(_ ctx: inout GraphicsContext, _ f: FieldFrame, _ calm: [FieldCalm], _ size: CGSize, k: Double, gain: Double) {
    let w = size.width, h = size.height
    let sh = min(w, h)
    let full = Path(CGRect(origin: .zero, size: size))
    let bg = FieldMath.labToRgb(f.bg)
    ctx.fill(full, with: .color(rgbColor(bg, 1)))
    for l in f.lobes {
        let c = FieldMath.labToRgb([l.lab[0], l.lab[1] * gain, l.lab[2] * gain])
        let a = f.cap * (1 - exp(-l.alpha))
        let ctr = CGPoint(x: w / 2 + CGFloat(l.x) * sh, y: h / 2 + CGFloat(l.y) * sh)
        let r = max(CGFloat(l.sigma) * sh * 2.2, 1)
        let g = Gradient(stops: [.init(color: rgbColor(c, a), location: 0), .init(color: rgbColor(c, a * 0.45), location: 0.45),
                                 .init(color: rgbColor(c, 0), location: 1)])
        ctx.fill(full, with: .radialGradient(g, center: ctr, startRadius: 0, endRadius: r))
    }
    if k > 0 {
        for p in calm.prefix(2) {
            let rx = max((p.hw + 44) / 0.55, 1), ry = max((p.hh + 30) / 0.55, 1)
            var c2 = ctx
            c2.translateBy(x: p.cx, y: p.cy)
            c2.scaleBy(x: 1, y: ry / rx)
            let g = Gradient(stops: [.init(color: rgbColor(bg, k), location: 0), .init(color: rgbColor(bg, k), location: 0.55),
                                     .init(color: rgbColor(bg, 0), location: 1)])
            c2.fill(Path(ellipseIn: CGRect(x: -rx, y: -rx, width: 2 * rx, height: 2 * rx)),
                    with: .radialGradient(g, center: .zero, startRadius: 0, endRadius: rx))
        }
    }
}

#Preview("Field, active") {
    CallFieldBackground(state: .active, muted: false, avatar: CGPoint(x: 195, y: 250), calm: [])
        .ignoresSafeArea()
        .tinTheme()
}
