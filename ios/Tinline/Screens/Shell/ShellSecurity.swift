import SwiftUI
import UIKit

/*
 * The recovery backup and the passphrase (android Settings.kt): the gate in front of the root secret, the reveal screen and
 * add / change passphrase. Profile and Privacy both use this gate; no credential or phrase enters synchronized profile data.
 */

/// Check it's you, then open the backup: the passphrase when there is one, else the phone's own screen lock.
struct ShellPhraseGateScreen: View {
    let core: ShellCore
    let onBack: () -> Void
    let onPhrase: (String) -> Void
    @Environment(\.tin) private var tin
    @Environment(\.scenePhase) private var phase
    @State private var hasPass: Bool?
    @State private var pass = ""
    @State private var busy = false
    @State private var prompting = false
    @State private var error: String?
    @State private var generation = 0
    @State private var alive = true

    private func reveal() {
        guard let ticket = core.profiles.session.ticket() else { return }
        generation += 1
        let attempt = generation, password = pass
        busy = true; error = nil
        Task {
            let result: Result<String, Swift.Error>
            do { result = .success(try await core.profileWork(ticket) { try $0.recoveryPhrase(passphrase: password) }) }
            catch { result = .failure(error) }
            guard alive, attempt == generation, core.profiles.session.accepts(ticket) else { return }
            busy = false; pass = ""
            guard phase == .active else { return }
            switch result {
            case .success(let p): onPhrase(p)
            case .failure(let e): error = ShellText.isWrongPassphrase(e) ? tr("security_wrong_pass") : tr("security_open_failed")
            }
        }
    }

    private func go() {
        if busy || hasPass == nil { return }
        error = nil
        if hasPass == true { if !pass.isEmpty { reveal() }; return }
        // Recovery must not treat "no prompt" as authentication.
        let auth = ShellWiring.deviceAuth
        guard auth.available else { error = tr("security_need_lock"); return }
        guard let ticket = core.profiles.session.ticket() else { return }
        busy = true; prompting = true
        Task {
            let ok = await auth.ask(reason: tr("security_recovery_title"))
            guard alive else { return }
            prompting = false; busy = false
            if ok && core.profiles.session.accepts(ticket) && phase == .active { reveal() }
        }
    }

    var body: some View {
        ShellPage {
            ShellTopBar(tr("security_recovery_title"), onBack: onBack)
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    ShellHint(tr("security_only_you"))
                    Text(tr("security_check_you")).font(TinType.bodyL).foregroundStyle(tin.ink2)
                    if hasPass == true {
                        ShellPassField(label: tr("security_your_pass"), text: $pass, enabled: !busy, msgLines: 0, onDone: go)
                            .onChange(of: pass) { _, _ in error = nil }
                    } else if hasPass == false {
                        ShellHint(tr("security_use_lock_hint"))
                    }
                    Text(error ?? "").font(TinType.bodyM).foregroundStyle(tin.er).accessibilityAddTraits(.updatesFrequently)
                }
                .padding(24).frame(maxWidth: .infinity, alignment: .leading)
            }
            ShellTinButton(text: hasPass == false ? tr("security_use_lock") : tr("security_continue"),
                           enabled: hasPass != nil && (hasPass == false || !pass.isEmpty) && !busy, action: go)
                .padding(.horizontal, 24).padding(.bottom, 24)
        }
        .task {
            guard let ticket = core.profiles.session.ticket() else { return }
            let result = try? await core.profileWork(ticket) { $0.hasPassphrase() }
            if core.profiles.session.accepts(ticket) { hasPass = result }
            if result == nil { error = tr("security_recovery_failed") }
        }
        // Leaving the app forgets what was typed and any answer still on its way.
        .onChange(of: phase) { _, p in if p != .active { pass = ""; generation += 1; if !prompting { busy = false } } }
        .onDisappear { alive = false; generation += 1; pass = "" }
    }
}

/// The backup itself: key or 24 words, hidden until Reveal, hidden again after 30 seconds (monotonic clock), and the gate
/// comes back when the app leaves the front.
struct ShellPhraseShownScreen: View {
    let phrase: String
    let onHide: () -> Void
    var onRelock: () -> Void
    @Environment(\.tin) private var tin
    @Environment(\.scenePhase) private var phase
    @State private var state = ShellRecoveryState()
    @State private var copied = false
    @State private var error = ""
    @State private var now = ShellClock.monoMs()

    init(phrase: String, onHide: @escaping () -> Void, onRelock: (() -> Void)? = nil) {
        self.phrase = phrase; self.onHide = onHide; self.onRelock = onRelock ?? onHide
    }

    var body: some View {
        let key = (try? recoveryKeyFromPhrase(phrase: phrase)) ?? ""
        let shown = state.visible(now)
        ShellPage {
            ShellTopBar(tr("security_backup_title"), onBack: onHide)
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    ShellHint(tr("security_only_you"))
                    Text(tr("security_keep_private")).font(TinType.bodyL).foregroundStyle(tin.ink2)
                    HStack(spacing: 8) {
                        ForEach([(true, tr("security_tab_key")), (false, trn("security_tab_words", 24))], id: \.1) { asKey, label in
                            let on = state.asKey == asKey
                            Button { state = state.select(asKey); copied = false; error = "" } label: {
                                Text(label).font(TinType.label).foregroundStyle(on ? tin.onPrc : tin.ink).padding(12)
                                    .frame(maxWidth: .infinity, minHeight: 44).background(RoundedRectangle(cornerRadius: 12).fill(on ? tin.prc : tin.sf))
                            }
                            .buttonStyle(.plain).accessibilityAddTraits(on ? [.isSelected, .isButton] : .isButton)
                        }
                    }
                    if state.asKey { ShellKeyCard(key: shown ? key : "\u{2022}\u{2022}\u{2022}\u{2022} \u{2022}\u{2022}\u{2022}\u{2022} \u{2022}\u{2022}\u{2022}\u{2022} \u{2022}\u{2022}\u{2022}\u{2022}") }
                    else { ShellWordGrid(phrase: phrase, hidden: !shown) }
                    if state.asKey && key.isEmpty { ShellHint(tr("security_key_failed", "24")) }
                    ShellTinButton(text: shown ? tr("security_hide") : tr("security_reveal"), style: .outlined, enabled: !state.asKey || !key.isEmpty) {
                        state = shown ? state.hide() : state.reveal(ShellClock.monoMs()); now = ShellClock.monoMs(); copied = false; error = ""
                    }
                    ShellHint(tr("security_hint_hides"))
                    ShellHint(tr("security_hint_same"))
                }
                .padding(.horizontal, 24).padding(.vertical, 16)
            }
            VStack(spacing: 8) {
                Text(error.isEmpty ? (copied && shown ? tr("security_copied_note") : "") : error)
                    .font(TinType.bodyM).foregroundStyle(tin.ink2).frame(maxWidth: .infinity, minHeight: 20, alignment: .leading)
                if state.asKey {
                    ShellTinButton(text: copied ? tr("security_copied") : tr("security_copy_key"), enabled: shown && !key.isEmpty) {
                        // Check the monotonic deadline at the gesture, not just at the last render.
                        if state.canCopyKey(ShellClock.monoMs()) && !key.isEmpty && phase == .active {
                            ShellClipboard.copySensitive(key); copied = true; error = ""
                        }
                    }
                } else {
                    ShellTinButton(text: tr("security_done"), action: onHide)
                }
            }
            .padding(.horizontal, 24).padding(.bottom, 24)
        }
        .privacySensitive()
        .task(id: state.revealUntil) {
            guard let deadline = state.revealUntil else { return }
            try? await Task.sleep(nanoseconds: UInt64(max(0, deadline - ShellClock.monoMs())) * 1_000_000)
            if Task.isCancelled { return }
            state = state.hide(); copied = false; now = ShellClock.monoMs()
        }
        // The app left the front (app switcher, another app): relock, so the words leave memory.
        .onChange(of: phase) { _, p in if p != .active { onRelock() } }
    }
}

/// Adds a passphrase when none is set (the data key is rewrapped, nothing re-encrypted), else changes it.
struct ShellChangePassphraseScreen: View {
    let core: ShellCore
    let onBack: () -> Void
    @Environment(\.tin) private var tin
    @State private var has: Bool?
    @State private var old = ""
    @State private var pass = ""
    @State private var busy = false
    @State private var error: String?

    var body: some View {
        let has = self.has ?? false
        let title = has ? tr("security_change_pass") : tr("security_add_pass")
        ShellPage {
            ShellTopBar(title, onBack: onBack)
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    if has {
                        ShellPassField(label: tr("security_current_pass"), text: $old, error: error ?? "", enabled: !busy, mono: true)
                            .onChange(of: old) { _, _ in error = nil }
                    } else {
                        Text(tr("security_pass_intro"))
                            .font(TinType.bodyL).foregroundStyle(tin.ink2)
                    }
                    ShellGField(label: has ? tr("security_new_pass") : tr("security_pass"), text: $pass, secret: true, enabled: !busy)
                        .onChange(of: pass) { _, _ in error = nil }
                    if !has {
                        ShellNotice(tr("security_pass_notice"), glyph: .lock)
                        if let error { Text(error).font(TinType.bodyM).foregroundStyle(tin.er) }
                    }
                    if busy { ProgressView().progressViewStyle(.linear).tint(tin.pr) }
                }
                .padding(.horizontal, 24).padding(.vertical, 8)
            }
            ShellTinButton(text: busy ? tr("security_encrypting") : title, enabled: self.has != nil && (!has || !old.isEmpty) && !pass.isEmpty && !busy) {
                busy = true; error = nil
                let o = has ? old : nil, n = pass
                Task {
                    do {
                        try await core.work { try $0.setPassphrase(old: o, new: n) }
                        _ = await core.persistIdentity()
                        old = ""; pass = ""; busy = false; onBack()
                    } catch { busy = false; self.error = ShellText.friendly(error) }
                }
            }
            .padding(.horizontal, 24).padding(.bottom, 24)
        }
        .task { self.has = (try? await core.work { $0.hasPassphrase() }) ?? false }
    }
}
