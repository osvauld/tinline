//! CSS `text-overflow: ellipsis; white-space: nowrap` for iced, which only clips. The playground
//! truncates names, previews and statuses with "…"; here the string is shortened to the widest
//! prefix that fits, measured with the same shaper the renderer uses.

use iced::advanced::text::{self, Paragraph as _};
use iced::{Font, Pixels, Size};

type Para = <iced::Renderer as iced::advanced::text::Renderer>::Paragraph;

/// Width of a single line of `s` in logical px.
pub fn width(s: &str, font: Font, size: f32) -> f32 {
    let p = Para::with_text(text::Text {
        content: s,
        bounds: Size::INFINITE,
        size: Pixels(size),
        line_height: text::LineHeight::default(),
        font,
        align_x: text::Alignment::Default,
        align_y: iced::alignment::Vertical::Top,
        shaping: text::Shaping::Advanced,
        wrapping: text::Wrapping::None,
    });
    p.min_width()
}

/// `s` unchanged if it fits in `max` px, else the longest prefix (at a char boundary, trailing
/// space trimmed) followed by "…" that fits.
pub fn ellipsize(s: &str, font: Font, size: f32, max: f32) -> String {
    let one_line: String = s.split(['\n', '\r']).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ");
    if max <= 0.0 || width(&one_line, font, size) <= max {
        return one_line;
    }
    let ends: Vec<usize> = one_line.char_indices().map(|(i, _)| i).skip(1).chain([one_line.len()]).collect();
    let (mut lo, mut hi) = (0usize, ends.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let cand = format!("{}\u{2026}", one_line[..ends[mid - 1]].trim_end());
        if width(&cand, font, size) <= max {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    if lo == 0 {
        return "\u{2026}".into();
    }
    format!("{}\u{2026}", one_line[..ends[lo - 1]].trim_end())
}
