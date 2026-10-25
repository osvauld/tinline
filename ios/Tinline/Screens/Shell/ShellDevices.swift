import SwiftUI

/*
 * Settings > Devices (android Devices.kt): the account's registry, one device's page (rename, sync now, unlink) with the board's
 * confirm that says what unlinking cannot do. Unlinking is honest: not a lock.
 */

/// The registry of the account, re-read whenever the core says it changed.
private struct ShellDevicesReader: ViewModifier {
    let core: ShellCore
    @Binding var devices: [LinkedDevice]?
    @ObservedObject var links: ShellLinkController
    func body(content: Content) -> some View {
        content.task(id: links.devicesVersion) {
            guard let node = core.node else { devices = []; return }
            devices = (try? await shellIO { node.linkedDevices() }) ?? []
        }
    }
}

/// Settings > Devices > Linked devices.
struct ShellLinkedDevicesScreen: View {
    let core: ShellCore
    let onBack: () -> Void
    let onLink: () -> Void
    let onDetail: (String) -> Void
    @Environment(\.tin) private var tin
    @State private var devices: [LinkedDevice]?

    var body: some View {
        let list = devices ?? []
        ShellPage {
            ShellTopBar(tr("devices_title"), onBack: onBack)
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    ShellHint(tr("devices_hint_rings")).padding(.horizontal, 20).padding(.vertical, 8)
                    ForEach(list.filter { !$0.removed }, id: \.device) { d in ShellDeviceRow(d: d) { onDetail(d.device) } }
                    ShellTinButton(text: tr("devices_link"), glyph: .plus, action: onLink).padding(.horizontal, 20).padding(.vertical, 16)
                    let removed = list.filter(\.removed)
                    if !removed.isEmpty {
                        ShellSectionLabel(tr("devices_unlinked_label"))
                        ForEach(removed, id: \.device) { d in ShellDeviceRow(d: d) { onDetail(d.device) } }
                    }
                    ShellSectionLabel(tr("devices_sync_label"))
                    ShellHint(tr("devices_sync_hint")).padding(.horizontal, 20)
                }
            }
        }
        .modifier(ShellDevicesReader(core: core, devices: $devices, links: core.links))
    }
}

private struct ShellDeviceRow: View {
    let d: LinkedDevice
    let action: () -> Void
    @Environment(\.tin) private var tin
    var body: some View {
        let title = d.thisDevice ? tr("devices_row_this_phone", ShellText.ifBlank(d.label, tr("devices_unnamed"))) : ShellText.ifBlank(d.label, tr("devices_unnamed_device"))
        let sub = d.removed ? tr("devices_row_unlinked") : d.thisDevice ? tr("devices_row_this_device") : d.lastSeen.map { tr("devices_row_synced", ShellText.agoText(Int64($0))) } ?? tr("devices_row_not_seen")
        ShellListItem(title, sub: sub, glyph: d.thisDevice ? .smartphone : .devices, titleColor: d.removed ? tin.ink2 : tin.ink, chevronColor: tin.ink2, action: action)
    }
}

/// One device: rename, sync now, unlink (with the board's confirm that says what unlinking cannot do).
struct ShellDeviceDetailScreen: View {
    let core: ShellCore
    let device: String
    let onBack: () -> Void
    let onMessage: (String) -> Void
    @EnvironmentObject private var app: AppEnv
    @Environment(\.tin) private var tin
    @State private var devices: [LinkedDevice]?
    @State private var hasPass = true
    @State private var renaming = false
    @State private var renameText = ""
    @State private var confirm = false
    @State private var pass = ""
    @State private var busy = false
    @State private var error: String?

    var body: some View {
        let d = devices?.first { $0.device == device }
        Group {
            if let d {
                page(d)
            } else {
                ShellPage { ShellTopBar(tr("devices_device"), onBack: onBack) }
            }
        }
        .modifier(ShellDevicesReader(core: core, devices: $devices, links: core.links))
        .onChange(of: devices) { _, list in if let list, !list.contains(where: { $0.device == device }) { onBack() } }
        .task { hasPass = (try? await core.work { $0.hasPassphrase() }) ?? true }
    }

    private func unlink(_ d: LinkedDevice) {
        busy = true; error = nil
        let did = app.accountDid, p = hasPass ? pass : nil
        Task {
            do {
                try await core.work { try $0.unlinkDevice(device: d.device, passphrase: p) }
                busy = false; pass = ""; confirm = false
                if d.thisDevice, let did { await core.accountGone(did, notify: false) }
                else { onMessage(tr("devices_unlinked_msg", ShellText.ifBlank(d.label, tr("devices_fb_device")))); onBack() }
            } catch {
                busy = false
                let msg = ShellText.isInCall(error) ? tr("devices_finish_call") : ShellText.friendly(error)
                self.error = msg
                if !ShellText.isWrongPassphrase(error) { confirm = false; onMessage(msg) }
            }
        }
    }

    private func confirmed(_ d: LinkedDevice) {
        let auth = ShellWiring.deviceAuth
        if hasPass || !auth.available { unlink(d); return }
        Task {
            if await auth.ask(reason: d.thisDevice ? tr("devices_remove_account") : tr("devices_unlink_device")) { unlink(d) }
            else { error = tr("devices_lock_cancelled") }
        }
    }

    @ViewBuilder private func page(_ d: LinkedDevice) -> some View {
        let others = (devices ?? []).filter { !$0.removed && !$0.thisDevice }.count
        ShellPage {
            ShellTopBar(d.thisDevice ? tr("devices_this_phone") : ShellText.ifBlank(d.label, tr("devices_device")), onBack: onBack)
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    ShellListItem(plain: tr("devices_prop_name"), sub: ShellText.ifBlank(d.label, tr("devices_unnamed")), glyph: .edit,
                                  action: d.removed ? nil : ({ renameText = d.label; renaming = true } as () -> Void))
                    ShellListItem(plain: tr("devices_prop_synced"), sub: d.thisDevice ? tr("devices_this_device") : d.lastSeen.map { ShellText.agoText(Int64($0)) } ?? tr("devices_not_yet"), glyph: .retry)
                    ShellListItem(plain: tr("devices_prop_rings"), sub: d.removed ? tr("devices_rings_no") : tr("devices_rings_yes"), glyph: .phone)
                    if !d.removed {
                        VStack(spacing: 8) {
                            if !d.thisDevice {
                                ShellTinButton(text: tr("devices_sync_now"), style: .outlined, glyph: .retry) {
                                    core.node?.networkChanged(); onMessage(tr("devices_syncing", ShellText.ifBlank(d.label, tr("devices_fb_the_device"))))
                                }
                            }
                            ShellTinButton(text: d.thisDevice ? tr("devices_remove_account") : tr("devices_unlink_this"), style: .danger, glyph: .off) {
                                pass = ""; error = nil; confirm = true
                            }
                        }
                        .padding(.horizontal, 20).padding(.vertical, 16)
                    }
                }
            }
        }
        .overlay {
            if renaming {
                ShellDialog(title: d.thisDevice ? tr("devices_name_phone") : tr("devices_rename"), confirm: tr("devices_save"),
                            confirmEnabled: !renameText.trimmingCharacters(in: .whitespaces).isEmpty, onDismiss: { renaming = false }, onConfirm: {
                    let n = renameText.trimmingCharacters(in: .whitespaces)
                    renaming = false
                    Task {
                        do { try await core.work { try $0.renameDevice(device: d.device, label: n) }; core.links.devicesChanged() }
                        catch { onMessage(ShellText.friendly(error)) }
                    }
                }) { ShellGField(label: tr("devices_name_field"), text: $renameText, capitalization: .words, autofocus: true) }
            }
            if confirm {
                let name = ShellText.ifBlank(d.label, tr("devices_fb_this_device"))
                ShellDialog(title: d.thisDevice ? tr("devices_remove_q") : tr("devices_unlink_q", name),
                            confirm: busy ? tr("devices_working") : d.thisDevice ? tr("devices_remove") : tr("devices_unlink"), destructive: true, glyph: .off,
                            confirmEnabled: !busy && (!hasPass || !pass.isEmpty), onDismiss: { if !busy { confirm = false } }, onConfirm: { confirmed(d) }) {
                    if d.thisDevice {
                        ShellDialogText(tr("devices_remove_body"))
                        if others == 0 {
                            ShellInfoCard(text: tr("devices_only_device"), glyph: .warning, kind: .error)
                        } else {
                            ShellDialogText(tr("devices_others_keep"))
                        }
                    } else {
                        ShellDialogText(tr("devices_unlink_body"))
                        ShellInfoCard(text: tr("devices_unlink_warn"),
                                      glyph: .warning, kind: .warn, bold: tr("devices_lost"))
                    }
                    if hasPass {
                        ShellPassField(label: tr("devices_your_pass"), text: $pass, error: error ?? "", enabled: !busy, mono: true,
                                       onDone: { if !pass.isEmpty && !busy { confirmed(d) } })
                            .onChange(of: pass) { _, _ in error = nil }
                    } else if let error {
                        Text(error).font(TinType.bodyM).foregroundStyle(tin.er)
                    }
                    if busy { ProgressView().progressViewStyle(.linear).tint(tin.pr) }
                }
            }
        }
    }
}
