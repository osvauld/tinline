import Foundation

/*
 * The Shell's pure logic, ported from the Kotlin so the XCTests can run it without a UI or a node: error words, list
 * rows (Chats.kt), call wording (HomeLists.kt), the empty state's copy (EmptyLine.kt),
 * the sign-up order and checks (Onboarding.kt), the 24-word restore rules, the search highlighter (Search.kt) and the
 * recovery reveal policy (RecoveryBackupState.kt). Shared words come from the generated `present` module, as on Android.
 */

enum ShellText {
    /// Friendly text for a core error; never includes secrets (android Support.kt `friendly`).
    static func friendly(_ e: Swift.Error) -> String {
        if let core = e as? Error {
            switch core {
            case .WrongPassphrase: return "That didn\u{2019}t match. Check the spelling and spaces."
            case .WeakPassphrase: return "Type a passphrase, or skip it"
            case .Locked: return "Locked \u{2014} enter your passphrase to unlock"
            case .BadPhrase: return "That recovery phrase is not valid"
            case .AccountExists: return "That account is already on this phone"
            case .InCall: return "Finish your call first"
            default: return core.errorDescription ?? "Something went wrong"
            }
        }
        if let s = e as? ShellFlowError, let d = s.errorDescription { return d }
        return "Something went wrong"
    }

    static func isWrongPassphrase(_ e: Swift.Error) -> Bool { if case .WrongPassphrase? = e as? Error { return true }; return false }
    static func isInCall(_ e: Swift.Error) -> Bool { if case .InCall? = e as? Error { return true }; return false }
    static func isAccountExists(_ e: Swift.Error) -> Bool { if case .AccountExists? = e as? Error { return true }; return false }
    static func isBadPhrase(_ e: Swift.Error) -> Bool { if case .BadPhrase? = e as? Error { return true }; return false }

    /// First word of a name, for "Arjun deleted this message" (android ChatUi.kt `firstName`).
    static func firstName(_ name: String) -> String {
        let t = name.trimmingCharacters(in: .whitespacesAndNewlines)
        let f = t.split(separator: " ", maxSplits: 1).first.map(String.init) ?? ""
        return f.isEmpty ? name : f
    }

    static func display(_ c: Contact) -> String { displayName(alias: c.alias, name: c.name) }

    static func ifBlank(_ s: String, _ other: String) -> String { s.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? other : s }

    /// Chat list time: "12:41", "Yesterday", "Monday", "12 Sep" (`present.listTime`).
    static func listTime(_ ms: Int64, _ nowMs: Int64) -> String {
        let offset = TimeZone.current.secondsFromGMT(for: Date(timeIntervalSince1970: Double(nowMs) / 1000))
        return Tinline.listTime(then: ms / 1000, now: nowMs / 1000, utcOffsetSecs: Int32(offset))
    }

    static func nowMs() -> Int64 { Int64(Date().timeIntervalSince1970 * 1000) }
    static func nowSecs() -> Int64 { Int64(Date().timeIntervalSince1970) }

    /// "2 min ago", "3 hours ago" for a unix-seconds timestamp (`present.ago`).
    static func agoText(_ secs: Int64, now: Int64 = ShellText.nowSecs()) -> String { ago(now: now, then: secs) }

    /// "Outgoing", "Missed", "No answer"...: CallsList.svelte `what()`, over the core's reasons.
    static func callWhat(_ r: CallRecord) -> String {
        if r.durationSecs > 0 { return r.incoming ? "Incoming" : "Outgoing" }
        if r.incoming && r.reason == "unavailable" { return "While not available" }
        if r.incoming { return r.missed ? "Missed" : "Declined" }
        switch r.reason {
        case "no_answer": return "No answer"
        case "declined", "busy": return "Declined"
        case "cancelled": return "Cancelled"
        case "unreachable": return "Couldn\u{2019}t connect"
        default: return "Call"
        }
    }

    /// Storage sizes (SettingsPages.kt `fmtBytes`).
    static func bytes(_ n: Int64) -> String {
        if n >= 1_000_000_000 { return String(format: "%.1f GB", Double(n) / 1e9) }
        if n >= 1_000_000 { return "\(Int((Double(n) / 1e6).rounded())) MB" }
        if n >= 1_000 { return "\(Int((Double(n) / 1e3).rounded())) KB" }
        return "\(n) B"
    }

    /// What to tell the user for a failed link (`LinkEvents.onLinkFailed` reasons).
    static func linkFail(_ reason: String, _ detail: String?) -> String { linkFailText(reason: reason, detail: detail, thisDevice: "phone") }
}

// ---------------------------------------------------------------- chat list rows (Chats.kt, ChatUi.kt)

/// One row of the Chats tab, already worded (android `ChatRowUi`).
struct ShellChatRow: Equatable, Identifiable {
    var did: String
    var name: String
    var preview: String
    var time: String
    var mine: Bool
    var tick: ChatTick
    var unread: Int
    /// The preview is a block state ("Blocked" / "Blocked you"), shown in red.
    var alert = false
    /// A group conversation (the Chats tab lists 1:1 and groups together); `did` is then the group id.
    var group = false
    /// Last activity, unix ms: the list's sort key.
    var atMs: Int64 = 0
    /// My last message's tick (core `chat_list_marks`); nil until read. `markSlot` keeps its room from the start.
    var mark: TickVis?
    var markSlot = false
    /// Identifies the marked message (the glyph animates only when the same message's state changes).
    var markKey = ""
    /// The last message's id: a mark shows only if it is for exactly this message.
    var lastId: String?

    var id: String { (group ? "g" : "d") + did }
}

enum ShellRows {
    /// Chat list rows from the core's conversations. Only conversations that have a message; a contact's name wins over what
    /// they call themselves. "You" (our DID: saved items for our own devices) is always there, pinned first.
    static func chatRows(_ chats: [Chat], _ contacts: [Contact], links: [String: ChatLink], online: Bool, nowMs: Int64,
                         myDid: String?, blocks: Blocks = Blocks()) -> [ShellChatRow] {
        let by = Dictionary(contacts.map { ($0.did, $0) }, uniquingKeysWith: { a, _ in a })
        var out: [ShellChatRow] = []
        if let ch = chats.first(where: { $0.peerDid == myDid }) {
            let at = Int64(ch.lastActivity)
            out.append(ShellChatRow(did: ch.peerDid, name: "You", preview: ShellText.ifBlank(ch.preview, chatSelfSub),
                                    time: at > 0 ? ShellText.listTime(at, nowMs) : "", mine: false, tick: .two, unread: Int(ch.unread), atMs: at))
        }
        for ch in chats where ch.lastActivity > 0 && ch.peerDid != myDid {
            let name = by[ch.peerDid].map(ShellText.display) ?? ShellText.ifBlank(ch.peerName, "Unknown")
            let at = Int64(ch.lastActivity)
            let pending = ch.lastOutgoing && ch.lastDelivery == .pending
            let link = links[ch.peerDid]
            let waiting = pending && (link == .offline || (link == nil && !online))
            // Blocked people stay in the list with a red line, so the state is never hidden.
            let status = blocks.status(ch.peerDid)
            out.append(ShellChatRow(
                did: ch.peerDid, name: name,
                preview: status ?? (waiting ? "Waiting to send \u{2014} \(ShellText.firstName(name)) isn\u{2019}t reachable yet" : ch.preview),
                time: ShellText.listTime(at, nowMs), mine: ch.lastOutgoing,
                // The list only draws the clock; delivered/read marks come from signed receipts in the conversation.
                tick: waiting ? .clock : .one, unread: Int(ch.unread), alert: status != nil, atMs: at, lastId: ch.lastId))
        }
        return out
    }

    /// The chat list's marks (DM and group) onto the rows: a row whose last message is mine keeps the mark's slot.
    static func withMarks(_ rows: [ShellChatRow], _ marks: [ListMark], myDid: String?) -> [ShellChatRow] {
        rows.map { r in
            var r = r
            let slot = r.mine && r.did != myDid
            let m = marks.first { $0.group == r.group && $0.conversationId == r.did }.flatMap { slot && $0.messageId == r.lastId ? $0 : nil }
            r.mark = m?.mark
            r.markSlot = slot
            r.markKey = m?.messageId ?? r.did
            return r
        }
    }

    /// Owner, admin and member are in the group; removed and left keep only an archive (GroupsCore.kt `active`).
    static func groupActive(_ r: GroupRole) -> Bool { r == .owner || r == .admin || r == .member }

    /// A system line in plain words (GroupsCore.kt `sysText`); `me` is our DID.
    static func groupSysText(_ m: GroupMessage, me: String) -> String {
        func who(_ did: String, _ name: String) -> String { did == me ? "You" : ShellText.ifBlank(name, "Someone") }
        let by = who(m.authorDid, m.authorName), sub = who(m.subjectDid, m.subjectName), you = m.subjectDid == me
        switch m.kind {
        case .post: return m.text
        case .joined: return "\(sub) joined"
        case .left: return "\(sub) left"
        case .removed: return you ? "You were removed after 3 admin flags" : "\(sub) was removed after 3 admin flags"
        case .adminGranted: return you ? "\(by) made you an admin" : "\(by) made \(sub) an admin"
        case .adminRevoked: return you ? "\(by) removed you as admin" : "\(by) removed \(sub) as admin"
        case .hostAdded: return you ? "A device of yours now hosts the group" : "A device of \(sub)\u{2019}s now hosts the group"
        case .hostRemoved: return you ? "A device of yours stopped hosting the group" : "A device of \(sub)\u{2019}s stopped hosting the group"
        }
    }

    /// The group row's second line (GroupsCore.kt `preview`).
    static func groupPreview(_ s: GroupSummary, me: String) -> String {
        switch s.myRole {
        case .removed: return "You were removed from this group"
        case .left: return "You left this group"
        default:
            guard let m = s.last else { return "" }
            if m.kind != .post { return groupSysText(m, me: me) }
            if m.outgoing { return "You: \(m.text)" }
            return "\(ShellText.firstName(ShellText.ifBlank(m.authorName, "Someone"))): \(m.text)"
        }
    }

    /// Active groups as Chats rows (the playground's Chats tab lists 1:1 and group conversations together, newest first).
    static func groupChatRows(_ list: [GroupSummary], me: String, nowMs: Int64) -> [ShellChatRow] {
        list.filter { groupActive($0.myRole) }.map { s in
            let at = Int64(s.last?.at ?? 0)
            let mine = s.last.map { $0.outgoing && $0.kind == .post } ?? false
            return ShellChatRow(did: s.id, name: s.name, preview: groupPreview(s, me: me), time: at > 0 ? ShellText.listTime(at, nowMs) : "",
                                mine: mine, tick: .two, unread: Int(s.unread), group: true, atMs: at, lastId: s.last?.id)
        }
    }

    /// "You" stays pinned first; the rest newest first, 1:1 and groups together.
    static func sorted(_ rows: [ShellChatRow], you: String?) -> [ShellChatRow] {
        rows.enumerated().sorted { a, b in
            let pa = a.element.did == you && !a.element.group, pb = b.element.did == you && !b.element.group
            if pa != pb { return pa }
            if a.element.atMs != b.element.atMs { return a.element.atMs > b.element.atMs }
            return a.offset < b.offset
        }.map(\.element)
    }
}

// ---------------------------------------------------------------- empty state (EmptyLine.kt)

struct ShellEmptyCopy: Equatable { let title: String; let line: String; let act: String }

enum ShellEmpty {
    static let chats = "Your chats show up here."
    static let unread = "You\u{2019}re all caught up."
    static let calls = "Calls you make and receive show up here."
    static let contacts = "The people you add show up here."

    /// The one function that decides the hero's copy from who you have (and the requests you sent, waiting for an answer).
    static func heroCopy(contacts: [Contact], sent: [ContactRequest] = []) -> ShellEmptyCopy {
        if contacts.isEmpty, let s = sent.first {
            return ShellEmptyCopy(title: "Your line is ready", line: "Your request to \(ShellText.firstName(s.name)) is waiting. Your line opens when they accept.", act: "Add another contact")
        }
        if contacts.isEmpty {
            return ShellEmptyCopy(title: "Your line is ready", line: "Add someone you\u{2019}d like to talk to, and you can message and call each other.", act: "Add my first contact")
        }
        if contacts.count == 1 {
            let f = ShellText.display(contacts[0]).trimmingCharacters(in: .whitespaces).split(separator: " ").first.map(String.init) ?? ""
            return ShellEmptyCopy(title: "Your line is open", line: "Say hello to \(f) whenever you like.", act: "Message \(f)")
        }
        return ShellEmptyCopy(title: "Your line is open", line: "Pick someone and say hello whenever you like.", act: "Start a chat")
    }
}

// The status mark (Mark.kt statusLook) is the Components area's `statusLook` / `MarkLook` (Components/MarkModel.swift).

// ---------------------------------------------------------------- sign-up (Onboarding.kt)

enum ShellStep: String, CaseIterable {
    case welcome, choose, done, name, pass, phrase, device, restored, terms, perms, restore, saveFailed, passAdd, linkName, linkQr, linkPass, syncing

    var hasTextInput: Bool {
        switch self { case .name, .pass, .device, .restore, .passAdd, .linkName, .linkPass: return true; default: return false }
    }

    var label: String {
        switch self {
        case .name: return "Your nickname"
        case .pass, .linkPass: return "Passphrase"
        case .phrase: return "Recovery phrase"
        case .device, .linkName: return "Device name"
        case .terms: return "Terms"
        case .perms: return "Permissions"
        case .restore: return "Restore account"
        case .linkQr: return "Link to your account"
        default: return ""
        }
    }
}

enum ShellGate {
    /// Position in the flow (gate.svelte.ts progressOf): create, restore and link each have their own order.
    static func progressOf(_ step: ShellStep, restoring: Bool, linking: Bool, forgot: Bool, adding: Bool) -> (Int, Int)? {
        if forgot { return nil }
        // Welcome and Choose are step 0 of the create count: the header and its empty line are already there.
        if (step == .welcome || step == .choose) && !adding { return (0, 6) }
        let tail: [ShellStep] = adding ? [] : [.terms, .perms]
        let order: [ShellStep]
        if linking { order = [.linkName, .linkQr, .linkPass] + tail }
        else if restoring { order = [.restore, .name, .pass, .device] + tail }
        else { order = [.name, .pass, .phrase, .device] + tail }
        guard let i = order.firstIndex(of: step) else { return nil }
        return (i + 1, order.count)
    }

    /// Pass.svelte strength: 0 empty, 1 short, 2 fair, 3 strong (16+ characters or 4+ words).
    static func strength(_ pass: String) -> Int {
        if pass.isEmpty { return 0 }
        let words = pass.trimmingCharacters(in: .whitespaces).split(whereSeparator: { $0.isWhitespace }).count
        if pass.count >= 16 || words >= 4 { return 3 }
        if pass.count >= 8 { return 2 }
        return 1
    }

    static let strengthSays = ["Any length works. A few random words are easy to remember.", "Short. A few more words make it stronger.", "Fair", "Strong"]

    /// The first word of a nickname for "Your line is open, Maya".
    static func first(_ name: String) -> String {
        name.trimmingCharacters(in: .whitespacesAndNewlines).split(whereSeparator: { $0.isWhitespace }).first.map(String.init) ?? ""
    }
}

/// The 24-word restore rules (Onboarding.kt RestoreStep): typing a space or Enter moves on (completing a word when the prefix
/// has one answer); pasting a run of words spreads them from that field; a word turns red once it is left (or after Continue)
/// if it isn't on the list, or at once when no word starts that way.
struct ShellRestoreWords {
    let list: [String]
    let set: Set<String>
    var words = Array(repeating: "", count: 24)
    var seen = Array(repeating: false, count: 24)
    var tried = false
    /// A pasted recovery key (one line, `tlkey-...`) replaces the boxes ("" = typing words).
    var recKey = ""

    init(list: [String]) { self.list = list; set = Set(list) }

    func suggest(_ p: String, _ n: Int) -> [String] { p.isEmpty ? [] : Array(list.lazy.filter { $0.hasPrefix(p) }.prefix(n)) }

    func bad(_ i: Int) -> Bool {
        let v = words[i]
        if v.isEmpty || set.contains(v) { return false }
        return seen[i] || tried || suggest(v, 1).isEmpty
    }

    var badList: [Int] { (0..<24).filter { bad($0) } }
    var filled: Int { words.filter { !$0.isEmpty }.count }

    /// Sets word `i` from what was typed or pasted. Returns the field to focus next when a paste spread over several fields.
    mutating func setWord(_ i: Int, _ raw: String, isKey: (String) -> Bool) -> Int? {
        if isKey(raw) { recKey = raw.trimmingCharacters(in: .whitespacesAndNewlines); return nil }
        let toks = raw.lowercased().split(whereSeparator: { !("a"..."z").contains($0) }).map(String.init)
        if toks.count > 1 {
            for (k, t) in toks.enumerated() where i + k < 24 { words[i + k] = t }
            return min(23, i + toks.count - 1)
        }
        words[i] = String((toks.first ?? "").prefix(12))
        return nil
    }

    /// Leaving field `i` forward: completes a unique prefix.
    mutating func complete(_ i: Int) {
        let w = words[i]
        if !w.isEmpty && !set.contains(w) { let s = suggest(w, 2); if s.count == 1 { words[i] = s[0] } }
    }

    /// The message line under the boxes (empty = nothing to say).
    func message(error: String?) -> String {
        let b = badList
        if let first = b.first {
            let more = b.count > 1 ? " and \(b.count - 1) more" : ""
            return "Word \(first + 1)\(more) \(b.count > 1 ? "aren\u{2019}t" : "isn\u{2019}t") in the recovery word list. Check the spelling."
        }
        if recKey.isEmpty && tried && filled < 24 { return "\(24 - filled) words are still missing." }
        return error ?? ""
    }

    /// Continue with words: nil when they are all in, else the field to focus.
    mutating func submitWords() -> (phrase: String?, focus: Int?) {
        tried = true
        if !badList.isEmpty || filled < 24 {
            let first = badList.first ?? words.firstIndex(where: { $0.isEmpty })
            return (nil, first)
        }
        return (words.joined(separator: " "), nil)
    }
}

/// The BIP-39 English list, bundled (the core does not expose it). Shell/Resources/bip39_english.txt.
enum ShellWordlist {
    static let words: [String] = {
        guard let u = Bundle.main.url(forResource: "bip39_english", withExtension: "txt"),
              let s = try? String(contentsOf: u, encoding: .utf8) else { return [] }
        return s.split(whereSeparator: \.isNewline).map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
    }()
}

// ---------------------------------------------------------------- recovery backup (RecoveryBackupState.kt)

/// Monotonic-time reveal policy used by the real backup screen. Contains no secret material. Times are milliseconds of a
/// monotonic clock (`ShellClock.monoMs`).
struct ShellRecoveryState: Equatable {
    var asKey = true
    var revealUntil: Int64?

    static let revealMs: Int64 = 30_000

    func visible(_ now: Int64) -> Bool { revealUntil.map { now < $0 } ?? false }
    func canCopyKey(_ now: Int64) -> Bool { asKey && visible(now) }
    func reveal(_ now: Int64) -> ShellRecoveryState { var s = self; s.revealUntil = now + Self.revealMs; return s }
    func hide() -> ShellRecoveryState { var s = self; s.revealUntil = nil; return s }
    func select(_ key: Bool) -> ShellRecoveryState { ShellRecoveryState(asKey: key) }
}

enum ShellClock {
    /// Monotonic milliseconds (android SystemClock.elapsedRealtime): wall-clock changes cannot extend a reveal.
    static func monoMs() -> Int64 { Int64(ProcessInfo.processInfo.systemUptime * 1000) }
}

// ---------------------------------------------------------------- search highlighting (Search.kt)

enum ShellSearchConst {
    static let limit: UInt32 = 20
    static let minChars = 2
    static let debounceMs: UInt64 = 150
    /// Characters kept before the first match in a one-line snippet (Marked.svelte `lead`).
    static let lead = 18
}

/// Text with bold hits (Marked.svelte, variant bold) as plain segments. Ranges are in Unicode scalars (the core counts code
/// points), half-open. `lead` keeps at most that many characters before the first hit, cut at a word when one is close.
enum ShellMarked {
    struct Segment: Equatable { let text: String; let bold: Bool }

    static func segments(_ text: String, _ ranges: [Range<Int>], lead: Int = 0) -> [Segment] {
        var t = Array(text.unicodeScalars)
        var rs = ranges.filter { $0.lowerBound < $0.upperBound }.sorted { $0.lowerBound < $1.lowerBound }
        if lead > 0, let r0 = rs.first, r0.lowerBound > 0, r0.lowerBound <= t.count, r0.lowerBound > lead {
            var cut = r0.lowerBound - lead
            if let sp = t[cut..<r0.lowerBound].firstIndex(where: { $0.properties.isWhitespace }), sp - cut < lead - 4 { cut = sp + 1 }
            let shift = cut - 1
            t = Array("\u{2026}".unicodeScalars) + Array(t[cut...])
            rs = rs.map { ($0.lowerBound - shift)..<($0.upperBound - shift) }
        }
        func str(_ a: Int, _ b: Int) -> String { var s = String.UnicodeScalarView(); s.append(contentsOf: t[a..<b]); return String(s) }
        var out: [Segment] = []
        var p = 0
        for r in rs {
            let a = min(max(r.lowerBound, p), t.count), b = min(max(r.upperBound, a), t.count)
            if a > p { out.append(Segment(text: str(p, a), bold: false)) }
            if b > a { out.append(Segment(text: str(a, b), bold: true)) }
            p = b
        }
        if p < t.count { out.append(Segment(text: str(p, t.count), bold: false)) }
        return out
    }

    static func hitRanges(_ hs: [HitRange]) -> [Range<Int>] {
        hs.compactMap { h in h.end > h.start ? Int(h.start)..<Int(h.end) : nil }
    }

    /// Names: the core matches folded substrings; mark the same stretch here (case-insensitive).
    static func nameRanges(_ name: String, _ q: String) -> [Range<Int>] {
        let needle = q.trimmingCharacters(in: .whitespaces)
        guard !needle.isEmpty, let r = name.range(of: needle, options: .caseInsensitive) else { return [] }
        let a = name.unicodeScalars.distance(from: name.unicodeScalars.startIndex, to: r.lowerBound)
        let b = name.unicodeScalars.distance(from: name.unicodeScalars.startIndex, to: r.upperBound)
        return [a..<b]
    }

    static func codePoints(_ s: String) -> Int { s.unicodeScalars.count }
}
