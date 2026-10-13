//! The playground icon set, verbatim from docs/design/playground/src/lib/ui/icons.ts (24 grid,
//! stroke 1.8, round caps/joins: Icon.svelte). Generated; the screens ported from the playground use
//! these, not the older Lucide set in ui.rs, so glyphs match stroke for stroke.

use iced::{Color, Element};

pub const STROKE: &str = r##"fill="none" stroke="#000" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round""##;

pub fn body(name: &str) -> &'static str {
    match name {
        "back" => r##"<path d="M15 5l-7 7 7 7"/>"##,
        "search" => r##"<circle cx="11" cy="11" r="7"/><path d="M20 20l-4-4"/>"##,
        "send" => r##"<path d="M22 2L11 13M22 2l-7 20-4-9-9-4 20-7z"/>"##,
        "clip" => r##"<path d="M21 11l-9 9a5 5 0 0 1-7-7l9-9a3.5 3.5 0 0 1 5 5l-9 9a2 2 0 0 1-3-3l8-8"/>"##,
        "chev" => r##"<path d="M7 10l5 5 5-5"/>"##,
        "sliders" => r##"<path d="M4 6h8M18 6h2M4 12h2M12 12h8M4 18h10M20 18h0"/><circle cx="15" cy="6" r="2.5"/><circle cx="9" cy="12" r="2.5"/><circle cx="17" cy="18" r="2.5"/>"##,
        "gear" => r##"<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/>"##,
        "sun" => r##"<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M2 12h2M20 12h2M5 5l1.5 1.5M17.5 17.5L19 19M5 19l1.5-1.5M17.5 6.5L19 5"/>"##,
        "moon" => r##"<path d="M20 14.5A8 8 0 0 1 9.5 4 8 8 0 1 0 20 14.5z"/>"##,
        "info" => r##"<circle cx="12" cy="12" r="9"/><path d="M12 11v6M12 7.5v.01"/>"##,
        "x" => r##"<path d="M6 6l12 12M18 6L6 18"/>"##,
        "reply" => r##"<path d="M9 14L4 9l5-5M4 9h10a6 6 0 0 1 6 6v3"/>"##,
        "edit" => r##"<path d="M4 20h4L19 9l-4-4L4 16v4zM13 7l4 4"/>"##,
        "trash" => r##"<path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13"/>"##,
        "copy" => r##"<rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V6a2 2 0 0 1 2-2h8"/>"##,
        "check" => r##"<path d="M5 12.5l4.5 4.5L19 7"/>"##,
        "clock" => r##"<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>"##,
        "retry" => r##"<path d="M20 12a8 8 0 1 1-2.6-5.9M20 4v5h-5"/>"##,
        "file" => r##"<path d="M7 3h7l5 5v13H7zM14 3v5h5"/>"##,
        "image" => r##"<rect x="3" y="4" width="18" height="16" rx="3"/><circle cx="9" cy="10" r="1.6"/><path d="M4 18l5-5 4 4 3-3 4 4"/>"##,
        "bell" => r##"<path d="M6 16v-5a6 6 0 0 1 12 0v5l2 2H4zM10 21h4"/>"##,
        "mute" => r##"<path d="M8.5 5.3A6 6 0 0 1 18 11v4M6 11v5l-2 2h12M10 21h4M3 3l18 18"/>"##,
        "shield" => r##"<path d="M12 3l8 3v6c0 5-3.5 8-8 9-4.5-1-8-4-8-9V6z"/>"##,
        "devices" => r##"<path d="M3 6h13v9H3zM1 18h17M18 9h4v9h-4z"/>"##,
        "plus" => r##"<path d="M12 5v14M5 12h14"/>"##,
        "off" => r##"<circle cx="12" cy="12" r="9"/><path d="M5.6 5.6l12.8 12.8"/>"##,
        "vol" => r##"<path d="M4 9v6h4l5 4V5L8 9zM16.5 9a4 4 0 0 1 0 6"/>"##,
        "phone" => r##"<path d="M5 4h3.5l1.5 4.5-2.2 1.4a11 11 0 0 0 6.3 6.3l1.4-2.2L20 15.5V19a1.5 1.5 0 0 1-1.6 1.5A16 16 0 0 1 3.5 5.6 1.5 1.5 0 0 1 5 4z"/>"##,
        "mic" => r##"<rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5 11a7 7 0 0 0 14 0M12 18v3"/>"##,
        "micoff" => r##"<path d="M15 9.3V6a3 3 0 0 0-5.7-1.3M9 9v2a3 3 0 0 0 4.6 2.5M5 11a7 7 0 0 0 11.5 5.4M19 11a7 7 0 0 1-.6 2.8M12 18v3M3 3l18 18"/>"##,
        "users" => r##"<circle cx="9" cy="8" r="3.5"/><path d="M2.5 20c.5-3.5 3.2-5.5 6.5-5.5s6 2 6.5 5.5"/><circle cx="17" cy="9" r="2.5"/><path d="M16.5 14.6c2.6.2 4.5 2 5 4.9"/>"##,
        "chat" => r##"<path d="M4 5h16v11H9l-5 4z"/>"##,
        "contact" => r##"<rect x="4" y="3" width="16" height="18" rx="2"/><circle cx="12" cy="10" r="3"/><path d="M7.5 17c.8-2 2.5-3 4.5-3s3.7 1 4.5 3"/><path d="M2 7h2M2 12h2M2 17h2"/>"##,
        "eye" => r##"<path d="M2 12s3.5-6.5 10-6.5S22 12 22 12s-3.5 6.5-10 6.5S2 12 2 12z"/><circle cx="12" cy="12" r="3"/>"##,
        "eyeoff" => r##"<path d="M3 3l18 18M10.6 5.6A9.6 9.6 0 0 1 12 5.5c6.5 0 10 6.5 10 6.5a17 17 0 0 1-3.2 3.9M6.4 6.9A16.6 16.6 0 0 0 2 12s3.5 6.5 10 6.5a9.5 9.5 0 0 0 4-.9M9.9 9.9a3 3 0 0 0 4.2 4.2"/>"##,
        "key" => r##"<circle cx="8" cy="15" r="4"/><path d="M11 12l9-9M16 7l3 3M14 9l2 2"/>"##,
        "lock" => r##"<rect x="5" y="11" width="14" height="10" rx="2"/><path d="M8 11V8a4 4 0 0 1 8 0v3"/>"##,
        "warning" => r##"<path d="M12 3l10 18H2zM12 10v5M12 18v.01"/>"##,
        "camera" => r##"<path d="M4 8h3l2-3h6l2 3h3v12H4z"/><circle cx="12" cy="13.5" r="3.5"/>"##,
        "battery" => r##"<rect x="2" y="7" width="17" height="10" rx="2"/><path d="M22 11v2M6 10v4M10 10v4"/>"##,
        "qr" => r##"<path d="M4 4h6v6H4zM14 4h6v6h-6zM4 14h6v6H4zM14 14h2v2h-2zM18 14h2v2M14 18h2v2h4v-2"/>"##,
        "link" => r##"<path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1"/>"##,
        "call-out" => r##"<path d="M7 17L17 7M9 7h8v8"/>"##,
        "flag" => r##"<path d="M5 21V4M5 4h11l-2 4 2 4H5"/>"##,
        "userplus" => r##"<circle cx="9" cy="8" r="3.5"/><path d="M2.5 20c.5-3.5 3.2-5.5 6.5-5.5s6 2 6.5 5.5M19 8v6M16 11h6"/>"##,
        "leave" => r##"<path d="M10 4H5v16h5M15 8l4 4-4 4M19 12H9"/>"##,
        "server" => r##"<rect x="3" y="4" width="18" height="7" rx="2"/><rect x="3" y="13" width="18" height="7" rx="2"/><path d="M7 7.5v.01M7 16.5v.01"/>"##,
        "chevr" => r##"<path d="M10 7l5 5-5 5"/>"##,
        "smartphone" => r##"<rect x="7" y="2.5" width="10" height="19" rx="2.5"/><path d="M11 18.5h2"/>"##,
        "laptop" => r##"<rect x="4.5" y="5" width="15" height="10" rx="1.5"/><path d="M2.5 19h19"/>"##,
        "call-in" => r##"<path d="M17 7L7 17M15 17H7V9"/>"##,
        _ => "",
    }
}

/// Icon.svelte: `name` at `size` px in `color`.
pub fn pgi<'a, M: 'a>(name: &'static str, size: f32, color: Color) -> Element<'a, M> {
    crate::ui::glyph(STROKE, body(name), size, color)
}

#[cfg(test)]
mod tests {
    #[test]
    fn pg_icons_cover_the_set() {
        for n in ["back", "search", "send", "clip", "chev", "sliders", "gear", "sun", "moon", "info", "x", "reply", "edit", "trash", "copy", "check", "clock", "retry", "file", "image", "bell", "mute", "shield", "devices", "plus", "off", "vol", "phone", "mic", "micoff", "users", "chat", "contact", "eye", "eyeoff", "key", "lock", "warning", "camera", "battery", "qr", "link", "call-out", "flag", "userplus", "leave", "server", "chevr", "smartphone", "laptop", "call-in"] {
            assert!(!super::body(n).is_empty(), "{n}");
        }
    }
}
