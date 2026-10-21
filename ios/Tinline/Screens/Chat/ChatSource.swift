import Combine
import Foundation

/// What the core pushes while a conversation or the chat list is on screen (android ChatSource.kt `ChatEvent`).
enum ChatEvent {
    case added(Message)
    case changed(Message)
    case delivery(peerDid: String, messageId: String, delivery: DeliveryState)
    case progress(peerDid: String, hash: String, done: UInt64, total: UInt64, outgoing: Bool)
}

/// Everything the chat screens need. `CoreChatSource` is backed by the `Node`; previews and tests swap in a fake. The
/// calls are `async throws`: the core's own methods block, so the real source runs them off the main thread. The core
/// reports "not implemented" until chat lands: callers show an empty state.
protocol ChatSource: AnyObject {
    /// Conversations, most recent activity first. Empty when the core cannot answer.
    var chats: [Chat] { get }
    var chatsPublisher: AnyPublisher<[Chat], Never> { get }
    var events: AnyPublisher<ChatEvent, Never> { get }
    /// Per contact; a contact missing from the map is `.unknown`.
    var links: [String: ChatLink] { get }
    var linksPublisher: AnyPublisher<[String: ChatLink], Never> { get }
    /// Re-reads `chats` from the core, off the caller's thread.
    func refresh()

    func day(peerDid: String, day: String?) async throws -> DayPage
    func fetchOlder(peerDid: String, beforeDay: String) async throws -> Int
    func sendText(peerDid: String, text: String, replyTo: String?) async throws -> Message
    func edit(peerDid: String, messageId: String, text: String) async throws -> Message
    func delete(peerDid: String, messageId: String) async throws -> Message
    func markRead(peerDid: String) async throws
    /// I'm typing to them (each non-empty composer edit): the core dials ahead and counts it as my activity, so the link
    /// stays open and carries my presence for a minute. Local only: they see no typing indicator. Cheap and
    /// non-blocking (throttled in the core); opening a conversation alone connects nothing (consent-based links).
    func typing(peerDid: String)
    func sendFile(peerDid: String, path: String, mime: String, text: String?) async throws -> Message
    func sendVoice(peerDid: String, path: String, durationMs: Int, waveform: Data) async throws -> Message
    func download(peerDid: String, messageId: String) async throws
    /// Stops a download (also one waiting to retry or failed); the attachment is REMOTE again.
    func cancel(peerDid: String, messageId: String) async throws
    func save(peerDid: String, messageId: String, destPath: String) async throws
    /// The core's recipient-signed status of one message (its `ReceiptTick` and proofs); nil when unknown.
    func receipts(peerDid: String, messageId: String) async -> MessageReceiptStatus?
    /// `receipts` for every message of one UTC day page.
    func dayReceipts(peerDid: String, day: String) async -> [MessageReceiptStatus]
    /// The core's find-in-chat page (`search_in`), or nil when this source is not the core (previews).
    func findInChat(peerDid: String, query: String, cursor: String?) async throws -> (hits: [FindHit], next: String?)?
}

extension ChatSource {
    func typing(peerDid: String) {}
    func receipts(peerDid: String, messageId: String) async -> MessageReceiptStatus? { nil }
    func dayReceipts(peerDid: String, day: String) async -> [MessageReceiptStatus] { [] }
    func findInChat(peerDid: String, query: String, cursor: String?) async throws -> (hits: [FindHit], next: String?)? { nil }
}

/// Where the app gets its `ChatSource`. One real source per node; the Shell calls `attach(node:)` after unlock and again
/// when the node is replaced (the conversation view also attaches the `AppEnv` node if nobody has).
@MainActor
enum ChatBackend {
    /// Previews and tests set this; release code never does.
    static var override: ChatSource?
    private static var core: CoreChatSource?

    static func current(node: Node?) -> ChatSource? {
        if let override { return override }
        guard let node else { return core }
        if let core, core.isAttached(to: node) { return core }
        let c = core ?? CoreChatSource()
        c.attach(node: node)
        core = c
        return c
    }
}

/// The real thing: the `Node` plus its `ChatEvents` callbacks (core threads: never block here).
final class CoreChatSource: ChatSource, ChatEvents, @unchecked Sendable {
    private let lock = NSLock()
    private var node: Node?
    private let chatsSubject = CurrentValueSubject<[Chat], Never>([])
    private let eventsSubject = PassthroughSubject<ChatEvent, Never>()
    private let linksSubject = CurrentValueSubject<[String: ChatLink], Never>([:])

    /// Side effects of an incoming message that live outside the conversation (notification, the `received` cue). The
    /// Shell fills these; both are called on a core thread.
    var onIncoming: ((Message) -> Void)?

    var chats: [Chat] { chatsSubject.value }
    var chatsPublisher: AnyPublisher<[Chat], Never> { chatsSubject.eraseToAnyPublisher() }
    var events: AnyPublisher<ChatEvent, Never> { eventsSubject.eraseToAnyPublisher() }
    var links: [String: ChatLink] { linksSubject.value }
    var linksPublisher: AnyPublisher<[String: ChatLink], Never> { linksSubject.eraseToAnyPublisher() }

    private var current: Node? { lock.lock(); defer { lock.unlock() }; return node }
    func isAttached(to n: Node) -> Bool { current === n }

    /// Registers for callbacks on the node; call again after the node is replaced.
    func attach(node n: Node) {
        lock.lock(); node = n; lock.unlock()
        n.setChatEvents(events: self)
    }

    private func io<T: Sendable>(_ f: @escaping @Sendable (Node) throws -> T) async throws -> T {
        guard let n = current else { throw ChatSourceError.noNode }
        return try await Task.detached(priority: .userInitiated) { try f(n) }.value
    }

    func refresh() {
        Task { [weak self] in
            guard let self else { return }
            let list = (try? await self.io { try $0.chats() }) ?? []
            self.chatsSubject.send(list)
        }
    }

    // MARK: ChatEvents (core threads)

    func onMessageAdded(message: Message) {
        eventsSubject.send(.added(message))
        if !message.outgoing { onIncoming?(message) }
    }
    func onMessageChanged(message: Message) { eventsSubject.send(.changed(message)) }
    func onChatChanged(chat: Chat) {
        var list = chatsSubject.value.filter { $0.peerDid != chat.peerDid }
        list.insert(chat, at: 0)
        chatsSubject.send(list.sorted { $0.lastActivity > $1.lastActivity })
    }
    func onDeliveryChanged(peerDid: String, messageId: String, delivery: DeliveryState) {
        eventsSubject.send(.delivery(peerDid: peerDid, messageId: messageId, delivery: delivery))
    }
    func onTransferProgress(peerDid: String, hash: String, done: UInt64, total: UInt64, outgoing: Bool) {
        eventsSubject.send(.progress(peerDid: peerDid, hash: hash, done: done, total: total, outgoing: outgoing))
    }
    /// The core's ephemeral presence: connected while one of the contact's devices is live.
    func onPresenceChanged(peerDid: String, online: Bool) {
        var m = linksSubject.value
        if online { m[peerDid] = .connected } else { m[peerDid] = nil }
        linksSubject.send(m)
    }

    // MARK: ChatSource

    func day(peerDid: String, day: String?) async throws -> DayPage { try await io { try $0.chatDay(peerDid: peerDid, day: day) } }
    func fetchOlder(peerDid: String, beforeDay: String) async throws -> Int {
        Int(try await io { try $0.fetchOlderHistory(peerDid: peerDid, beforeDay: beforeDay) })
    }
    func sendText(peerDid: String, text: String, replyTo: String?) async throws -> Message {
        try await io { try $0.sendText(peerDid: peerDid, text: text, replyTo: replyTo) }
    }
    func edit(peerDid: String, messageId: String, text: String) async throws -> Message {
        try await io { try $0.editMessage(peerDid: peerDid, messageId: messageId, text: text) }
    }
    func delete(peerDid: String, messageId: String) async throws -> Message {
        try await io { try $0.deleteMessage(peerDid: peerDid, messageId: messageId) }
    }
    func markRead(peerDid: String) async throws { try await io { try $0.markRead(peerDid: peerDid) } }
    func typing(peerDid: String) { current?.chatTyping(peerDid: peerDid) }
    func sendFile(peerDid: String, path: String, mime: String, text: String?) async throws -> Message {
        try await io { try $0.sendFile(peerDid: peerDid, path: path, mime: mime, text: text) }
    }
    func sendVoice(peerDid: String, path: String, durationMs: Int, waveform: Data) async throws -> Message {
        try await io { try $0.sendVoice(peerDid: peerDid, path: path, durationMs: UInt32(durationMs), waveform: waveform) }
    }
    func download(peerDid: String, messageId: String) async throws { try await io { try $0.downloadAttachment(peerDid: peerDid, messageId: messageId) } }
    func cancel(peerDid: String, messageId: String) async throws { try await io { try $0.cancelDownload(peerDid: peerDid, messageId: messageId) } }
    func save(peerDid: String, messageId: String, destPath: String) async throws {
        try await io { try $0.saveAttachment(peerDid: peerDid, messageId: messageId, destPath: destPath) }
    }
    func receipts(peerDid: String, messageId: String) async -> MessageReceiptStatus? {
        try? await io { try $0.chatMessageReceipts(peerDid: peerDid, messageId: messageId) }
    }
    func dayReceipts(peerDid: String, day: String) async -> [MessageReceiptStatus] {
        (try? await io { try $0.chatDayReceipts(peerDid: peerDid, day: day) }) ?? []
    }
    func findInChat(peerDid: String, query: String, cursor: String?) async throws -> (hits: [FindHit], next: String?)? {
        let r = try await io { try $0.searchIn(kind: .dm, conv: peerDid, query: query, limit: 20, cursor: cursor) }
        return (r.messages.map { FindHit(id: $0.id, day: $0.day) }, r.next)
    }
}

enum ChatSourceError: Error { case noNode }
