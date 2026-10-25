import SwiftUI

/*
 * Settings (playground screens/settings: Settings.svelte, Notifications.svelte, Sounds.svelte, PermBanner.svelte, perms.ts) and
 * the Privacy landing (screens/privacy/Privacy.svelte, CallsAllowed.svelte), with the native pages About, Licences and
 * Diagnostics. Ports of android Settings.kt, SettingsPages.kt and SettingsRoutes.kt. Profile card, accounts, appearance, then
 * Devices / Privacy / Notifications / Sounds / Video / Storage / About.
 */

/// perms.ts Problem: one OS-permission problem with its calm copy (same wording on the landing strip and the Notifications page).
struct ShellPermProblem: Identifiable {
    let need: ShellNeed
    let glyph: ShellGlyph
    let title: String
    let body: String
    var fix = tr("settings_perm_fix")
    var id: ShellNeed { need }
}

/// perms.ts problems(), in its order (iOS has no battery-saving problem to report).
func shellPermProblems(_ missing: [ShellNeed]) -> [ShellPermProblem] {
    var out: [ShellPermProblem] = []
    if missing.contains(.notifications) {
        out.append(ShellPermProblem(need: .notifications, glyph: .bell, title: tr("settings_perm_notif_title"),
            body: tr("settings_perm_notif_body")))
    }
    return out
}

/// PermBanner.svelte: warn-bg card (r 14, padding 12 14, gap 12), icon 20, title 650/20, body 13/19, an outline button in the ink.
struct ShellPermBanner: View {
    let p: ShellPermProblem
    var action: String? = nil
    let onAction: () -> Void
    @Environment(\.tin) private var tin
    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            ShellIcon(glyph: p.glyph, size: 20, color: tin.onWarn).padding(.top, 1)
            VStack(alignment: .leading, spacing: 4) {
                Text(p.title).shellStyle(.bodyB).foregroundStyle(tin.onWarn)
                Text(p.body).shellStyle(.sub19).foregroundStyle(tin.onWarn)
                ShellPBtn(text: action ?? p.fix, height: 36, colorOverride: tin.onWarn, action: onAction).padding(.top, 6)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(.horizontal, 14).padding(.vertical, 12)
        .background(RoundedRectangle(cornerRadius: 14).fill(tin.warn))
        .padding(.bottom, 12)
    }
}

// ------------------------------------------------------------------ the landing (SettingsRoutes.kt SettingsRoute + Settings.svelte)

struct ShellSettingsRoute: View {
    let core: ShellCore
    let missing: [ShellNeed]
    let onBack: () -> Void
    let onCreate: () -> Void
    let onRestore: () -> Void
    let onLink: () -> Void
    @EnvironmentObject private var app: AppEnv
    @EnvironmentObject private var nav: ShellNav
    @EnvironmentObject private var people: PeopleModel
    @Environment(\.tin) private var tin
    @AppStorage("appearance.dark") private var appearanceDark: Bool?
    @State private var accounts: [AccountSummary] = []
    @State private var fallback = ""
    @State private var adding = false
    @State private var video = ShellPrefs().videoEnabled
    @State private var nowMs = ShellText.nowMs()

    var body: some View {
        let p = ShellPalette(tin)
        let me = ShellWiring.selfView(people: people)
        let name = me?.text("name").flatMap { $0.isEmpty ? nil : $0 } ?? fallback
        let status = me?.text("status").flatMap { $0.isEmpty ? nil : $0 }
        let av = effectiveAvailability(me?.fields, nowMs: nowMs)
        let avText = av.available ? tr("settings_status_available") : av.untilMs.map { tr("settings_status_busy_until", ShellWiring.busyUntilLabel($0)) } ?? tr("settings_status_busy")
        ShellPfPage(title: tr("settings_title"), onBack: onBack) {
            // Permission problems: said once here, with the details on the Notifications page.
            if let pr = shellPermProblems(missing).first {
                ShellPfSec { ShellPermBanner(p: pr, action: tr("settings_perm_see_details")) { nav.push(.notifications) } }
            }
            ShellPfSec(first: true) {
                ShellPfCard {
                    ShellPfAct(title: name, chevron: true, minHeight: 88, titleSize: 17, gap: 16, padV: 12, action: { nav.push(.me) },
                               leading: { ShellAvatar(did: app.accountDid ?? "", name: name, size: 64) }, trailing: { EmptyView() }, extra: {
                        if let status { Text(status).shellStyle(.sub).foregroundStyle(tin.ink2) }
                        Text(avText).shellStyle(.sub, weight: .semibold).monospacedDigit().foregroundStyle(av.available ? tin.ink2 : tin.onWarn)
                    })
                }
            }
            ShellPfSec {
                ShellPfCard {
                    ForEach(accounts.filter(\.current), id: \.did) { a in
                        ShellPfAct(title: a.name, sub: tr("settings_account_selected"), subColor: p.accentInk, subBold: true,
                                   leading: { ShellAvatar(did: a.did, name: a.name, size: 36) },
                                   trailing: { ShellIcon(glyph: .check, size: 18, color: tin.ink2) }, extra: { EmptyView() })
                    }
                    ForEach(accounts.filter { !$0.current }, id: \.did) { a in
                        ShellPfAct(title: a.name, sub: tr("settings_account_paused"), action: { nav.push(.switchConfirm(a.did)) },
                                   leading: { ShellAvatar(did: a.did, name: a.name, size: 36) }, trailing: { EmptyView() }, extra: { EmptyView() })
                    }
                    ShellPfAct(tr("settings_add_account"), glyph: .plus, chevron: true) { adding = true }
                }
                Text(tr("settings_accounts_note"))
                    .shellStyle(.sub).foregroundStyle(tin.ink2).padding(.horizontal, 16).padding(.top, 8)
            }
            // Appearance (Settings.svelte LOOKS, 2026-10-14): the same choice the sign-up switch makes; switches at once.
            ShellPfSec(title: tr("settings_appearance")) {
                ShellPfCard {
                    ForEach([(nil, tr("settings_appearance_match"), tr("settings_appearance_match_sub")), (false, tr("settings_appearance_light"), nil), (true, tr("settings_appearance_dark"), nil)] as [(Bool?, String, String?)],
                            id: \.1) { v, label, sub in
                        ShellPfAct(radio: label, sub: sub, on: appearanceDark == v) { appearanceDark = v }
                    }
                }
            }
            ShellPfSec {
                ShellPfCard {
                    ShellPfAct(tr("settings_row_devices"), glyph: .devices, chevron: true) { nav.push(.devices) }
                    ShellPfAct(tr("settings_row_privacy"), glyph: .shield, chevron: true) { nav.push(.privacy) }
                    ShellPfAct(tr("settings_notifications"), glyph: .bell, chevron: true) { nav.push(.notifications) }
                    ShellPfAct(tr("settings_row_sounds"), glyph: .vol, chevron: true) { nav.push(.sounds) }
                    ShellPfAct(tr("settings_row_video"), glyph: .video, chevron: true) { nav.push(.video) }
                    ShellPfAct(tr("settings_row_storage"), glyph: .save, chevron: true) { nav.push(.storage) }
                    ShellPfAct(tr("settings_row_about"), glyph: .info, chevron: true) { nav.push(.about) }
                }
            }
            // Not in the playground: the app's "Video in calls" offer, and developer tools.
            ShellPfSec {
                ShellPfCard {
                    ShellPfAct(tr("settings_video_in_calls"), glyph: .video, sub: video ? tr("settings_video_in_calls_on") : tr("settings_video_in_calls_off"),
                               traits: .isToggle, action: { video.toggle(); core.setVideoEnabled(video) }) {
                        ShellSwitch(isOn: video, label: tr("settings_video_in_calls")) { video = $0; core.setVideoEnabled($0) }
                    }
                    #if DEBUG
                    ShellPfAct(tr("settings_diagnostics"), sub: tr("settings_diagnostics_sub"), chevron: true) { nav.push(.diagnostics) }
                    #endif
                }
            }
        }
        .overlay {
            // gate.beginAdd(): the playground's Choose step. The app picks the path before it begins adding.
            if adding {
                ShellDialog(title: tr("settings_add_account"), confirm: tr("settings_cancel"), dismiss: nil, onDismiss: { adding = false }, onConfirm: { adding = false }) {
                    VStack(spacing: 0) {
                        ShellPfAct(tr("settings_add_create"), glyph: .plus) { adding = false; onCreate() }
                        ShellPfAct(tr("settings_add_link"), glyph: .devices) { adding = false; onLink() }
                        ShellPfAct(tr("settings_add_restore"), glyph: .key) { adding = false; onRestore() }
                    }
                    .padding(.horizontal, -12)
                }
            }
        }
        .task(id: app.accountDid) {
            accounts = await core.accounts()
            fallback = (try? await core.work { $0.profile()?.name ?? "" }) ?? ""
        }
        .task(id: av.untilMs) {
            if let u = av.untilMs {
                try? await Task.sleep(nanoseconds: UInt64(max(0, u - ShellText.nowMs()) + 50) * 1_000_000)
                nowMs = ShellText.nowMs()
            }
        }
    }
}

// ------------------------------------------------------------------ Notifications (Notifications.svelte)

/// Settings > Notifications. `missing` is what iOS reports; `fix` asks the system or opens Settings.
struct ShellNotificationsPage: View {
    let missing: [ShellNeed]
    let onBack: () -> Void
    let fix: (ShellNeed) -> Void
    @Environment(\.tin) private var tin
    @State private var prefs = ShellPrefs()
    @State private var tick = 0

    var body: some View {
        let probs = shellPermProblems(missing)
        let off = missing.contains(.notifications)
        let warnInk = tin.dark ? ShellPalette(tin).goldInk : tin.onWarn
        ShellPfPage(title: tr("settings_notifications"), onBack: onBack) {
            if !probs.isEmpty { ShellPfSec(first: true) { ForEach(probs) { p in ShellPermBanner(p: p) { fix(p.need) } } } }
            ShellPfSec(title: tr("settings_notif_alert_me"), top: 12) {
                ShellPfCard {
                    ForEach([("messages", ShellGlyph.chat, tr("settings_notif_messages"), tr("settings_notif_messages_sub"), tr("settings_notif_messages_label")),
                             ("calls", .phone, tr("settings_notif_calls"), tr("settings_notif_calls_sub"), tr("settings_notif_calls_label")),
                             ("groups", .users, tr("settings_notif_groups"), tr("settings_notif_groups_sub"), tr("settings_notif_groups_label"))], id: \.0) { k, g, t, sub, label in
                        let on = prefs.alert(k)
                        ShellPfAct(t, glyph: g, sub: sub, traits: .isToggle, action: off ? nil : ({ prefs.setAlert(k, !on); tick += 1 } as () -> Void)) {
                            ShellSwitch(isOn: on, label: label, enabled: !off) { prefs.setAlert(k, $0); tick += 1 }
                        }
                    }
                }
                .opacity(off ? 0.55 : 1).id(tick)
                ShellPfNote(tr("settings_notif_note"))
            }
            ShellPfSec(title: tr("settings_perms")) {
                ShellPfCard {
                    ShellPfAct(tr("settings_notifications"), glyph: .bell, sub: off ? tr("settings_perm_off") : tr("settings_perm_allowed"), subColor: off ? warnInk : nil, subBold: off, action: nil) {
                        ZStack(alignment: .trailing) { if off { ShellPBtn(text: tr("settings_perm_fix_btn"), height: 36, padH: 14) { fix(.notifications) } } }.frame(minWidth: 64, alignment: .trailing)
                    }
                    // The microphone is asked for when a call needs it, so it is never a problem to fix here.
                    ShellPfAct(tr("settings_perm_mic"), glyph: .mic, sub: missing.contains(.mic) ? tr("settings_perm_mic_ask") : tr("settings_perm_allowed"))
                }
                ShellPfNote(tr("settings_perms_note"))
            }
        }
    }
}

// ------------------------------------------------------------------ Sounds (Sounds.svelte)

/// Quiet, and the call sound set with previews (the core renders the same cues as the desktop).
struct ShellSoundsPage: View {
    let onBack: () -> Void
    @State private var prefs = ShellPrefs()
    @State private var quiet = ShellPrefs().alert("quiet")
    @State private var palette = ShellPrefs().callPalette

    var body: some View {
        let sets = [("classic", tr("settings_sound_classic"), tr("settings_sound_classic_sub")),
                    ("slack", tr("settings_sound_slack"), tr("settings_sound_slack_sub")),
                    ("triad", tr("settings_sound_triad"), tr("settings_sound_triad_sub"))]
        ShellPfPage(title: tr("settings_row_sounds"), onBack: onBack) {
            ShellPfSec(title: tr("settings_quiet"), first: true) {
                ShellPfCard {
                    ShellPfAct(tr("settings_quiet"), glyph: .mute, sub: tr("settings_quiet_sub"), traits: .isToggle,
                               action: { quiet.toggle(); prefs.setAlert("quiet", quiet) }) {
                        ShellSwitch(isOn: quiet, label: tr("settings_quiet")) { quiet = $0; prefs.setAlert("quiet", $0) }
                    }
                }
            }
            ShellPfSec(title: tr("settings_call_sounds")) {
                ShellPfCard {
                    ForEach(sets, id: \.0) { k, label, sub in
                        ShellPfAct(radio: label, sub: sub, on: palette == k, minHeight: 64) { palette = k; prefs.callPalette = k; ShellWiring.previewSound("ring") }
                    }
                }
                HStack(spacing: 8) {
                    ForEach([("ring", tr("settings_cue_ring")), ("connected", tr("settings_cue_connected")), ("ended", tr("settings_cue_ended"))], id: \.0) { cue, label in
                        ShellChip(text: label, glyph: .vol) { ShellWiring.previewSound(cue) }
                    }
                }
                .padding(.horizontal, 4).padding(.top, 12)
                ShellPfNote(quiet ? tr("settings_sounds_note_quiet") : tr("settings_sounds_note"))
            }
        }
    }
}

// ------------------------------------------------------------------ Video (CallsVideo.svelte, phone form)

/// "Sharper video" only; the phone picks its own cameras (Flip in a call). Each video call starts this way (the HD chip in a
/// call changes only that call).
struct ShellVideoSettingsPage: View {
    let core: ShellCore
    let onBack: () -> Void
    @State private var hd = ShellPrefs().preferHd
    var body: some View {
        let set = { (on: Bool) in hd = on; core.prefs.preferHd = on }
        ShellPfPage(title: tr("settings_row_video"), onBack: onBack) {
            ShellPfSec(title: tr("settings_video_quality"), first: true) {
                ShellPfCard {
                    ShellPfAct(tr("settings_video_sharper"), glyph: .video, sub: hd ? tr("settings_video_hd_on") : tr("settings_video_hd_auto"),
                               subReserve: [tr("settings_video_hd_on"), tr("settings_video_hd_auto")],
                               traits: .isToggle, action: { set(!hd) }) {
                        ShellSwitch(isOn: hd, label: tr("settings_video_sharper"), onChange: set)
                    }
                }
                ShellPfNote(tr("settings_video_note"))
            }
        }
    }
}

// ------------------------------------------------------------------ Storage (Storage.svelte)

/// What Tinline keeps here (one bar), when pictures download, and clearing the cache.
struct ShellStoragePage: View {
    let core: ShellCore
    let onBack: () -> Void
    @Environment(\.tin) private var tin
    @State private var sizes: [Int64] = [0, 0, 0, 0]
    @State private var download: PictureDownload = .always
    @State private var cleared = false
    @State private var tick = 0

    var body: some View {
        let p = ShellPalette(tin)
        let kinds: [(String, Int64, Color)] = [(tr("settings_storage_pictures"), sizes[0], p.accent), (tr("settings_storage_files"), sizes[1], tin.thread), (tr("settings_storage_messages"), sizes[2], tin.ink2), (tr("settings_storage_cache"), sizes[3], tin.ln)]
        let total = kinds.reduce(0) { $0 + $1.1 }
        ShellPfPage(title: tr("settings_row_storage"), onBack: onBack) {
            ShellPfSec(title: tr("settings_storage_on_phone"), first: true) {
                ShellPfCard {
                    VStack(alignment: .leading, spacing: 12) {
                        Text(ShellText.bytes(total)).shellStyle(.title).foregroundStyle(tin.ink)
                        GeometryReader { g in
                            let shown = kinds.filter { $0.1 > 0 }
                            let gaps = CGFloat(max(0, shown.count - 1)) * 2
                            HStack(spacing: 2) {
                                ForEach(shown, id: \.0) { k in
                                    k.2.frame(width: total > 0 ? max(0, (g.size.width - gaps) * CGFloat(k.1) / CGFloat(total)) : 0)
                                }
                            }
                        }
                        .frame(height: 10).background(tin.sf2).clipShape(RoundedRectangle(cornerRadius: 5))
                        VStack(spacing: 6) {
                            ForEach(kinds, id: \.0) { label, n, col in
                                HStack(spacing: 8) {
                                    RoundedRectangle(cornerRadius: 3).fill(col).frame(width: 10, height: 10)
                                    Text(label).shellStyle(.sub).foregroundStyle(tin.ink).frame(maxWidth: .infinity, alignment: .leading)
                                    Text(ShellText.bytes(n)).shellStyle(.sub).monospacedDigit().foregroundStyle(tin.ink2)
                                }
                            }
                        }
                    }
                    .padding(16)
                }
            }
            ShellPfSec(title: tr("settings_storage_download")) {
                ShellPfCard {
                    ForEach([(PictureDownload.always, tr("settings_dl_always"), tr("settings_dl_always_sub")), (.wifi, tr("settings_dl_wifi"), tr("settings_dl_wifi_sub")),
                             (.tap, tr("settings_dl_tap"), tr("settings_dl_tap_sub"))], id: \.1) { d, label, sub in
                        ShellPfAct(radio: label, sub: sub, on: download == d) { setDownload(d) }
                    }
                }
            }
            ShellPfSec {
                ShellPfCard {
                    ShellPfAct(tr("settings_clear_cache"), glyph: .trash,
                               sub: sizes[3] > 0 ? tr("settings_clear_cache_sub", ShellText.bytes(sizes[3]))
                                : cleared ? tr("settings_cache_cleared") : tr("settings_cache_empty"),
                               action: sizes[3] > 0 ? ({ ShellMediaCache.clear(); cleared = true; tick += 1 } as () -> Void) : nil)
                }
                ShellPfNote(tr("settings_storage_note"))
            }
        }
        .task(id: tick) {
            guard let node = core.node else { return }
            let r = try? await shellIO { (try? node.storageUsage(), try? node.pictureDownload()) }
            let u = r?.0 ?? nil
            if let d = r?.1 ?? nil { download = d }
            sizes = [Int64(u?.pictures ?? 0), Int64(u?.files ?? 0), Int64(u?.messages ?? 0), ShellMediaCache.size()]
        }
    }

    private func setDownload(_ d: PictureDownload) {
        download = d
        guard let node = core.node else { return }
        Task.detached { try? node.setPictureDownload(policy: d) }
    }
}

/// The opened-media cache (android ChatMedia.size / clear): the Chat area's decrypted copies (Chat/ChatMedia.swift).
enum ShellMediaCache {
    static func size() -> Int64 { ChatMedia.size() }
    static func clear() { ChatMedia.clear() }
}

// ------------------------------------------------------------------ Privacy (Privacy.svelte, CallsAllowed.svelte)

private let readReceiptsOn = tr("settings_reads_on")
private let readReceiptsOff = tr("settings_reads_off")

struct ShellPrivacyPage: View {
    let core: ShellCore
    let onBack: () -> Void
    @EnvironmentObject private var app: AppEnv
    @EnvironmentObject private var nav: ShellNav
    @EnvironmentObject private var people: PeopleModel
    @State private var hasPass = true
    @State private var reads = true

    var body: some View {
        let blockedN = people.blocks.list.count
        let msgOnly = people.contacts.filter { !people.blocks.of($0.did).blockedByMe && !people.blocks.callsAllowed($0.did) }.count
        ShellPfPage(title: tr("settings_row_privacy"), onBack: onBack) {
            ShellPfSec(first: true) {
                ShellPfCard {
                    ShellPfAct(tr("settings_blocked_people"), glyph: .off, sub: blockedN > 0 ? trn("settings_blocked_count", blockedN) : tr("settings_blocked_none"), chevron: true) { nav.push(.blocked) }
                    ShellPfAct(tr("settings_who_can_call"), glyph: .phone, sub: msgOnly > 0 ? trn("settings_msg_only_count", msgOnly) : tr("settings_all_can_call"),
                               chevron: true) { nav.push(.callsAllowed) }
                    // Native: without a passphrase the phrase is behind the phone's screen lock instead.
                    ShellPfAct(tr("settings_recovery"), glyph: .key, sub: hasPass ? tr("settings_recovery_pass") : tr("settings_recovery_lock"), chevron: true) { nav.push(.phraseGate) }
                    ShellPfAct(tr("settings_passphrase"), glyph: .lock, sub: tr("settings_passphrase_sub"), chevron: true) { nav.push(.passphrase) }
                }
                ShellPfNote(tr("settings_privacy_note"))
            }
            // Core account setting (default on). Same words as desktop.
            ShellPfSec {
                ShellPfCard {
                    ShellPfAct(tr("settings_read_receipts"), glyph: .check, sub: reads ? readReceiptsOn : readReceiptsOff,
                               subReserve: [readReceiptsOn, readReceiptsOff], // both heights reserved: flipping moves nothing
                               traits: .isToggle, action: { setReads(!reads) }) {
                        ShellSwitch(isOn: reads, label: tr("settings_read_receipts"), onChange: setReads)
                    }
                }
                ShellPfNote(tr("settings_reads_note"))
            }
        }
        .task(id: app.accountDid) {
            hasPass = (try? await core.work { $0.hasPassphrase() }) ?? true
            if let r = try? await core.work({ try $0.readReceipts() }) { reads = r }
        }
    }

    private func setReads(_ on: Bool) {
        reads = on
        guard let node = core.node else { return }
        Task.detached { try? node.setReadReceipts(on: on) }
    }
}

/// CallsAllowed.svelte: each contact's Allow calls (the same switch as on their profile).
struct ShellCallsAllowedPage: View {
    let onBack: () -> Void
    @EnvironmentObject private var people: PeopleModel
    @Environment(\.tin) private var tin
    var body: some View {
        let rows = people.contacts.filter { !people.blocks.of($0.did).blockedByMe }
            .map { (did: $0.did, name: ShellText.display($0), allowed: people.blocks.callsAllowed($0.did)) }
            .sorted { $0.name.lowercased() < $1.name.lowercased() }
        ShellPfPage(title: tr("settings_who_can_call"), onBack: onBack) {
            ShellPfSec(title: tr("settings_contacts"), first: true) {
                ShellPfCard {
                    ForEach(Array(rows.enumerated()), id: \.element.did) { i, r in
                        if i > 0 { ShellPfHairline() }
                        HStack(spacing: 12) {
                            ShellAvatar(did: r.did, name: r.name, size: 44)
                            VStack(alignment: .leading, spacing: 1) {
                                Text(r.name).shellStyle(.body, weight: .semibold).foregroundStyle(tin.ink).lineLimit(1)
                                Text(r.allowed ? tr("settings_can_call_you") : tr("settings_messages_only")).shellStyle(.sub).foregroundStyle(tin.ink2)
                            }
                            .frame(maxWidth: .infinity, alignment: .leading)
                            ShellSwitch(isOn: r.allowed, label: tr("settings_allow_calls_from", r.name)) { people.allowCalls(r.did, $0) }
                        }
                        .padding(.horizontal, 12).frame(minHeight: 64)
                    }
                }
                ShellPfNote(tr("settings_calls_note"))
            }
        }
    }
}

// ------------------------------------------------------------------ About, Licences, Diagnostics (Settings.kt)

/// About.svelte: the mark, "Tinline", the version, who makes it, the expandable open-source list and the source. Terms, Privacy and
/// the website follow, then the emergency-calls note. The libraries are the core's list plus the iOS-only ones.
struct ShellAboutScreen: View {
    let onBack: () -> Void
    let onLicences: () -> Void
    @Environment(\.tin) private var tin
    @State private var open = false

    /// The libraries only the iOS app ships (the core's own are `openSourceLicences()`).
    static let iosLibs = [("Figtree and IBM Plex Mono", "SIL OFL 1.1")]

    var body: some View {
        let libs = openSourceLicences().map { ($0.name, $0.licence) } + Self.iosLibs
        let version = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? ""
        ShellPfPage(title: tr("settings_row_about"), onBack: onBack) {
            VStack(spacing: 4) {
                TinMark(size: 56)
                Text("Tinline").shellStyle(.title).foregroundStyle(tin.ink).padding(.top, 8)
                Text(tr("settings_about_version", version)).shellStyle(.sub).monospacedDigit().foregroundStyle(tin.ink2)
                Text(tr("settings_about_made_by")).shellStyle(.sub).foregroundStyle(tin.ink2)
            }
            .frame(maxWidth: .infinity).padding(.horizontal, 16).padding(.top, 24).padding(.bottom, 8)
            ShellPfSec {
                ShellPfCard {
                    ShellPfAct(tr("settings_licences"), glyph: .licence, sub: tr("settings_licences_sub"), action: { open.toggle() }) {
                        ShellIcon(glyph: open ? .chevup : .chev, size: 18, color: tin.ink2)
                    }
                    if open {
                        VStack(spacing: 8) {
                            ForEach(libs, id: \.0) { n, l in
                                HStack(alignment: .top, spacing: 12) {
                                    Text(n).shellStyle(.sub, weight: .semibold).foregroundStyle(tin.ink).frame(maxWidth: .infinity, alignment: .leading)
                                    Text(l).shellStyle(.sub).foregroundStyle(tin.ink2).multilineTextAlignment(.trailing)
                                }
                            }
                        }
                        .padding(.leading, 50).padding(.trailing, 16).padding(.top, 4).padding(.bottom, 12)
                        ShellPfAct(tr("settings_licence_texts"), glyph: .file, chevron: true, action: onLicences)
                    }
                    ShellPfAct(tr("settings_source"), glyph: .code, sub: tr("settings_source_sub"), chevron: true) { ShellURL.open(ShellURL.source) }
                }
                ShellPfNote(tr("settings_about_note"))
            }
            ShellPfSec {
                ShellPfCard {
                    ShellPfAct(tr("settings_terms"), glyph: .file, chevron: true) { ShellURL.open(ShellURL.terms) }
                    ShellPfAct(tr("settings_privacy_policy"), glyph: .shield, chevron: true) { ShellURL.open(ShellURL.privacy) }
                    ShellPfAct("tinline.osvauld.com", glyph: .globe, chevron: true) { ShellURL.open(ShellURL.site) }
                }
                ShellPfNote(tr("settings_not_emergency"))
            }
            // TODO(ios-device): "Copy debug log" / "Send debug log" (android DebugLog over logcat) need an OSLogStore reader; left out.
        }
    }
}

struct ShellLicencesScreen: View {
    let onBack: () -> Void
    @Environment(\.tin) private var tin
    static let libs = [
        "Tinline \u{2014} GPL-3.0-or-later", "iroh and the Rust crates in the core \u{2014} Apache-2.0 / MIT", "Opus \u{2014} BSD-3-Clause",
        "libaom AV1 encoder \u{2014} BSD-2-Clause + AOMedia Patent License 1.0", "rav1d AV1 decoder \u{2014} BSD-2-Clause",
        "BIP-39 English word list \u{2014} MIT",
    ]
    /// Bundled licence texts (android assets/licenses); a missing file shows its title only.
    static let texts = [
        ("libaom-LICENSE", "libaom (BSD-2-Clause)"), ("libaom-PATENTS", "libaom (Alliance for Open Media Patent License 1.0)"),
        ("rav1d-COPYING", "rav1d (BSD-2-Clause)"),
        ("Figtree-OFL", "Figtree (SIL Open Font License 1.1)"), ("IBMPlexMono-OFL", "IBM Plex Mono (SIL Open Font License 1.1)"),
    ]
    var body: some View {
        ShellPage {
            ShellTopBar(tr("settings_licences"), onBack: onBack)
            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    ForEach(Self.libs, id: \.self) { Text($0).font(ShellFont.figtree(15)).foregroundStyle(tin.ink) }
                    ForEach(Self.texts, id: \.0) { f, t in
                        Text(t).font(TinType.titleM).foregroundStyle(tin.ink).padding(.top, 12)
                        Text(Bundle.main.url(forResource: f, withExtension: "txt").flatMap { try? String(contentsOf: $0, encoding: .utf8) } ?? "")
                            .font(ShellFont.mono(11, .regular)).foregroundStyle(tin.ink2)
                    }
                }
                .padding(.horizontal, 20).padding(.vertical, 8)
            }
        }
    }
}

/// Diagnostics (debug builds): the 440 Hz test tone and the core version.
struct ShellDiagnosticsScreen: View {
    let core: ShellCore
    let onBack: () -> Void
    @State private var tone = false
    var body: some View {
        ShellPage {
            ShellTopBar(tr("settings_diagnostics"), onBack: onBack)
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    ShellListItem(title: tr("settings_test_tone"), sub: tr("settings_test_tone_sub")) {
                        ShellSwitch(isOn: tone, label: tr("settings_test_tone")) { on in tone = on; core.node?.setTestTone(hz: on ? 440 : nil) }
                    }
                    ShellHint("p2pcore \(coreVersion())").padding(.horizontal, 20).padding(.vertical, 8)
                }
            }
        }
    }
}
