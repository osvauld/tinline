import SwiftUI

/*
 * Unlock (android Unlock.kt) and the accounts on this phone (Accounts.kt): the switcher sheet and "Switch to X?".
 * The five-try / 30-second lockout is a core gap, not a UI-only security policy: the message area is reserved for it.
 * One account is online on this phone at a time (the core has one selected account: `switch_account`); the others are paused
 * and ring only on their other devices.
 */

/// Shown while the identity is Locked and no remembered key could unlock it.
struct ShellUnlockScreen: View {
    let core: ShellCore
    let onForgot: () -> Void
    @State private var pass = ""
    @State private var busy = false
    @State private var error: String?
    @State private var name = ""
    @State private var many = false
    @State private var hasPass: Bool?
    @State private var sheet = false

    var body: some View {
        Group {
            if hasPass == false {
                // No passphrase and the Keychain key is gone: the 24 words are the only way back.
                ShellKeyLostContent(name: name, busy: busy, onRetry: {
                    busy = true
                    Task { _ = await core.tryAutoUnlock(); busy = false; core.identityReady() }
                }, onRestore: onForgot, onSwitch: many ? ({ sheet = true } as () -> Void) : nil)
            } else if hasPass == true {
                ShellUnlockContent(name: name, pass: $pass, busy: busy, error: error, onForgot: onForgot, onUnlock: unlock,
                                   onSwitch: many ? ({ sheet = true } as () -> Void) : nil)
                    .onChange(of: pass) { _, _ in error = nil }
            } else {
                ShellPage { EmptyView() }
            }
        }
        .task {
            guard let node = core.node else { return }
            let (n, m, h) = (try? await shellIO { (node.profile()?.name ?? "", node.accounts().count > 1, node.hasPassphrase()) }) ?? ("", false, true)
            name = n; many = m; hasPass = h
        }
        .sheet(isPresented: $sheet) {
            ShellAccountSheet(core: core, atLogin: true, onDismiss: { sheet = false }, onPick: { did in
                sheet = false; busy = true
                Task {
                    do { try await core.switchTo(did, passphrase: nil) } catch { self.error = ShellText.friendly(error) }
                    busy = false
                }
            })
        }
    }

    private func unlock() {
        busy = true; error = nil
        let p = pass
        Task {
            do {
                try await core.work { try $0.unlock(passphrase: p) }
                pass = ""
                busy = false
                core.identityReady()
            } catch {
                busy = false
                self.error = ShellText.isWrongPassphrase(error) ? "That passphrase didn\u{2019}t open the account." : ShellText.friendly(error)
            }
        }
    }
}

/// Failed.svelte (keylost): locked with no passphrase, this phone's own key is gone, so only the recovery phrase brings the
/// account back.
struct ShellKeyLostContent: View {
    let name: String
    let busy: Bool
    let onRetry: () -> Void
    let onRestore: () -> Void
    var onSwitch: (() -> Void)?
    var body: some View {
        let n = name.trimmingCharacters(in: .whitespaces)
        ShellPage {
            ShellGateHead(onBack: nil, progress: nil)
            ShellGateStep(n.isEmpty ? "Can\u{2019}t open your account" : "Can\u{2019}t open \(n)\u{2019}s account",
                          lead: "This phone no longer has the key that opens your account. That happens after a screen-lock reset or a restored backup. Once you restore, your calls come through again.",
                          center: true, anchor: true,
                          hero: { ShellIconDisc(glyph: .key, size: 72, iconSize: 32).padding(.top, 12) },
                          content: {
                ShellNotice("You didn\u{2019}t set a passphrase, so the 24-word recovery phrase is the only way back. Your contacts stay.", kind: .warn, glyph: .info)
            }, foot: {
                ShellGBtn(text: "Restore with recovery phrase", enabled: !busy, action: onRestore)
                ShellGBtn(text: "Try again", kind: .text, busy: busy, action: onRetry)
                if let onSwitch { ShellGBtn(text: "Switch account", kind: .text, enabled: !busy, action: onSwitch) }
            })
        }
    }
}

/// Unlock.svelte: the avatar hangs above the heading, the field's message area is always 4.25 lines tall (error, the "deliberately
/// slow" hint and the lockout line all fit), so nothing below moves when any of them appears.
struct ShellUnlockContent: View {
    let name: String
    @Binding var pass: String
    let busy: Bool
    let error: String?
    let onForgot: () -> Void
    let onUnlock: () -> Void
    var onSwitch: (() -> Void)?
    @EnvironmentObject private var app: AppEnv

    var body: some View {
        // Identities have one nickname: the whole of it, never just the first word.
        let nick = name.trimmingCharacters(in: .whitespaces)
        let submit = { if !pass.isEmpty && !busy { onUnlock() } }
        ShellPage {
            ShellGateHead(onBack: nil, progress: nil)
            ShellGateStep(
                title: nick.isEmpty ? "Welcome back" : "Welcome back, \(nick)", center: true, anchor: true,
                hero: { ShellAvatar(did: app.accountDid ?? "", name: ShellText.ifBlank(name, "?"), size: 72).padding(.top, 12) },
                // With other identities here, the nickname in the title is the switcher (lab A, 2026-10-14).
                titleSlot: { if let onSwitch { ShellWelcomeSwitcher(nick: nick, enabled: !busy, onSwitch: onSwitch) } },
                hasTitleSlot: onSwitch != nil && !nick.isEmpty,
                content: {
                    ShellGField(label: "Passphrase", text: $pass, secret: true, error: error,
                                hint: error != nil ? "Checking takes a second. It\u{2019}s deliberately slow to stop guessing." : nil,
                                reserveLines: 4.25, enabled: !busy, autofocus: true, onDone: submit)
                },
                foot: {
                    ShellGBtn(text: busy ? "Unlocking\u{2026}" : "Unlock", enabled: !pass.isEmpty, busy: busy, action: submit)
                    ShellGBtn(text: "I forgot it", kind: .text, enabled: !busy, action: onForgot)
                })
        }
    }
}

/// "Welcome back, Maya v": the nickname opens "Open as" (the identities on this phone). Same type as the title, a quiet chevron,
/// a 48 pt target.
private struct ShellWelcomeSwitcher: View {
    let nick: String
    let enabled: Bool
    let onSwitch: () -> Void
    @Environment(\.tin) private var tin
    var body: some View {
        HStack(spacing: 0) {
            Text("Welcome back, ").shellStyle(.h1).foregroundStyle(tin.ink)
            Button(action: onSwitch) {
                HStack(spacing: 0) {
                    Text(nick).shellStyle(.h1).foregroundStyle(tin.ink).lineLimit(1)
                    ShellIcon(glyph: .chev, size: 22, color: tin.ink2)
                }
                .padding(.horizontal, 4).frame(minHeight: 48)
            }
            .buttonStyle(ShellPressStyle(radius: 10)).disabled(!enabled)
            .accessibilityLabel("\(nick), switch identity").accessibilityHint("Open as another identity")
        }
        .frame(maxWidth: .infinity).accessibilityAddTraits(.isHeader)
    }
}

// ------------------------------------------------------------------ the account switcher (Accounts.kt)

/// What the sheet's three "add" rows do; nil (onboarding) hides them.
struct ShellAccountActions {
    let onCreate: () -> Void
    let onRestore: () -> Void
    let onLink: () -> Void
}

/// The account switcher bottom sheet. `onPick` gets a non-current account; the current one is just ticked.
struct ShellAccountSheet: View {
    let core: ShellCore
    var atLogin = false
    var actions: ShellAccountActions? = nil
    let onDismiss: () -> Void
    let onPick: (String) -> Void
    @EnvironmentObject private var app: AppEnv
    @Environment(\.tin) private var tin
    @State private var list: [AccountSummary] = []

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(atLogin ? "Open as" : "Accounts").font(TinType.titleL).foregroundStyle(tin.ink).padding(.top, 4).padding(.bottom, 8)
            ForEach(list, id: \.did) { a in
                // At login nothing is open yet: say only whether picking it opens right away (key remembered here, or no
                // passphrase) or asks for its passphrase.
                let opensNow = !a.hasPassphrase || core.keys.exists(a.did)
                let sub = atLogin ? (opensNow ? "Opens right away" : "Asks for your passphrase")
                    : a.current && app.status?.online == true ? "Online on this phone" : a.current ? "On this phone" : "Locked \u{00B7} not ringing here"
                Button { if a.current { onDismiss() } else { onPick(a.did) } } label: {
                    HStack(spacing: 14) {
                        ShellAvatar(did: a.did, name: ShellText.ifBlank(a.name, "?"), size: 44)
                        VStack(alignment: .leading, spacing: 0) {
                            Text(a.name).font(TinType.bodyL.weight(.semibold)).foregroundStyle(tin.ink).lineLimit(1)
                            ShellHint(sub)
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                        if a.current { TinIcon(TinIcons.check, size: 24, tint: tin.pr, label: "Current account") }
                    }
                    .frame(minHeight: 64)
                }
                .buttonStyle(ShellPressStyle(radius: 12))
            }
            if let actions {
                Spacer().frame(height: 8)
                ShellSheetRow(glyph: .plus, title: "Create a new account", action: actions.onCreate)
                ShellSheetRow(glyph: .link, title: "Link an account from another device", action: actions.onLink)
                ShellSheetRow(glyph: .key, title: "Restore with recovery phrase", action: actions.onRestore)
            }
            if !atLogin {
                ShellHint("One account is online at a time. Calls to a locked account ring only on your other devices.").padding(.top, 12)
            }
        }
        .padding(.horizontal, 24).padding(.top, 20).padding(.bottom, 24)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(tin.sfRaised.ignoresSafeArea())
        .presentationDetents([.medium, .large]).presentationDragIndicator(.visible)
        .task { list = await core.accounts() }
    }
}

private struct ShellSheetRow: View {
    let glyph: ShellGlyph
    let title: String
    let action: () -> Void
    @Environment(\.tin) private var tin
    var body: some View {
        Button(action: action) {
            HStack(spacing: 14) {
                ShellIcon(glyph: glyph, size: 24, color: tin.pr).padding(.leading, 10)
                Text(title).font(TinType.bodyL).foregroundStyle(tin.ink).frame(maxWidth: .infinity, alignment: .leading)
            }
            .frame(minHeight: 56)
        }
        .buttonStyle(ShellPressStyle(radius: 12))
    }
}

/// "Switch to X?" with the passphrase field only when X has one and this phone has no remembered key for it.
struct ShellSwitchConfirmScreen: View {
    let core: ShellCore
    let did: String
    let onBack: () -> Void
    let onSwitched: () -> Void
    let onMessage: (String) -> Void
    @EnvironmentObject private var app: AppEnv
    @EnvironmentObject private var calls: CallsEnv
    @Environment(\.tin) private var tin
    @State private var target: AccountSummary?
    @State private var current = ""
    @State private var loaded = false
    @State private var pass = ""
    @State private var busy = false
    @State private var error: String?

    private var needsPass: Bool { (target?.hasPassphrase ?? false) && !core.keys.exists(did) }

    private func go() {
        busy = true; error = nil
        Task {
            do { try await core.switchTo(did, passphrase: needsPass ? pass : nil); pass = ""; busy = false; onSwitched() }
            catch {
                busy = false
                if ShellText.isInCall(error) { onMessage("Finish your call first"); onBack() } else { self.error = ShellText.friendly(error) }
            }
        }
    }

    var body: some View {
        ShellPage {
            ShellTopBar("Switch account", onBack: onBack)
            if let target {
                ScrollView {
                    VStack(spacing: 16) {
                        ShellAvatar(did: target.did, name: ShellText.ifBlank(target.name, "?"), size: 72)
                        Text("Switch to \(target.name)?").font(TinType.h1).foregroundStyle(tin.ink).multilineTextAlignment(.center)
                        Text("\(current) goes offline on this phone. Calls to \(current) will ring only on your other linked devices until you switch back.")
                            .font(TinType.bodyL).foregroundStyle(tin.ink2).multilineTextAlignment(.center)
                        if needsPass {
                            ShellPassField(label: "Passphrase for \(target.name)", text: $pass, error: error ?? "", enabled: !busy, mono: true,
                                           onDone: { if !pass.isEmpty && !busy { go() } })
                                .onChange(of: pass) { _, _ in error = nil }
                        } else if let error {
                            Text(error).font(TinType.bodyM).foregroundStyle(tin.er)
                        }
                        if calls.controller.ui == nil {
                            ShellHint("You\u{2019}re not on a call. If you were, switching would end it.", alignment: .center)
                        }
                        if busy { ProgressView().progressViewStyle(.linear).tint(tin.pr) }
                    }
                    .padding(.horizontal, 24).padding(.top, 16).padding(.bottom, 8)
                }
                VStack(spacing: 4) {
                    ShellTinButton(text: busy ? "Switching\u{2026}" : needsPass ? "Unlock and switch" : "Switch",
                                   enabled: !busy && (!needsPass || !pass.isEmpty), action: go)
                    ShellTinButton(text: "Cancel", style: .text, enabled: !busy, action: onBack)
                }
                .padding(.horizontal, 24).padding(.bottom, 24)
            } else {
                Spacer()
            }
        }
        .task {
            guard !loaded, let node = core.node else { return }
            loaded = true
            let (all, me) = (try? await shellIO { (node.accounts(), node.profile()?.name ?? "") }) ?? ([], "")
            current = me
            target = all.first { $0.did == did }
            if target == nil { onBack() }
        }
    }
}
