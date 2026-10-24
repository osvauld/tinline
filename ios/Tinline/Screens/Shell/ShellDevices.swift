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
            ShellTopBar("Linked devices", onBack: onBack)
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    ShellHint("Calls ring on all of these. People you call never see these names.").padding(.horizontal, 20).padding(.vertical, 8)
                    ForEach(list.filter { !$0.removed }, id: \.device) { d in ShellDeviceRow(d: d) { onDetail(d.device) } }
                    ShellTinButton(text: "Link a device", glyph: .plus, action: onLink).padding(.horizontal, 20).padding(.vertical, 16)
                    let removed = list.filter(\.removed)
                    if !removed.isEmpty {
                        ShellSectionLabel("UNLINKED")
                        ForEach(removed, id: \.device) { d in ShellDeviceRow(d: d) { onDetail(d.device) } }
                    }
                    ShellSectionLabel("SYNC")
                    ShellHint("Devices sync directly with each other whenever two are online at once. There is no copy on any server.").padding(.horizontal, 20)
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
        let title = d.thisDevice ? "This phone \u{2014} \(ShellText.ifBlank(d.label, "Unnamed"))" : ShellText.ifBlank(d.label, "Unnamed device")
        let sub = d.removed ? "Unlinked" : d.thisDevice ? "This device \u{00B7} iOS" : d.lastSeen.map { "Synced \(ShellText.agoText(Int64($0)))" } ?? "Not seen yet"
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
                ShellPage { ShellTopBar("Device", onBack: onBack) }
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
                else { onMessage("Unlinked \(ShellText.ifBlank(d.label, "device"))"); onBack() }
            } catch {
                busy = false
                let msg = ShellText.isInCall(error) ? "Finish your call first" : ShellText.friendly(error)
                self.error = msg
                if !ShellText.isWrongPassphrase(error) { confirm = false; onMessage(msg) }
            }
        }
    }

    private func confirmed(_ d: LinkedDevice) {
        let auth = ShellWiring.deviceAuth
        if hasPass || !auth.available { unlink(d); return }
        Task {
            if await auth.ask(reason: d.thisDevice ? "Remove account from this phone" : "Unlink device") { unlink(d) }
            else { error = "Screen lock check cancelled" }
        }
    }

    @ViewBuilder private func page(_ d: LinkedDevice) -> some View {
        let others = (devices ?? []).filter { !$0.removed && !$0.thisDevice }.count
        ShellPage {
            ShellTopBar(d.thisDevice ? "This phone" : ShellText.ifBlank(d.label, "Device"), onBack: onBack)
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    ShellListItem(plain: "Name", sub: ShellText.ifBlank(d.label, "Unnamed"), glyph: .edit,
                                  action: d.removed ? nil : ({ renameText = d.label; renaming = true } as () -> Void))
                    ShellListItem(plain: "Last synced", sub: d.thisDevice ? "This device" : d.lastSeen.map { ShellText.agoText(Int64($0)) } ?? "Not yet", glyph: .retry)
                    ShellListItem(plain: "Rings for calls", sub: d.removed ? "No, it was unlinked" : "Yes, when it\u{2019}s on", glyph: .phone)
                    if !d.removed {
                        VStack(spacing: 8) {
                            if !d.thisDevice {
                                ShellTinButton(text: "Sync now", style: .outlined, glyph: .retry) {
                                    core.node?.networkChanged(); onMessage("Syncing with \(ShellText.ifBlank(d.label, "the device"))\u{2026}")
                                }
                            }
                            ShellTinButton(text: d.thisDevice ? "Remove account from this phone" : "Unlink this device", style: .danger, glyph: .off) {
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
                ShellDialog(title: d.thisDevice ? "Name this phone" : "Rename device", confirm: "Save",
                            confirmEnabled: !renameText.trimmingCharacters(in: .whitespaces).isEmpty, onDismiss: { renaming = false }, onConfirm: {
                    let n = renameText.trimmingCharacters(in: .whitespaces)
                    renaming = false
                    Task {
                        do { try await core.work { try $0.renameDevice(device: d.device, label: n) }; core.links.devicesChanged() }
                        catch { onMessage(ShellText.friendly(error)) }
                    }
                }) { ShellGField(label: "Device name", text: $renameText, capitalization: .words, autofocus: true) }
            }
            if confirm {
                let name = ShellText.ifBlank(d.label, "this device")
                ShellDialog(title: d.thisDevice ? "Remove your account from this phone?" : "Unlink \(name)?",
                            confirm: busy ? "Working\u{2026}" : d.thisDevice ? "Remove" : "Unlink", destructive: true, glyph: .off,
                            confirmEnabled: !busy && (!hasPass || !pass.isEmpty), onDismiss: { if !busy { confirm = false } }, onConfirm: { confirmed(d) }) {
                    if d.thisDevice {
                        ShellDialogText("This phone stops syncing and ringing, and your account, contacts, chats and call history are deleted from it.")
                        if others == 0 {
                            ShellInfoCard(text: "This is the only device with your account. Without your recovery phrase nothing can bring it back.", glyph: .warning, kind: .error)
                        } else {
                            ShellDialogText("Your other devices keep everything.")
                        }
                    } else {
                        ShellDialogText("It stops syncing and stops ringing for your calls. If it\u{2019}s online, it removes your account from itself.")
                        ShellInfoCard(text: "Unlinking can\u{2019}t lock it from here. Anyone who can unlock it can still use your account. The only full cut-off is a new account.",
                                      glyph: .warning, kind: .warn, bold: "Lost or stolen?")
                    }
                    if hasPass {
                        ShellPassField(label: "Your passphrase", text: $pass, error: error ?? "", enabled: !busy, mono: true,
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
