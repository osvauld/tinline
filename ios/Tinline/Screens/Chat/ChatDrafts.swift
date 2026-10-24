import Combine
import Foundation

/// In-memory only, wiped by the secure gate. Reply survives navigation; edit never replaces the saved draft.
/// (android ConversationDrafts.kt)
@MainActor
final class ConversationDraft: ObservableObject {
    @Published var text = ""
    @Published var reply: String?
    @Published var edit: String?
    private var stash = ""

    func beginEdit(id: String, body: String) {
        if edit == nil { stash = text }
        edit = id; reply = nil; text = body
    }
    func cancel() {
        if edit != nil { text = stash }
        stash = ""; edit = nil; reply = nil
    }
    func beginReply(id: String) { cancelEdit(); reply = id }
    func cancelEdit() { if edit != nil { text = stash; stash = ""; edit = nil } }
    func sent() { if edit != nil { cancelEdit() } else { text = "" }; reply = nil }
}

@MainActor
enum ConversationDrafts {
    private static var entries: [String: ConversationDraft] = [:]
    /// Integration hook for the shell's draft previews (current account only): peer DID to draft text.
    static let previews = CurrentValueSubject<[String: String], Never>([:])

    static func forPeer(_ did: String) -> ConversationDraft {
        if let d = entries[did] { return d }
        let d = ConversationDraft()
        entries[did] = d
        return d
    }

    static func publish(_ did: String, _ draft: ConversationDraft) {
        // A disposed screen must not republish private data after account/secure-gate clearing.
        guard entries[did] === draft else { return }
        if draft.edit == nil {
            var p = previews.value
            if draft.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { p[did] = nil } else { p[did] = draft.text }
            previews.send(p)
        }
    }

    static func clear() { entries = [:]; previews.send([:]) }
}
