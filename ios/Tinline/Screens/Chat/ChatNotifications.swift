import Combine
import Foundation

/// Which conversation is on screen (active), so its own messages do not notify. (android ChatNotifications.kt `OpenChat`)
@MainActor
enum OpenChat {
    static var peer: String?
    /// The group conversation in front of the user (its posts don't notify).
    static var group: String?
    /// A tap on a message notification asks the UI to open this conversation (peer DID).
    static let request = CurrentValueSubject<String?, Never>(nil)
    /// A tap on a group invitation notification asks Home to show the Groups tab.
    static let groupsTab = CurrentValueSubject<Bool, Never>(false)
}

/// What a message looks like in a notification, a quote or a list: its text, or a word for what it carries.
func chatPreviewOf(_ m: Message) -> String {
    if m.deleted { return "" }
    if let a = m.attachment, m.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
        if a.kind == .voice { return tr("chat_preview_voice") }
        if a.mime.hasPrefix("image/") { return tr("chat_preview_photo") }
        return a.name
    }
    return m.text
}

/// One notification per conversation, with an inline reply, hooked to `onIncoming` of the chat source. Android builds a
/// MessagingStyle notification with a RemoteInput; the iOS equivalent is a communication notification
/// (`UNNotificationContent` with an `INSendMessageIntent`) with a `UNTextInputNotificationAction`. Only the contract is
/// here: delivery of notifications needs the app's notification entitlement and the push design (Shell area).
protocol ChatNotifying: AnyObject {
    /// An incoming message arrived. Implementations skip it when `OpenChat.peer` is that conversation, the Messages
    /// switch is off, or the preview is empty.
    func incoming(_ m: Message)
    /// After an inline reply: show it in the thread so the notification stops spinning.
    func replied(peer: String, text: String)
    /// The conversation was opened or read: drop its notification and its kept lines.
    func cancel(peer: String)
}

/// Settings > Notifications > Groups: a post from someone else in a group that isn't in front of the user, one
/// notification per group, plus the invitation notification ("Book club · Priya invited you to the group"). The contract
/// only; the group area decides the model types it passes.
protocol GroupNotifying: AnyObject {
    func post(groupId: String, groupName: String?, authorName: String, text: String, at: UInt64)
    func invitation(id: String, groupName: String, inviterName: String)
    func cancel(group: String)
}

/// Does nothing. // TODO(ios-device): replace with a UNUserNotificationCenter implementation (communication notifications,
/// an inline-reply `UNTextInputNotificationAction` that calls `ChatSource.sendText`, tap = `OpenChat.request`). Needs a
/// device or simulator to verify; nothing here can be checked without one.
final class ChatNotifierStub: ChatNotifying {
    func incoming(_ m: Message) {}
    func replied(peer: String, text: String) {}
    func cancel(peer: String) {}
}

/// The notifier the app uses; the Shell swaps in the real one at start.
@MainActor
enum ChatNotifier {
    static var shared: ChatNotifying = ChatNotifierStub()
}
