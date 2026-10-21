//! The size of a photo message's colour preview (`tinline/chat/5`); p2pcore's `chat::photo`
//! makes the previews.

/// Preview grid: columns x rows, RGB.
pub const PREVIEW_W: u32 = 4;
pub const PREVIEW_H: u32 = 3;
pub const PREVIEW_LEN: usize = (PREVIEW_W * PREVIEW_H * 3) as usize;
