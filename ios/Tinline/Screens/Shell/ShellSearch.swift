import SwiftUI

/*
 * Search (screens/search/, docs/search.md; android Search.kt): a full-screen search opened from the dock's pill, with the field at
 * the BOTTOM (thumb reach). Results: "People & groups" (one fixed-height strip), then "Messages" (fixed 64 pt rows, newest first,
 * 20 a page). Rows are keyed by position, so a new query changes the rows' text instead of pushing them around; hints are
 * overlays in reserved space. Names from 1 character, messages from 2; 150 ms debounce. Wired to Node.search. Recents live only
 * for this screen; message queries are never stored.
 */

struct ShellSearchScreen: View {
    let onBack: () -> Void
    /// A chat (and the message hit to jump to, if a message was opened).
    let onChat: (String, MessageHit?) -> Void
    let onGroup: (String) -> Void
    let onContact: (String) -> Void
    @EnvironmentObject private var app: AppEnv
    @Environment(\.tin) private var tin
    @Environment(\.accessibilityReduceMotion) private var reduced
    @State private var q = ""
    @State private var applied = ""
    @State private var recents: [String] = []
    @State private var names: [NameHit] = []
    @State private var msgs: [MessageHit] = []
    @State private var next: String?
    @State private var doneFor: String?
    @State private var paging = false
    @State private var error: String?
    @State private var shownHint = ""
    @FocusState private var focused: Bool

    private var t: String { applied.trimmingCharacters(in: .whitespacesAndNewlines) }
    private var msgsOk: Bool { ShellMarked.codePoints(t) >= ShellSearchConst.minChars }

    private func keepRecent() {
        let s = q.trimmingCharacters(in: .whitespaces)
        guard ShellMarked.codePoints(s) >= ShellSearchConst.minChars else { return }
        recents = Array(([s] + recents.filter { $0.caseInsensitiveCompare(s) != .orderedSame }).prefix(8))
    }

    private func openName(_ n: NameHit) {
        keepRecent()
        switch n.kind {
        case .me, .contact: onChat(n.id, nil)
        case .group: onGroup(n.id)
        }
    }

    private func openHit(_ h: MessageHit) {
        keepRecent()
        if h.kind == .group { app.searchJump = h; onGroup(h.conv) } else { onChat(h.conv, h) }
    }

    private func run() async {
        error = nil
        let t = self.t
        if t.isEmpty { names = []; msgs = []; next = nil; doneFor = t; return }
        guard let node = app.node else { return }
        let ok = ShellMarked.codePoints(t) >= ShellSearchConst.minChars
        do {
            let r = try await shellIO { try node.search(query: t, limit: ok ? ShellSearchConst.limit : 1, cursor: nil) }
            guard t == self.t else { return }
            names = r.names; msgs = ok ? r.messages : []; next = ok ? r.next : nil
        } catch {
            self.error = ShellText.friendly(error)
            names = []; msgs = []; next = nil
        }
        doneFor = t
    }

    /// Next page when the end of the list is near.
    private func page(at i: Int) {
        guard i >= msgs.count - 4, let cur = next, !paging, doneFor == t, error == nil, let node = app.node else { return }
        paging = true
        let t = self.t
        Task {
            defer { paging = false }
            do {
                let r = try await shellIO { try node.search(query: t, limit: ShellSearchConst.limit, cursor: cur) }
                guard doneFor == t else { return }
                var seen = Set(msgs.map { "\($0.kind)|\($0.conv)|\($0.id)" })
                msgs += r.messages.filter { seen.insert("\($0.kind)|\($0.conv)|\($0.id)").inserted }
                next = r.next
            } catch { self.error = ShellText.friendly(error) }
        }
    }

    var body: some View {
        let me = app.accountDid
        let hint: String = {
            if let error { return error }
            if t.isEmpty { return "" }
            if !msgsOk { return tr("shell_search_min_chars") }
            if doneFor == t && msgs.isEmpty { return tr("shell_search_no_messages", t) }
            return ""
        }()
        let noNames = error == nil && doneFor == t && !t.isEmpty && names.isEmpty
        VStack(spacing: 0) {
            ZStack {
                if q.trimmingCharacters(in: .whitespaces).isEmpty {
                    ShellRecentResults(recents: recents, onPick: { q = $0; applied = $0 }, onClear: { recents = [] })
                } else {
                    VStack(spacing: 0) {
                        ShellResultsHeader(title: tr("shell_search_people_groups"))
                        ZStack {
                            ScrollView(.horizontal, showsIndicators: false) {
                                LazyHStack(spacing: 2) {
                                    ForEach(0..<names.count, id: \.self) { i in ShellNameTile(n: names[i], q: t) { openName(names[i]) } }
                                }
                                .padding(.horizontal, 8).padding(.bottom, 8)
                            }
                            Text(tr("shell_search_no_names")).shellStyle(.sub).foregroundStyle(tin.ink2).opacity(noNames ? 1 : 0)
                        }
                        .frame(height: 100)
                        ShellResultsHeader(title: tr("shell_search_messages"))
                        ZStack {
                            ScrollViewReader { proxy in
                                ScrollView {
                                    LazyVStack(spacing: 0) {
                                        ForEach(0..<msgs.count, id: \.self) { i in
                                            ShellMessageRow(h: msgs[i], me: me) { openHit(msgs[i]) }.id(i).onAppear { page(at: i) }
                                        }
                                    }
                                    .padding(.horizontal, 8).padding(.bottom, 12)
                                }
                                .onChange(of: t) { _, _ in proxy.scrollTo(0, anchor: .top) }
                            }
                            Text(shownHint).shellStyle(.sub19).foregroundStyle(tin.ink2).multilineTextAlignment(.center).padding(.horizontal, 32)
                                .opacity(hint.isEmpty ? 0 : 1)
                        }
                        .frame(maxHeight: .infinity)
                        .animation(reduced ? nil : .linear(duration: TinMotion.crossfade), value: hint.isEmpty)
                    }
                }
            }
            .padding(.top, 12).frame(maxHeight: .infinity)
            // The field at the bottom: Back + the 48 pt field, on the surface, a hairline above.
            VStack(spacing: 0) {
                ShellPalette(tin).hairline.frame(height: 1)
                HStack(spacing: 4) {
                    ShellIconButton(glyph: .back, label: tr("shell_back")) { focused = false; onBack() }
                    ShellSearchField(value: $q, focused: $focused, onSubmit: {
                        if let n = names.first { openName(n) } else if let m = msgs.first { openHit(m) } else { keepRecent() }
                    })
                }
                .padding(.leading, 4).padding(.trailing, 12).padding(.vertical, 8)
            }
            .background(tin.sf.ignoresSafeArea(edges: .bottom))
        }
        .background(tin.bg.ignoresSafeArea())
        .task(id: q) {
            if q.trimmingCharacters(in: .whitespaces).isEmpty { applied = ""; return }
            try? await Task.sleep(nanoseconds: ShellSearchConst.debounceMs * 1_000_000)
            if !Task.isCancelled { applied = q }
        }
        .task(id: "\(t)|\(app.chats.count)") { await run() }
        .onChange(of: hint, initial: true) { _, h in if !h.isEmpty { shownHint = h } }
        .onAppear { focused = true }
    }
}

/// SearchField.svelte: 48 tall pill on surface-2; focused = surface with a 2 px accent ring. The clear button stays in the
/// layout and fades.
struct ShellSearchField: View {
    @Binding var value: String
    var focused: FocusState<Bool>.Binding
    var height: CGFloat = 48
    let onSubmit: () -> Void
    @Environment(\.tin) private var tin
    @Environment(\.accessibilityReduceMotion) private var reduced
    var body: some View {
        let p = ShellPalette(tin), on = focused.wrappedValue
        HStack(spacing: 0) {
            ShellIcon(glyph: .search, size: 20, color: tin.ink2).frame(width: 44)
            TextField("", text: $value, prompt: Text(tr("shell_search")).foregroundColor(tin.ink2))
                .font(ShellFont.of(.body)).foregroundStyle(tin.ink).tint(p.accentInk)
                .textInputAutocapitalization(.never).autocorrectionDisabled(true).submitLabel(.search).onSubmit(onSubmit)
                .focused(focused).accessibilityLabel(tr("shell_search"))
            ShellIconButton(glyph: .x, label: tr("shell_search_clear"), tint: tin.ink2, iconSize: 18) { value = ""; focused.wrappedValue = true }
                .opacity(value.isEmpty ? 0 : 1).disabled(value.isEmpty).padding(.leading, 2).padding(.trailing, 4)
        }
        .frame(height: height)
        .background(Capsule().fill(on ? tin.sf : tin.sf2))
        .overlay(Capsule().stroke(on ? p.accent : .clear, lineWidth: 2))
        .animation(reduced ? nil : TinMotion.easeOut(TinMotion.switchTime), value: on)
        .animation(reduced ? nil : TinMotion.easeOut(TinMotion.crossfade), value: value.isEmpty)
    }
}

private struct ShellResultsHeader<Action: View>: View {
    let title: String
    @ViewBuilder var action: () -> Action
    var body: some View {
        HStack { ShellListSection(title); Spacer(minLength: 0); action() }.frame(height: 36).padding(.horizontal, 16)
    }
}

extension ShellResultsHeader where Action == EmptyView {
    init(title: String) { self.title = title; self.action = { EmptyView() } }
}

private struct ShellRecentResults: View {
    let recents: [String]
    let onPick: (String) -> Void
    let onClear: () -> Void
    @Environment(\.tin) private var tin
    var body: some View {
        let p = ShellPalette(tin)
        ZStack {
            VStack(spacing: 0) {
                ShellResultsHeader(title: tr("shell_search_recent")) {
                    if !recents.isEmpty {
                        Button(action: onClear) {
                            Text(tr("shell_clear")).shellStyle(.sub, weight: .semibold).foregroundStyle(p.accentInk).padding(.horizontal, 10).padding(.vertical, 12)
                        }
                        .buttonStyle(ShellPressStyle(wash: .clear, radius: 8))
                    }
                }
                VStack(spacing: 0) {
                    ForEach(recents, id: \.self) { r in
                        Button { onPick(r) } label: {
                            HStack(spacing: 12) {
                                ShellIcon(glyph: .clock, size: 18, color: tin.ink2).frame(width: 32)
                                Text(r).shellStyle(.body).foregroundStyle(tin.ink).lineLimit(1).frame(maxWidth: .infinity, alignment: .leading)
                            }
                            .padding(.horizontal, 8).frame(minHeight: 48)
                        }
                        .buttonStyle(ShellPressStyle())
                    }
                }
                .padding(.horizontal, 8)
                Spacer(minLength: 0)
            }
            VStack(spacing: 4) {
                Text(tr("shell_search_empty_title")).shellStyle(.sub19).foregroundStyle(tin.ink2)
                Text(tr("shell_search_empty_sub")).shellStyle(.sub19).foregroundStyle(tin.ink2)
            }
            .multilineTextAlignment(.center).padding(.horizontal, 32).opacity(recents.isEmpty ? 1 : 0)
        }
    }
}

/// Bold hits as one Text (Marked.svelte, variant bold).
func shellMarkedText(_ segs: [ShellMarked.Segment], ink: Color) -> Text {
    segs.reduce(Text("")) { acc, s in acc + (s.bold ? Text(s.text).bold().foregroundColor(ink) : Text(s.text)) }
}

/// A tile in the People & groups strip: 78 x 92, avatar 48, the name in 12/15 over two lines with the match bold.
private struct ShellNameTile: View {
    let n: NameHit
    let q: String
    let action: () -> Void
    @Environment(\.tin) private var tin
    var body: some View {
        let what: String = { switch n.kind { case .group: tr("shell_search_kind_group"); case .me: tr("shell_search_kind_me"); case .contact: tr("shell_search_kind_contact") } }()
        Button(action: action) {
            VStack(spacing: 6) {
                if n.kind == .group { TinGroupAvatar(groupID: n.id, size: 48) } else { ShellAvatar(did: n.id, name: n.name, size: 48) }
                shellMarkedText(ShellMarked.segments(n.name, ShellMarked.nameRanges(n.name, q)), ink: tin.ink)
                    .font(ShellFont.figtree(12, relativeTo: .caption)).foregroundStyle(tin.ink).multilineTextAlignment(.center).lineLimit(2)
            }
            .padding(.horizontal, 4).padding(.top, 6).frame(width: 78, height: 92, alignment: .top)
        }
        .buttonStyle(ShellPressStyle()).accessibilityLabel(tr("shell_search_tile_a11y", n.name, what))
    }
}

/// A message hit: 64 pt fixed; avatar 44; name 600 (20) and time 11 (min 56, end); a one-line snippet with the match kept in view.
private struct ShellMessageRow: View {
    let h: MessageHit
    let me: String?
    let action: () -> Void
    @Environment(\.tin) private var tin
    var body: some View {
        let mine = h.author == me
        let by = h.kind == .group ? tr("shell_search_author_prefix", mine ? tr("shell_you") : ShellText.firstName(ShellText.ifBlank(h.authorName, tr("shell_someone")))) : (mine ? tr("shell_search_author_prefix", tr("shell_you")) : "")
        let snip = h.snippet.replacingOccurrences(of: "\n", with: " ")
        Button(action: action) {
            HStack(alignment: .top, spacing: 12) {
                if h.kind == .group { TinGroupAvatar(groupID: h.conv, size: 44) } else { ShellAvatar(did: h.conv, name: h.convName, size: 44) }
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 8) {
                        Text(h.convName).shellStyle(.body, weight: .semibold).foregroundStyle(tin.ink).lineLimit(1).frame(maxWidth: .infinity, alignment: .leading)
                        Text(ShellText.listTime(Int64(h.at), ShellText.nowMs())).shellStyle(.meta).monospacedDigit().foregroundStyle(tin.ink2)
                            .frame(minWidth: 56, alignment: .trailing)
                    }
                    (Text(by) + shellMarkedText(ShellMarked.segments(snip, ShellMarked.hitRanges(h.highlights), lead: ShellSearchConst.lead), ink: tin.ink))
                        .shellStyle(.sub).foregroundStyle(tin.ink2).lineLimit(1)
                }
            }
            .padding(.horizontal, 8).padding(.vertical, 11).frame(height: 64)
        }
        .buttonStyle(ShellPressStyle())
    }
}
