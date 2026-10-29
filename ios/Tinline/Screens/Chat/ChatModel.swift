import Combine
import Foundation

/// One row of the thread, oldest first (android Conversation.kt `ConvRow`; Android reverses the list for its
/// reverse layout, the SwiftUI list scrolls from the bottom instead).
enum ConvRow: Identifiable, Equatable {
    case day(label: String, key: Int)
    case msg(Message, BubblePos)
    /// "n new messages": before the oldest of the messages that were unread when the chat opened.
    case unread(Int)
    /// A call with this person (MessageRow `.callrec`), on the caller's side.
    case call(CallRecord)

    var id: String {
        switch self {
        case .day(let l, let k): return "d\(k)\(l)"
        case .msg(let m, _): return m.id
        case .unread: return "unread"
        case .call(let r): return "call\(r.callId)"
        }
    }
}

extension Array where Element == Message {
    /// Order key: when the author sent it, then the id.
    func sortedChat() -> [Message] { sorted { ($0.at, $0.id) < ($1.at, $1.id) } }
}

enum ChatRows {
    /// Same sender, same day, within 5 minutes: one run (playground Conversation.svelte), so stack-facing corners go tight.
    static func joined(_ a: Message?, _ b: Message?) -> Bool {
        guard let a, let b else { return false }
        let (x, y) = (Int64(a.at), Int64(b.at))
        return a.outgoing == b.outgoing && ChatFmt.sameDay(x, y) && abs(y - x) <= 5 * 60_000
    }

    /// The thread: day pills, calls interleaved by start time, the unread divider, and each message's place in its run.
    static func build(messages msgs: [Message], calls: [CallRecord], nowMs: Int64, dividerId: String?, unreadAtOpen: Int) -> [ConvRow] {
        var rows: [ConvRow] = []
        var last: Int64?
        var pending = calls.sorted { $0.startedAt < $1.startedAt }
        var dayKey = 0
        func addDay(_ at: Int64) {
            if last == nil || !ChatFmt.sameDay(last!, at) { rows.append(.day(label: ChatFmt.dayLabel(at, now: nowMs), key: dayKey)); dayKey += 1 }
        }
        func flushCalls(upTo: Int64) {
            while let first = pending.first, Int64(first.startedAt) * 1000 <= upTo {
                pending.removeFirst()
                let at = Int64(first.startedAt) * 1000
                addDay(at)
                rows.append(.call(first)); last = at
            }
        }
        for (i, m) in msgs.enumerated() {
            let at = Int64(m.at)
            let callBefore = pending.first.map { Int64($0.startedAt) * 1000 <= at } ?? false
            flushCalls(upTo: at)
            addDay(at)
            if m.id == dividerId { rows.append(.unread(unreadAtOpen)) }
            let next = i + 1 < msgs.count ? msgs[i + 1] : nil
            let callAfter = pending.first.flatMap { p in next.map { Int64(p.startedAt) * 1000 <= Int64($0.at) } } ?? false
            let a = !callBefore && m.id != dividerId && joined(i > 0 ? msgs[i - 1] : nil, m)
            let b = !callAfter && joined(m, next)
            rows.append(.msg(m, a && b ? .mid : (a ? .last : (b ? .first : .solo))))
            last = at
        }
        flushCalls(upTo: Int64.max)
        return rows
    }
}

/// A request for the list to scroll to a message (find, search hit, send). `tick` makes repeats distinct.
struct ChatScrollRequest: Equatable { let id: String?; let tick: Int }

/// The state and rules of one open conversation, ported from the body of android `ConversationScreen`. Core calls go
/// through the injected `ChatSource`, so tests drive it with a fake.
@MainActor
final class ConversationModel: ObservableObject {
    let peerDid: String
    let source: ChatSource

    @Published private(set) var messages: [Message] = []
    @Published private(set) var rows: [ConvRow] = []
    @Published private(set) var loaded = false
    @Published private(set) var olderDay: String?
    /// Their phone answered "nothing older": the banner would be false.
    @Published private(set) var noOlder = false
    @Published private(set) var progress: [String: (done: UInt64, total: UInt64)] = [:]
    @Published var requested: Set<String> = []
    /// The core's recipient-signed status per message id (mark + provenance). Missing = not read yet: the mark's slot stays
    /// reserved and empty, never "delivered".
    @Published private(set) var receipts: [String: MessageReceiptStatus] = [:]
    @Published private(set) var link: ChatLink = .unknown
    @Published private(set) var nowMs = Int64(Date().timeIntervalSince1970 * 1000)
    @Published private(set) var sending = false
    /// Ids of our pending messages that won't arrive because they blocked us.
    @Published private(set) var undelivered: Set<String> = []
    @Published var brokenImages: Set<String> = []
    @Published var foundId: String?
    @Published var toast: String?
    @Published private(set) var scroll = ChatScrollRequest(id: nil, tick: 0)
    /// Bumped when older rows were added above, so the view can keep its place.
    @Published private(set) var prependTick = 0

    private(set) var unreadAtOpen = 0
    /// The last find ran on the core's index (else over what is loaded, so it must rerun when messages arrive).
    private(set) var findUsesCore = true
    private var dividerId: String?
    private var calls: [CallRecord] = []
    private var fetched: Set<String> = []
    private var cancellables = Set<AnyCancellable>()
    private var loadingOlder = false
    private var scrollTick = 0

    init(peerDid: String, source: ChatSource) {
        self.peerDid = peerDid
        self.source = source
        unreadAtOpen = Int(source.chats.first { $0.peerDid == peerDid }?.unread ?? 0)
    }

    // MARK: lifecycle

    /// Loads the newest day and starts listening. Call once, from `.task`.
    func start() async {
        source.events.receive(on: DispatchQueue.main).sink { [weak self] e in self?.handle(e) }.store(in: &cancellables)
        // Only this contact's own state, and only when it changes: a @Published set announces even an equal value, and
        // every contact's presence event used to re-render (and re-measure) the open chat.
        let did = peerDid
        source.linksPublisher.map { $0[did] ?? .unknown }.removeDuplicates().receive(on: DispatchQueue.main)
            .sink { [weak self] l in self?.link = l }.store(in: &cancellables)
        link = source.links[peerDid] ?? .unknown
        if let p = try? await source.day(peerDid: peerDid, day: nil) { await absorb(p) }
        loaded = true
        if unreadAtOpen > 0 { dividerId = messages.filter { !$0.outgoing }.suffix(unreadAtOpen).first?.id }
        rebuild()
        await markRead()
    }

    func stop() { cancellables.removeAll() }

    /// The clock for "waiting" ticks: call every 15 s while the view is up.
    func tickClock() { nowMs = Int64(Date().timeIntervalSince1970 * 1000); rebuild() }

    func setCalls(_ list: [CallRecord]) { calls = list; rebuild() }

    private func rebuild() {
        rows = ChatRows.build(messages: messages, calls: calls, nowMs: nowMs, dividerId: dividerId, unreadAtOpen: unreadAtOpen)
    }

    private func absorb(_ p: DayPage) async {
        p.messages.forEach(upsert)
        olderDay = p.olderDay
        await loadDayReceipts(p.day)
    }

    func upsert(_ m: Message) {
        if let i = messages.firstIndex(where: { $0.id == m.id }) { messages[i] = m } else { messages.append(m); messages = messages.sortedChat() }
        rebuild()
    }

    func message(_ id: String?) -> Message? { id.flatMap { id in messages.first { $0.id == id } } }

    // MARK: events

    private func handle(_ e: ChatEvent) {
        switch e {
        case .added(let m), .changed(let m):
            // Changed is also raised when the contact's signed receipts for the shown version were committed.
            guard m.peerDid == peerDid else { return }
            upsert(m)
            if m.outgoing { loadReceipt(m.id) }
            Task { await markRead() }
        case .delivery(let peer, let id, let d):
            guard peer == peerDid, let i = messages.firstIndex(where: { $0.id == id }) else { return }
            messages[i].delivery = d
            rebuild()
            loadReceipt(id)
        case .progress(let peer, let hash, let done, let total, _):
            guard peer == peerDid else { return }
            if done >= total { progress[hash] = nil } else { progress[hash] = (done, total) }
        }
    }

    // MARK: receipts

    func loadReceipt(_ id: String) {
        Task { if let st = await source.receipts(peerDid: peerDid, messageId: id) { receipts[st.messageId] = st } }
    }

    private func loadDayReceipts(_ day: String) async {
        for r in await source.dayReceipts(peerDid: peerDid, day: day) { receipts[r.messageId] = r }
    }

    // MARK: reading

    /// Reading: the open conversation's unread count is cleared (and the list refreshed).
    func markRead() async {
        _ = try? await source.markRead(peerDid: peerDid)
        source.refresh()
    }

    // MARK: older days

    /// Reached the oldest loaded day: read the next older one from this phone, else ask their phone once.
    func loadOlder() async {
        guard loaded, !loadingOlder else { return }
        loadingOlder = true
        defer { loadingOlder = false }
        let before = messages.count
        if let od = olderDay {
            if let p = try? await source.day(peerDid: peerDid, day: od) { await absorb(p) }
        } else if let first = messages.first, link != .offline {
            let oldest = Self.utcDay(Int64(first.at))
            if fetched.insert(oldest).inserted, let n = try? await source.fetchOlder(peerDid: peerDid, beforeDay: oldest) {
                if n == 0 { noOlder = true }
                if n > 0, let p = try? await source.day(peerDid: peerDid, day: oldest) { await absorb(p) }
            }
        }
        if messages.count > before { prependTick += 1 }
    }

    static func utcDay(_ ms: Int64) -> String {
        var cal = Calendar(identifier: .gregorian)
        cal.timeZone = TimeZone(identifier: "UTC")!
        let c = cal.dateComponents([.year, .month, .day], from: Date(timeIntervalSince1970: Double(ms) / 1000))
        return String(format: "%04d-%02d-%02d", c.year ?? 0, c.month ?? 0, c.day ?? 0)
    }

    /// Only when this phone may lack earlier days: the contact was added before our oldest day and their phone has not
    /// said "nothing older".
    func mayHaveOlder(addedAtSecs: Int64?) -> Bool {
        guard loaded, olderDay == nil, let first = messages.first, let added = addedAtSecs, !noOlder else { return false }
        return Self.utcDay(added * 1000) < Self.utcDay(Int64(first.at))
    }

    // MARK: sending

    func requestScroll(_ id: String?) { scrollTick += 1; scroll = ChatScrollRequest(id: id, tick: scrollTick) }

    /// Sends (or saves the edit of) the draft. The draft is cleared only if it did not change meanwhile.
    func send(_ draft: ConversationDraft) {
        let text = draft.text.trimmingCharacters(in: .whitespacesAndNewlines)
        if text.isEmpty || sending { return }
        let edit = draft.edit, reply = draft.reply, original = draft.text
        sending = true
        Task {
            defer { sending = false }
            do {
                let m = try await (edit != nil ? source.edit(peerDid: peerDid, messageId: edit!, text: text)
                                              : source.sendText(peerDid: peerDid, text: text, replyTo: reply))
                upsert(m)
                loadReceipt(m.id)
                if draft.text == original && draft.edit == edit && draft.reply == reply { draft.sent() }
                requestScroll(nil)
            } catch { toast = ChatErrors.friendly(error) }
        }
    }

    func sendFile(_ url: URL, mime: String) {
        Task {
            do {
                let m = try await source.sendFile(peerDid: peerDid, path: url.path, mime: mime, text: nil)
                upsert(m); requestScroll(nil)
            } catch { toast = tr("chat_err_send_file") }
            ChatMedia.discardOut(url)
        }
    }

    func sendPicked(_ url: URL) {
        Task {
            do { let (f, mime) = try ChatMedia.copyIn(url); sendFile(f, mime: mime) } catch { toast = tr("chat_err_read_file") }
        }
    }

    func sendVoice(_ v: RecordedVoice) {
        Task {
            do { upsert(try await source.sendVoice(peerDid: peerDid, path: v.path, durationMs: v.durationMs, waveform: v.waveform)); requestScroll(nil) }
            catch { toast = tr("chat_err_send_voice") }
        }
    }

    func delete(_ id: String) {
        Task {
            do { let m = try await source.delete(peerDid: peerDid, messageId: id); upsert(m); loadReceipt(m.id) }
            catch { toast = tr("chat_err_delete") }
        }
    }

    func download(_ m: Message) {
        requested.insert(m.id)
        Task { do { try await source.download(peerDid: peerDid, messageId: m.id) } catch { requested.remove(m.id); toast = tr("chat_err_download") } }
    }

    func cancelDownload(_ m: Message) {
        requested.remove(m.id)
        Task { _ = try? await source.cancel(peerDid: peerDid, messageId: m.id) }
    }

    func saveToPhone(_ m: Message) {
        guard let a = m.attachment else { return }
        Task {
            let ok: Bool
            if let f = try? await ChatMedia.decrypt(source: source, message: m) { ok = await ChatMedia.saveToPhone(f, name: a.name, mime: a.mime) } else { ok = false }
            toast = ok ? tr("chat_saved") : tr("chat_err_save")
        }
    }

    func share(_ m: Message) {
        Task { if let f = try? await ChatMedia.decrypt(source: source, message: m) { ChatMedia.share(f) } }
    }

    // MARK: blocked-undelivered

    /// Pending messages of mine that the core says will not arrive (a block), by id. `reason` is `Node.notDeliveredReason`.
    func refreshUndelivered(blockedMe: Bool, reason: @escaping @Sendable (String) -> String?) async {
        let ids = messages.filter { $0.outgoing && $0.delivery == .pending }.map(\.id)
        if !blockedMe { undelivered = []; return }
        undelivered = Set(await Task.detached { ids.filter { reason($0) != nil } }.value)
    }

    // MARK: find and jump

    /// Loads the day of `hit` and scrolls to it.
    func jump(to hit: FindHit) async {
        if !hit.day.isEmpty, let page = try? await source.day(peerDid: peerDid, day: hit.day) {
            page.messages.forEach(upsert)
            await loadDayReceipts(page.day)
        }
        requestScroll(hit.id)
    }

    /// Global-search integration: scroll to a hit and light it for 2.6 s.
    func showSearchHit(_ hit: MessageHit) async {
        guard loaded, hit.conv == peerDid else { return }
        await jump(to: FindHit(id: hit.id, day: hit.day))
        foundId = hit.id
        try? await Task.sleep(nanoseconds: 2_600_000_000)
        foundId = nil
    }

    /// Runs the query (after a 150 ms pause for typing): the core's index when this source is the core, else what is loaded.
    func runFind(_ find: FindState) async {
        guard find.open else { return }
        let query = find.q.trimmingCharacters(in: .whitespacesAndNewlines)
        find.error = nil; find.cur = 0; find.next = nil
        if query.unicodeScalars.count < 2 { find.hits = []; return }
        try? await Task.sleep(nanoseconds: 150_000_000)
        if Task.isCancelled { return }
        do {
            if let page = try await source.findInChat(peerDid: peerDid, query: query, cursor: nil) {
                findUsesCore = true
                find.hits = page.hits; find.next = page.next
            } else { findUsesCore = false; find.hits = ChatFind.local(messages, query) }
        } catch { find.hits = []; find.error = ChatErrors.friendly(error) }
    }

    /// The next older page of matches, when the current hits are exhausted.
    func findOlderPage(_ find: FindState) async {
        guard let cursor = find.next else { return }
        let query = find.q
        do {
            guard let page = try await source.findInChat(peerDid: peerDid, query: query.trimmingCharacters(in: .whitespacesAndNewlines), cursor: cursor),
                  find.open, query == find.q else { return }
            let old = find.hits.count
            var seen = Set(find.hits.map(\.id))
            find.hits += page.hits.filter { seen.insert($0.id).inserted }
            find.next = page.next
            if find.hits.count > old { find.cur = old }
        } catch { find.error = ChatErrors.friendly(error) }
    }
}

/// Friendly text for a core error; never includes secrets (android Support.kt `friendly`). The Shell may own a fuller
/// version; this keeps the chat area self-contained.
enum ChatErrors {
    static func friendly(_ e: Swift.Error) -> String {
        let s = String(describing: e)
        if s.contains("inCall") { return tr("chat_err_in_call") }
        if s.contains("locked") { return tr("chat_err_locked") }
        let m = (e as NSError).localizedDescription
        return m.isEmpty || m.hasPrefix("The operation") ? tr("chat_err_generic") : m
    }
}
