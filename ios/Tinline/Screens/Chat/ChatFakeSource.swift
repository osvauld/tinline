import Combine
import Foundation

// A `ChatSource` with no core behind it, for #Preview and the unit tests (android's debug `ChatBackend.override`).
// Everything here uses synthetic identities.

enum ChatFixtures {
    static let me = "did:tin:me-synthetic"
    static let ana = "did:tin:ana-synthetic"

    static func message(_ id: String, _ text: String, at: UInt64, outgoing: Bool = false, peer: String = ana, replyTo: String? = nil,
                        attachment: Attachment? = nil, delivery: DeliveryState = .delivered, deleted: Bool = false, editedAt: UInt64? = nil) -> Message {
        Message(id: id, peerDid: peer, authorDid: outgoing ? me : peer, outgoing: outgoing, at: at, text: text, editedAt: editedAt,
                deleted: deleted, replyTo: replyTo, attachment: attachment, delivery: delivery)
    }

    static func attachment(name: String, mime: String, size: UInt64 = 2_400_000, state: TransferState = .ready, kind: AttachmentKind = .file) -> Attachment {
        Attachment(hash: "hash-\(name)", name: name, size: size, mime: mime, kind: kind, durationMs: 0, waveform: Data(), width: 0, height: 0,
                   preview: Data(), state: state, transferred: 0)
    }

    static func chat(unread: UInt32 = 0, peer: String = ana, name: String = "Ana Rao") -> Chat {
        Chat(peerDid: peer, peerName: name, preview: "", lastOutgoing: false, lastDelivery: .delivered, lastActivity: 0, unread: unread, lastId: nil)
    }

    /// 2026-10-14 12:00 UTC in ms plus `minutes`.
    static func at(minutes: Int) -> UInt64 { 1_791_979_200_000 + UInt64(minutes) * 60_000 }
}

final class ChatFakeSource: ChatSource {
    var chatsValue: [Chat]
    /// Day key ("yyyy-MM-dd", "" = newest) to page.
    var pages: [String: DayPage] = [:]
    var receiptsById: [String: MessageReceiptStatus] = [:]
    var failSend = false
    private(set) var sentTexts: [(text: String, replyTo: String?)] = []
    private(set) var edits: [(id: String, text: String)] = []
    private(set) var markReadCount = 0
    private(set) var typingCount = 0
    private let eventsSubject = PassthroughSubject<ChatEvent, Never>()
    private let linksSubject = CurrentValueSubject<[String: ChatLink], Never>([:])
    private let chatsSubject: CurrentValueSubject<[Chat], Never>

    init(chats: [Chat] = [], messages: [Message] = [], olderDay: String? = nil) {
        chatsValue = chats
        chatsSubject = CurrentValueSubject(chats)
        pages[""] = DayPage(day: "2026-10-14", messages: messages, olderDay: olderDay)
    }

    var chats: [Chat] { chatsValue }
    var chatsPublisher: AnyPublisher<[Chat], Never> { chatsSubject.eraseToAnyPublisher() }
    var events: AnyPublisher<ChatEvent, Never> { eventsSubject.eraseToAnyPublisher() }
    var links: [String: ChatLink] { linksSubject.value }
    var linksPublisher: AnyPublisher<[String: ChatLink], Never> { linksSubject.eraseToAnyPublisher() }
    func refresh() {}

    func emit(_ e: ChatEvent) { eventsSubject.send(e) }
    func setLink(_ peer: String, _ l: ChatLink?) { var m = linksSubject.value; m[peer] = l; linksSubject.send(m) }

    func day(peerDid: String, day: String?) async throws -> DayPage {
        guard let p = pages[day ?? ""] else { throw ChatSourceError.noNode }
        return p
    }
    func fetchOlder(peerDid: String, beforeDay: String) async throws -> Int { 0 }
    func sendText(peerDid: String, text: String, replyTo: String?) async throws -> Message {
        if failSend { throw ChatSourceError.noNode }
        sentTexts.append((text, replyTo))
        return ChatFixtures.message("sent\(sentTexts.count)", text, at: ChatFixtures.at(minutes: 100 + sentTexts.count), outgoing: true, peer: peerDid, replyTo: replyTo, delivery: .pending)
    }
    func edit(peerDid: String, messageId: String, text: String) async throws -> Message {
        edits.append((messageId, text))
        return ChatFixtures.message(messageId, text, at: ChatFixtures.at(minutes: 1), outgoing: true, peer: peerDid, editedAt: ChatFixtures.at(minutes: 200))
    }
    func delete(peerDid: String, messageId: String) async throws -> Message {
        ChatFixtures.message(messageId, "", at: ChatFixtures.at(minutes: 1), outgoing: true, peer: peerDid, deleted: true)
    }
    func markRead(peerDid: String) async throws { markReadCount += 1 }
    func typing(peerDid: String) { typingCount += 1 }
    func sendFile(peerDid: String, path: String, mime: String, text: String?) async throws -> Message {
        ChatFixtures.message("file1", text ?? "", at: ChatFixtures.at(minutes: 300), outgoing: true, peer: peerDid,
                             attachment: ChatFixtures.attachment(name: (path as NSString).lastPathComponent, mime: mime))
    }
    func sendVoice(peerDid: String, path: String, durationMs: Int, waveform: Data) async throws -> Message {
        ChatFixtures.message("voice1", "", at: ChatFixtures.at(minutes: 301), outgoing: true, peer: peerDid,
                             attachment: ChatFixtures.attachment(name: "voice.ogg", mime: "audio/ogg", kind: .voice))
    }
    func download(peerDid: String, messageId: String) async throws {}
    func cancel(peerDid: String, messageId: String) async throws {}
    func save(peerDid: String, messageId: String, destPath: String) async throws {}
    func receipts(peerDid: String, messageId: String) async -> MessageReceiptStatus? { receiptsById[messageId] }
}
