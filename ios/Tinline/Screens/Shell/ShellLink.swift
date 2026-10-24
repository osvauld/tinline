import SwiftUI

/// Device linking (docs/design/device-linking.md): one link at a time, driven by the core's `LinkEvents`. `ui` is the state the
/// screens render; `newDevice` says which side this phone is (N = new, shows or scans with `link_new_*`; E = existing,
/// `link_*`). Core callbacks arrive on core threads: they only hop to the main actor.
@MainActor
final class ShellLinkController: ObservableObject, LinkEvents, @unchecked Sendable {
    enum Ui: Equatable {
        case idle
        /// Our QR is on screen until `expiresAt` (epoch ms).
        case showing(qr: String, expiresAt: Int64)
        /// QR read or scanned; waiting for the other device.
        case connecting
        /// Both ends proved the QR: compare `code` (E approves, N waits).
        case code(code: String, peer: String)
        /// E approved; the grant is on its way.
        case approving(code: String, peer: String)
        case done(did: String, name: String, peer: String)
        case failed(reason: String, detail: String?)

        var busy: Bool { switch self { case .idle, .done: return false; default: return true } }
    }

    static let qrLifetimeMs: Int64 = 5 * 60 * 1000

    @Published private(set) var ui: Ui = .idle
    /// Bumped when the registry changed (linked, renamed, unlinked, seen): lists re-read `linkedDevices()`.
    @Published private(set) var devicesVersion = 0
    private(set) var newDevice = false
    private var peer = ""
    weak var core: ShellCore?
    /// Link operations run one at a time, in order, off the callers' threads.
    private let ops = ShellSerial("p2p-link")

    private var node: Node? { core?.node }

    private func supersede() {
        let busy = ui.busy
        ui = .idle
        if busy, let node { ops.submit { node.linkCancel() } }
    }

    /// Shows our QR (E: `link_show_qr`; N: `link_new_show_qr` with this install's name). Replaces any link in progress.
    func showQr(new: Bool, label: String) {
        newDevice = new; peer = ""
        supersede()
        guard let node else { return }
        ops.submit { [weak self] in
            do {
                let qr = new ? try node.linkNewShowQr(deviceLabel: label) : try node.linkShowQr()
                let exp = ShellText.nowMs() + Self.qrLifetimeMs
                Task { @MainActor in self?.ui = .showing(qr: qr, expiresAt: exp) }
            } catch {
                let d = ShellText.friendly(error)
                Task { @MainActor in self?.ui = .failed(reason: "error", detail: d) }
            }
        }
    }

    /// The other device's QR text (camera, or typed).
    func scan(_ text: String, new: Bool, label: String) {
        newDevice = new; peer = ""
        supersede()
        ui = .connecting
        guard let node else { return }
        let t = text.trimmingCharacters(in: .whitespacesAndNewlines)
        ops.submit { [weak self] in
            do {
                if new { try node.linkNewScan(qrText: t, deviceLabel: label) } else { try node.linkScan(qrText: t) }
            } catch {
                let d = ShellText.friendly(error)
                Task { @MainActor in self?.ui = .failed(reason: "error", detail: d) }
            }
        }
    }

    /// E: the codes match. A wrong passphrase throws and keeps the link open. Blocking in the core (Argon2): off the main thread.
    func approve(passphrase: String?) async throws {
        guard case .code(let code, let peer) = ui, let node else { return }
        try await shellIO { try node.linkApprove(passphrase: passphrase) }
        if case .code = ui { ui = .approving(code: code, peer: peer) }
    }

    /// Abandons whatever is in progress and goes back to Idle.
    func cancel() {
        let busy = ui.busy
        ui = .idle
        if busy, let node { ops.submit { node.linkCancel() } }
    }

    func reset() { ui = .idle }
    func devicesChanged() { devicesVersion += 1 }

    // ---- LinkEvents (core threads) ----
    nonisolated func onLinkCode(code: String, peerLabel: String) {
        Task { @MainActor in self.peer = peerLabel; self.ui = .code(code: code, peer: peerLabel) }
    }
    nonisolated func onLinkDone(did: String, name: String) {
        Task { @MainActor in self.ui = .done(did: did, name: name, peer: self.peer); self.devicesVersion += 1 }
    }
    nonisolated func onLinkFailed(reason: String) {
        Task { @MainActor in
            if case .done = self.ui { return }
            self.ui = .failed(reason: reason, detail: nil)
        }
    }
    nonisolated func onDevicesChanged() { Task { @MainActor in self.devicesVersion += 1 } }
    nonisolated func onHistoryChanged() { Task { @MainActor in self.core?.refreshHistory() } }
    nonisolated func onUnlinked(did: String) { Task { @MainActor in await self.core?.accountGone(did, notify: true) } }
}

// ------------------------------------------------------------------ the flow (both sides)

private enum ShellLinkStage { case intro, scan, show }

/// Link a device, one side of it. `newDevice` = false: this phone has the account (Settings > Link a device; the playground's
/// devices/LinkDevice.svelte page); true: this phone is the new one (onboarding; gate/Link.svelte). `label` is the new device's
/// name. The screen follows `ShellLinkController.ui`; `onClose` leaves, `onDone` is called once on the new device when the
/// account has arrived (the existing device shows its own "linked" screen). The real QR and the core's code are used throughout.
struct ShellLinkSession: View {
    @ObservedObject var links: ShellLinkController
    let newDevice: Bool
    var label = ""
    var progress: (Int, Int)? = nil
    let onClose: () -> Void
    var onDone: (_ name: String, _ peer: String) -> Void = { _, _ in }
    @State private var stage: ShellLinkStage
    @State private var scanNote: String?
    @State private var scanNonce = 0
    @State private var regen = 0

    init(links: ShellLinkController, newDevice: Bool, label: String = "", progress: (Int, Int)? = nil,
         onClose: @escaping () -> Void, onDone: @escaping (String, String) -> Void = { _, _ in }) {
        self.links = links; self.newDevice = newDevice; self.label = label; self.progress = progress
        self.onClose = onClose; self.onDone = onDone
        _stage = State(initialValue: newDevice ? .show : .intro)
    }

    private func onText(_ text: String) {
        if let bad = linkQrProblem(text: text) { scanNote = bad; return }
        scanNote = nil
        links.scan(text, new: newDevice, label: label)
    }

    private func again() { links.reset(); scanNote = nil; scanNonce += 1; regen += 1; if !newDevice { stage = .intro } }

    var body: some View {
        Group {
            let ui = links.ui
            let scanning = { if case .idle = ui { return true }; if case .showing = ui { return true }; return false }()
            if stage == .scan && scanning {
                ShellLinkScanScreen(newDevice: newDevice, note: scanNote, nonce: scanNonce,
                                    onBack: { scanNote = nil; stage = newDevice ? .show : .intro },
                                    onText: onText, onRetry: { scanNote = nil; scanNonce += 1 }, onShow: { scanNote = nil; stage = .show })
            } else if newDevice {
                ShellNewDeviceLink(ui: ui, progress: progress, onBack: { links.cancel(); onClose() }, onNew: { links.cancel(); regen += 1 },
                                   onScanInstead: { links.cancel(); stage = .scan }, onCancel: { links.cancel(); again() }, onRetry: again)
            } else {
                ShellExistingDeviceLink(links: links, stage: stage, note: scanNote, onClose: onClose, onScan: { stage = .scan },
                                        onShow: { stage = .show }, onTyped: onText, onClearNote: { scanNote = nil },
                                        onNew: { links.cancel(); regen += 1 }, onCancel: { links.cancel(); again() }, onRetry: again)
            }
        }
        .onAppear { links.reset() }
        .onDisappear { links.cancel() }
        .task(id: "\(stage)-\(regen)") { if stage == .show, case .idle = links.ui { links.showQr(new: newDevice, label: label) } }
        .onChange(of: links.ui) { _, ui in if newDevice, case .done(_, let name, let peer) = ui { onDone(name, peer) } }
    }
}

// ------------------------------------------------------------------ new device (gate/Link.svelte)

private struct ShellNewDeviceLink: View {
    let ui: ShellLinkController.Ui
    let progress: (Int, Int)?
    let onBack: () -> Void
    let onNew: () -> Void
    let onScanInstead: () -> Void
    let onCancel: () -> Void
    let onRetry: () -> Void
    @Environment(\.tin) private var tin

    var body: some View {
        let p = ShellPalette(tin)
        let canBack: Bool = { switch ui { case .idle, .showing, .failed: return true; default: return false } }()
        VStack(spacing: 0) {
            ShellGateHead(onBack: canBack ? onBack : nil, progress: progress, label: "Link to your account")
            switch ui {
            case .code(let code, let peer):
                ShellGateStep("Same code on both?", lead: "Check that \(ShellText.ifBlank(peer, "your other device")) shows this code, then approve there.",
                              center: true, anchor: true, content: {
                    ShellBigCode(code: code)
                    ShellNotice("Different code, or you didn\u{2019}t start this? Cancel. Someone else may have scanned the QR.", kind: .warn)
                    ShellWaitingLine(text: "Waiting for approval\u{2026}")
                }, foot: { ShellGBtn(text: "Cancel", kind: .secondary, action: onCancel) })
            case .approving, .connecting, .done:
                ShellGateStep("Linking\u{2026}", lead: "Receiving your account. Keep both devices open.", center: true, anchor: true, content: {
                    ShellSpinner(size: 40, color: p.accent, stroke: 4).padding(.vertical, 40)
                }, foot: { EmptyView() })
            case .failed(let reason, let detail):
                ShellGateStep("Couldn\u{2019}t link", center: true, anchor: true, content: {
                    ShellNotice(ShellText.linkFail(reason, detail), kind: .error)
                }, foot: {
                    ShellGBtn(text: "Try again", action: onRetry)
                    ShellGBtn(text: "Not now", kind: .text, action: onBack)
                })
            case .showing, .idle:
                let showing: (String, Int64)? = { if case .showing(let q, let e) = ui { return (q, e) }; return nil }()
                ShellGateStep("Link to your account", lead: "On a device that already has your account, open Settings \u{203A} Link a device and scan this.",
                              center: true, anchor: true, content: {
                    ShellQrPanel(qr: showing?.0, expiresAt: showing?.1 ?? 0)
                    Text("Anyone who can see this code in the next few minutes could try to use it. Show it only to your own device.")
                        .shellStyle(.sub).foregroundStyle(tin.ink2).multilineTextAlignment(.center).frame(maxWidth: 300)
                }, foot: {
                    ShellGBtn(text: "New code", kind: .text, enabled: showing != nil, action: onNew)
                    ShellGBtn(text: "Scan instead", kind: .text, action: onScanInstead)
                })
            }
        }
        .background(tin.bg.ignoresSafeArea())
    }
}

/// The real QR in a white tile (padding 12, r 20, line border), and the expiry line under it (reserved, tabular).
struct ShellQrPanel: View {
    let qr: String?
    let expiresAt: Int64
    @Environment(\.tin) private var tin
    @State private var left: Int64 = 0

    var body: some View {
        let expired = qr != nil && left <= 0
        VStack(spacing: 16) {
            ZStack {
                if let qr, let img = ShellQrImage.make(qr) {
                    Image(uiImage: img).interpolation(.none).resizable().scaledToFit()
                        .clipShape(SmoothRoundedRectangle(cornerRadius: 12)).opacity(expired ? 0.18 : 1)
                        .accessibilityLabel("QR code to link this device")
                } else {
                    ShellSpinner(size: 28, color: Color(hex: 0x0B6B5B), stroke: 3)
                }
            }
            .frame(width: 208, height: 208).padding(12)
            .background(SmoothRoundedRectangle(cornerRadius: 20).fill(Color.white))
            .overlay(SmoothRoundedRectangle(cornerRadius: 20).stroke(tin.ln, lineWidth: 1))
            Text(qr == nil ? "Preparing your code\u{2026}" : expired ? "Expired. Make a new code."
                 : "Expires in \(left / 60000):\(String(format: "%02d", left / 1000 % 60)) \u{00B7} works once")
                .shellStyle(.sub).monospacedDigit().foregroundStyle(expired ? tin.er : tin.ink2)
        }
        .task(id: qr) {
            left = qr == nil ? 0 : max(0, expiresAt - ShellText.nowMs())
            while qr != nil && left > 0 {
                try? await Task.sleep(nanoseconds: 1_000_000_000)
                if Task.isCancelled { return }
                left = max(0, expiresAt - ShellText.nowMs())
            }
        }
    }
}

/// Six digits, 44/52 mono, letter-spaced .12em.
struct ShellBigCode: View {
    let code: String
    var weight: Font.Weight = .semibold
    @Environment(\.tin) private var tin
    var body: some View {
        Text(code).font(ShellFont.mono(44, weight == .bold ? .medium : weight)).fontWeight(weight).monospacedDigit().tracking(44 * 0.12)
            .foregroundStyle(tin.ink).frame(maxWidth: .infinity).padding(.vertical, 8)
            .accessibilityLabel("Link code \(code.map(String.init).joined(separator: " "))")
    }
}

/// "Waiting for approval..." with an 8 px accent dot pulsing to 25 % every 1.2 s (still when motion is reduced).
struct ShellWaitingLine: View {
    let text: String
    @Environment(\.tin) private var tin
    @Environment(\.accessibilityReduceMotion) private var reduced
    @State private var dim = false
    var body: some View {
        HStack(spacing: 10) {
            Circle().fill(ShellPalette(tin).accent).frame(width: 8, height: 8).opacity(dim && !reduced ? 0.25 : 1)
                .animation(reduced ? nil : .easeInOut(duration: 0.6).repeatForever(autoreverses: true), value: dim)
            Text(text).shellStyle(.sub).foregroundStyle(tin.ink2)
        }
        .onAppear { dim = true }
    }
}

// ------------------------------------------------------------------ existing device (devices/LinkDevice.svelte)

private struct ShellExistingDeviceLink: View {
    @ObservedObject var links: ShellLinkController
    let stage: ShellLinkStage
    let note: String?
    let onClose: () -> Void
    let onScan: () -> Void
    let onShow: () -> Void
    let onTyped: (String) -> Void
    let onClearNote: () -> Void
    let onNew: () -> Void
    let onCancel: () -> Void
    let onRetry: () -> Void
    // The codes are compared first, then the passphrase approves: two steps over the core's one Code state.
    @State private var matched = false
    @Environment(\.tin) private var tin
    @Environment(\.accessibilityReduceMotion) private var reduced

    private var key: String {
        switch links.ui {
        case .code: return matched ? "auth" : "verify"
        case .approving, .connecting: return "transfer"
        case .done: return "done"
        case .failed: return "failed"
        default: return stage == .show ? "show" : "scan"
        }
    }

    var body: some View {
        ShellPfPage(title: "Link a new device", onBack: { onCancel(); onClose() }) {
            VStack(alignment: .leading, spacing: 12) {
                switch key {
                case "scan": ShellLinkScanStep(note: note, onScan: onScan, onShow: onShow, onTyped: onTyped, onClearNote: onClearNote)
                case "show":
                    ShellPageH1(text: "Show this code to your new device")
                    Text("On the new device choose \u{201C}Link to a device\u{201D}, then \u{201C}Scan instead\u{201D}, and point it at this code.")
                        .shellStyle(.sub19).foregroundStyle(tin.ink2)
                    let s: (String, Int64)? = { if case .showing(let q, let e) = links.ui { return (q, e) }; return nil }()
                    ShellQrPanel(qr: s?.0, expiresAt: s?.1 ?? 0).frame(maxWidth: .infinity)
                    ShellActs { ShellPBtn(text: "Back", variant: .quiet, action: onCancel); ShellPBtn(text: "New code", action: onNew) }
                case "verify":
                    if case .code(let code, let peer) = links.ui {
                        let who = ShellText.ifBlank(peer, "The new device")
                        ShellPageH1(text: "Do the codes match?")
                        (Text(who).bold() + Text(" wants to join your account. It should be showing the same six digits as below."))
                            .shellStyle(.body).foregroundStyle(tin.ink)
                        ShellBigCode(code: code, weight: .bold)
                        Text("Compare them yourself. If they differ, someone else may have scanned the code first.").shellStyle(.sub19).foregroundStyle(tin.ink2)
                        ShellActs { ShellPBtn(text: "They don\u{2019}t match", action: onCancel); ShellPBtn(text: "They match", variant: .primary) { matched = true } }
                    }
                case "auth":
                    if case .code(_, let peer) = links.ui { ShellApproveStep(links: links, who: ShellText.ifBlank(peer, "the new device"), onCancel: onCancel) }
                case "transfer":
                    let connecting = links.ui == .connecting
                    let peer: String = { if case .approving(_, let p) = links.ui, !p.isEmpty { return p }; return "the new device" }()
                    ShellPageH1(text: connecting ? "Connecting\u{2026}" : "Linking \(peer)")
                    HStack(spacing: 12) {
                        ShellSpinner(size: 14, color: ShellPalette(tin).accent).frame(width: 24, height: 24)
                        Text(connecting ? "Reaching the other device" : "Sending your account").shellStyle(.body).foregroundStyle(tin.ink)
                            .frame(maxWidth: .infinity, alignment: .leading)
                        Text("Working").shellStyle(.sub).foregroundStyle(tin.ink2)
                    }
                    .frame(minHeight: 48)
                    Text("Keep both devices open until this finishes.").shellStyle(.sub19).foregroundStyle(tin.ink2)
                    if connecting { ShellActs { ShellPBtn(text: "Cancel", variant: .quiet, action: onCancel) } }
                case "done":
                    let peer: String = { if case .done(_, _, let p) = links.ui, !p.isEmpty { return p }; return "The new device" }()
                    ShellResultBlock(glyph: .check, ok: true, title: "\(peer) is linked",
                                     text: "It syncs when both devices are online at the same time. Contacts you haven\u{2019}t blocked get your updated device list when they\u{2019}re next online.")
                    ShellActs { ShellPBtn(text: "Done", variant: .primary, action: onClose) }
                default:
                    if case .failed(let reason, let detail) = links.ui {
                        ShellResultBlock(glyph: .info, ok: false, title: "Couldn\u{2019}t link", text: ShellText.linkFail(reason, detail))
                        ShellActs { ShellPBtn(text: "Close", variant: .quiet, action: onClose); ShellPBtn(text: "Try again", variant: .primary, action: onRetry) }
                    }
                }
            }
            .padding(.horizontal, 20).padding(.top, 20)
            .id(key).transition(.opacity.animation(reduced ? nil : TinMotion.easeOut(TinMotion.crossfade)))
        }
        .onChange(of: links.ui) { _, _ in matched = false }
    }
}

/// .pf-acts: buttons at the end, gap 8.
struct ShellActs<Content: View>: View {
    var top: CGFloat = 6
    @ViewBuilder var content: () -> Content
    var body: some View { HStack(spacing: 8) { Spacer(minLength: 0); content() }.padding(.top, top) }
}

struct ShellPageH1: View {
    let text: String
    var center = false
    @Environment(\.tin) private var tin
    var body: some View {
        Text(text).shellStyle(.h1Page).foregroundStyle(tin.ink).multilineTextAlignment(center ? .center : .leading)
            .frame(maxWidth: .infinity, alignment: center ? .center : .leading).accessibilityAddTraits(.isHeader)
    }
}

private struct ShellLinkScanStep: View {
    let note: String?
    let onScan: () -> Void
    let onShow: () -> Void
    let onTyped: (String) -> Void
    let onClearNote: () -> Void
    @State private var text = ""
    @Environment(\.tin) private var tin

    var body: some View {
        let p = ShellPalette(tin)
        ShellPageH1(text: "Scan the code on your new device")
        VStack(alignment: .leading, spacing: 4) {
            (Text("1.  On the new device, open Tinline and choose ") + Text("Link to a device").foregroundColor(tin.ink).fontWeight(.semibold) + Text("."))
                .shellStyle(.sub19).foregroundStyle(tin.ink2)
            Text("2.  It shows a QR code and a short code. They work for 5 minutes, once.").shellStyle(.sub19).foregroundStyle(tin.ink2)
        }
        .padding(.leading, 4)
        VStack(spacing: 14) {
            ShellIcon(glyph: .camera, size: 30, color: Color(hex: 0x9DB0AA))
            ShellPBtn(text: "Scan QR code", variant: .primary, action: onScan)
        }
        .frame(width: 220, height: 220).background(SmoothRoundedRectangle(cornerRadius: 20).fill(Color(hex: 0x0E1513)))
        .frame(maxWidth: .infinity)
        VStack(alignment: .leading, spacing: 6) {
            Text("Or type the code").shellStyle(.sub, weight: .semibold).foregroundStyle(tin.ink)
            HStack(spacing: 8) {
                ShellPassField(label: "", text: $text, secret: false, mono: true, placeholder: "OSVL1:\u{2026}", msgLines: 0,
                               onDone: { if !text.trimmingCharacters(in: .whitespaces).isEmpty { onTyped(text) } })
                    .onChange(of: text) { _, _ in onClearNote() }
                ShellPBtn(text: "Continue", enabled: !text.trimmingCharacters(in: .whitespaces).isEmpty) { onTyped(text) }
            }
            Text(note ?? "").shellStyle(.sub19).foregroundStyle(tin.er).frame(maxWidth: .infinity, minHeight: 38, alignment: .topLeading)
        }
        ShellPBtn(text: "Show a QR code here instead", variant: .quiet, colorOverride: p.accentInk, action: onShow)
    }
}

private struct ShellApproveStep: View {
    @EnvironmentObject private var app: AppEnv
    @ObservedObject var links: ShellLinkController
    let who: String
    let onCancel: () -> Void
    @Environment(\.tin) private var tin
    @State private var hasPass: Bool?
    @State private var pass = ""
    @State private var busy = false
    @State private var error = ""

    private func approve() {
        busy = true; error = ""
        Task {
            do { try await links.approve(passphrase: hasPass == true ? pass : nil); pass = "" }
            catch { self.error = ShellText.isWrongPassphrase(error) ? "That passphrase isn\u{2019}t right." : ShellText.friendly(error) }
            busy = false
        }
    }

    private func go() {
        if hasPass == true { approve(); return }
        let auth = ShellWiring.deviceAuth
        if !auth.available { approve(); return }
        Task { if await auth.ask(reason: "Link a new device") { approve() } else { error = "Screen lock check cancelled" } }
    }

    var body: some View {
        ShellPageH1(text: "Approve \(who)?")
        HStack(alignment: .top, spacing: 10) {
            ShellIcon(glyph: .warning, size: 18, color: tin.onWarn).padding(.top, 1)
            (Text("Approving sends your ") + Text("recovery phrase").bold() + Text(" to \(who). It becomes a full copy of you: it can read your chats and act as you. Approve only a device you hold."))
                .font(ShellFont.of(.body)).lineSpacing(2).foregroundStyle(tin.onWarn)
        }
        .padding(12).background(SmoothRoundedRectangle(cornerRadius: 12).fill(tin.warn))
        if hasPass == true {
            ShellPassField(label: "Your passphrase", text: $pass, error: error, enabled: !busy, autofocus: true,
                           onDone: { if !pass.isEmpty && !busy { go() } })
                .onChange(of: pass) { _, _ in error = "" }
        } else if !error.isEmpty {
            Text(error).shellStyle(.sub19).foregroundStyle(tin.er)
        }
        ShellActs {
            ShellPBtn(text: "Don\u{2019}t link", variant: .quiet, enabled: !busy, action: onCancel)
            ShellPBtn(text: busy ? "Approving\u{2026}" : "Approve", variant: .primary, enabled: !busy && hasPass != nil && (hasPass == false || !pass.isEmpty), action: go)
        }
        .task {
            guard let core = app.core else { return }
            hasPass = (try? await core.work { $0.hasPassphrase() }) ?? true
        }
    }
}

private struct ShellResultBlock: View {
    let glyph: ShellGlyph
    let ok: Bool
    let title: String
    let text: String
    @Environment(\.tin) private var tin
    var body: some View {
        let p = ShellPalette(tin)
        VStack(spacing: 10) {
            ShellIcon(glyph: glyph, size: ok ? 28 : 26, color: ok ? p.accentInk : tin.onWarn)
                .frame(width: 56, height: 56).background(Circle().fill(ok ? p.accentSoft : tin.warn))
            ShellPageH1(text: title, center: true)
            Text(text).shellStyle(.body).foregroundStyle(tin.ink2).multilineTextAlignment(.center).frame(maxWidth: 340)
        }
        .frame(maxWidth: .infinity).padding(.top, 24)
    }
}

// ------------------------------------------------------------------ camera

private struct ShellLinkScanScreen: View {
    let newDevice: Bool
    let note: String?
    let nonce: Int
    let onBack: () -> Void
    let onText: (String) -> Void
    let onRetry: () -> Void
    let onShow: () -> Void
    @State private var granted = true
    @State private var torch = false
    @Environment(\.tin) private var tin

    var body: some View {
        VStack(spacing: 0) {
            ShellTopBar(title: newDevice ? "Scan instead" : "Scan the code", onBack: onBack) {
                if granted { ShellIconButton(glyph: torch ? .torch : .torchoff, label: "Torch", size: 48, iconSize: 24) { torch.toggle() } }
            }
            ZStack {
                ShellWiring.qrScanner.view(prompt: newDevice ? "Point at the code on your other device" : "Point the camera at the QR code on your new device",
                                           torch: torch, onGranted: { granted = $0 }, onText: onText)
                    .id(nonce)
                if let note {
                    VStack(spacing: 12) {
                        ShellIcon(glyph: .warning, size: 32, color: Color(hex: 0xF2B8B5))
                        Text(note).shellStyle(.body).foregroundStyle(.white).multilineTextAlignment(.center)
                        ShellPBtn(text: "Scan again", variant: .primary, action: onRetry)
                    }
                    .padding(20).background(SmoothRoundedRectangle(cornerRadius: 20).fill(Color(.sRGB, red: 0x10 / 255, green: 0x16 / 255, blue: 0x14 / 255, opacity: 0.9)))
                    .padding(24)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity).padding(.top, 8)
            VStack(spacing: 8) {
                Text("The code works once and expires in 5 minutes.").shellStyle(.sub).foregroundStyle(tin.ink2).multilineTextAlignment(.center)
                    .frame(maxWidth: .infinity)
                if !newDevice { ShellGBtn(text: "Show a QR code here instead", kind: .text, action: onShow) }
            }
            .padding(.horizontal, 24).padding(.top, 16).padding(.bottom, 24)
        }
        .background(Color(hex: 0x050807).ignoresSafeArea())
        .environment(\.tin, .darkTheme).environment(\.colorScheme, .dark)
    }
}

// ------------------------------------------------------------------ new device: unlinked

/// "This phone was unlinked": another device removed the account; the core already deleted it here.
struct ShellUnlinkedScreen: View {
    let name: String
    let onOk: () -> Void
    let onLinkAgain: () -> Void
    var body: some View {
        ShellPage {
            ShellGateHead(onBack: nil, progress: nil)
            ShellGateStep("This phone was unlinked",
                          lead: "\(ShellText.ifBlank(name, "Your account")) was removed from this phone by one of your other devices. Its contacts, chats and calls are deleted here. Your other devices are not affected.",
                          center: true, anchor: true,
                          hero: { ShellIconDisc(glyph: .off, size: 72, iconSize: 32, danger: true).padding(.top, 12) },
                          content: { EmptyView() },
                          foot: {
                              ShellGBtn(text: "OK", action: onOk)
                              ShellGBtn(text: "Link it again", kind: .text, action: onLinkAgain)
                          })
        }
    }
}
