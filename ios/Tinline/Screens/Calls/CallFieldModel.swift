import Foundation

/*
 * The call screen's colour field model: five soft, hue-separated masses on Lissajous paths that swell and warm with
 * the call's cue notes. A line-for-line port of android/.../CallFieldModel.kt, itself a port of
 * crates/desktop/src/call_field.rs (a numerical port of the playground's lib/calls/field/model.ts): same constants,
 * same state table, same OKLab mixing, same reduced-motion crossfade.
 *
 * Non-UI logic normally lives in Rust (shared across clients). This is per-frame animation maths for drawing only,
 * already duplicated on desktop and Android with golden parity, so a Swift port guarded by the same golden file
 * (crates/desktop/src/call_field_golden.txt, see CallFieldTests) is accepted here instead of a new FFI surface.
 *
 * The model is pure: time is passed in (ms), no clocks, no audio, no microphone. The connected field breathes with a
 * simulated voice (makeVoice bursts), as on desktop.
 */

enum FieldMath {
    static let fMin = 196.0
    static let fMax = 1568.0
    static let k = 5
    static func clamp(_ x: Double, _ a: Double, _ b: Double) -> Double { Swift.max(a, Swift.min(b, x)) }
    static func smooth(_ x: Double) -> Double { let c = clamp(x, 0, 1); return c * c * (3 - 2 * c) }
    static func lerp(_ a: Double, _ b: Double, _ s: Double) -> Double { a + (b - a) * s }
    /** JS Math.round (half up). */
    static func jsRound(_ x: Double) -> Double { (x + 0.5).rounded(.down) }

    private static func s2l(_ v: Double) -> Double { let c = v / 255; return c <= 0.04045 ? c / 12.92 : pow((c + 0.055) / 1.055, 2.4) }
    private static func l2s(_ v: Double) -> Double {
        let c = clamp(v, 0, 1)
        return 255 * (c <= 0.0031308 ? 12.92 * c : 1.055 * pow(c, 1 / 2.4) - 0.055)
    }
    static func rgbToLab(_ rgb: [Double]) -> [Double] {
        let r = s2l(rgb[0]), g = s2l(rgb[1]), b = s2l(rgb[2])
        let l = cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b)
        let m = cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b)
        let s = cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b)
        return [
            0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
            1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
            0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
        ]
    }
    /** OKLab to sRGB 0..255 (used by the gradient renderer). */
    static func labToRgb(_ c: [Double]) -> [Double] {
        let l = pow(c[0] + 0.3963377774 * c[1] + 0.2158037573 * c[2], 3)
        let m = pow(c[0] - 0.1055613458 * c[1] - 0.0638541728 * c[2], 3)
        let s = pow(c[0] - 0.0894841775 * c[1] - 1.2914855480 * c[2], 3)
        return [
            l2s(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s),
            l2s(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s),
            l2s(-0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s),
        ]
    }
    /** OKLab mix; chroma is interpolated separately so a teal-to-gold mid-mix does not go grey/olive. */
    static func mixLab(_ a: [Double], _ b: [Double], _ s: Double) -> [Double] {
        let l = lerp(a[0], b[0], s), aa = lerp(a[1], b[1], s), bb = lerp(a[2], b[2], s)
        let c0 = hypot(a[1], a[2]), c1 = hypot(b[1], b[2]), cm = hypot(aa, bb)
        let ct = lerp(c0, c1, s)
        let k = cm > 1e-5 ? ct / cm : 1.0
        return [l, aa * k, bb * k]
    }
}

/* `pow(Double, 3)` of a negative base is fine (integral exponent), as Kotlin's `Double.pow(3)`. */

enum FieldTheme { case light, dark }

private struct FLabs { let bg: [Double]; let teal: [Double]; let gold: [Double] }
private func fLabs(_ t: FieldTheme) -> FLabs {
    switch t {
    case .light: FLabs(bg: FieldMath.rgbToLab([245, 246, 243]), teal: FieldMath.rgbToLab([110, 210, 186]), gold: FieldMath.rgbToLab([248, 206, 104]))
    case .dark: FLabs(bg: FieldMath.rgbToLab([15, 21, 19]), teal: FieldMath.rgbToLab([20, 184, 160]), gold: FieldMath.rgbToLab([236, 126, 64]))
    }
}

enum FieldTune {
    static let omega = 0.09
    static let sigma0 = 0.26
    static let gamma = 0.5
    static let aNorm = 0.55
    static let spill = 1.6
    static let colourMs = 150.0
    static let capLight = 0.76
    static let capDark = 0.74
    static let warpScale = 1.9
    static let noiseRate = 0.35
    static let dtMax = 100.0
    static let xfMs = 180.0
    static let pullAmt = 0.65
    static let connectHitS = 0.12
    static let markHitMs = 217.0
    /** hx, hy, ax, ay, wx, wy, px, py */
    static let lj: [[Double]] = [
        [-0.55, -0.50, 0.30, 0.34, 1.0000, 1.3247, 0.3, 1.9],
        [0.62, -0.20, 0.34, 0.28, 0.7071, 1.6180, 2.2, 0.7],
        [-0.30, 0.10, 0.28, 0.38, 1.4142, 0.8660, 4.1, 3.3],
        [0.50, 0.45, 0.36, 0.30, 1.1180, 1.3591, 1.0, 5.0],
        [-0.15, 0.80, 0.32, 0.32, 0.9069, 1.7321, 5.5, 2.4],
    ]
}

/** Notes are: freq Hz, at s, dur s, peak, attack s. */
struct FieldPattern { let periodMs: Double; let notes: [[Double]] }

private let patRing = FieldPattern(periodMs: 3200, notes: [[784, 0, 1.0, 0.5, 0.02], [988, 0.18, 1.0, 0.45, 0.02], [1175, 0.36, 1.0, 0.4, 0.02], [1568, 0.7, 1.4, 0.3, 0.02]])
private let patBack = FieldPattern(periodMs: 4000, notes: [[392, 0, 1.2, 0.4, 0.18], [494, 0, 1.2, 0.35, 0.18]])
private let patConnected = FieldPattern(periodMs: 0, notes: [[523, 0, 0.5, 0.45, 0.02], [784, 0.12, 0.7, 0.45, 0.02]])
private let patEnded = FieldPattern(periodMs: 0, notes: [[784, 0, 0.45, 0.4, 0.02], [523, 0.12, 0.7, 0.4, 0.02]])
private let patFailed = FieldPattern(periodMs: 0, notes: [[247, 0, 0.5, 0.45, 0.02], [196, 0.24, 0.8, 0.45, 0.02]])
private let patPulse = FieldPattern(periodMs: 0, notes: [[330, 0, 0.6, 0.35, 0.02]])
private let patWaiting = FieldPattern(periodMs: 3000, notes: [[784, 0, 0.16, 0.22, 0.012], [784, 0.22, 0.2, 0.22, 0.012]])

private func noteLevel(_ n: [Double], _ tau: Double) -> Double {
    let dur = n[2], peak = n[3], atk = n[4]
    if tau <= 0 || tau >= dur { return 0 }
    if tau < atk { return peak * tau / atk }
    return peak * pow(0.0001 / peak, (tau - atk) / (dur - atk))
}
private func pitchU(_ f: Double) -> Double { FieldMath.clamp(log(f / FieldMath.fMin) / log(FieldMath.fMax / FieldMath.fMin), 0, 1) }

private struct ActiveNote { let n: [Double]; let s: Double }
private func activeNotes(_ p: FieldPattern, _ tauMs: Double, _ looped: Bool) -> [ActiveNote] {
    var out: [ActiveNote] = []
    out.reserveCapacity(4)
    if tauMs < 0 { return out }
    let tau = tauMs / 1000
    let per = p.periodMs / 1000
    var maxEnd = -Double.greatestFiniteMagnitude
    for n in p.notes { maxEnd = max(maxEnd, n[1] + n[2]) }
    let lp = looped && per != 0
    let kHi: Int64 = lp ? Int64((tau / per).rounded(.down)) : 0
    let kLo: Int64 = lp ? max(Int64(((tau - maxEnd) / per).rounded(.down)), 0) : 0
    for k in stride(from: kLo, through: kHi, by: 1) {
        for n in p.notes {
            let s = tau - Double(k) * per - n[1]
            if s > 0 && s < n[2] { out.append(ActiveNote(n: n, s: s)) }
        }
    }
    return out
}

/** Mulberry32, bit-exact with the JS rng(). */
struct FieldRng {
    private var a: UInt32
    init(_ seed: Int) { a = UInt32(truncatingIfNeeded: seed) }
    mutating func next() -> Double {
        a = a &+ 0x6D2B_79F5
        let s = a
        var t = (s ^ (s >> 15)) &* (1 | s)
        t = (t &+ (t ^ (t >> 7)) &* (61 | t)) ^ t
        return Double(t ^ (t >> 14)) / 4_294_967_296.0
    }
}

private struct Burst { let s: Double; let e: Double; let amp: Double; let ph: Double }
/** Simulated speech bursts (model.ts makeVoice). */
private struct Voice {
    private var r: FieldRng
    private var bursts: [Burst] = []
    private var t = 600.0
    init(_ seed: Int) { r = FieldRng(seed) }
    mutating func at(_ ms: Double) -> Double {
        while t < ms + 6000 {
            let d = 700 + r.next() * 1800
            let g = 500 + r.next() * 1500
            let amp = 0.55 + r.next() * 0.45
            let ph = r.next() * 6.28
            bursts.append(Burst(s: t, e: t + d, amp: amp, ph: ph))
            t += d + g
        }
        var v = 0.0
        let n = bursts.count
        var i = n - 1
        while i >= 0 && i > n - 40 {
            let b = bursts[i]
            i -= 1
            if ms < b.s { continue }
            let up = FieldMath.smooth((ms - b.s) / 160)
            let rel = ms > b.e ? exp(-(ms - b.e) / 260) : 1.0
            let syl = 0.78 + 0.22 * sin(ms / 1000 * 2 * Double.pi * 3.6 + b.ph) * sin(ms / 1000 * 2 * Double.pi * 0.9 + 1)
            v = max(v, b.amp * up * rel * syl)
            if ms > b.e + 2000 { break }
        }
        return v
    }
}

enum CallFieldState: Int, CaseIterable {
    case calm, incoming, outgoing, connecting, active, waiting, reconnecting, ended, failed

    /** The cue pattern this state's notes come from (STATE_CUE in cues.ts). */
    fileprivate func pattern() -> FieldPattern? {
        switch self {
        case .incoming: patRing
        case .outgoing: patBack
        case .active: patConnected
        case .waiting: patWaiting
        case .reconnecting: patPulse
        case .ended: patEnded
        case .failed: patFailed
        case .calm, .connecting: nil
        }
    }
    /** How long after the state change the cue's first note sounds: connect waits so its landing note meets the string going taut. */
    func cueDelayMs(reduced: Bool) -> Double {
        self == .active && !reduced ? max(FieldTune.markHitMs - FieldTune.connectHitS * 1000, 0) : 0
    }
    fileprivate func cfg() -> FieldCfg { fieldStates[rawValue] }
}

private let ne = 9
private let iWarp = 2
private let iPull = 3
private let iSpread = 4
private let iChroma = 5

/** Eased fields e, in this order: v, a0, warp, pull, spread, chroma, kappa, swell, ak. */
private struct FieldCfg {
    let cue: Bool, looped: Bool, e: [Double], bias: Double, blend: Double, voice: Bool
    /** delay, dur (ms) */
    let fade: [Double]?
    /** Per-field time-constant overrides (ms); 0 = use blend. */
    let tc: [Double]
    init(_ cue: Bool, _ looped: Bool, _ e: [Double], _ bias: Double, _ blend: Double, _ voice: Bool, _ fade: [Double]?,
         _ tcs: [(Int, Double)] = []) {
        self.cue = cue; self.looped = looped; self.e = e; self.bias = bias; self.blend = blend; self.voice = voice; self.fade = fade
        var tc = [Double](repeating: 0, count: ne)
        for (i, v) in tcs { tc[i] = v }
        self.tc = tc
    }
}

private let fieldStates: [FieldCfg] = [
    FieldCfg(false, false, [0.50, 0.8, 0.08, 0.0, 0.0, 1.0, 1.0, 0.30, 0.45], 0.35, 900, false, nil),
    FieldCfg(true, true, [1.00, 0.9, 0.12, 0.0, 0.0, 1.0, 2.6, 0.38, 1.0], 0.74, 300, false, nil),
    FieldCfg(true, true, [0.42, 0.85, 0.12, 0.0, 0.0, 1.0, 2.2, 0.38, 1.1], 0.08, 450, false, nil),
    FieldCfg(false, false, [0.80, 0.85, 0.012, 1.0, 0.0, 1.0, 1.6, 0.30, 0.7], 0.12, 350, false, nil, [(iWarp, 330), (iPull, 520)]),
    FieldCfg(true, false, [0.30, 0.6, 0.07, 0.0, 0.0, 1.0, 1.0, 0.30, 0.6], 0.30, 1200, true, nil, [(iPull, 1400)]),
    FieldCfg(true, true, [0.34, 0.65, 0.07, 0.0, 0.0, 1.0, 1.2, 0.32, 0.8], 0.52, 900, true, nil),
    FieldCfg(true, false, [0.22, 0.55, 0.05, 0.5, 0.0, 0.45, 1.2, 0.30, 0.8], 0.20, 700, false, nil, [(iPull, 900), (iChroma, 500)]),
    FieldCfg(true, false, [0.60, 0.8, 0.10, 0.0, 0.55, 1.0, 1.6, 0.30, 0.6], 0.40, 300, false, [0, 300], [(iSpread, 320)]),
    FieldCfg(true, false, [0.30, 0.7, 0.05, 0.0, 0.0, 0.0, 1.2, 0.30, 0.6], 0.20, 500, false, [350, 650], [(iChroma, 160)]),
]

struct FieldLobe { let x: Double, y: Double, sigma: Double, alpha: Double, lab: [Double] }
struct FieldFrame {
    let tau: Double, warp: Double, cap: Double, bg: [Double], lobes: [FieldLobe]
    /** False once an end/failed fade has finished: nothing moves any more. */
    let animating: Bool
    /** Reduced motion: when the next note lands (model ms), so a sleeping loop knows when to wake. */
    let wakeAt: Double?
}

private struct Cur { let id: Int64; let state: CallFieldState; let t0: Double; let c0: Double; let pat: FieldPattern? }
private struct Prev { let st: Cur; let at: Double; let ms: Double }
private struct Disp { let alpha: [Double]; let u: [Double]; let chroma: Double }
private struct Xf { let t0: Double; let from: Disp; let to: Disp }
private struct OnsetKey: Equatable { let k: Int64; let i: Int }
private struct Onset { let t: Double; let key: OnsetKey; let freq: Double }
private struct MarkKey: Equatable { let a: Int64; let b: Int64 }

final class CallField {
    private let K = FieldMath.k
    private var theme: FieldTheme
    private var labs: FLabs
    private(set) var reduced: Bool
    private var muteG = 1.0
    private var muted = false
    private var voice = Voice(7)
    /** w, h, avatar x, avatar y (px) */
    private var view: [Double] = [390, 844, 195, 178]
    private var cur: Cur?
    private var prevs: [Prev] = []
    private var id: Int64 = 0
    private var s: [Double]?
    private var lu: [Double]
    private var tu: [Double]
    private var key: [MarkKey?]
    private var tau = 0.0
    private var lastT: Double?
    private var fadeU = 1.0
    private var hasFrame = false
    private var xf: Xf?
    private var rKey: OnsetKey?
    private var rNoteU: Double?
    private var disp: Disp

    init(theme: FieldTheme, reduced: Bool) {
        self.theme = theme
        self.labs = fLabs(theme)
        self.reduced = reduced
        lu = [Double](repeating: 0.5, count: FieldMath.k)
        tu = [Double](repeating: 0.5, count: FieldMath.k)
        key = [MarkKey?](repeating: nil, count: FieldMath.k)
        disp = Disp(alpha: [Double](repeating: 0, count: FieldMath.k), u: [Double](repeating: 0.5, count: FieldMath.k), chroma: 1)
    }

    var state: CallFieldState? { cur?.state }
    func setView(w: Double, h: Double, avx: Double, avy: Double) { view = [w, h, avx, avy] }
    func setMute(_ m: Bool) { muted = m }
    func setTheme(_ t: FieldTheme) { if t != theme { theme = t; labs = fLabs(t) } }
    func setReduced(_ r: Bool) {
        if r == reduced { return }
        reduced = r
        if r { rKey = nil; rNoteU = nil; xf = nil }
        else if let c = cur { lu = disp.u; s = c.state.cfg().e }
    }

    private func paletteU(_ cfg: FieldCfg, _ i: Int) -> Double { FieldMath.clamp(cfg.bias + (Double(i) - 2) * 0.07, 0, 1) }
    private func curve(_ u: Double) -> Double {
        let x = FieldMath.smooth(FieldMath.clamp((u - 0.2) / 0.75, 0, 1))
        return theme == .dark ? FieldMath.smooth(FieldMath.smooth(x)) : x
    }
    private func colourAt(_ u: Double, _ chroma: Double) -> [Double] {
        let c = FieldMath.mixLab(labs.teal, labs.gold, curve(u))
        return [c[0], c[1] * chroma, c[2] * chroma]
    }
    private func halfExt() -> [Double] { let sh = min(view[0], view[1]); return [view[0] / sh / 2, view[1] / sh / 2] }
    private func avatarPt() -> [Double] { let sh = min(view[0], view[1]); return [(view[2] - view[0] / 2) / sh, (view[3] - view[1] / 2) / sh] }
    private func lobePos(_ i: Int, _ tau: Double, _ h: [Double], _ pull: Double, _ spread: Double) -> [Double] {
        let j = FieldTune.lj[i]
        let sx = max(h[0] / 0.5, 1), sy = max(h[1] / 0.5, 1)
        var x = j[0] * h[0] + j[2] * sx * sin(j[4] * FieldTune.omega * tau + j[6])
        var y = j[1] * h[1] + j[3] * sy * sin(j[5] * FieldTune.omega * tau + j[7])
        if pull > 0 {
            let a = avatarPt(), g = pull * FieldTune.pullAmt
            x = FieldMath.lerp(x, a[0], g); y = FieldMath.lerp(y, a[1], g)
        }
        if spread != 0 { x *= 1 + spread; y *= 1 + spread }
        return [x, y]
    }
    private func cap() -> Double { theme == .dark ? FieldTune.capDark : FieldTune.capLight }

    /** `cueStart`: absolute model time (ms) of the cue's first note; nil anchors it on the state change. */
    func setState(_ state: CallFieldState, t: Double, cueStart: Double? = nil) {
        let cfg = state.cfg()
        if let c = cur { prevs.append(Prev(st: c, at: t, ms: 220)) }
        id += 1
        cur = Cur(id: id, state: state, t0: t, c0: cueStart ?? t, pat: state.pattern())
        if s == nil { s = cfg.e }
        for i in 0..<K {
            tu[i] = paletteU(cfg, i)
            key[i] = nil
            if !hasFrame { lu[i] = tu[i] }
        }
        if lastT == nil { lastT = t }
        rKey = nil; rNoteU = nil
        if reduced { startXf(t) }
    }

    // ---- reduced motion: static gradient, per-note colour crossfade ----
    private func staticTarget(_ noteU: Double?) -> Disp {
        let cfg = cur!.state.cfg()
        let a = cfg.fade != nil ? 0 : cfg.e[1]
        let u = (0..<K).map { i -> Double in let p = paletteU(cfg, i); return noteU == nil ? p : FieldMath.lerp(p, noteU!, 0.75) }
        return Disp(alpha: [Double](repeating: a, count: K), u: u, chroma: cfg.e[iChroma])
    }
    private func startXf(_ t0: Double) { xf = Xf(t0: t0, from: disp, to: staticTarget(rNoteU)) }

    /** Latest onset <= t and next onset > t of a cue started at c0. */
    private func scanOnsets(_ p: FieldPattern?, _ looped: Bool, _ c0: Double, _ t: Double) -> (Onset?, Double?) {
        guard let p else { return (nil, nil) }
        let per = p.periodMs
        let lp = looped && per != 0
        let lo: Int64 = lp ? Int64(((t - c0) / per).rounded(.down)) : 0
        var latest: Onset?
        var next: Double?
        let hi: Int64 = lp ? lo + 1 : 0
        for k in stride(from: max(lo - 1, 0), through: hi, by: 1) {
            for (i, n) in p.notes.enumerated() {
                let ot = c0 + Double(k) * per + n[1] * 1000
                if ot <= t {
                    if latest == nil || ot >= latest!.t { latest = Onset(t: ot, key: OnsetKey(k: k, i: i), freq: n[0]) }
                } else if next == nil || ot < next! { next = ot }
            }
        }
        return (latest, next)
    }

    private func reducedFrame(_ t: Double) -> FieldFrame {
        let cu = cur!
        let cfg = cu.state.cfg()
        let (lt, next) = scanOnsets(cu.pat, cfg.looped, cu.c0, t)
        if cfg.cue, let lt, cu.pat != nil, lt.key != rKey {
            rKey = lt.key
            rNoteU = pitchU(lt.freq)
            startXf(max(lt.t, t - FieldTune.xfMs))
        }
        if xf == nil { startXf(t - FieldTune.xfMs) }
        let x = xf!
        let e = FieldMath.smooth((t - x.t0) / FieldTune.xfMs)
        let alpha = (0..<K).map { FieldMath.lerp(x.from.alpha[$0], x.to.alpha[$0], e) }
        let u = (0..<K).map { FieldMath.lerp(x.from.u[$0], x.to.u[$0], e) }
        let chroma = FieldMath.lerp(x.from.chroma, x.to.chroma, e)
        disp = Disp(alpha: alpha, u: u, chroma: chroma)
        let h = halfExt()
        let lobes = (0..<K).map { i -> FieldLobe in
            let p = lobePos(i, 7.3, h, 0, 0)
            return FieldLobe(x: p[0], y: p[1], sigma: FieldTune.sigma0, alpha: alpha[i], lab: colourAt(u[i], chroma))
        }
        let animating = e < 1
        return FieldFrame(tau: 7.3, warp: 0, cap: cap(), bg: labs.bg, lobes: lobes, animating: animating, wakeAt: animating ? nil : next)
    }

    private func addNotes(_ st: Cur, _ t: Double, _ k: Double, _ lraw: inout [Double], _ mark: inout [(Int64, Double)?], _ marking: Bool) {
        let cfg = st.state.cfg()
        guard let pat = st.pat else { return }
        if !cfg.cue || k <= 0 { return }
        for an in activeNotes(pat, t - st.c0, cfg.looped) {
            let n = an.n
            let e = noteLevel(n, an.s)
            let pk = n[3]
            let lv = pk * pow(e / pk, FieldTune.gamma) * k
            let uu = pitchU(n[0]) * Double(K)
            for i in 0..<K {
                let w = max(1 - abs(uu - (Double(i) + 0.5)) / FieldTune.spill, 0)
                if w <= 0 { continue }
                lraw[i] += lv * w
                if marking && w >= 0.3 {
                    let onset = Int64(FieldMath.jsRound(t - an.s * 1000))
                    if let p = mark[i] { if onset >= p.0 { mark[i] = (onset, n[0]) } } else { mark[i] = (onset, n[0]) }
                }
            }
        }
    }

    /** Advance to model time `t` (ms) and return the frame to draw. */
    func update(_ t: Double) -> FieldFrame {
        guard let cu = cur else { fatalError("setState first") }
        let dt = lastT.map { FieldMath.clamp(t - $0, 0, FieldTune.dtMax) } ?? 0
        lastT = t
        muteG += ((muted ? 0 : 1) - muteG) * (1 - exp(-dt / 120))
        if reduced { let f = reducedFrame(t); hasFrame = true; return f }
        let cfg = cu.state.cfg()
        var sv = s ?? cfg.e
        for k in 0..<ne {
            let tc = (cfg.tc[k] != 0 ? cfg.tc[k] : cfg.blend) / 3.2
            sv[k] += (cfg.e[k] - sv[k]) * (1 - exp(-dt / tc))
        }
        s = sv
        var lraw = [Double](repeating: 0, count: K)
        var mark = [(Int64, Double)?](repeating: nil, count: K)
        addNotes(cu, t, 1, &lraw, &mark, true)
        prevs.removeAll { t - $0.at >= $0.ms }
        var none = [(Int64, Double)?](repeating: nil, count: K)
        for p in prevs { addNotes(p.st, t, 1 - FieldMath.smooth((t - p.at) / p.ms), &lraw, &none, false) }
        let l = lraw.map { tanh($0 / FieldTune.aNorm) }
        let e = l.reduce(0, +)
        let ctc = max(FieldTune.colourMs, cfg.blend / 4)
        for i in 0..<K {
            if let m = mark[i] {
                let k2 = MarkKey(a: cu.id, b: m.0)
                if key[i] != k2 { key[i] = k2; tu[i] = pitchU(m.1) }
            }
            lu[i] += (tu[i] - lu[i]) * (1 - exp(-dt / ctc))
        }
        let since = t - cu.t0
        let vo = cfg.voice ? voice.at(since) * muteG : 0
        tau += dt / 1000 * sv[0] * (1 + sv[6] * e + 0.5 * vo)
        let fd = cfg.fade
        let inFade = fd != nil && since >= fd![0]
        let step = inFade ? -dt / fd![1] : dt / 400
        fadeU = FieldMath.clamp(fadeU + step, 0, 1)
        let fade = FieldMath.smooth(fadeU)
        let h = halfExt()
        let lobes = (0..<K).map { i -> FieldLobe in
            let p = lobePos(i, tau, h, sv[iPull], sv[iSpread])
            let sigma = FieldTune.sigma0 * (1 + sv[7] * l[i] + 0.06 * vo) * (1 - 0.3 * sv[iPull])
            let alpha = (sv[1] + 0.07 * vo + sv[8] * l[i]) * fade
            return FieldLobe(x: p[0], y: p[1], sigma: sigma, alpha: alpha, lab: colourAt(lu[i], sv[iChroma]))
        }
        disp = Disp(alpha: lobes.map(\.alpha), u: lu, chroma: sv[iChroma])
        let animating = !(cfg.fade != nil && fadeU <= 0)
        hasFrame = true
        return FieldFrame(tau: tau, warp: sv[iWarp], cap: cap(), bg: labs.bg, lobes: lobes, animating: animating, wakeAt: nil)
    }
}
