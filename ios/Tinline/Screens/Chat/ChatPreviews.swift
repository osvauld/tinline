import SwiftUI

#if DEBUG
private func previewSource() -> ChatFakeSource {
    let f = ChatFixtures.self
    let msgs = [
        f.message("a", "Did the file reach you?", at: f.at(minutes: 0)),
        f.message("b", "Yes, thank you.", at: f.at(minutes: 1), outgoing: true),
        f.message("c", "Lovely. Lunch on Friday?", at: f.at(minutes: 2)),
        f.message("d", "😀", at: f.at(minutes: 3), outgoing: true),
        f.message("e", "Friday works. I’ll bring the notes we talked about, and the long message that wraps onto a second line.", at: f.at(minutes: 4), outgoing: true, replyTo: "c"),
        f.message("f", "", at: f.at(minutes: 6), attachment: f.attachment(name: "menu.pdf", mime: "application/pdf")),
    ]
    return ChatFakeSource(chats: [f.chat(unread: 2)], messages: msgs)
}

#Preview("Conversation, light") {
    ConversationView(chatId: ChatFixtures.ana, name: "Ana Rao", source: previewSource())
        .environmentObject(AppEnv()).tinTheme()
}

#Preview("Conversation, dark") {
    ConversationView(chatId: ChatFixtures.ana, name: "Ana Rao", source: previewSource())
        .environmentObject(AppEnv()).tinTheme().preferredColorScheme(.dark)
}

#Preview("Bubbles") {
    VStack(spacing: 8) {
        ChatDayPill(label: "Today")
        ChatUnreadDivider(n: 2)
        ChatTextBubble(text: "Hello there", pos: .solo, mine: false, meta: ChatBubbleMeta(time: "12:41"))
        ChatTextBubble(text: "A longer reply that has to wrap across more than one line of the bubble.", pos: .solo, mine: true,
                       meta: ChatBubbleMeta(time: "12:42", edited: true, receipt: ChatMetaReceipt(tick: .delivered, key: "x", label: "On their device")))
    }
    .padding().frame(maxHeight: .infinity).background(Color(hex: 0xEDEFE9)).tinTheme()
}
#endif
