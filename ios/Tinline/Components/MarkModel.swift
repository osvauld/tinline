import Foundation

/*
 * The brand mark D, ported from docs/design/playground/src/lib/brand/{tinmark,tiers}.js (android Mark.kt).
 * Same constants, same model: a fixed-length quadratic Bezier string between two cans, damped springs that
 * retarget mid-flight (never keyframes), string length conserved. Geometry is in the 24-unit model space,
 * cropped per optical-size tier. Change the numbers there first, then here.
 * This file is the pure model (no UI, no clocks); TinMark.swift draws it.
 */

enum MarkState { case slack, ringing, taut }

/// In-app palette of Mark.svelte: can = --accent (dark theme #117e6b, not the mint text colour pr), mouth always #9fd6c8,
/// string = --gold (tin.thread).
enum MarkColors {
    static let canDark: UInt32 = 0x117E6B
    static let mouth: UInt32 = 0x9FD6C8
}

private enum MarkDef {
    static let C = 7.5, B = 5.5, THETA = 30.6, MU = 0.62, L = 3.0, S0 = 3.5
    static let STROKE = 1.5, PHI0 = 12.0, T = 1.6, A = 0.5, PLUCK_A = 0.32, PLUCK_SWING = 150.0
    static let F = 7.0, TAU = 0.11, CONNECT_HZ = 3.0, END_ZETA = 1.6, END_MS = 260.0
}

private func rad(_ d: Double) -> Double { d * Double.pi / 180 }

/// Offset of the `line` cue's second note (s): the two tugs land this far apart (Cues.kt LINE).
let markLineGapS = 0.12

/// Per-use spring parameters of the one model (tinmark.js DEF and overrides, MARK-MATH.md "Connect", 2026-10-13). Defaults are
/// the everyday pull: it arrives with speed (`hitOver`), the cans bounce back off the tight string (`recoil` = restitution), and an
/// undamped `settleHz` pull carries them out again until a bounce is below `recoilMin`. `tugs` > 0 tugs exactly that many times.
struct MarkSpring: Equatable {
    var hitOver = 1.6, recoil = 0.5, recoilMin = 1.2, settleHz = 2.0
    var windup = 0.0, windupMs = 180.0
    var tugs = 0

    /// The old dead stop (recoil 0).
    static let deadStop = MarkSpring(hitOver: 1.1, recoil: 0.0)
    /// springs.ts LINE_SPRING: two tugs on the rhythm of the `line` cue (sign-up Done, the first contact).
    static let line = MarkSpring(recoil: 0.95, recoilMin: 0.0, settleHz: 1 / (2 * markLineGapS), tugs: 2)
}

private func lenExact(_ w: Double, _ s0: Double) -> Double {
    let s = abs(s0)
    if s < 1e-9 { return w }
    return 0.5 * (w * w + 16 * s * s).squareRoot() + (w * w / (8 * s)) * asinh(4 * s / w)
}

private func sOfW(_ w: Double, _ ell: Double) -> Double {
    if w >= ell { return 0 }
    let sa = (3 * w * (ell - w) / 8).squareRoot()
    var lo = 0.0, hi = max(sa * 1.5, 1e-6)
    while lenExact(w, hi) < ell { hi *= 2 }
    for _ in 0..<40 {
        let m = (lo + hi) / 2
        if lenExact(w, m) < ell { lo = m } else { hi = m }
    }
    return (lo + hi) / 2
}

/// Tier parameters in model units (tiers.js tierModel).
struct MarkTier {
    let c: Double, ko: Double, a: Double, b: Double, len: Double, mu: Double, sw: Double, s0: Double
    let vb: Double, vbSize: Double, sagOnly: Bool, sT: Double, hasMouth: Bool, flat: Bool
    let w0: Double, ell: Double, dt: Double

    init(c: Double, ko: Double, a: Double, b: Double, len: Double, mu: Double, sw: Double, s0: Double,
         vb: Double, vbSize: Double, sagOnly: Bool, sT: Double, hasMouth: Bool, flat: Bool) {
        self.c = c; self.ko = ko; self.a = a; self.b = b; self.len = len; self.mu = mu; self.sw = sw; self.s0 = s0
        self.vb = vb; self.vbSize = vbSize; self.sagOnly = sagOnly; self.sT = sT; self.hasMouth = hasMouth; self.flat = flat
        w0 = 24 - 2 * c
        ell = lenExact(w0, s0)
        dt = max(0, (ell - w0) / 2)
    }
}

private struct SmallTier {
    let c, a, b, l, mu: Double
    let flat: Bool
    let ko, sw, s: Double
    let sT: Double?
}

private let smallTiers: [Int: SmallTier] = [
    12: SmallTier(c: 3.5, a: 1.5, b: 4.0, l: 1.5, mu: 0.5, flat: true, ko: 0.75, sw: 1.5, s: 2.5, sT: 0.5),
    14: SmallTier(c: 4.5, a: 1.5, b: 5.0, l: 2.0, mu: 0.5, flat: true, ko: 0.75, sw: 1.5, s: 3.0, sT: 0.5),
    16: SmallTier(c: 5.0, a: 2.0, b: 5.0, l: 2.0, mu: 0.5, flat: true, ko: 1.25, sw: 1.75, s: 3.0, sT: 0.5),
    20: SmallTier(c: 6.0, a: 2.5, b: 6.0, l: 2.5, mu: 0.5, flat: true, ko: 1.25, sw: 1.75, s: 3.5, sT: 0.75),
    24: SmallTier(c: 7.5, a: 3.0, b: 6.0, l: 3.0, mu: 0.6, flat: false, ko: 0.0, sw: 1.75, s: 3.5, sT: nil),
    28: SmallTier(c: 8.5, a: 3.5, b: 7.0, l: 3.5, mu: 0.6, flat: false, ko: 0.0, sw: 1.75, s: 4.0, sT: nil),
]
private let tierSizes = [12, 14, 16, 20, 24, 28, 32, 40, 48, 64, 96, 128, 256, 512, 1024]

/// The optical-size tier nearest `px` (ties go to the smaller).
func markPickTier(_ px: Double) -> Int {
    var best = tierSizes[0]
    for t in tierSizes where abs(Double(t) - px) < abs(Double(best) - px) || (abs(Double(t) - px) == abs(Double(best) - px) && t < best) { best = t }
    return px > 1024 ? 1024 : best
}

private func half(_ v: Double) -> Double { (v * 2).rounded(.toNearestOrEven) / 2 }

/// kotlin.math.round: ties to even.
private func roundHalf(_ v: Double) -> Double { v.rounded(.toNearestOrEven) }

func markTierFor(_ px: Double) -> MarkTier {
    let bx = markPickTier(px)
    let u: Double, c: Double, a: Double, b: Double, l: Double, mu: Double, sw: Double, s: Double
    let sT: Double?, ko: Double, flat: Bool
    if let sm = smallTiers[bx] {
        u = 1; c = sm.c; a = sm.a; b = sm.b; l = sm.l; mu = sm.mu; sw = sm.sw; s = sm.s; sT = sm.sT; ko = sm.ko; flat = sm.flat
    } else {
        u = Double(bx) / 24
        let exact = bx >= 96
        func sn(_ v: Double, _ f: (Double) -> Double) -> Double { exact ? v : f(v) }
        b = sn(MarkDef.B * u, roundHalf)
        a = sn(MarkDef.B * sin(rad(MarkDef.THETA)) * u, half)
        c = sn(MarkDef.C * u, half); l = sn(MarkDef.L * u, half); mu = MarkDef.MU
        sw = max(1.5, sn(MarkDef.STROKE * u, half)); s = MarkDef.S0 * u; sT = nil; ko = 0; flat = false
    }
    let half0 = Double(bx) / (2 * u)
    return MarkTier(
        c: 12 - (Double(bx) / 2 - c) / u, ko: ko / u, a: a / u, b: b / u, len: l / u, mu: mu, sw: sw / u, s0: s / u,
        vb: 12 - half0, vbSize: Double(bx) / u, sagOnly: sT != nil, sT: (sT ?? 0) / u, hasMouth: mu != 0, flat: flat)
}

/// The spring model (tinmark.js attach). Pure state: `step()` advances it, no clocks or UI in here.
final class MarkModel {
    private let p: MarkTier
    var spring: MarkSpring
    private(set) var d = 0.0
    private var v = 0.0
    private(set) var phi = 0.0
    private var phv = 0.0
    private var tugsNow = 0, v0 = 0.0, wind = 0.0
    /// Every tug so far (the widget reports each new one); `tight`: the string went tight since the last slack.
    private(set) var tugCount = 0
    private(set) var tight = false
    private var ring = false, ringPhase = 0.0
    private var taut = false, hit = false
    private var tw = -1.0, twA = 0.0
    private var swayT = MarkDef.T, swayAmp = MarkDef.PHI0

    init(_ p: MarkTier, spring: MarkSpring = MarkSpring()) {
        self.p = p
        self.spring = spring
        let z = MarkDef.END_ZETA, k = z + (z * z - 1).squareRoot(), m = z - (z * z - 1).squareRoot()
        let thr = 0.01 / max(p.dt, 1e-6)
        func x(_ u: Double) -> Double { (k * exp(-m * u) - m * exp(-k * u)) / (k - m) }
        var lo = 0.0, hi = 200.0
        for _ in 0..<60 {
            let mid = (lo + hi) / 2
            if x(mid) > thr { lo = mid } else { hi = mid }
        }
        endOmega = hi / (MarkDef.END_MS / 1000)
    }

    func setSway(period: Double, amp: Double) { swayT = period; swayAmp = amp }

    private let endOmega: Double
    private let connectW = 2 * Double.pi * MarkDef.CONNECT_HZ

    var active: Bool {
        ring || tw >= 0 || wind > 0 || (taut ? (d < p.dt || v != 0) : d >= 0.01) || abs(phi) > rad(0.2) || abs(phv) > rad(2.0)
    }

    private func sag() -> Double {
        if p.sagOnly { return p.s0 + (p.sT - p.s0) * (p.dt > 0 ? min(1, d / p.dt) : 0) }
        return sOfW(24 - 2 * (p.c - d), p.ell)
    }

    private func twang() -> Double {
        tw < 0 ? 0 : (twA != 0 ? twA : MarkDef.A) * exp(-tw / MarkDef.TAU) * sin(2 * Double.pi * MarkDef.F * tw)
    }

    var sagNow: Double { sag() + twang() }
    /// Outward can displacement; small tiers keep the cans still.
    var delta: Double { p.sagOnly ? 0 : d }

    func step(_ h: Double) {
        let sp = spring
        let z: Double, w: Double, tgt: Double
        if wind > 0 {
            wind -= h; z = 1; w = 2 * Double.pi / (sp.windupMs / 1000) * 1.2; tgt = -sp.windup
        } else if taut && hit {
            z = 0; w = 2 * Double.pi * sp.settleHz; tgt = p.dt   // the string pulls the recoiling cans back out
        } else if taut {
            z = 1; w = connectW; tgt = p.dt * sp.hitOver
        } else {
            z = MarkDef.END_ZETA; w = endOmega; tgt = 0
        }
        v += (-2 * z * w * v - w * w * (d - tgt)) * h; d += v * h
        if taut && d >= p.dt && v <= 0 {
            d = p.dt   // resting at taut is not a tug (tinmark.js re-plucks here every frame)
        } else if taut && d >= p.dt {
            // The string goes tight and tugs the cans back: velocity reflected (restitution `recoil`); each bounce re-plucks.
            let vin = v; d = p.dt; tugsNow += 1
            let last = sp.tugs > 0 && tugsNow >= sp.tugs
            v = (!last && sp.recoil > 0 && vin * sp.recoil >= sp.recoilMin) ? -vin * sp.recoil : 0
            if !hit {
                hit = true; tw = 0; twA = 0
            } else if sp.tugs > 0 {
                tw = 0; twA = MarkDef.A * 0.8
            } else if vin > sp.recoilMin {
                tw = 0; twA = min(MarkDef.A, MarkDef.A * vin / (v0 > 0 ? v0 : vin)) * 0.6
            }
            tugCount += 1; tight = true
            if v0 == 0 { v0 = vin }
        }
        if d < -sp.windup { d = -sp.windup; if v < 0 { v = 0 } }
        if d < 0 && wind <= 0 && !taut { d = 0; if v < 0 { v = 0 } }
        if ring {
            ringPhase += 2 * Double.pi / swayT * h
            let tgtPhi = rad(swayAmp) * sin(ringPhase), k = 1 - exp(-h / 0.05)
            let old = phi; phi += (tgtPhi - phi) * k; phv = (phi - old) / h
        } else {
            let zz = taut ? 1.0 : 0.5, ww = taut ? connectW : 2 * Double.pi * 3.5
            phv += (-2 * zz * ww * phv - ww * ww * phi) * h; phi += phv * h
        }
        if tw >= 0 {
            tw += h
            if (twA != 0 ? twA : MarkDef.A) * exp(-tw / MarkDef.TAU) < 0.03 { tw = -1 }
        }
    }

    /// Advance by real `dt0` seconds in 1/480 s sub-steps; returns false once everything has settled.
    @discardableResult
    func advance(_ dt0: Double) -> Bool {
        let dt = min(dt0, 0.05)
        let n = max(1, Int((dt / (1.0 / 480)).rounded(.up))), h = dt / Double(n)
        for _ in 0..<n { step(h) }
        if !active { finish(); return false }
        return true
    }

    private func finish() {
        if !taut { d = 0; v = 0 }
        phi = 0; phv = 0; tw = -1
    }

    /// Reduced motion: jump to the end pose.
    func snap(toTaut: Bool) {
        let fresh = toTaut && !hit
        ring = false; taut = toTaut; d = toTaut ? p.dt : 0; v = 0; phi = 0; phv = 0; tw = -1; hit = toTaut; wind = 0; tight = toTaut
        if fresh { tugCount += 1 }
    }

    /// Mount pose: taut without counting a tug (a mark that mounts taut doesn't replay it).
    func set(_ state: MarkState) { snap(toTaut: state == .taut); tugCount = 0 }

    func setRing(_ on: Bool) { ring = on; if on { taut = false; hit = false } }

    func connect() {
        ring = false; taut = true; hit = hit && d >= p.dt; v0 = 0; tugsNow = 0
        wind = spring.windup > 0 && d < 0.01 ? spring.windupMs / 1000 : 0
    }

    func end() {
        ring = false; taut = false; hit = false; tight = false; wind = 0; tw = -1
        if abs(phi) < rad(1.0) { phv += rad(242.0) }
    }

    func pluck() {
        if taut && d >= p.dt - 1e-9 {
            if tw < 0 { twA = MarkDef.PLUCK_A; tw = 0 }
        } else if abs(phi) < rad(1.0) && abs(phv) < rad(20.0) {
            phv += rad(MarkDef.PLUCK_SWING)
        }
    }
}

/// A model at a rendered size (tests).
func markModelFor(_ px: Double, spring: MarkSpring = MarkSpring()) -> MarkModel { MarkModel(markTierFor(px), spring: spring) }

/// Ringing sway: period in seconds, amplitude in degrees.
struct Sway: Equatable { var period: Double; var amp: Double }

/// What the app-bar mark shows (status.svelte.ts MarkLook).
struct MarkLook: Equatable {
    var state: MarkState
    var sway: Sway?
    var dim = false
    var label: String
}

// Prefixed `Mark…` so the Calls and Shell areas keep their own phase enums; map yours onto these for `statusLook`.
enum MarkCallPhase { case none, incoming, outgoing, connecting, active, reconnecting }
enum MarkGroupCallPhase { case none, starting, connecting, active, reconnecting }
enum MarkConn { case offline, connecting, online }

/// Port of statusLook(): call phase wins, then group call phase, then the network connection.
/// Sway periods are locked to the cue patterns: ringtone every 3.2 s (two swings per ring), ringback every 4 s.
func statusLook(call: MarkCallPhase = .none, gcall: MarkGroupCallPhase = .none, conn: MarkConn) -> MarkLook {
    switch call {
    case .incoming: return MarkLook(state: .ringing, sway: Sway(period: 1.6, amp: 12), label: "Incoming call")
    case .outgoing: return MarkLook(state: .ringing, sway: Sway(period: 2.0, amp: 9), label: "Calling")
    case .connecting: return MarkLook(state: .ringing, sway: Sway(period: 1.2, amp: 6), label: "Connecting")
    case .active: return MarkLook(state: .taut, label: "In a call")
    case .reconnecting: return MarkLook(state: .ringing, sway: Sway(period: 2.0, amp: 4), label: "Reconnecting")
    case .none: break
    }
    switch gcall {
    case .starting: return MarkLook(state: .ringing, sway: Sway(period: 2.0, amp: 9), label: "Starting a group call")
    case .connecting: return MarkLook(state: .ringing, sway: Sway(period: 1.2, amp: 6), label: "Connecting")
    case .active: return MarkLook(state: .taut, label: "In a group call")
    case .reconnecting: return MarkLook(state: .ringing, sway: Sway(period: 2.0, amp: 4), label: "Reconnecting")
    case .none: break
    }
    switch conn {
    case .offline: return MarkLook(state: .slack, dim: true, label: "No connection")
    case .connecting: return MarkLook(state: .ringing, sway: Sway(period: 3.2, amp: 5), label: "Connecting")
    case .online: return MarkLook(state: .taut, label: "Connected to the network")
    }
}
