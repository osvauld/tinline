//! Tinline look: colour tokens (docs/design/Main.dc.html), fonts, Lucide-style icons and the
//! widget styles built from them.

use iced::widget::{button, container, pick_list, scrollable, svg, text_input, toggler};
use iced::{font, Background, Border, Color, Element, Font, Length, Theme};

/// Figtree (variable, 400-700) and IBM Plex Mono, embedded; licences in assets/fonts.
pub const FIGTREE_BYTES: &[u8] = include_bytes!("../assets/fonts/figtree_variable.ttf");
pub const PLEX_REGULAR_BYTES: &[u8] = include_bytes!("../assets/fonts/plexmono_regular.ttf");
pub const PLEX_MEDIUM_BYTES: &[u8] = include_bytes!("../assets/fonts/plexmono_medium.ttf");

pub const SANS: Font = Font::with_name("Figtree");
pub const SANS_SEMI: Font = Font { weight: font::Weight::Semibold, ..Font::with_name("Figtree") };
pub const SANS_BOLD: Font = Font { weight: font::Weight::Bold, ..Font::with_name("Figtree") };
pub const MONO: Font = Font::with_name("IBM Plex Mono");

const fn rgb(hex: u32) -> Color {
    Color {
        r: ((hex >> 16) & 0xFF) as f32 / 255.0,
        g: ((hex >> 8) & 0xFF) as f32 / 255.0,
        b: (hex & 0xFF) as f32 / 255.0,
        a: 1.0,
    }
}

/// The design tokens for one scheme. Names follow Main.dc.html.
#[derive(Clone, Copy, PartialEq)]
pub struct Tok {
    pub dark: bool,
    pub bg: Color,
    pub surface: Color,
    pub surface2: Color,
    pub surface3: Color,
    pub ink: Color,
    pub ink2: Color,
    pub line: Color,
    pub primary: Color,
    pub on_primary: Color,
    pub primary_c: Color,
    pub on_primary_c: Color,
    pub thread: Color,
    pub thread_text: Color,
    pub relay: Color,
    pub on_relay_c: Color,
    pub relay_c: Color,
    pub warn: Color,
    pub on_warn: Color,
    pub error: Color,
    pub error_c: Color,
    pub on_error_c: Color,
    pub accept: Color,
    pub end: Color,
}

pub const LIGHT: Tok = Tok {
    dark: false,
    bg: rgb(0xF5F6F3),
    surface: rgb(0xFFFFFF),
    surface2: rgb(0xECEFEB),
    surface3: rgb(0xE2E6E1),
    ink: rgb(0x17201D),
    ink2: rgb(0x4D5853),
    line: rgb(0xD3D9D4),
    primary: rgb(0x0B6B5B),
    on_primary: rgb(0xFFFFFF),
    primary_c: rgb(0xCDEBE3),
    on_primary_c: rgb(0x06352D),
    thread: rgb(0xC98A1E),
    thread_text: rgb(0x8A5A00),
    relay: rgb(0x4B5AA8),
    relay_c: rgb(0xE1E4F7),
    on_relay_c: rgb(0x4B5AA8),
    warn: rgb(0xFCEFD2),
    on_warn: rgb(0x5C3B00),
    error: rgb(0xB3261E),
    error_c: rgb(0xF9DEDC),
    on_error_c: rgb(0x8C1D18),
    accept: rgb(0x0E7C69),
    end: rgb(0xC8372D),
};

pub const DARK: Tok = Tok {
    dark: true,
    bg: rgb(0x0F1513),
    surface: rgb(0x161D1B),
    surface2: rgb(0x1D2623),
    surface3: rgb(0x26302D),
    ink: rgb(0xE6ECE9),
    ink2: rgb(0xA6B2AD),
    line: rgb(0x33403B),
    primary: rgb(0x6FD3BD),
    on_primary: rgb(0x00382E),
    primary_c: rgb(0x0E4E43),
    on_primary_c: rgb(0xBDEFE2),
    thread: rgb(0xE8B04A),
    thread_text: rgb(0xE8B04A),
    relay: rgb(0xB8C1F5),
    relay_c: rgb(0x2F3A75),
    on_relay_c: rgb(0xB8C1F5),
    warn: rgb(0x4A3500),
    on_warn: rgb(0xFFDFA0),
    error: rgb(0xF2B8B5),
    error_c: rgb(0x601410),
    on_error_c: rgb(0xF9DEDC),
    accept: rgb(0x0E7C69),
    end: rgb(0xC8372D),
};

pub fn tok(dark: bool) -> Tok {
    if dark { DARK } else { LIGHT }
}

/// The iced theme with the exact palette; widgets without a custom style derive from it.
pub fn theme(dark: bool) -> Theme {
    let t = tok(dark);
    Theme::custom(
        if dark { "Tinline dark" } else { "Tinline light" },
        iced::theme::Palette {
            background: t.bg,
            text: t.ink,
            primary: t.primary,
            success: t.accept,
            warning: t.thread,
            danger: t.error,
        },
    )
}

pub fn alpha(c: Color, a: f32) -> Color {
    Color { a, ..c }
}

// ---- icons: Lucide, 24 grid, 1.75 stroke ----

#[derive(Clone, Copy)]
pub enum Icon {
    Phone,
    PhoneOff,
    Mic,
    MicOff,
    UserPlus,
    Sliders,
    ArrowLeft,
    Copy,
    ShieldCheck,
    Check,
    X,
    Lock,
    ChevronRight,
    ChevronLeft,
    ExternalLink,
    Pencil,
    Trash,
    Incoming,
    Outgoing,
    Direct,
    Relayed,
    LogOut,
    Key,
    CheckCheck,
    Clock,
    Paperclip,
    Send,
    Download,
    File,
    Reply,
    MessageSquare,
    RotateCw,
    More,
}

impl Icon {
    fn body(self) -> &'static str {
        match self {
            Icon::Phone => r#"<path d="M22 16.92v3a2 2 0 0 1-2.18 2 19.79 19.79 0 0 1-8.63-3.07 19.5 19.5 0 0 1-6-6 19.79 19.79 0 0 1-3.07-8.67A2 2 0 0 1 4.11 2h3a2 2 0 0 1 2 1.72 12.84 12.84 0 0 0 .7 2.81 2 2 0 0 1-.45 2.11L8.09 9.91a16 16 0 0 0 6 6l1.27-1.27a2 2 0 0 1 2.11-.45 12.84 12.84 0 0 0 2.81.7A2 2 0 0 1 22 16.92z"/>"#,
            Icon::PhoneOff => r#"<path d="M10.68 13.31a16 16 0 0 0 3.41 2.6l1.27-1.27a2 2 0 0 1 2.11-.45 12.84 12.84 0 0 0 2.81.7 2 2 0 0 1 1.72 2v3a2 2 0 0 1-2.18 2 19.79 19.79 0 0 1-8.63-3.07 19.42 19.42 0 0 1-3.33-2.67m-2.67-3.34a19.79 19.79 0 0 1-3.07-8.63A2 2 0 0 1 4.11 2h3a2 2 0 0 1 2 1.72 12.84 12.84 0 0 0 .7 2.81 2 2 0 0 1-.45 2.11L8.09 9.91"/><path d="M22 2 2 22"/>"#,
            Icon::Mic => r#"<path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z"/><path d="M19 10v2a7 7 0 0 1-14 0v-2"/><path d="M12 19v3"/>"#,
            Icon::MicOff => r#"<path d="M2 2l20 20"/><path d="M18.89 13.23A7.12 7.12 0 0 0 19 12v-2"/><path d="M5 10v2a7 7 0 0 0 12 5"/><path d="M15 9.34V5a3 3 0 0 0-5.68-1.33"/><path d="M9 9v3a3 3 0 0 0 5.12 2.12"/><path d="M12 19v3"/>"#,
            Icon::UserPlus => r#"<path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M19 8v6"/><path d="M22 11h-6"/>"#,
            Icon::Sliders => r#"<path d="M21 4h-7"/><path d="M10 4H3"/><path d="M21 12h-9"/><path d="M8 12H3"/><path d="M21 20h-5"/><path d="M12 20H3"/><path d="M14 2v4"/><path d="M8 10v4"/><path d="M16 18v4"/>"#,
            Icon::ArrowLeft => r#"<path d="m12 19-7-7 7-7"/><path d="M19 12H5"/>"#,
            Icon::Copy => r#"<rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/>"#,
            Icon::ShieldCheck => r#"<path d="M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z"/><path d="m9 12 2 2 4-4"/>"#,
            Icon::Check => r#"<path d="M20 6 9 17l-5-5"/>"#,
            Icon::X => r#"<path d="M18 6 6 18"/><path d="m6 6 12 12"/>"#,
            Icon::Lock => r#"<rect width="18" height="11" x="3" y="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/>"#,
            Icon::ChevronLeft => r#"<path d="m15 18-6-6 6-6"/>"#,
            Icon::ExternalLink => r#"<path d="M15 3h6v6"/><path d="M10 14 21 3"/><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/>"#,
            Icon::ChevronRight => r#"<path d="m9 18 6-6-6-6"/>"#,
            Icon::Pencil => r#"<path d="M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z"/><path d="m15 5 4 4"/>"#,
            Icon::Trash => r#"<path d="M3 6h18"/><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"/><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"/><path d="M10 11v6"/><path d="M14 11v6"/>"#,
            Icon::Incoming => r#"<path d="M17 7 7 17"/><path d="M17 17H7V7"/>"#,
            Icon::Outgoing => r#"<path d="M7 17 17 7"/><path d="M7 7h10v10"/>"#,
            // Tinline's own glyphs: a solid line between two points, and the same line passing
            // through a locked box.
            Icon::Direct => r#"<circle cx="4" cy="12" r="2"/><circle cx="20" cy="12" r="2"/><path d="M6 12h12"/>"#,
            Icon::Relayed => r#"<circle cx="3" cy="12" r="2"/><circle cx="21" cy="12" r="2"/><path d="M5 12h4"/><path d="M15 12h4"/><rect width="6" height="6" x="9" y="9" rx="1"/><path d="M10.5 9V8a1.5 1.5 0 0 1 3 0v1"/>"#,
            Icon::LogOut => r#"<path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4"/><path d="m16 17 5-5-5-5"/><path d="M21 12H9"/>"#,
            Icon::CheckCheck => r#"<path d="M18 6 7 17l-5-5"/><path d="m22 10-7.5 7.5L13 16"/>"#,
            Icon::Clock => r#"<circle cx="12" cy="12" r="10"/><path d="M12 6v6l4 2"/>"#,
            Icon::Paperclip => r#"<path d="m21.44 11.05-9.19 9.19a6 6 0 0 1-8.49-8.49l8.57-8.57A4 4 0 1 1 18 8.84l-8.59 8.57a2 2 0 0 1-2.83-2.83l8.49-8.48"/>"#,
            Icon::Send => r#"<path d="m22 2-7 20-4-9-9-4Z"/><path d="M22 2 11 13"/>"#,
            Icon::Download => r#"<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><path d="m7 10 5 5 5-5"/><path d="M12 15V3"/>"#,
            Icon::File => r#"<path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"/><path d="M14 2v4a2 2 0 0 0 2 2h4"/>"#,
            Icon::Reply => r#"<path d="m9 17-5-5 5-5"/><path d="M20 18v-2a4 4 0 0 0-4-4H4"/>"#,
            Icon::MessageSquare => r#"<path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/>"#,
            Icon::RotateCw => r#"<path d="M21 12a9 9 0 1 1-3-6.7L21 8"/><path d="M21 3v5h-5"/>"#,
            Icon::More => r#"<circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/><circle cx="5" cy="12" r="1"/>"#,
            Icon::Key => r#"<path d="m15.5 7.5 2.3 2.3a1 1 0 0 0 1.4 0l2.1-2.1a1 1 0 0 0 0-1.4L19 4"/><path d="m21 2-9.6 9.6"/><circle cx="7.5" cy="15.5" r="5.5"/>"#,
        }
    }
}

fn from_markup(markup: String) -> svg::Handle {
    svg::Handle::from_memory(markup.into_bytes())
}

/// An icon of `size` px in `color`. The SVG is black; the widget tints it.
pub fn icon<'a, M: 'a>(i: Icon, size: f32, color: Color) -> Element<'a, M> {
    let markup = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">{}</svg>"##,
        i.body()
    );
    svg(from_markup(markup))
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .style(move |_: &Theme, _| svg::Style { color: Some(color) })
        .into()
}

/// The brand mark: two cans joined by the amber string. Own colours, never tinted.
pub fn logo<'a, M: 'a>(size: f32, can: Color, string: Color) -> Element<'a, M> {
    let hex = |c: Color| format!("#{:02X}{:02X}{:02X}", (c.r * 255.0) as u8, (c.g * 255.0) as u8, (c.b * 255.0) as u8);
    let m = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><rect x="1.5" y="8" width="6" height="8" rx="1.5" fill="{c}"/><rect x="16.5" y="8" width="6" height="8" rx="1.5" fill="{c}"/><path d="M8.5 12 Q12 14.5 15.5 12" fill="none" stroke="{s}" stroke-width="1.75" stroke-linecap="round"/></svg>"#,
        c = hex(can),
        s = hex(string)
    );
    svg(from_markup(m)).width(Length::Fixed(size)).height(Length::Fixed(size)).into()
}

/// Four quality bars, `n` of them filled.
pub fn bars<'a, M: 'a>(n: u8, on: Color, off: Color) -> Element<'a, M> {
    let mut body = String::new();
    for i in 0..4u8 {
        let h = 5 + i as u32 * 4;
        let c = if i < n { on } else { off };
        body += &format!(
            r##"<rect x="{}" y="{}" width="3.5" height="{}" rx="1" fill="#{:02X}{:02X}{:02X}" fill-opacity="{}"/>"##,
            2 + i as u32 * 5,
            21 - h,
            h,
            (c.r * 255.0) as u8,
            (c.g * 255.0) as u8,
            (c.b * 255.0) as u8,
            c.a
        );
    }
    svg(from_markup(format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">{body}</svg>"#)))
        .width(Length::Fixed(18.0))
        .height(Length::Fixed(18.0))
        .into()
}

// ---- widget styles ----

pub fn card(t: Tok) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(t.surface)),
        border: Border { color: t.line, width: 1.0, radius: 14.0.into() },
        text_color: Some(t.ink),
        ..Default::default()
    }
}

pub fn plain(bg: Color, radius: f32) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(bg)),
        border: Border { radius: radius.into(), ..Default::default() },
        ..Default::default()
    }
}

/// A flat filled container with a 1 px outline.
pub fn outlined(bg: Color, line: Color, radius: f32) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(bg)),
        border: Border { color: line, width: 1.0, radius: radius.into() },
        ..Default::default()
    }
}

/// Sidebar: surface with a 1 px right edge.
pub fn sidebar(t: Tok) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(t.surface)),
        border: Border { color: t.line, width: 1.0, radius: 0.0.into() },
        ..Default::default()
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    /// Filled brand teal.
    Primary,
    /// Tonal (primary container).
    Tonal,
    /// Outlined / surface-2.
    Quiet,
    /// No chrome until hovered.
    Ghost,
    Accept,
    End,
    /// Red text on a quiet button.
    Danger,
}

pub fn button_style(t: Tok, kind: Kind, radius: f32) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let (bg, fg, border) = match kind {
            Kind::Primary => (t.primary, t.on_primary, None),
            Kind::Tonal => (t.primary_c, t.on_primary_c, None),
            Kind::Quiet => (t.surface2, t.ink, Some(t.line)),
            Kind::Ghost => (Color::TRANSPARENT, t.ink2, None),
            Kind::Accept => (t.accept, Color::WHITE, None),
            Kind::End => (t.end, Color::WHITE, None),
            Kind::Danger => (t.surface2, t.error, Some(t.line)),
        };
        let (bg, fg) = match status {
            button::Status::Hovered if kind == Kind::Ghost => (t.surface2, t.ink),
            button::Status::Hovered | button::Status::Pressed => {
                (mix(bg, if t.dark { Color::WHITE } else { Color::BLACK }, 0.08), fg)
            }
            button::Status::Disabled => (alpha(bg, bg.a * 0.45), alpha(fg, 0.55)),
            _ => (bg, fg),
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: fg,
            border: Border {
                color: border.unwrap_or(Color::TRANSPARENT),
                width: if border.is_some() { 1.0 } else { 0.0 },
                radius: radius.into(),
            },
            ..Default::default()
        }
    }
}

/// A list row: transparent, tinted when selected or hovered.
pub fn row_style(t: Tok, selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let bg = match (selected, status) {
            (true, _) => t.primary_c,
            (_, button::Status::Hovered | button::Status::Pressed) => t.surface2,
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: if selected { t.on_primary_c } else { t.ink },
            border: Border { radius: 12.0.into(), ..Default::default() },
            ..Default::default()
        }
    }
}

pub fn mix(a: Color, b: Color, t: f32) -> Color {
    Color { r: a.r + (b.r - a.r) * t, g: a.g + (b.g - a.g) * t, b: a.b + (b.b - a.b) * t, a: a.a }
}

pub fn input_style(t: Tok) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    move |_, status| {
        let focused = matches!(status, text_input::Status::Focused { .. });
        text_input::Style {
            background: Background::Color(t.surface),
            border: Border {
                color: if focused { t.primary } else { t.line },
                width: if focused { 2.0 } else { 1.0 },
                radius: 8.0.into(),
            },
            icon: t.ink2,
            placeholder: alpha(t.ink2, 0.8),
            value: t.ink,
            selection: alpha(t.primary, 0.3),
        }
    }
}

pub fn pick_style(t: Tok) -> impl Fn(&Theme, pick_list::Status) -> pick_list::Style {
    move |_, status| pick_list::Style {
        text_color: t.ink,
        placeholder_color: t.ink2,
        handle_color: t.ink2,
        background: Background::Color(t.surface),
        border: Border {
            color: if matches!(status, pick_list::Status::Opened { .. }) { t.primary } else { t.line },
            width: 1.0,
            radius: 8.0.into(),
        },
    }
}

pub fn menu_style(t: Tok) -> impl Fn(&Theme) -> iced::overlay::menu::Style {
    move |_| iced::overlay::menu::Style {
        background: Background::Color(t.surface),
        border: Border { color: t.line, width: 1.0, radius: 8.0.into() },
        text_color: t.ink,
        selected_text_color: t.on_primary_c,
        selected_background: Background::Color(t.primary_c),
        shadow: Default::default(),
    }
}

pub fn toggle_style(t: Tok) -> impl Fn(&Theme, toggler::Status) -> toggler::Style {
    move |theme, status| {
        let on = matches!(status, toggler::Status::Active { is_toggled: true } | toggler::Status::Hovered { is_toggled: true });
        toggler::Style {
            background: Background::Color(if on { t.primary } else { t.surface3 }),
            foreground: Background::Color(if on { t.on_primary } else { t.surface }),
            background_border_color: t.line,
            foreground_border_color: Color::TRANSPARENT,
            ..toggler::default(theme, status)
        }
    }
}

/// Avatar tints: four tinted containers picked by hashing the contact key (stable across devices).
pub fn avatar_colors(t: Tok, key: &str) -> (Color, Color) {
    let h = key.bytes().fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
    if t.dark {
        match h % 4 {
            0 => (rgb(0x0E4E43), rgb(0xBDEFE2)),
            1 => (rgb(0x2F3A75), rgb(0xD9DEFB)),
            2 => (rgb(0x5A3F00), rgb(0xFFDFA0)),
            _ => (rgb(0x5C2336), rgb(0xFFD9E2)),
        }
    } else {
        match h % 4 {
            0 => (rgb(0xCDEBE3), rgb(0x06352D)),
            1 => (rgb(0xE1E4F7), rgb(0x2F3A75)),
            2 => (rgb(0xFCEFD2), rgb(0x5C3B00)),
            _ => (rgb(0xF8DDE5), rgb(0x5C2336)),
        }
    }
}

pub fn initials(name: &str) -> String {
    // First letter or digit of each word, so "Lena (work)" is "LW", not "L(".
    let mut it = name.split_whitespace().filter_map(|w| w.chars().find(|c| c.is_alphanumeric()));
    let a = it.next();
    let b = it.next();
    match (a, b) {
        (Some(a), Some(b)) => format!("{}{}", a.to_uppercase(), b.to_uppercase()),
        (Some(a), None) => a.to_uppercase().to_string(),
        _ => "?".into(),
    }
}

/// Thin, quiet scroll rails.
pub fn scroll_style(t: Tok) -> impl Fn(&Theme, scrollable::Status) -> scrollable::Style {
    move |th, st| {
        let mut s = scrollable::default(th, st);
        for r in [&mut s.vertical_rail, &mut s.horizontal_rail] {
            r.background = None;
            r.scroller.background = Background::Color(alpha(t.ink2, 0.35));
        }
        s
    }
}
