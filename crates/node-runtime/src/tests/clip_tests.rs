use crate::*;

#[test]
fn clip_reason_strips_control_characters_and_caps_the_length() {
    assert_eq!(clip_reason("a\nb\r\tc\u{7}\u{1b}[31m"), "abc[31m");
    assert_eq!(clip_reason(""), "");
    let long = "x".repeat(MAX_REASON_CHARS * 3);
    assert_eq!(clip_reason(&long).chars().count(), MAX_REASON_CHARS);
    // Multi-byte characters count as one and are never cut in half.
    let wide = "é".repeat(MAX_REASON_CHARS + 5);
    assert_eq!(clip_reason(&wide), "é".repeat(MAX_REASON_CHARS));
}

#[test]
fn short_is_the_shared_group_shortener() {
    assert_eq!(short("0123456789abcdef"), "01234567");
    assert_eq!(short("did:key:z6MkpTHR8VNsBxYAAWHut2Geadd9jSwuBV8xRoAnwWsdvktH"), "pTHR8VNs");
}
