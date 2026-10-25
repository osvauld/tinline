import SwiftUI

/*
 * Sign-up, restore and link on a new install: the playground's gate (docs/design/playground/src/screens/gate/), phone form:
 * a full-screen stepped flow with the progress segments in the header and a 36 pt slide between steps. The account logic
 * (create / restore / link, persist, save-failed) is android Onboarding.kt's.
 *
 * Kept as the app had them (SCREENS-HANDOFF section 7, not decided in the playground): no "Remember on this phone" switch, no
 * camera row in Permissions, and separate end screens (Restored, Syncing) instead of one shared Done step. The Permissions step
 * lists only what iOS asks for (microphone, notifications): there is no battery-optimisation or full-screen switch to send
 * people to.
 */

/// First run: Welcome - Name - Passphrase (optional) - Recovery phrase - Name this phone - Terms - Permissions, or the restore
/// path (recovery words - name - passphrase - Name this phone - Restored - Terms - Permissions), or link (name this phone - QR -
/// passphrase - Syncing). `forgot` is the "forgot my passphrase" restore over a locked identity: words, new passphrase, done.
/// When `app.adding` is set the flow adds an account next to the existing ones: it starts at the name or the words, skips Terms
/// and Permissions, and backing out returns to the account that was left.
struct ShellOnboardingFlow: View {
    let core: ShellCore
    let missing: [ShellNeed]
    let onFix: (ShellNeed) -> Void
    var forgot = false
    var onCancelForgot: () -> Void = {}
    var onAddFirst: (() -> Void)? = nil
    let onDone: () -> Void

    @EnvironmentObject private var app: AppEnv
    @EnvironmentObject private var people: PeopleModel
    @Environment(\.tin) private var tin
    @Environment(\.accessibilityReduceMotion) private var reduced

    private let addState: ShellAdding?
    @State private var step: ShellStep
    @State private var dir = 1
    @State private var name: String
    @State private var restoring: Bool
    @State private var linking: Bool
    // Secrets live only in view state: never in saved state.
    @State private var words = ""
    @State private var pass = ""
    @State private var phrase: String?
    @State private var busy = false
    @State private var error: String?
    @State private var created = false
    @State private var existsDialog = false
    @State private var chooseAccount = false
    @State private var saveError = ""
    // Linking to an existing account: set once the account has arrived (the peer's name, for the Syncing screen).
    @State private var linkedPeer: String?
    @State private var linkedName = ""
    @State private var label = tr("onboarding_dev_personal")
    // "Copy my words" was pressed: the step keeps "I've saved them" (gate.phraseShown).
    @State private var phraseCopied = false

    init(core: ShellCore, missing: [ShellNeed], onFix: @escaping (ShellNeed) -> Void, forgot: Bool = false, onCancelForgot: @escaping () -> Void = {},
         onAddFirst: (() -> Void)? = nil, onDone: @escaping () -> Void) {
        self.core = core; self.missing = missing; self.onFix = onFix; self.forgot = forgot; self.onCancelForgot = onCancelForgot
        self.onAddFirst = onAddFirst; self.onDone = onDone
        let a = core.app.adding
        addState = a
        let first: ShellStep = forgot ? .restore : a.map { $0.link ? .linkName : $0.restore ? .restore : .name } ?? .welcome
        _step = State(initialValue: first)
        _name = State(initialValue: forgot ? core.node?.profile()?.name ?? "" : "")
        _restoring = State(initialValue: forgot || a?.restore == true)
        _linking = State(initialValue: a?.link == true)
    }

    private var adding: Bool { addState != nil }

    private func go(_ s: ShellStep) { dir = 1; withAnimation(reduced ? nil : .tinCubicOut(TinMotion.enter)) { step = s } }
    private func back(_ s: ShellStep) { dir = -1; withAnimation(reduced ? nil : .tinCubicOut(TinMotion.enter)) { step = s } }
    private func finish() { core.finishAdding(); onDone() }
    private func cancelAdd() {
        busy = true
        Task { await core.cancelAdding(); busy = false; onDone() }
    }
    private func afterTerms() { if adding { finish() } else { go(.terms) } }
    private func afterDevice() { if restoring && !forgot { go(.restored) } else { afterTerms() } }

    /// The identity exists in memory (or on disk): make it durable (S1), then go on.
    private func persistThenContinue() {
        busy = true; error = nil
        Task {
            let err = await core.persistIdentity()
            busy = false
            if let err { saveError = err; go(.saveFailed); return }
            await core.startServices()
            if forgot { onDone() }
            else if linkedPeer != nil { go(.syncing) }
            else if phrase != nil { go(.phrase) }
            else { go(.device) }
        }
    }

    /// An empty `pass` means no passphrase.
    private func finishIdentity(_ pass: String) {
        busy = true; error = nil
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines), w = words
        Task {
            do {
                let p: String?
                if forgot { try await core.restoreOverLocked(phrase: w, name: trimmed, passphrase: pass); p = nil }
                else if restoring { try await core.work { try $0.restoreIdentity(phrase: w, name: trimmed, passphrase: pass) }; p = nil }
                else { p = try await core.work { try $0.createIdentity(name: trimmed, passphrase: pass) } }
                busy = false
                created = true
                phrase = p
                persistThenContinue()
            } catch {
                busy = false
                self.error = ShellText.friendly(error)
                if ShellText.isAccountExists(error) { existsDialog = true; back(.restore) }
                // A bad phrase belongs on the words screen.
                else if ShellText.isBadPhrase(error) || (forgot && error is ShellFlowError) { back(.restore) }
            }
        }
    }

    private func setPassphrase(_ p: String) {
        busy = true; error = nil
        Task {
            do { try await core.work { try $0.setPassphrase(old: nil, new: p) }; busy = false; persistThenContinue() }
            catch { busy = false; self.error = ShellText.friendly(error) }
        }
    }

    var body: some View {
        let prog = ShellGate.progressOf(step, restoring: restoring, linking: linking, forgot: forgot, adding: adding)
        ZStack {
            stepView(step, prog: prog)
                .id(step)
                .transition(.shellGate(dir: dir, reduced: reduced))
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(tin.bg.ignoresSafeArea())
        .clipped()
        // The identity appeared some other way (a restored process): leave onboarding.
        .onChange(of: app.hasIdentity, initial: true) { _, has in
            if !forgot && !adding && has && !created && !busy && step != .linkQr { onDone() }
        }
        .onChange(of: step) { _, s in
            // Reading steps release focus as their entrance begins.
            if !s.hasTextInput { UIApplication.shared.sendAction(#selector(UIResponder.resignFirstResponder), to: nil, from: nil, for: nil) }
        }
        .overlay {
            if existsDialog {
                ShellDialog(title: tr("onboarding_exists_title"), confirm: tr("onboarding_exists_confirm"), dismiss: tr("onboarding_not_now"),
                            onDismiss: { existsDialog = false }, onConfirm: { existsDialog = false; chooseAccount = true }) {
                    ShellDialogText(tr("onboarding_exists_body"))
                }
            }
        }
        .sheet(isPresented: $chooseAccount) {
            ShellAccountSheet(core: core, atLogin: false, onDismiss: { chooseAccount = false }, onPick: { did in
                chooseAccount = false; busy = true
                Task {
                    do { try await core.switchTo(did, passphrase: nil); busy = false; finish() }
                    catch { busy = false; self.error = ShellText.friendly(error) }
                }
            })
        }
    }

    @ViewBuilder private func head(_ onBack: (() -> Void)?, _ prog: (Int, Int)?) -> some View {
        ShellGateHead(onBack: onBack, progress: prog, label: step.label)
    }

    @ViewBuilder private func stepView(_ s: ShellStep, prog: (Int, Int)?) -> some View {
        VStack(spacing: 0) {
            switch s {
            case .welcome:
                head(nil, prog)
                ShellWelcomeStep(onCreate: { restoring = false; linking = false; go(.name) }, onHave: { go(.choose) })
            case .choose:
                head({ back(.welcome) }, prog)
                ShellChooseStep(onLink: { restoring = false; linking = true; go(.linkName) },
                                onRestore: { restoring = true; linking = false; go(.restore) })
            case .done:
                head(nil, prog)
                ShellDoneStep(name: name, person: app.accountDid, onAdd: { (onAddFirst ?? onDone)() }, onNotNow: onDone)
            case .linkName:
                head({ if adding { cancelAdd() } else { back(.choose) } }, prog)
                ShellDeviceStep(label: $label, busy: false, error: nil, onNext: { label = label.trimmingCharacters(in: .whitespaces); go(.linkQr) })
            case .linkQr:
                ShellLinkSession(links: core.links, newDevice: true, label: label.trimmingCharacters(in: .whitespaces), progress: prog,
                                 onClose: { back(.linkName) },
                                 onDone: { n, peer in created = true; linkedPeer = ShellText.ifBlank(peer, tr("onboarding_fb_other_device")); linkedName = n; go(.linkPass) })
            case .linkPass:
                head(nil, prog)
                ShellPassStep(kind: .link, pass: $pass, busy: busy, error: error,
                              onNext: { let p = pass; pass = ""; setPassphrase(p) }, onSkip: { pass = ""; persistThenContinue() })
                    .onChange(of: pass) { _, _ in error = nil }
            case .syncing:
                head(nil, prog)
                ShellSyncingStep(name: people.node?.profile()?.name ?? linkedName, peer: linkedPeer ?? "", contacts: people.contacts.count,
                                 calls: app.history.count, onStart: afterTerms)
            case .restore:
                head({ if forgot { onCancelForgot() } else if adding { cancelAdd() } else { back(.choose) } }, prog)
                ShellRestoreStep(
                    error: $error,
                    warning: forgot ? tr("onboarding_restore_warn_forgot")
                        : tr("onboarding_restore_warn"),
                    onSubmit: { w in words = w; error = nil; go(forgot ? .pass : .name) })
            case .name:
                head({ if restoring { back(.restore) } else if adding { cancelAdd() } else { back(.welcome) } }, prog)
                ShellNameStep(name: $name, onNext: { go(.pass) })
            case .pass:
                head({ back(forgot ? .restore : .name) }, prog)
                ShellPassStep(kind: forgot ? .forgot : restoring ? .restore : .create, pass: $pass, busy: busy, error: error,
                              onNext: { let p = pass; pass = ""; finishIdentity(p) }, onSkip: { pass = ""; finishIdentity("") })
                    .onChange(of: pass) { _, _ in error = nil }
            case .phrase:
                head(nil, prog)
                ShellPhraseStep(phrase: phrase ?? "", copiedAlready: phraseCopied, person: app.accountDid,
                                onNext: { go(.device) }, onCopied: { phraseCopied = true })
            case .device:
                head(nil, prog)
                ShellDeviceStep(label: $label, busy: busy, error: error, onNext: {
                    busy = true; error = nil
                    let l = label.trimmingCharacters(in: .whitespaces)
                    Task {
                        do { try await core.work { try $0.setDeviceLabel(label: l) }; busy = false; afterDevice() }
                        catch { busy = false; self.error = ShellText.friendly(error) }
                    }
                })
            case .restored:
                head(nil, prog)
                ShellRestoredStep(name: people.node?.profile()?.name ?? name, adding: adding, onStart: afterTerms)
            case .terms:
                head(nil, prog)
                ShellTermsStep { core.prefs.acceptTerms(); go(.perms) }
            case .perms:
                head(nil, prog)
                // A new account ends on Done ("Your line is open"); restore and link already had their own end screen.
                ShellPermsStep(missing: missing, onFix: onFix, onDone: { if !restoring && !linking { go(.done) } else { onDone() } })
            case .saveFailed:
                head(nil, prog)
                ShellSaveFailedStep(message: saveError, busy: busy, onRetry: persistThenContinue,
                                    onPassphrase: { pass = ""; error = nil; go(.passAdd) })
            case .passAdd:
                head({ back(.saveFailed) }, prog)
                ShellPassStep(kind: .save, pass: $pass, busy: busy, error: error, onNext: { let p = pass; pass = ""; setPassphrase(p) }, onSkip: {})
                    .onChange(of: pass) { _, _ in error = nil }
            }
        }
    }
}

// ------------------------------------------------------------------ welcome (Welcome.svelte)

private struct ShellWelcomeStep: View {
    let onCreate: () -> Void
    let onHave: () -> Void
    @Environment(\.tin) private var tin
    var body: some View {
        // Laid out like every step: title and lead top-start, the mark (slack: nobody on the line yet) filling the free space, the
        // emergency note at the bottom, the two choices stacked full width with "Create my account" on top (thumb reach).
        ShellGateStep(tr("onboarding_welcome_title"),
                      lead: tr("onboarding_welcome_lead"), fill: true, content: {
            TinMark(size: 120, live: true, description: "Tinline", lineColor: true).frame(maxWidth: .infinity, minHeight: 120, maxHeight: .infinity)
            (Text(tr("onboarding_emergency")).bold() + Text("\n" + tr("onboarding_emergency_rest")))
                .shellStyle(.sub).foregroundStyle(tin.ink2)
        }, foot: {
            ShellGBtn(text: tr("onboarding_create"), action: onCreate)
            ShellGBtn(text: tr("onboarding_have"), kind: .secondary, action: onHave)
        })
    }
}

// ------------------------------------------------------------------ choose (Choose.svelte)

private struct ShellChooseStep: View {
    let onLink: () -> Void
    let onRestore: () -> Void
    var body: some View {
        ShellGateStep(tr("onboarding_choose_title"), lead: tr("onboarding_choose_lead"), fill: true, content: {
            // Touch: the choices sit at the bottom, in thumb reach, where Welcome's buttons were.
            Spacer(minLength: 0)
            VStack(spacing: 12) {
                ShellChooseOpt(glyph: .qr, title: tr("onboarding_opt_link"),
                               sub: tr("onboarding_opt_link_sub"), action: onLink)
                ShellChooseOpt(glyph: .key, title: tr("onboarding_opt_restore"),
                               sub: tr("onboarding_opt_restore_sub"), action: onRestore)
            }
            .padding(.bottom, 8)
        }, foot: { EmptyView() })
    }
}

private struct ShellChooseOpt: View {
    let glyph: ShellGlyph
    let title: String
    let sub: String
    let action: () -> Void
    @Environment(\.tin) private var tin
    var body: some View {
        Button(action: action) {
            HStack(spacing: 14) {
                ShellIconDisc(glyph: glyph, size: 44, iconSize: 24)
                VStack(alignment: .leading, spacing: 2) {
                    Text(title).shellStyle(.body, weight: .semibold).foregroundStyle(tin.ink)
                    Text(sub).shellStyle(.sub).foregroundStyle(tin.ink2).multilineTextAlignment(.leading)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                ShellIcon(glyph: .chevr, size: 20, color: tin.ink2).flipsForRightToLeftLayoutDirection(true)
            }
            .padding(16).frame(minHeight: 76)
            .background(SmoothRoundedRectangle(cornerRadius: 14).fill(tin.sf))
            .overlay(SmoothRoundedRectangle(cornerRadius: 14).stroke(tin.ln, lineWidth: 1))
        }
        .buttonStyle(ShellPressStyle(wash: .clear))
    }
}

// ------------------------------------------------------------------ done (Done.svelte)

private struct ShellDoneStep: View {
    let name: String
    let person: String?
    let onAdd: () -> Void
    let onNotNow: () -> Void
    @Environment(\.tin) private var tin
    // The line arrives slack and pulls tight 450 ms after the step arrives, in two tugs on the `line` cue's two notes; the cue plays
    // on the first tug, and the line warms from faded gold to gold. Reduced motion: it settles taut, colour and cue still come.
    @State private var taut = false

    var body: some View {
        let first = ShellGate.first(name)
        ShellGateStep(first.isEmpty ? tr("onboarding_done_title") : tr("onboarding_done_title_named", first),
                      lead: tr("onboarding_done_lead"), center: true, hero: {
            Group {
                if let person { ShellAvatar(did: person, name: name, size: 96) }
                else {
                    TinMark(size: 96, state: taut ? .taut : .slack, live: true, description: "Tinline", spring: .line, lineColor: true,
                            onTug: { n in if n == 1 { ShellWiring.cue(.line) } })
                }
            }
            .padding(.top, 28).padding(.bottom, 4)
        }, content: {
            // Two gentle "good to know" notes, said as tips at the happy moment, not as warnings.
            VStack(alignment: .leading, spacing: 12) {
                ShellDoneTip(glyph: .devices, title: tr("onboarding_tip_devices"), text: tr("onboarding_tip_devices_body"))
                ShellDoneTip(glyph: .laptop, title: tr("onboarding_tip_groups"), text: tr("onboarding_tip_groups_body"))
            }
            .frame(maxWidth: 420).padding(.top, 8)
        }, foot: {
            ShellGBtn(text: tr("onboarding_add_contact"), action: onAdd)
            ShellGBtn(text: tr("onboarding_not_now"), kind: .text, action: onNotNow)
        })
        .task {
            try? await Task.sleep(nanoseconds: 450_000_000)
            taut = true
            if person != nil { ShellWiring.cue(.line) }
        }
    }
}

private struct ShellDoneTip: View {
    let glyph: ShellGlyph
    let title: String
    let text: String
    @Environment(\.tin) private var tin
    var body: some View {
        HStack(alignment: .top, spacing: 14) {
            ShellIconDisc(glyph: glyph, size: 36, iconSize: 20)
            VStack(alignment: .leading, spacing: 0) {
                Text(title).shellStyle(.body, weight: .bold).foregroundStyle(tin.ink)
                Text(text).shellStyle(.sub19).foregroundStyle(tin.ink2)
            }
        }
    }
}

// ------------------------------------------------------------------ name (Name.svelte)

private struct ShellNameStep: View {
    @Binding var name: String
    let onNext: () -> Void
    var body: some View {
        let ok = !name.trimmingCharacters(in: .whitespaces).isEmpty
        let next = { if ok { onNext() } }
        ShellGateStep(tr("onboarding_name_title"), lead: tr("onboarding_name_lead"), content: {
            ShellGField(label: tr("onboarding_nickname"), text: $name, hint: tr("onboarding_nickname_hint"), capitalization: .words,
                        autofocus: true, onDone: next)
        }, foot: { ShellGNextRow(next: ShellGNext(label: tr("onboarding_continue"), enabled: ok, action: next)) })
    }
}

// ------------------------------------------------------------------ passphrase (Pass.svelte)

enum ShellPassKind { case create, restore, forgot, link, save }

struct ShellPassStep: View {
    let kind: ShellPassKind
    @Binding var pass: String
    let busy: Bool
    let error: String?
    let onNext: () -> Void
    let onSkip: () -> Void

    var body: some View {
        let title: String = { switch kind { case .forgot: tr("onboarding_pass_title_forgot"); case .link: tr("onboarding_pass_title_link"); default: tr("onboarding_pass_title") } }()
        let cta: String = { switch kind { case .forgot, .restore: tr("onboarding_cta_restore"); case .link: tr("onboarding_cta_add"); case .save: tr("onboarding_cta_save"); case .create: tr("onboarding_continue") } }()
        let submit = { if !pass.isEmpty && !busy { onNext() } }
        // Says what the lock gives you (tone pass 2026-10-13); the way back comes before anything that sounds final.
        ShellGateStep(title, lead: tr("onboarding_pass_lead"), content: {
            VStack(alignment: .leading, spacing: 6) {
                ShellGField(label: tr("onboarding_pass"), text: $pass, secret: true, enabled: !busy, autofocus: true, onDone: submit)
                ShellStrengthMeter(pass: pass)
            }
            ShellNotice(tr("onboarding_pass_notice"), glyph: .lock)
            if let error { ShellNotice(error, kind: .error) }
        }, foot: {
            if kind == .create {
                ShellGNextRow(next: ShellGNext(label: busy ? tr("onboarding_encrypting") : cta, enabled: !pass.isEmpty, busy: busy, action: submit)) {
                    ShellGBtn(text: tr("onboarding_skip"), kind: .text, enabled: !busy, fill: false, action: onSkip)
                }
            } else {
                ShellGBtn(text: busy ? tr("onboarding_encrypting") : cta, enabled: !pass.isEmpty, busy: busy, action: submit)
                if kind != .save { ShellGBtn(text: tr("onboarding_skip"), kind: .text, enabled: !busy, fill: false, action: onSkip) }
            }
        })
    }
}

/// Pass.svelte: three 28 x 6 bars and one line; every line shares one cell (the tallest sets the height), so nothing below moves.
private struct ShellStrengthMeter: View {
    let pass: String
    @Environment(\.tin) private var tin
    var body: some View {
        let p = ShellPalette(tin)
        let s = ShellGate.strength(pass)
        let col: Color = s == 1 ? p.danger : s == 2 ? tin.thread : p.accent
        HStack(alignment: .top, spacing: 10) {
            HStack(spacing: 4) {
                ForEach(1...3, id: \.self) { i in RoundedRectangle(cornerRadius: 3).fill(s >= i ? col : tin.ln).frame(width: 28, height: 6) }
            }
            .padding(.top, 6)
            ZStack(alignment: .topLeading) {
                ForEach(0..<ShellGate.strengthSays.count, id: \.self) { i in
                    Text(ShellGate.strengthSays[i]).shellStyle(.sub).foregroundStyle(tin.ink2).opacity(i == s ? 1 : 0).accessibilityHidden(i != s)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .accessibilityElement(children: .combine)
    }
}

// ------------------------------------------------------------------ recovery phrase (Phrase.svelte)

/// Two columns, column-major (1-12 left, 13-24 right): cells 32 tall, gap 4, r 8, line border, number 20 wide, mono 15/20.
struct ShellWordGrid: View {
    let phrase: String
    var hidden = false
    @Environment(\.tin) private var tin
    var body: some View {
        let words = phrase.split(whereSeparator: { $0.isWhitespace }).map(String.init)
        let half = (words.count + 1) / 2
        VStack(spacing: 4) {
            ForEach(0..<half, id: \.self) { r in
                HStack(spacing: 10) {
                    ForEach(0..<2, id: \.self) { col in
                        let i = r + col * half
                        if i < words.count {
                            HStack(spacing: 10) {
                                Text("\(i + 1)").font(ShellFont.figtree(11)).monospacedDigit().foregroundStyle(tin.ink2).frame(width: 20, alignment: .trailing)
                                Text(hidden ? "\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}" : words[i]).font(ShellFont.mono(15))
                                    .foregroundStyle(hidden ? tin.ink2 : tin.ink).lineLimit(1)
                                Spacer(minLength: 0)
                            }
                            .padding(.horizontal, 12).frame(maxWidth: .infinity, minHeight: 32)
                            .background(SmoothRoundedRectangle(cornerRadius: 8).fill(tin.sf))
                            .overlay(SmoothRoundedRectangle(cornerRadius: 8).stroke(tin.ln, lineWidth: 1))
                        } else {
                            Spacer().frame(maxWidth: .infinity)
                        }
                    }
                }
            }
        }
        .padding(4)
        .privacySensitive()
    }
}

/// Words | Key: the same secret two ways (recovery step, Settings). Hugs its two buttons.
struct ShellWordsKeySeg: View {
    let asKey: Bool
    let onChange: (Bool) -> Void
    @Environment(\.tin) private var tin
    var body: some View {
        HStack(spacing: 4) {
            ForEach([false, true], id: \.self) { k in
                let on = k == asKey
                Button { onChange(k) } label: {
                    Text(k ? tr("onboarding_seg_key") : tr("onboarding_seg_words")).font(TinType.bodyM.weight(on ? .semibold : .regular)).foregroundStyle(tin.ink)
                        .padding(.horizontal, 14).frame(minHeight: 40)
                        .background(SmoothRoundedRectangle(cornerRadius: 9).fill(on ? tin.sf : .clear))
                }
                .buttonStyle(.plain).accessibilityAddTraits(on ? [.isSelected, .isButton] : .isButton)
            }
        }
        .padding(3).background(SmoothRoundedRectangle(cornerRadius: 12).fill(tin.sf3))
    }
}

/// The recovery key: monospace, broken anywhere (it has no spaces), the same card look as the words.
struct ShellKeyCard: View {
    let key: String
    @Environment(\.tin) private var tin
    var body: some View {
        Text(key).font(ShellFont.mono(15)).lineSpacing(6).foregroundStyle(tin.ink).frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 16).padding(.vertical, 14)
            .background(SmoothRoundedRectangle(cornerRadius: 10).fill(tin.sf))
            .overlay(SmoothRoundedRectangle(cornerRadius: 10).stroke(tin.ln, lineWidth: 1))
            .privacySensitive()
    }
}

private struct ShellPhraseStep: View {
    let phrase: String
    var copiedAlready = false
    let person: String?
    let onNext: () -> Void
    let onCopied: () -> Void
    // One primary slot, two moments: "Copy my words" first; once copied the same button says "I've saved them" (same size, so
    // nothing moves). Calm, never alarming: no blur, no warning box; paper is a suggestion, not a rule.
    @State private var copied = false
    // Words or key: the same secret two ways (core recovery_key.rs). Words suit paper, the key a password manager. Either copy
    // counts as saved. There is no quiz: the key's checksum catches a bad copy when it is pasted, and restore checks the words'
    // own checksum.
    @State private var asKey = false

    var body: some View {
        let key = (try? recoveryKeyFromPhrase(phrase: phrase)) ?? ""
        let copy = { if asKey { ShellClipboard.copySensitive(key) } else { ShellClipboard.copySensitive(phrase) } }
        ShellGateStep(asKey ? tr("onboarding_rec_title_key") : tr("onboarding_rec_title_words"),
                      lead: asKey ? tr("onboarding_rec_lead_key")
                        : tr("onboarding_rec_lead_words"),
                      content: {
            if let person { ShellAvatar(did: person, name: tr("onboarding_fb_your_account"), size: 80).frame(maxWidth: .infinity) }
            ShellWordsKeySeg(asKey: asKey) { asKey = $0 }
            if asKey { ShellKeyCard(key: key) } else { ShellWordGrid(phrase: phrase) }
        }, foot: {
            if copied || copiedAlready {
                ShellGBtn(text: asKey ? tr("onboarding_saved_key") : tr("onboarding_saved_words"), action: onNext)
                // Keeps the footer two buttons tall in both moments, so nothing moves.
                ShellGBtn(text: tr("onboarding_copy_again"), kind: .text, action: copy)
            } else {
                ShellGBtn(text: asKey ? tr("onboarding_copy_key") : tr("onboarding_copy_words")) { copy(); copied = true; onCopied() }
                ShellGBtn(text: asKey ? tr("onboarding_elsewhere_key") : tr("onboarding_elsewhere_words"), kind: .text, action: onNext)
            }
        })
    }
}

// ------------------------------------------------------------------ device name (Device.svelte)

private struct ShellDeviceStep: View {
    @Binding var label: String
    let busy: Bool
    let error: String?
    let onNext: () -> Void
    var body: some View {
        let ok = !label.trimmingCharacters(in: .whitespaces).isEmpty
        let next = { if ok && !busy { onNext() } }
        ShellGateStep(tr("onboarding_dev_title"), lead: tr("onboarding_dev_lead"), content: {
            ShellGField(label: tr("onboarding_dev_field"), text: $label, hint: tr("onboarding_dev_hint"), enabled: !busy, capitalization: .words, autofocus: true, onDone: next)
            HStack(spacing: 8) {
                ForEach([tr("onboarding_dev_personal"), tr("onboarding_dev_work"), tr("onboarding_dev_tablet")], id: \.self) { s in
                    ShellChip(text: s, pressed: label.trimmingCharacters(in: .whitespaces) == s, minHeight: 44, padH: 16) { label = s }
                }
            }
            if let error { ShellNotice(error, kind: .error) }
        }, foot: { ShellGNextRow(next: ShellGNext(label: tr("onboarding_continue"), enabled: ok, busy: busy, action: next)) })
    }
}

// ------------------------------------------------------------------ restored / linked (the app's own end screens, Done.svelte look)

private struct ShellRestoredStep: View {
    let name: String
    let adding: Bool
    let onStart: () -> Void
    var body: some View {
        let first = ShellGate.first(name), whole = name.trimmingCharacters(in: .whitespaces)
        ShellGateStep(adding ? tr("onboarding_back_added", whole.isEmpty ? tr("onboarding_fb_your_account_lc") : whole) : first.isEmpty ? tr("onboarding_back_title") : tr("onboarding_back_title_named", first),
                      lead: adding ? tr("onboarding_back_added_lead", whole.isEmpty ? tr("onboarding_fb_this_account") : whole)
                        : tr("onboarding_back_lead"),
                      center: true, bigTitle: true,
                      hero: { TinMark(size: 96, state: .taut, live: true, description: "Tinline").padding(.top, 28).padding(.bottom, 4) },
                      content: { EmptyView() },
                      foot: { ShellGBtn(text: adding ? tr("onboarding_open") : tr("onboarding_continue"), action: onStart) })
    }
}

private struct ShellSyncingStep: View {
    let name: String
    let peer: String
    let contacts: Int
    let calls: Int
    let onStart: () -> Void
    @State private var slow = false
    var body: some View {
        let from = ShellText.ifBlank(peer, tr("onboarding_fb_other_device"))
        ShellGateStep(tr("onboarding_linked_title", from), lead: tr("onboarding_linked_lead", from), center: true, bigTitle: true,
                      hero: { TinMark(size: 96, state: .taut, live: true, description: "Tinline").padding(.top, 28).padding(.bottom, 4) },
                      content: {
            ShellPfCard {
                ShellSyncRow(glyph: .users, title: tr("onboarding_sync_contacts"), value: contacts > 0 ? "\(contacts)" : tr("onboarding_sync_waiting"), done: contacts > 0)
                ShellPfHairline()
                ShellSyncRow(glyph: .clock, title: tr("onboarding_sync_calls"), value: calls > 0 ? "\(calls)" : tr("onboarding_sync_waiting"), done: calls > 0)
            }
            ShellReservedLine(text: slow && contacts == 0 && calls == 0
                              ? tr("onboarding_sync_slow")
                              : tr("onboarding_sync_keep"), alignment: .center, lines: 2)
        }, foot: { ShellGBtn(text: tr("onboarding_open"), action: onStart) })
        .task { try? await Task.sleep(nanoseconds: 45_000_000_000); slow = true }
    }
}

private struct ShellSyncRow: View {
    let glyph: ShellGlyph
    let title: String
    let value: String
    let done: Bool
    @Environment(\.tin) private var tin
    var body: some View {
        let p = ShellPalette(tin)
        ShellPfAct(title, glyph: glyph, minHeight: 56, action: nil) {
            HStack(spacing: 4) {
                if done { ShellIcon(glyph: .check, size: 18, color: p.accentInk) }
                Text(value).shellStyle(.sub, weight: .semibold).monospacedDigit().foregroundStyle(done ? p.accentInk : tin.ink2)
                    .frame(minWidth: 56, alignment: .trailing)
            }
        }
    }
}

// ------------------------------------------------------------------ save failed (Failed.svelte, savefailed)

private struct ShellSaveFailedStep: View {
    let message: String
    let busy: Bool
    let onRetry: () -> Void
    let onPassphrase: () -> Void
    var body: some View {
        ShellGateStep(tr("onboarding_nosave_title"), lead: tr("onboarding_nosave_lead"),
                      center: true, anchor: true,
                      hero: { ShellIconDisc(glyph: .warning, size: 72, iconSize: 32, danger: true).padding(.top, 12) },
                      content: {
            ShellNotice(tr("onboarding_nosave_notice"))
            if !message.trimmingCharacters(in: .whitespaces).isEmpty { ShellNotice(message, kind: .error) }
        }, foot: {
            ShellGBtn(text: busy ? tr("onboarding_trying") : tr("onboarding_try_again"), busy: busy, action: onRetry)
            ShellGBtn(text: tr("onboarding_pass_instead"), kind: .text, enabled: !busy, action: onPassphrase)
        })
    }
}

// ------------------------------------------------------------------ terms (Terms.svelte)

private struct ShellTermsStep: View {
    let onAgree: () -> Void
    @Environment(\.tin) private var tin
    var body: some View {
        let p = ShellPalette(tin)
        // No checkbox: the line right above the button is the consent, and the button ("I agree", the arrow) is the agreement.
        ShellGateStep(tr("onboarding_terms_title"), fill: true, content: {
            VStack(alignment: .leading, spacing: 14) {
                ShellTermPoint(glyph: .shield, bold: tr("onboarding_terms1"), rest: tr("onboarding_terms1_rest"))
                ShellTermPoint(glyph: .lock, bold: tr("onboarding_terms2"), rest: tr("onboarding_terms2_rest"))
                ShellTermPoint(glyph: .key, bold: tr("onboarding_terms3"), rest: tr("onboarding_terms3_rest"))
                ShellTermPoint(glyph: .warning, bold: tr("onboarding_emergency"), rest: "")
            }
            Spacer(minLength: 0)
            Text(LocalizedStringKey(tr("onboarding_terms_consent", "\(ShellURL.terms)", "\(ShellURL.privacy)")))
                .shellStyle(.sub19).foregroundStyle(tin.ink2).tint(p.accentInk)
        }, foot: { ShellGNextRow(next: ShellGNext(label: tr("onboarding_agree"), action: onAgree)) })
    }
}

private struct ShellTermPoint: View {
    let glyph: ShellGlyph
    let bold: String
    let rest: String
    @Environment(\.tin) private var tin
    var body: some View {
        HStack(alignment: .top, spacing: 14) {
            ShellIconDisc(glyph: glyph, size: 36, iconSize: 20)
            (Text(bold).foregroundColor(tin.ink).bold() + Text(rest.isEmpty ? "" : "\n" + rest)).shellStyle(.body).foregroundStyle(tin.ink2)
        }
    }
}

/// Terms on their own (an existing account that hasn't accepted the current terms).
struct ShellTermsScreen: View {
    let onAgree: () -> Void
    var body: some View {
        ShellPage { ShellGateHead(onBack: nil, progress: nil); ShellTermsStep(onAgree: onAgree) }
    }
}

// ------------------------------------------------------------------ permissions (Perms.svelte)

private struct ShellPermsStep: View {
    let missing: [ShellNeed]
    let onFix: (ShellNeed) -> Void
    let onDone: () -> Void
    var body: some View {
        let rows: [(ShellNeed, ShellGlyph, String, String)] = [
            (.mic, .mic, tr("onboarding_perm_mic"), tr("onboarding_perm_mic_why")),
            (.notifications, .bell, tr("onboarding_perm_notif"), tr("onboarding_perm_notif_why")),
        ]
        let touched = rows.contains { !missing.contains($0.0) }
        ShellGateStep(tr("onboarding_perms_title"),
                      lead: tr("onboarding_perms_lead"),
                      content: {
            VStack(spacing: 4) {
                ForEach(rows, id: \.2) { r in ShellPermRow(glyph: r.1, title: r.2, why: r.3, granted: !missing.contains(r.0)) { onFix(r.0) } }
            }
        }, foot: { ShellGBtn(text: touched ? tr("onboarding_continue") : tr("onboarding_not_now"), kind: touched ? .primary : .secondary, action: onDone) })
    }
}

private struct ShellPermRow: View {
    let glyph: ShellGlyph
    let title: String
    let why: String
    let granted: Bool
    let onAllow: () -> Void
    @Environment(\.tin) private var tin
    var body: some View {
        let p = ShellPalette(tin)
        HStack(spacing: 14) {
            ShellIconDisc(glyph: glyph, size: 40, iconSize: 20)
            VStack(alignment: .leading, spacing: 1) {
                Text(title).shellStyle(.body, weight: .bold).foregroundStyle(tin.ink)
                Text(why).shellStyle(.sub).foregroundStyle(tin.ink2)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            ZStack {
                if granted { TinIcon(TinIcons.check, size: 22, tint: p.accentInk, label: tr("onboarding_perm_allowed", title)) }
                else {
                    ShellPBtn(text: tr("onboarding_allow"), variant: .outline, padH: 14, colorOverride: p.accentInk, minWidth: 72, action: onAllow)
                        .accessibilityLabel(tr("onboarding_allow_a11y", title.lowercased()))
                }
            }
            .frame(minWidth: 72)
        }
        .padding(.vertical, 8)
    }
}
