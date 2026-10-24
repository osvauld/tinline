import SwiftUI

/// Receipt ticks drawn as the mark's string (the user's pick from the playground's `?lab=ticks`, preset "crisp", string variant at
/// every size; android ReceiptTicks.kt). Geometry and the spring's morph curves are generated from Rust (`TinTicks`,
/// crates/tin-icons/src/ticks.rs); this file only picks the visual state and plays the one-shot transition:
/// slack -> one (pulled taut), one -> two (the second string pulls out of the first), two -> read (one pluck while the colour
/// eases to lavender), waiting -> not delivered (colour ease only): those rules are core's (`p2pcore::receipts::ticks`).
/// Nothing loops; reduced motion shows the end state.

/// The core decides the mark (`ReceiptTick`, signed receipts only; `notDelivered` = a block stopped it): `tickVis`.
func tickVisOf(_ tick: ReceiptTick?, notDelivered: Bool = false) -> TickVis? { tickVis(tick: tick, notDelivered: notDelivered) }

/// One frame: string 1 morph, string 2 morph + offset fraction (nil = hidden), colour mix.
private struct TickFrame {
    var k0: CGFloat
    var second: (k: CGFloat, frac: CGFloat)?
    var mix: Double
}

private func fromCore(_ p: TickPose, _ mix: Double) -> TickFrame {
    TickFrame(k0: CGFloat(p.k0), second: p.second ? (CGFloat(p.k1), CGFloat(p.k1Offset)) : nil, mix: mix)
}

private func restPose(_ v: TickVis) -> TickFrame { fromCore(tickPose(from: v, to: v, x: nil), 1) }

private func curveSample(_ c: [CGFloat], _ secs: Double) -> Float? {
    let i = Int(secs * Double(TinTicks.frameHz))
    return i < c.count ? Float(c[i]) : nil
}

/// Colour-ease length (OVERLAY_OPEN).
private let tickColourSecs = TinMotion.overlayOpen

/// The transition from `from` to `to` at `secs`; nil once settled. The rule (which curve, whether the ink eases, the shape per sample)
/// is core's (`tickTransition` / `tickPose`, shared with the desktop); the curves, colour ease and clock are ours.
private func tickFrame(_ from: TickVis, _ to: TickVis, _ secs: Double) -> TickFrame? {
    let tr = tickTransition(from: from, to: to)
    let mix = tr.colour ? TinMotion.out.transform(min(secs / tickColourSecs, 1)) : 1
    let x: Float?
    switch tr.curve {
    case .pull: x = curveSample(TinTicks.pull, secs)
    case .pluck: x = curveSample(TinTicks.pluck, secs)
    case .none: x = nil
    }
    return x != nil || (tr.colour && secs < tickColourSecs) ? fromCore(tickPose(from: from, to: to, x: x), mix) : nil
}

/// How long the transition from `from` to `to` runs, so the view can stop its clock without polling.
private func tickDuration(_ from: TickVis, _ to: TickVis) -> Double {
    let tr = tickTransition(from: from, to: to)
    let curve: Double
    switch tr.curve {
    case .pull: curve = Double(TinTicks.pull.count) / Double(TinTicks.frameHz)
    case .pluck: curve = Double(TinTicks.pluck.count) / Double(TinTicks.frameHz)
    case .none: curve = 0
    }
    return max(curve, tr.colour ? tickColourSecs : 0)
}

/// The slot's width for a glyph `size` tall (two ticks wide, so nothing moves as the state advances).
func tickSlotWidth(_ size: CGFloat) -> CGFloat { size * (TinTicks.vbW / TinTicks.vbH) }

/// Screen-reader words per state (VOICE.md: how things are). `first` is the other person's first name.
func tickLabel(_ v: TickVis, first: String) -> String {
    switch v {
    case .slack: return "Waiting to send"
    case .one: return "Sent, waiting for \(first)’s device to confirm"
    case .two: return "On \(first)’s device"
    case .read: return "Read on \(first)’s device"
    case .bad: return "Not delivered"
    }
}

/// The receipt tick of one message, drawn from the core's `ReceiptTick`. Reserves its two-tick slot even when the tick says nothing,
/// so a status change never narrows a bubble. `key` identifies the message (the glyph animates only when the state changes while it is on
/// screen, never on first appearance or scroll-in); `bg` is the bubble colour behind it (the knot's halo); `label` is what a screen reader
/// hears. Defaults: meta ink `ink2`, bubble `prc` (an outgoing bubble).
struct ReceiptTickView: View {
    var tick: ReceiptTick
    var size: CGFloat
    var key: String = ""
    var notDelivered = false
    var meta: Color?
    var bg: Color?
    var label: String?

    @Environment(\.tin) private var tin

    var body: some View {
        if let vis = tickVisOf(tick, notDelivered: notDelivered) {
            ReceiptTickGlyph(key: key, vis: vis, meta: meta ?? tin.ink2, read: TickInk.read(tin), bad: TickInk.bad(tin), bg: bg ?? tin.prc,
                             label: label ?? tickLabel(vis, first: "their"), size: size)
        } else {
            Color.clear.frame(width: tickSlotWidth(size), height: size)
        }
    }
}

/// Tick inks (android Theme.kt `tickRead`, `tickBad`; kept here until Theme.swift carries them).
enum TickInk {
    static func read(_ c: TinColors) -> Color { c.dark ? Color(hex: 0xB8B0FF) : Color(hex: 0x374582) }
    static func bad(_ c: TinColors) -> Color { c.dark ? Color(hex: 0xFF6B6B) : c.er }
}

/// The glyph for a known `TickVis`. `meta`, `read`, `bad` are the inks of the states; `bg` is the bubble (the knot's halo).
struct ReceiptTickGlyph: View {
    var key: String
    var vis: TickVis
    var meta: Color, read: Color, bad: Color, bg: Color
    var label: String
    var size: CGFloat = 12

    @Environment(\.accessibilityReduceMotion) private var reduced
    @State private var shown: TickVis?
    @State private var run: (from: TickVis, to: TickVis, start: Date)?

    private func ink(_ v: TickVis) -> Color {
        switch v {
        case .read: return read
        case .bad: return bad
        default: return meta
        }
    }

    var body: some View {
        Group {
            if let run {
                TimelineView(.animation) { ctx in
                    let secs = ctx.date.timeIntervalSince(run.start)
                    canvas(tickFrame(run.from, run.to, secs) ?? restPose(run.to), from: run.from, to: run.to)
                }
            } else {
                canvas(restPose(vis), from: nil, to: vis)
            }
        }
        .frame(width: tickSlotWidth(size), height: size)
        .accessibilityElement()
        .accessibilityLabel(label)
        .onAppear { if shown == nil { shown = vis } }
        .onChange(of: key) { _, _ in shown = vis; run = nil }
        .onChange(of: vis) { _, new in
            guard let prev = shown, prev != new else { return }
            shown = new
            if reduced { run = nil; return }
            let r = (from: prev, to: new, start: Date())
            run = r
            Task { @MainActor in
                try? await Task.sleep(nanoseconds: UInt64((tickDuration(prev, new) + 0.05) * 1e9))
                if run?.start == r.start { run = nil }
            }
        }
    }

    private func canvas(_ pose: TickFrame, from: TickVis?, to: TickVis) -> some View {
        let color = from.map { Color.tinMix(ink($0), ink(to), by: pose.mix) } ?? ink(to)
        return Canvas { ctx, sz in
            let s = sz.height / TinTicks.vbH
            let stroke = StrokeStyle(lineWidth: TinTicks.sw * s, lineCap: .round, lineJoin: .round)
            func path(_ k: CGFloat, _ ox: CGFloat) -> Path {
                var p = Path()
                for i in 0..<TinTicks.n {
                    let sx = TinTicks.slack[2 * i], sy = TinTicks.slack[2 * i + 1]
                    let tx = TinTicks.tick[2 * i], ty = TinTicks.tick[2 * i + 1]
                    let pt = CGPoint(x: (sx + (tx - sx) * k + ox - TinTicks.vbX) * s, y: (sy + (ty - sy) * k) * s)
                    if i == 0 { p.move(to: pt) } else { p.addLine(to: pt) }
                }
                return p
            }
            ctx.stroke(path(pose.k0, 0), with: .color(color), style: stroke)
            if let second = pose.second {
                let p2 = path(second.k, TinTicks.dx * second.frac)
                if TinTicks.gap > 0 {
                    ctx.stroke(p2, with: .color(bg), style: StrokeStyle(lineWidth: (TinTicks.sw + 2 * TinTicks.gap) * s, lineCap: .round, lineJoin: .round))
                }
                ctx.stroke(p2, with: .color(color), style: stroke)
            }
        }
    }
}
