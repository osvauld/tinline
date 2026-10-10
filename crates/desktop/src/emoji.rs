//! Emoji in message text: a message of only 1-3 emojis shows big, emojis inside text a little
//! larger than the letters (same rules as Android's `Emoji.kt`).

pub const BIG_MAX: usize = 3;
pub const INLINE_SCALE: f32 = 1.25;

fn is_emoji(c: char) -> bool {
    matches!(c as u32, 0x1F000..=0x1FAFF | 0x2600..=0x27BF | 0x2300..=0x23FF | 0x2B00..=0x2BFF)
}

/// Parts of an emoji that never stand alone: joiner, variation selectors, keycap, skin tones, tags.
fn is_part(c: char) -> bool {
    matches!(c as u32, 0x200D | 0xFE0E | 0xFE0F | 0x20E3 | 0x1F3FB..=0x1F3FF | 0xE0020..=0xE007F)
}

fn is_flag_half(c: char) -> bool {
    matches!(c as u32, 0x1F1E6..=0x1F1FF)
}

/// How many emojis `text` holds if it is nothing but emojis (and spaces).
pub fn only_count(text: &str) -> Option<usize> {
    let (mut n, mut after_joiner, mut open_flag) = (0, false, false);
    for c in text.chars() {
        if c.is_whitespace() {
            (after_joiner, open_flag) = (false, false);
        } else if is_part(c) {
            after_joiner = c == '\u{200D}';
        } else if is_flag_half(c) {
            if !open_flag {
                n += 1;
            }
            (open_flag, after_joiner) = (!open_flag, false);
        } else if is_emoji(c) {
            if !after_joiner {
                n += 1;
            }
            (after_joiner, open_flag) = (false, false);
        } else {
            return None;
        }
    }
    (n > 0).then_some(n)
}

pub fn is_big(text: &str) -> bool {
    only_count(text).is_some_and(|n| n <= BIG_MAX)
}

/// `text` cut into runs, each flagged whether it is emoji.
pub fn runs(text: &str) -> Vec<(&str, bool)> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut cur: Option<bool> = None;
    for (i, c) in text.char_indices() {
        let e = is_emoji(c) || is_part(c);
        if cur.is_some_and(|x| x != e) {
            out.push((&text[start..i], cur.unwrap()));
            start = i;
        }
        cur = Some(e);
    }
    if let Some(e) = cur {
        out.push((&text[start..], e));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_emoji_only_messages() {
        assert_eq!(only_count("👍"), Some(1));
        assert_eq!(only_count(" 😂😂 "), Some(2));
        assert_eq!(only_count("👨‍👩‍👧"), Some(1), "a ZWJ family is one emoji");
        assert_eq!(only_count("👍🏽"), Some(1), "a skin tone is part of the emoji");
        assert_eq!(only_count("🇮🇳🇩🇪"), Some(2), "two flags");
        assert_eq!(only_count("❤️"), Some(1));
        assert_eq!(only_count("ok 👍"), None);
        assert_eq!(only_count(""), None);
        assert!(is_big("🎉🎉🎉") && !is_big("🎉🎉🎉🎉"));
    }

    #[test]
    fn splits_text_and_emoji_runs() {
        assert_eq!(runs("hi 👋🏽 there"), vec![("hi ", false), ("👋🏽", true), (" there", false)]);
        assert_eq!(runs("plain"), vec![("plain", false)]);
    }
}
