//! Attachments the other person sent, turned into something to look at: what may be opened,
//! the private cache the decrypted copies live in, and image thumbnails.
//!
//! File names and mime types come from the sender and are untrusted. Only the allowlist below
//! may be handed to the system's default app, and only when the declared mime type and the
//! extension agree. The cached file's extension always comes from the allowlist entry.

use std::path::{Path, PathBuf};

/// What the app does with an attachment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// Shown as a thumbnail and in the viewer.
    Image(&'static str),
    /// Shown in the in-app text viewer.
    Text(&'static str),
    /// Handed to the default app.
    Open(&'static str),
    /// Save only.
    SaveOnly,
}

impl Class {
    pub fn openable(self) -> bool {
        !matches!(self, Class::SaveOnly)
    }
}

/// extension, canonical extension to cache under, accepted mime types
type Entry = (&'static str, &'static str, &'static [&'static str]);

const IMAGES: &[Entry] = &[
    ("png", "png", &["image/png"]),
    ("jpg", "jpg", &["image/jpeg"]),
    ("jpeg", "jpg", &["image/jpeg"]),
    ("gif", "gif", &["image/gif"]),
    ("webp", "webp", &["image/webp"]),
    ("bmp", "bmp", &["image/bmp", "image/x-ms-bmp"]),
];

const TEXT: &[Entry] = &[
    ("txt", "txt", &["text/plain"]),
    ("md", "md", &["text/markdown", "text/x-markdown", "text/plain"]),
    ("csv", "csv", &["text/csv", "text/plain"]),
    ("log", "log", &["text/plain", "text/x-log"]),
];

const OTHER: &[Entry] = &[
    ("pdf", "pdf", &["application/pdf"]),
    ("doc", "doc", &["application/msword"]),
    ("docx", "docx", &["application/vnd.openxmlformats-officedocument.wordprocessingml.document"]),
    ("xls", "xls", &["application/vnd.ms-excel"]),
    ("xlsx", "xlsx", &["application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"]),
    ("ppt", "ppt", &["application/vnd.ms-powerpoint"]),
    ("pptx", "pptx", &["application/vnd.openxmlformats-officedocument.presentationml.presentation"]),
    ("odt", "odt", &["application/vnd.oasis.opendocument.text"]),
    ("ods", "ods", &["application/vnd.oasis.opendocument.spreadsheet"]),
    ("odp", "odp", &["application/vnd.oasis.opendocument.presentation"]),
    ("rtf", "rtf", &["application/rtf", "text/rtf"]),
    ("epub", "epub", &["application/epub+zip"]),
    ("mp3", "mp3", &["audio/mpeg", "audio/mp3"]),
    ("ogg", "ogg", &["audio/ogg"]),
    ("opus", "opus", &["audio/ogg", "audio/opus"]),
    ("wav", "wav", &["audio/wav", "audio/x-wav", "audio/wave"]),
    ("flac", "flac", &["audio/flac", "audio/x-flac"]),
    ("m4a", "m4a", &["audio/mp4", "audio/x-m4a", "audio/m4a"]),
    ("aac", "aac", &["audio/aac"]),
    ("mp4", "mp4", &["video/mp4"]),
    ("webm", "webm", &["video/webm"]),
    ("mkv", "mkv", &["video/x-matroska"]),
    ("mov", "mov", &["video/quicktime"]),
];

/// Characters that make a name misleading: controls and the bidi overrides that turn
/// "gpj.exe" into "exe.jpg" on screen.
fn is_tricky(c: char) -> bool {
    c.is_control() || matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}')
}

/// The last extension of `name`, lower case, if the name is plain enough to judge.
fn extension(name: &str) -> Option<String> {
    if name.chars().any(is_tricky) || name.ends_with(['.', ' ']) {
        return None;
    }
    let (_, ext) = name.rsplit_once('.')?;
    (!ext.is_empty() && ext.len() <= 8).then(|| ext.to_ascii_lowercase())
}

/// The mime type without parameters, lower case.
fn bare_mime(mime: &str) -> String {
    mime.split(';').next().unwrap_or("").trim().to_ascii_lowercase()
}

/// Decides by both the declared mime type and the extension; when they disagree, or either is
/// not on the allowlist, the file can only be saved.
pub fn classify(name: &str, mime: &str) -> Class {
    let Some(ext) = extension(name) else { return Class::SaveOnly };
    let mime = bare_mime(mime);
    let hit = |t: &'static [Entry]| t.iter().find(|e| e.0 == ext && e.2.contains(&mime.as_str())).map(|e| e.1);
    if let Some(e) = hit(IMAGES) {
        Class::Image(e)
    } else if let Some(e) = hit(TEXT) {
        Class::Text(e)
    } else if let Some(e) = hit(OTHER) {
        Class::Open(e)
    } else {
        Class::SaveOnly
    }
}

/// A file name safe to create inside the cache: no separators, no leading dots, no odd
/// characters, capped in length, and the extension taken from the allowlist (`ext`).
pub fn cache_name(name: &str, ext: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let stem = base.rsplit_once('.').map(|(s, _)| s).unwrap_or(base);
    let mut s: String = stem
        .chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '(' | ')') { c } else { '_' })
        .collect();
    s = s.trim_matches(|c: char| c == '.' || c == '_' || c.is_whitespace()).to_string();
    let s: String = s.chars().take(60).collect();
    let s = if s.is_empty() { "attachment".to_string() } else { s };
    format!("{s}.{ext}")
}

/// A hash from the wire as a directory name.
fn hash_dir(hash: &str) -> String {
    let h: String = hash.chars().filter(|c| c.is_ascii_alphanumeric()).take(64).collect();
    if h.is_empty() { "x".into() } else { h }
}

// ---- the cache ----

fn root() -> PathBuf {
    crate::data_dir().join("cache").join("media")
}

#[cfg(unix)]
fn private_dir(p: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(p)
}

#[cfg(not(unix))]
fn private_dir(p: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(p)
}

/// Bytes in the decrypted-copy cache (Settings > Storage).
pub fn cache_bytes() -> u64 {
    fn walk(p: &Path) -> u64 {
        let Ok(rd) = std::fs::read_dir(p) else { return 0 };
        rd.filter_map(|e| e.ok()).map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => walk(&e.path()),
            Ok(m) => m.len(),
            Err(_) => 0,
        }).sum()
    }
    walk(&root())
}

/// Empties the cache; at start, and when the app quits.
pub fn wipe(data: &Path) {
    let _ = std::fs::remove_dir_all(data.join("cache").join("media"));
}

pub fn wipe_now() {
    wipe(&crate::data_dir());
}

/// Where the decrypted copy of an attachment goes.
pub fn cache_path(hash: &str, name: &str, ext: &str) -> std::io::Result<PathBuf> {
    let dir = root().join(hash_dir(hash));
    // Parents are created private first so the files never sit in a readable directory.
    if let Some(p) = root().parent() {
        private_dir(p)?;
    }
    private_dir(&dir)?;
    Ok(dir.join(cache_name(name, ext)))
}

/// Decrypts into the cache once: `write` gets a temporary path and the file is renamed into
/// place, so a half-written file is never opened. Returns the final path.
pub fn ensure_cached(
    hash: &str,
    name: &str,
    ext: &str,
    write: impl FnOnce(&str) -> Result<(), String>,
) -> Result<PathBuf, String> {
    let dest = cache_path(hash, name, ext).map_err(|e| e.to_string())?;
    if dest.is_file() {
        return Ok(dest);
    }
    // Two jobs may want the same file at once (thumbnail and viewer): each writes its own copy.
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = dest.with_extension(format!("{ext}.part{}-{n}", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    if let Err(e) = write(&tmp.to_string_lossy()) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&tmp, &dest) {
        let _ = std::fs::remove_file(&tmp);
        if !dest.is_file() {
            return Err(e.to_string());
        }
    }
    Ok(dest)
}

// ---- images ----

pub const MAX_SIDE: u32 = 12_000;
pub const MAX_PIXELS: u64 = 64_000_000;
/// Decoded at twice the size shown, for sharp thumbnails on high-density screens.
pub const THUMB_W: u32 = 640;
pub const THUMB_H: u32 = 720;
pub const VIEW_SIDE: u32 = 2048;
pub const TEXT_MAX: u64 = 1_000_000;

/// A decoded picture: RGBA pixels and size.
#[derive(Clone)]
pub struct Pixels {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

impl std::fmt::Debug for Pixels {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Pixels({}x{})", self.w, self.h)
    }
}

/// The mime type to declare for a file we send, from its extension (the allowlist's types).
pub fn mime_for_ext(ext: &str) -> Option<&'static str> {
    let ext = ext.to_ascii_lowercase();
    [IMAGES, TEXT, OTHER].iter().flat_map(|t| t.iter()).find(|e| e.0 == ext).map(|e| e.2[0])
}

fn dimensions_ok(w: u32, h: u32) -> bool {
    w > 0 && h > 0 && w <= MAX_SIDE && h <= MAX_SIDE && (w as u64) * (h as u64) <= MAX_PIXELS
}

/// Reads the header first, refuses oversized pictures, then decodes and shrinks to fit
/// `max_w` x `max_h` (never enlarging). The format is taken from the bytes, not the name.
pub fn decode_scaled(path: &Path, max_w: u32, max_h: u32) -> Result<Pixels, String> {
    use image::{ImageDecoder, ImageReader};
    let open = || -> Result<_, String> {
        ImageReader::open(path).map_err(|e| e.to_string())?.with_guessed_format().map_err(|e| e.to_string())
    };
    let dec = open()?.into_decoder().map_err(|e| e.to_string())?;
    let (w, h) = dec.dimensions();
    if !dimensions_ok(w, h) {
        return Err("image is too large".into());
    }
    let mut r = open()?;
    let mut lim = image::Limits::default();
    lim.max_image_width = Some(MAX_SIDE);
    lim.max_image_height = Some(MAX_SIDE);
    lim.max_alloc = Some(512 * 1024 * 1024);
    r.limits(lim);
    let img = r.decode().map_err(|e| e.to_string())?;
    let img = if img.width() > max_w || img.height() > max_h { img.thumbnail(max_w, max_h) } else { img };
    let rgba = img.to_rgba8();
    Ok(Pixels { w: rgba.width(), h: rgba.height(), rgba: rgba.into_raw() })
}

/// Reads a small text file for the in-app viewer; invalid UTF-8 is replaced.
pub fn read_text(path: &Path) -> Result<String, String> {
    let md = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if md.len() > TEXT_MAX {
        return Err("file is too large to show here".into());
    }
    let b = std::fs::read(path).map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&b).into_owned())
}

/// Hands a file from the cache to the system's default app.
pub fn open_external(path: &Path) -> Result<(), String> {
    open::that_detached(path).map_err(|e| e.to_string())
}

#[cfg(test)]
#[path = "media_tests.rs"]
mod tests;
