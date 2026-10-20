//! Emoji in message text: a message of only 1-3 emojis shows big, emojis inside text a little
//! larger than the letters.

/// Most emojis a message can hold and still be shown big.
pub const BIG_MAX: u32 = 3;
/// How much larger than the letters an emoji inside text is drawn.
pub const INLINE_SCALE: f32 = 1.25;

/// [`INLINE_SCALE`] for Kotlin (`pub const` is not exported by UniFFI).
#[uniffi::export]
pub fn emoji_inline_scale() -> f32 {
    INLINE_SCALE
}

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

/// How many emojis `text` holds if it is nothing but emojis (and spaces); `None` otherwise.
#[uniffi::export]
pub fn emoji_only_count(text: String) -> Option<u32> {
    let (mut n, mut after_joiner, mut open_flag) = (0u32, false, false);
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

/// A message to show with big emoji.
#[uniffi::export]
pub fn emoji_is_big(text: String) -> bool {
    emoji_only_count(text).is_some_and(|n| n <= BIG_MAX)
}

/// A stretch of text that is all emoji or none.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct EmojiRun {
    pub text: String,
    pub is_emoji: bool,
}

/// `text` cut into runs, each flagged whether it is emoji. Concatenating the runs gives `text`.
#[uniffi::export]
pub fn emoji_runs(text: String) -> Vec<EmojiRun> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut cur: Option<bool> = None;
    for (i, c) in text.char_indices() {
        let e = is_emoji(c) || is_part(c);
        if let Some(prev) = cur.filter(|x| *x != e) {
            out.push(EmojiRun {
                text: text[start..i].to_string(),
                is_emoji: prev,
            });
            start = i;
        }
        cur = Some(e);
    }
    if let Some(e) = cur {
        out.push(EmojiRun {
            text: text[start..].to_string(),
            is_emoji: e,
        });
    }
    out
}

#[cfg(test)]
mod tests;
