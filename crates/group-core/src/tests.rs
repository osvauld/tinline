use super::*;

#[test]
fn short_takes_eight_chars_and_never_panics() {
    assert_eq!(short(&"ab".repeat(32)), "abababab");
    assert_eq!(short("abc"), "abc");
    assert_eq!(short(""), "");
    // Eight bytes would cut inside the multi-byte character: the whole string is returned.
    assert_eq!(short("1234567é"), "1234567é");
}

#[test]
fn short_skips_the_constant_did_prefix() {
    assert_eq!(short("did:key:z6MkAbCdEfGhIjKl"), "AbCdEfGh");
    assert_eq!(short("did:key:z6Mk"), "");
    assert_eq!(short("did:key:zOther"), "did:key:");
}
