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
                self.error = ShellText.isWrongPassphrase(error) ? tr("unlock_wrong_pass") : ShellText.friendly(error)
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
            ShellGateStep(n.isEmpty ? tr("unlock_lost_title") : tr("unlock_lost_title_named", n),
                          lead: tr("unlock_lost_lead"),
                          center: true, anchor: true,
                          hero: { ShellIconDisc(glyph: .key, size: 72, iconSize: 32).padding(.top, 12) },
                          content: {
                ShellNotice(tr("unlock_lost_notice", "24"), kind: .warn, glyph: .info)
            }, foot: {
                ShellGBtn(text: tr("unlock_restore"), enabled: !busy, action: onRestore)
                ShellGBtn(text: tr("unlock_try_again"), kind: .text, busy: busy, action: onRetry)
                if let onSwitch { ShellGBtn(text: tr("unlock_switch"), kind: .text, enabled: !busy, action: onSwitch) }
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
                title: nick.isEmpty ? tr("unlock_welcome") : tr("unlock_welcome_named", nick), center: true, anchor: true,
                hero: { ShellAvatar(did: app.accountDid ?? "", name: ShellText.ifBlank(name, "?"), size: 72).padding(.top, 12) },
                // With other identities here, the nickname in the title is the switcher (lab A, 2026-10-14).
                titleSlot: { if let onSwitch { ShellWelcomeSwitcher(nick: nick, enabled: !busy, onSwitch: onSwitch) } },
                hasTitleSlot: onSwitch != nil && !nick.isEmpty,
                content: {
                    ShellGField(label: tr("unlock_pass"), text: $pass, secret: true, error: error,
                                hint: error != nil ? tr("unlock_slow_hint") : nil,
                                reserveLines: 4.25, enabled: !busy, autofocus: true, onDone: submit)
                },
                foot: {
                    ShellGBtn(text: busy ? tr("unlock_unlocking") : tr("unlock_unlock"), enabled: !pass.isEmpty, busy: busy, action: submit)
                    ShellGBtn(text: tr("unlock_forgot"), kind: .text, enabled: !busy, action: onForgot)
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
            Text(tr("unlock_welcome_prefix")).shellStyle(.h1).foregroundStyle(tin.ink)
            Button(action: onSwitch) {
                HStack(spacing: 0) {
                    Text(nick).shellStyle(.h1).foregroundStyle(tin.ink).lineLimit(1)
                    ShellIcon(glyph: .chev, size: 22, color: tin.ink2)
                }
                .padding(.horizontal, 4).frame(minHeight: 48)
            }
            .buttonStyle(ShellPressStyle(radius: 10)).disabled(!enabled)
            .accessibilityLabel(tr("unlock_switch_identity", nick)).accessibilityHint(tr("unlock_switch_identity_hint"))
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
            Text(atLogin ? tr("unlock_open_as") : tr("unlock_accounts")).font(TinType.titleL).foregroundStyle(tin.ink).padding(.top, 4).padding(.bottom, 8)
            ForEach(list, id: \.did) { a in
                // At login nothing is open yet: say only whether picking it opens right away (key remembered here, or no
                // passphrase) or asks for its passphrase.
                let opensNow = !a.hasPassphrase || core.keys.exists(a.did)
                let sub = atLogin ? (opensNow ? tr("unlock_opens_now") : tr("unlock_asks_pass"))
                    : a.current && app.status?.online == true ? tr("unlock_acc_online") : a.current ? tr("unlock_acc_here") : tr("unlock_acc_locked")
                Button { if a.current { onDismiss() } else { onPick(a.did) } } label: {
                    HStack(spacing: 14) {
                        ShellAvatar(did: a.did, name: ShellText.ifBlank(a.name, "?"), size: 44)
                        VStack(alignment: .leading, spacing: 0) {
                            Text(a.name).font(TinType.bodyL.weight(.semibold)).foregroundStyle(tin.ink).lineLimit(1)
                            ShellHint(sub)
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                        if a.current { TinIcon(TinIcons.check, size: 24, tint: tin.pr, label: tr("unlock_current_account")) }
                        else { ShellAccountUnreadDot(unread: a.unread).frame(width: 24, height: 24) }
                    }
                    .frame(minHeight: 64)
                }
                .buttonStyle(ShellPressStyle(radius: 12))
            }
            if let actions {
                Spacer().frame(height: 8)
                ShellSheetRow(glyph: .plus, title: tr("unlock_create"), action: actions.onCreate)
                ShellSheetRow(glyph: .link, title: tr("unlock_link"), action: actions.onLink)
                ShellSheetRow(glyph: .key, title: tr("unlock_restore"), action: actions.onRestore)
            }
            if !atLogin {
                ShellHint(tr("unlock_one_online")).padding(.top, 12)
            }
        }
        .padding(.horizontal, 24).padding(.top, 20).padding(.bottom, 24)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(tin.sfRaised.ignoresSafeArea())
        .presentationDetents([.medium, .large]).presentationDragIndicator(.visible)
        .task(id: app.accountsTick) { list = await core.accounts() }
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
                if ShellText.isInCall(error) { onMessage(tr("unlock_finish_call")); onBack() } else { self.error = ShellText.friendly(error) }
            }
        }
    }

    var body: some View {
        ShellPage {
            ShellTopBar(tr("unlock_switch"), onBack: onBack)
            if let target {
                ScrollView {
                    VStack(spacing: 16) {
                        ShellAvatar(did: target.did, name: ShellText.ifBlank(target.name, "?"), size: 72)
                        Text(tr("unlock_switch_to", target.name)).font(TinType.h1).foregroundStyle(tin.ink).multilineTextAlignment(.center)
                        Text(tr("unlock_switch_body", current))
                            .font(TinType.bodyL).foregroundStyle(tin.ink2).multilineTextAlignment(.center)
                        if needsPass {
                            ShellPassField(label: tr("unlock_pass_for", target.name), text: $pass, error: error ?? "", enabled: !busy, mono: true,
                                           onDone: { if !pass.isEmpty && !busy { go() } })
                                .onChange(of: pass) { _, _ in error = nil }
                        } else if let error {
                            Text(error).font(TinType.bodyM).foregroundStyle(tin.er)
                        }
                        if calls.controller.ui == nil {
                            ShellHint(tr("unlock_not_in_call"), alignment: .center)
                        }
                        if busy { ProgressView().progressViewStyle(.linear).tint(tin.pr) }
                    }
                    .padding(.horizontal, 24).padding(.top, 16).padding(.bottom, 8)
                }
                VStack(spacing: 4) {
                    ShellTinButton(text: busy ? tr("unlock_switching") : needsPass ? tr("unlock_unlock_switch") : tr("unlock_switch_btn"),
                                   enabled: !busy && (!needsPass || !pass.isEmpty), action: go)
                    ShellTinButton(text: tr("unlock_cancel"), style: .text, enabled: !busy, action: onBack)
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
