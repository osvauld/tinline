import SwiftUI
import PhotosUI

/*
 * My profile (playground screens/profile/MyProfile.svelte, route `#/me`; android ProfileScreens.kt): picture, name, status, about,
 * availability and additional information. Each piece saves on its own and patches one key (ProfileOp -> one core call). Accepted
 * state lives in the profile controller; drafts live in the rows. Layers: PictureFlow (+ Cropper), Remove picture, AddInfo, OpenLink.
 * The Android debug-gallery preview states (MyProfilePreview) are not ported.
 */

private enum ProfileLayer: Identifiable {
    case picture(Picked)
    case remove
    case add(String?)
    case link(String)

    var id: String {
        switch self {
        case .picture(let p): return "picture-\(p.id)"
        case .remove: return "remove"
        case .add: return "add"
        case .link(let u): return "link-\(u)"
        }
    }
}

/// Public root: the signed-in person's own profile. The Shell injects `PeopleModel` (which owns the `ProfilesController`).
struct ProfileScreen: View {
    @EnvironmentObject private var people: PeopleModel
    var fallbackName = ""
    var title = "My profile"
    let onBack: () -> Void
    var body: some View {
        ProfileScreenContent(store: people.profiles, fallbackName: fallbackName, title: title, onBack: onBack)
            .id(people.profiles.selfDid)   // a different account is a fresh screen: drafts and layers reset (Android `remember(me)`)
    }
}

private struct ProfileScreenContent: View {
    @Environment(\.tin) private var c
    @ObservedObject var store: ProfilesController
    let fallbackName: String
    let title: String
    let onBack: () -> Void

    @State private var layer: ProfileLayer?
    @State private var picError = ""
    @State private var removeError = ""
    /// px.pending: saves accepted while offline, waiting for the network. No per-contact delivery state exists.
    @State private var pending = 0
    @State private var photoItem: PhotosPickerItem?
    @State private var pickerOpen = false

    private var me: String? { store.selfDid }
    private var view: ProfileView? { me.flatMap { store.views[$0] } }
    private var fields: PeopleObject { view?.fields ?? PeopleObject() }
    private var offline: Bool { !store.online }
    private var name: String {
        if let t = view?.text("name"), !t.trimmingCharacters(in: .whitespaces).isEmpty { return t }
        return fallbackName
    }

    private func announce(_ s: String) { UIAccessibility.post(notification: .announcement, argument: s) }

    private func accepted(_ r: String?) -> String? { if r == nil && !store.online { pending += 1 }; return r }
    private func op(_ o: ProfileOp) async -> String? { accepted(await store.apply(o)) }

    /// profile.ts setField: the playground's checks first (same messages), then `set_profile_field(key, json?)`.
    private func setField(_ key: String, _ value: String?) async -> String? {
        if let m = ProfileRules.checkField(key: key, value: value, fields: fields) { return m }
        if key == "name" { return await op(.setName(value!.trimmingCharacters(in: .whitespacesAndNewlines))) }
        if value == nil && !fields.has(key) { return nil }
        return await op(.setField(key: key, json: value.map { PeopleJSON.quote($0) }))
    }
    private func saveName(_ v: String) async -> String? {
        if let m = ProfileRules.checkName(v) { return m }
        return await op(.setName(v.trimmingCharacters(in: .whitespacesAndNewlines)))
    }
    /* ---- field saves (MyProfile.svelte optionalText / saveLink / drop) ---- */
    private func optionalText(_ key: String) -> (String) async -> String? {
        { v in let t = v.trimmingCharacters(in: .whitespacesAndNewlines); return await setField(key, t.isEmpty ? nil : t) }
    }
    private func saveLink(_ key: String) -> (String) async -> String? {
        { v in
            if v.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { return await setField(key, nil) }
            guard let n = ProfileInfo.normaliseLink(v) else { return "Enter a web address that starts with http:// or https://." }
            return await setField(key, n)
        }
    }
    private func drop(_ key: String) -> () async -> String? { { await setField(key, nil) } }

    private func choose(_ item: PhotosPickerItem) {
        picError = ""
        Task {
            guard let data = try? await item.loadTransferable(type: Data.self) else {
                picError = "That file couldn\u{2019}t be read as a picture. It may be damaged."; return
            }
            let r = await Task.detached { inspectImage(data) }.value
            switch r { case .refused(let m): picError = m; case .ok(let p): layer = .picture(p) }
        }
    }

    var body: some View {
        let extras = ProfileInfo.extraKeys(fields)
        let hasPic = view?.hash != nil
        let hasOther = extras.contains { ProfileInfo.known[$0] == nil }
        PeopleProfilePage(title: title, onBack: onBack, bottomPad: 96, overlay: { floating }) {
            hero(hasPic: hasPic)

            PeopleSection(title: "Profile") {
                PeopleCard {
                    PeopleFieldRow(label: "Name", value: .string(name), limit: ProfileRules.nameLimit, offline: offline, placeholder: "Your name",
                                   onSave: saveName)
                    PeopleHairline()
                    PeopleFieldRow(label: "Status", value: fields["status"], optional: true, limit: ProfileRules.textLimit, offline: offline,
                                   empty: "Add a status", placeholder: "What are you up to?", onSave: optionalText("status"), onRemove: drop("status"))
                    PeopleHairline()
                    PeopleFieldRow(label: "About", value: fields["about"], kind: .multiline, optional: true, limit: ProfileRules.textLimit, offline: offline,
                                   empty: "Add a few words about you", onSave: optionalText("about"), onRemove: drop("about"))
                }
            }

            PeopleSection(title: "Availability") {
                PeopleCard { PeopleAvailabilityCard(fields: fields, offline: offline) { a, until in await op(.setAvailable(a, untilSecs: until)) } }
                PeopleNote("Your contacts can see this, and it\u{2019}s the same on all your devices. It affects whether calls reach you, and it\u{2019}s separate from whether you\u{2019}re online.")
            }

            PeopleSection(title: "Additional information") {
                PeopleCard {
                    ForEach(extras, id: \.self) { k in
                        let known = ProfileInfo.known[k]
                        PeopleFieldRow(label: ProfileInfo.labelOf(k), value: fields[k], kind: known?.link == true ? .link : .text, hint: known?.hint ?? "",
                                       optional: true, limit: ProfileRules.textLimit, offline: offline,
                                       onSave: known?.link == true ? saveLink(k) : optionalText(k), onRemove: drop(k), onOpen: { layer = .link($0) })
                            .id("\(me ?? "")-\(k)")
                        PeopleHairline()
                    }
                    // `.pf-act { border: 0 }`: the action row runs on from the card's rows.
                    PeopleAct(title: "Add information", action: { layer = .add(nil) }, icon: .plus)
                }
                PeopleNote(fieldHelp)
                if hasOther { PeopleNote("Some information came from another device or app version. It\u{2019}s shown as stored and kept when you edit anything else.") }
            }
        }
        .onChange(of: store.online) { _, on in if on { pending = 0 } }
        .onChange(of: photoItem) { _, item in if let item { choose(item); photoItem = nil } }
        .photosPicker(isPresented: $pickerOpen, selection: $photoItem, matching: .images)
        .peopleSheet(item: $layer) { l in layerContent(l, hasPic: hasPic) }
    }

    // MARK: hero

    private func hero(hasPic: Bool) -> some View {
        VStack(spacing: 0) {
            // Tap-to-view-large is the Components agent's avatar; here the avatar is the picture itself.
            TinAvatar(did: me ?? "", size: 112)
            HStack(spacing: 8) {
                if hasPic {
                    PeopleButton("Change photo", icon: .camera, minWidth: 140) { pickerOpen = true }
                    PeopleButton("Remove", minWidth: 140) { removeError = ""; layer = .remove }
                } else {
                    PeopleButton("Add photo", icon: .camera, minWidth: 140) { pickerOpen = true }
                }
            }
            .padding(.top, 18)
        }
        .padding(.horizontal, 20).padding(.top, 24).padding(.bottom, 4).frame(maxWidth: .infinity)
        .accessibilityElement(children: .contain).accessibilityLabel("Picture")
    }

    /// Floating, not in the page flow: these appear on their own and must not move the page.
    private var floating: some View {
        VStack {
            Spacer()
            if !picError.isEmpty {
                HStack(spacing: 8) {
                    Text(picError).peopleStyle(.sub19).foregroundStyle(c.peopleDanger).frame(maxWidth: .infinity, alignment: .leading)
                    PeopleIconButton(icon: .x, label: "Dismiss", tint: c.peopleDanger, iconSize: 18) { picError = "" }
                }
                .padding(.leading, 12).padding(.trailing, 4).padding(.vertical, 4)
                .background(RoundedRectangle(cornerRadius: 12, style: .circular).fill(c.erc)).peopleDepth(.pop)
                .frame(maxWidth: 520).accessibilityAddTraits(.updatesFrequently).transition(.opacity)
            } else if pending > 0 {
                PeopleStrip(text: "Saved on this device. Will sync when you\u{2019}re connected.", icon: .clock, floating: true)
                    .frame(maxWidth: 520).transition(.opacity)
            }
        }
        .padding(.horizontal, 16).padding(.bottom, 16)
        .peopleAnimation(PeopleMotion.crossfade, value: picError.isEmpty ? (pending > 0 ? 1 : 0) : 2)
        .allowsHitTesting(!picError.isEmpty)
    }

    // MARK: layers

    @ViewBuilder private func layerContent(_ l: ProfileLayer, hasPic: Bool) -> some View {
        switch l {
        case .picture(let picked):
            PeoplePictureFlow(picked: picked, replacing: hasPic, onClose: { layer = nil }) { png in
                if png.count > ImageLimits.outMaxBytes { return "That picture is too detailed to save. Try a different one." }
                let r = await op(.replacePicture(png))
                if r == nil { layer = nil; announce("Picture updated") }
                return r
            }
        case .remove:
            PeopleDialogBody(title: "Remove your picture?") {
                PeopleDialogText("Your initials will show instead.", lead: true)
                PeopleDialogText(pictureHelp)
                if !removeError.isEmpty { PeopleErr(text: removeError) }
                PeopleActs {
                    PeopleButton("Cancel", variant: .quiet) { layer = nil }
                    PeopleButton("Remove picture", variant: .danger) {
                        Task {
                            let r = await op(.removePicture)
                            if r == nil { layer = nil; removeError = ""; announce("Picture removed") } else { removeError = r! }
                        }
                    }
                }
            }
        case .add(let pick):
            PeopleAddInfo(fields: fields, startPick: pick, onClose: { layer = nil }) { k, v in await setField(k, v) }
        case .link(let url):
            PeopleOpenLink(url: url) { layer = nil }
        }
    }
}

// MARK: - Availability.svelte

/// Available | Busy segmented control (radio group), the line under it (Available / Busy until ... / the refusal) which always holds
/// its 19 pt and carries the Saved tag at its end, and, when busy, the "for how long" chips. One account-wide setting, separate
/// from being online. A deadline that passes reads Available again.
struct PeopleAvailabilityCard: View {
    @Environment(\.tin) private var c
    let fields: PeopleObject
    let offline: Bool
    let save: (_ available: Bool, _ untilSecs: Int64?) async -> String?

    @State private var now = ProfileInfo.nowMillis()
    @State private var choice = "1h"
    @State private var error = ""
    @State private var saved = 0

    var body: some View {
        let av = effectiveAvailability(fields, nowMs: now)
        let active: String? = av.available ? nil : av.untilMs == nil ? "open" : choice
        let line = av.available ? "Available" : av.untilMs.map { "Busy until \(ProfileInfo.untilLabel(atMs: $0))" } ?? "Busy until you change it"
        VStack(alignment: .leading, spacing: 10) {
            PeopleSegmented(options: ["Available", "Busy"], selected: av.available ? 0 : 1, label: "Availability") { i in
                if i == 0 { apply(true, nil) } else if av.available { setBusy(choice) }
            }
            HStack(spacing: 10) {
                Group {
                    if !error.isEmpty { Text(error).peopleStyle(.sub19).foregroundStyle(c.peopleDanger) }
                    else { Text(line).peopleStyle(.sub19, tnum: true).foregroundStyle(c.ink) }
                }
                .lineLimit(1)
                Spacer(minLength: 0)
                if saved > 0 { PeopleSavedTag(offline: offline) }
            }
            .frame(height: 19).accessibilityElement(children: .combine).accessibilityAddTraits(.updatesFrequently)
            if !av.available {
                PeopleFlow(spacing: 8) {
                    ForEach(busyChoices, id: \.id) { b in PeopleChip(text: b.label, pressed: active == b.id, minHeight: 36) { setBusy(b.id) } }
                }
                .accessibilityLabel("For how long")
            }
        }
        .padding(16).padding(.vertical, -2)
        .task(id: av) {
            if let until = av.untilMs {
                let wait = max(0, until - ProfileInfo.nowMillis()) + 50
                try? await Task.sleep(nanoseconds: UInt64(wait) * 1_000_000)
                now = ProfileInfo.nowMillis()
            }
        }
        .task(id: saved) { if saved > 0 { try? await Task.sleep(nanoseconds: savedMs * 1_000_000); saved = 0 } }
    }

    private func apply(_ available: Bool, _ untilMs: Int64?) {
        if !available, let untilMs, untilMs <= ProfileInfo.nowMillis() { error = "Choose a time that hasn\u{2019}t passed yet."; return }
        Task {
            let r = await save(available, untilMs.map { $0 / 1000 })
            if let r { error = r } else { error = ""; saved += 1; now = ProfileInfo.nowMillis() }
        }
    }
    private func setBusy(_ id: String) {
        choice = id
        apply(false, busyChoices.first { $0.id == id }?.until(ProfileInfo.nowMillis()) ?? nil)
    }
}

/// Wrapping row of views (Compose FlowRow).
struct PeopleFlow: Layout {
    var spacing: CGFloat = 8
    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        arrange(proposal.width ?? .infinity, subviews).size
    }
    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let r = arrange(bounds.width, subviews)
        for (i, f) in r.frames.enumerated() { subviews[i].place(at: CGPoint(x: bounds.minX + f.minX, y: bounds.minY + f.minY), proposal: ProposedViewSize(f.size)) }
    }
    private func arrange(_ width: CGFloat, _ subviews: Subviews) -> (size: CGSize, frames: [CGRect]) {
        var x: CGFloat = 0, y: CGFloat = 0, rowH: CGFloat = 0, maxX: CGFloat = 0
        var frames: [CGRect] = []
        for s in subviews {
            let sz = s.sizeThatFits(.unspecified)
            if x > 0 && x + sz.width > width { x = 0; y += rowH + spacing; rowH = 0 }
            frames.append(CGRect(origin: CGPoint(x: x, y: y), size: sz))
            x += sz.width + spacing; rowH = max(rowH, sz.height); maxX = max(maxX, x - spacing)
        }
        return (CGSize(width: maxX, height: y + rowH), frames)
    }
}

// MARK: - AddInfo.svelte

private struct PeopleAddInfo: View {
    @Environment(\.tin) private var c
    let fields: PeopleObject
    let startPick: String?
    let onClose: () -> Void
    let save: (_ key: String, _ value: String) async -> String?

    /// A KNOWN key, or "" for something else; nil is the chooser.
    @State private var pick: String?
    @State private var name = ""
    @State private var v = ""
    @State private var error = ""

    init(fields: PeopleObject, startPick: String?, onClose: @escaping () -> Void, save: @escaping (String, String) async -> String?) {
        self.fields = fields; self.startPick = startPick; self.onClose = onClose; self.save = save
        _pick = State(initialValue: startPick)
    }

    private var have: Set<String> { Set(fields.keys) }
    private var key: String {
        if let p = pick, !p.isEmpty { return p }
        return name.trimmingCharacters(in: .whitespacesAndNewlines).lowercased().replacingOccurrences(of: "\\s+", with: "-", options: .regularExpression)
    }
    private var link: Bool { pick.flatMap { ProfileInfo.known[$0]?.link } == true }
    private let icons: [String: PeopleIcon] = ["pronouns": .users, "location": .info, "website": .link]

    private func submit() {
        if pick == "" {
            if let m = ProfileRules.checkKey(name) { error = m; return }
            if ProfileRules.reserved.contains(key) || ProfileInfo.known[key] != nil || have.contains(key) { error = "You already have information with that name."; return }
        }
        var value: String? = v.trimmingCharacters(in: .whitespacesAndNewlines)
        if value?.isEmpty != false { error = "Add something to save."; return }
        if link {
            value = ProfileInfo.normaliseLink(v)
            if value == nil { error = "Enter a web address that starts with http:// or https://."; return }
        }
        let k = key, val = value!
        Task { let r = await save(k, val); if r == nil { onClose() } else { error = r! } }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if let p = pick {
                PeopleDialogTitle(p.isEmpty ? "Something else" : ProfileInfo.labelOf(p))
                VStack(alignment: .leading, spacing: 6) {
                    if p.isEmpty {
                        Text("What is it called?").peopleStyle(.sub, weight: .semibold).foregroundStyle(c.ink2).padding(.top, 6)
                        PeopleInput(text: $name, label: "What is it called?", placeholder: "For example, Favourite tea", focus: true, onSubmit: submit)
                    }
                    Text(p.isEmpty ? "Value" : ProfileInfo.labelOf(p)).peopleStyle(.sub, weight: .semibold).foregroundStyle(c.ink2).padding(.top, 6)
                    PeopleInput(text: $v, label: p.isEmpty ? "Value" : ProfileInfo.labelOf(p), placeholder: ProfileInfo.known[p]?.hint ?? "",
                                link: link, focus: !p.isEmpty, plain: true, onSubmit: submit)
                    if !error.isEmpty { PeopleErr(text: error).padding(.top, -10) }
                }
                .padding(.top, 12)
                PeopleActs {
                    PeopleButton("Back", variant: .quiet) { pick = nil; error = "" }
                    PeopleButton("Add", variant: .primary, action: submit)
                }
            } else {
                PeopleDialogTitle("Add information")
                PeopleDialogText("Your contacts will see it on your profile.")
                VStack(spacing: 0) {
                    ForEach(ProfileInfo.knownKeys.filter { !have.contains($0) }, id: \.self) { k in
                        PeopleAct(title: ProfileInfo.labelOf(k), action: { pick = k }, icon: icons[k] ?? .info, padH: 12)
                    }
                    PeopleAct(title: "Something else", action: { pick = "" }, icon: .plus, sub: "Name it yourself", padH: 12)
                }
                .padding(.top, 10).padding(.horizontal, -12).accessibilityLabel("Kinds of information")
                PeopleActs { PeopleButton("Cancel", variant: .quiet, action: onClose) }
            }
        }
        .padding(.horizontal, 20).padding(.top, 20).padding(.bottom, 16)
    }
}

// MARK: - OpenLink.svelte

private struct PeopleOpenLink: View {
    @Environment(\.tin) private var c
    @Environment(\.openURL) private var openURL
    let url: String
    let onClose: () -> Void
    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            PeopleDialogTitle("Open this link?")
            Text(url).font(.system(size: 13, design: .monospaced)).foregroundStyle(c.ink)
                .environment(\.layoutDirection, .leftToRight).frame(maxWidth: .infinity, alignment: .leading).padding(.top, 8)
            PeopleDialogText("It opens in your browser, outside Tinline. Tinline doesn\u{2019}t load it for you, and can\u{2019}t vouch for what\u{2019}s there.")
            PeopleActs {
                PeopleButton("Cancel", variant: .quiet, action: onClose)
                PeopleButton("Open link", variant: .primary) { if let u = URL(string: url) { openURL(u) }; onClose() }
            }
        }
        .padding(.horizontal, 20).padding(.top, 20).padding(.bottom, 16)
    }
}

// MARK: - PictureFlow.svelte + Cropper.svelte

private let cropView: CGFloat = 264
private let cropSizes: [CGFloat] = [72, 48, 40]

private struct PeoplePictureFlow: View {
    @Environment(\.tin) private var c
    let picked: Picked
    let replacing: Bool
    let onClose: () -> Void
    let use: (Data) async -> String?

    @State private var crop: Crop
    @State private var busy = false
    @State private var error = ""

    init(picked: Picked, replacing: Bool, onClose: @escaping () -> Void, use: @escaping (Data) async -> String?) {
        self.picked = picked; self.replacing = replacing; self.onClose = onClose; self.use = use
        _crop = State(initialValue: centred(picked.w, picked.h, view: cropView))
    }

    var body: some View {
        VStack(spacing: 0) {
            Text("Position your picture").peopleStyle(.title).foregroundStyle(c.ink).accessibilityAddTraits(.isHeader)
                .frame(maxWidth: .infinity, alignment: .leading).padding(.bottom, 14)
            PeopleCropper(picked: picked, crop: $crop)
            HStack(alignment: .bottom, spacing: 14) {
                ForEach(cropSizes, id: \.self) { px in PeopleCropPreview(picked: picked, crop: crop, px: px) }
            }
            .padding(.top, 6).accessibilityHidden(true)
            if replacing { Text(pictureHelp).peopleStyle(.sub19).foregroundStyle(c.ink2).frame(maxWidth: .infinity, alignment: .leading).padding(.top, 14) }
            if !error.isEmpty { PeopleErr(text: error) }
            PeopleActs {
                PeopleButton("Cancel", variant: .quiet, action: onClose)
                PeopleButton("Use photo", variant: .primary, enabled: !busy) {
                    guard !busy else { return }
                    busy = true; error = ""
                    let p = picked, cr = crop
                    Task {
                        let png = await Task.detached { renderSquare(p, cr, view: cropView) }.value
                        if let png { error = await use(png) ?? "" } else { error = "Couldn\u{2019}t prepare that picture. Try a different one." }
                        busy = false
                    }
                }
            }
        }
        .padding(.horizontal, 20).padding(.top, 20).padding(.bottom, 16)
    }
}

/// Cropper: drag to move, the slider zooms (1..4). The window is the preview.
private struct PeopleCropper: View {
    @Environment(\.tin) private var c
    let picked: Picked
    @Binding var crop: Crop
    @State private var last = CGSize.zero

    private func move(_ dx: CGFloat, _ dy: CGFloat) { crop = clampCrop(Crop(ox: crop.ox + dx, oy: crop.oy + dy, zoom: crop.zoom), picked.w, picked.h, view: cropView) }
    private func zoomTo(_ z: CGFloat) { crop = zoomAt(crop, min(4, max(1, z)), picked.w, picked.h, view: cropView) }

    var body: some View {
        let s = coverScale(picked.w, picked.h, view: cropView, zoom: crop.zoom)
        VStack(spacing: 12) {
            ZStack(alignment: .topLeading) {
                Image(uiImage: picked.image).resizable()
                    .frame(width: CGFloat(picked.w) * s, height: CGFloat(picked.h) * s).offset(x: crop.ox, y: crop.oy)
                // The round window: everything outside it dimmed, a thin light edge where the circle ends.
                Path { p in
                    let r = CGRect(x: 0, y: 0, width: cropView, height: cropView)
                    p.addRect(r); p.addEllipse(in: r)
                }
                .fill(peopleScrim, style: FillStyle(eoFill: true))
                Circle().stroke(Color.white.opacity(0.85), lineWidth: 1.5).frame(width: cropView - 1.5, height: cropView - 1.5).offset(x: 0.75, y: 0.75)
            }
            .frame(width: cropView, height: cropView, alignment: .topLeading).background(c.sf2)
            .clipShape(RoundedRectangle(cornerRadius: 14, style: .circular))
            .contentShape(Rectangle())
            .gesture(DragGesture(minimumDistance: 0)
                .onChanged { v in move(v.translation.width - last.width, v.translation.height - last.height); last = v.translation }
                .onEnded { _ in last = .zero })
            .accessibilityElement(children: .ignore).accessibilityLabel("Position your picture")
            .accessibilityAdjustableAction { dir in zoomTo(crop.zoom + (dir == .increment ? 0.1 : -0.1)) }
            Text("Drag to move the picture. Use the slider to zoom.").peopleStyle(.sub19).foregroundStyle(c.ink2).multilineTextAlignment(.center)
            HStack(spacing: 10) {
                PeopleZoomGlyph(px: 16)
                Slider(value: Binding(get: { Double(crop.zoom) }, set: { zoomTo(CGFloat($0)) }), in: 1...4).tint(c.peopleAccent).accessibilityLabel("Zoom")
                PeopleZoomGlyph(px: 22)
            }
            .frame(maxWidth: 260)
        }
    }
}

private struct PeopleZoomGlyph: View {
    @Environment(\.tin) private var c
    let px: CGFloat
    var body: some View {
        RoundedRectangle(cornerRadius: 3 * px / 24).stroke(c.ink2, lineWidth: 1.8 * px / 24)
            .frame(width: 18 * px / 24, height: 16 * px / 24).frame(width: px, height: px)
    }
}

/// A round preview at `px`: the same crop drawn at that size, with the avatar's hairline.
private struct PeopleCropPreview: View {
    @Environment(\.tin) private var c
    let picked: Picked
    let crop: Crop
    let px: CGFloat
    var body: some View {
        let r = px / cropView
        let s = coverScale(picked.w, picked.h, view: cropView, zoom: crop.zoom)
        ZStack(alignment: .topLeading) {
            Image(uiImage: picked.image).resizable().frame(width: CGFloat(picked.w) * s * r, height: CGFloat(picked.h) * s * r).offset(x: crop.ox * r, y: crop.oy * r)
        }
        .frame(width: px, height: px, alignment: .topLeading).background(c.sf2).clipShape(Circle())
        .overlay(Circle().stroke(c.dark ? Color.white.opacity(0.12) : Color.black.opacity(0.1), lineWidth: 1))
    }
}
