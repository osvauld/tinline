#[test]
fn pg_icons_cover_the_set() {
    for n in ["back", "search", "send", "clip", "chev", "sliders", "gear", "sun", "moon", "info", "x", "reply", "edit", "trash", "copy", "check", "clock", "retry", "file", "image", "bell", "mute", "shield", "devices", "plus", "off", "vol", "phone", "mic", "micoff", "users", "chat", "contact", "eye", "eyeoff", "key", "lock", "warning", "camera", "battery", "qr", "link", "call-out", "flag", "userplus", "leave", "server", "chevr", "smartphone", "laptop", "call-in"] {
        assert!(!super::body(n).is_empty(), "{n}");
    }
}
