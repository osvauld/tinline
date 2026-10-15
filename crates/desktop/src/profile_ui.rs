//! Settings, My profile and Person profile (playground screens/settings, screens/profile) on the
//! core's real profile API. Blocking Node and image work runs on workers; core callbacks only
//! enqueue invalidations (`Ev::Profile`). Every completion carries the account epoch and owner DID
//! it was started for and is dropped if the selected account changed meanwhile.
use super::*;
use iced::widget::{image::Handle, text_editor};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::io::Cursor;

pub const PICTURE_HELP: &str = "Replacing or removing your picture requests deletion of the previous cached copy on your linked devices and contacts when they sync. Screenshots, backups, and copies retained by modified clients cannot be erased by the app.";
pub const FIELD_HELP: &str = "Your contacts can see your profile. Removing information hides it from your profile; earlier versions can remain in synced history.";
const SAVE_FAILED: &str = "Couldn\u{2019}t save that. Your profile is unchanged and what you typed is still here.";
pub const BUSY_UNSUPPORTED: &str = "Busy is not available yet. Your call settings are unchanged.";
fn availability_problem(available: bool) -> Option<&'static str> {
    if available { None } else { Some(BUSY_UNSUPPORTED) }
}
fn mutation_allowed(transition_busy: bool, pending: bool) -> bool {
    !transition_busy && !pending
}
/// FieldRow / Availability: how long "Saved" stays (a UI constant, not a motion timing).
const SAVED_MS: u64 = 1800;
/// profile.ts LIMITS.
pub const LIMIT_NAME: usize = 256;
pub const LIMIT_TEXT: usize = 4096;
/// Cropper.svelte VIEW: the stage is 264 css px square.
pub const VIEW: f32 = 264.0;

/// Where the open-source code lives (the same address as Android's About).
pub const SOURCE_URL: &str = "https://github.com/osvauld/tinline";

/// One measurement for Settings > Storage.
#[derive(Debug, Clone, Copy)]
pub struct StorageInfo {
    pub usage: p2pcore::StorageUsage,
    /// Decrypted copies made for viewing (the cache).
    pub cache: u64,
    pub download: p2pcore::PictureDownload,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Page {
    Settings,
    /// Settings > Notifications / Sounds (the `section` of the settings route).
    Notifications,
    Sounds,
    /// Settings > Video / Storage / About Tinline.
    Video,
    Storage,
    About,
    Me,
    Person(String),
    /// Native-only pages kept reachable from "This computer" (audio, video, security, ...).
    Native(u8),
}

/// Known additional-information keys (info.ts KNOWN): label, link?, hint, AddInfo icon.
pub const KNOWN: [(&str, &str, bool, &str, &str); 3] = [
    ("pronouns", "Pronouns", false, "For example, she/her", "users"),
    ("location", "Location", false, "City or region", "info"),
    ("website", "Website", true, "https://", "link"),
];
pub fn known(k: &str) -> Option<(&'static str, bool, &'static str, &'static str)> {
    KNOWN.iter().find(|x| x.0 == k).map(|x| (x.1, x.2, x.3, x.4))
}

/// Availability.svelte BUSY_CHOICES: id, label.
pub const BUSY_CHOICES: [(&str, &str); 5] = [("30m", "30 minutes"), ("1h", "1 hour"), ("4h", "4 hours"), ("tom", "Tomorrow morning"), ("open", "No end time")];

#[derive(Debug, Clone)]
pub enum Action {
    Open(Page),
    Back,
    StorageLoaded(Option<StorageInfo>),
    SetDownload(p2pcore::PictureDownload),
    ClearCache,
    CacheCleared,
    /// About Tinline: expand or fold the licence list.
    ToggleLicences,
    /// About Tinline: open the source code page in the browser.
    OpenSource,
    // FieldRow
    Edit(String),
    Input(String),
    Body(text_editor::Action),
    CancelEdit,
    Save,
    Remove(String),
    AskRemove(String),
    SavedOff(String, u64),
    // Availability
    Available,
    Busy(&'static str),
    // AddInfo
    Add,
    AddPick(Option<String>),
    AddName(String),
    AddValue(String),
    AddSave,
    // Picture
    PickPhoto,
    Picked(u64, Result<Option<Picked>, String>),
    DismissPicError,
    Nudge(f32, f32),
    Zoom(f32),
    Recenter,
    DragStart,
    DragAt(iced::Point),
    DragEnd,
    UsePhoto,
    RemovePhoto,
    ConfirmRemovePhoto,
    Preview { epoch: u64, generation: u64, result: Result<Preview, String> },
    // Layers
    CloseLayer,
    AskLink(String),
    OpenLink,
    AskBlock(String),
    ConfirmBlock,
    // Node completions
    Loaded { epoch: u64, owner: String, did: String, generation: u64, result: Result<Option<Cached>, String> },
    Saved { epoch: u64, owner: String, what: Save, result: Result<(), String> },
}

/// What a write was, so its completion lands on the right row or layer.
#[derive(Debug, Clone, PartialEq)]
pub enum Save {
    Field(String),
    Remove(String),
    Availability,
    Add(String),
    Picture,
    RemovePicture,
}

#[derive(Debug, Clone, Default)]
pub struct Cached {
    pub fields: Map<String, Value>,
    pub hash: Option<String>,
    /// Decoded, circle-masked picture (the Avatar's `img` with object-fit: cover).
    pub photo: Option<Handle>,
    /// A picture is set but not cached yet (Avatar `awaiting`: initials with a breathing ring).
    pub awaiting: bool,
}

#[derive(Debug, Clone)]
pub struct Picked {
    pub source: Arc<image::RgbaImage>,
}

#[derive(Debug, Clone)]
pub struct Preview {
    pub stage: Handle,
    pub sizes: [Handle; 3],
}

/// images.ts Crop: the picture's top-left offset in the 264px stage, and its zoom (1..4).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crop {
    pub ox: f32,
    pub oy: f32,
    pub zoom: f32,
}

pub fn cover_scale(w: f32, h: f32, view: f32, zoom: f32) -> f32 {
    (view / w).max(view / h) * zoom
}
pub fn clamp_crop(c: Crop, w: f32, h: f32, view: f32) -> Crop {
    let s = cover_scale(w, h, view, c.zoom);
    let (min_x, min_y) = (view - w * s, view - h * s);
    Crop { zoom: c.zoom, ox: c.ox.max(min_x).min(0.0), oy: c.oy.max(min_y).min(0.0) }
}
pub fn centred(w: f32, h: f32, view: f32, zoom: f32) -> Crop {
    let s = cover_scale(w, h, view, zoom);
    Crop { zoom, ox: (view - w * s) / 2.0, oy: (view - h * s) / 2.0 }
}
pub fn zoom_at(c: Crop, zoom: f32, w: f32, h: f32, view: f32) -> Crop {
    let (s0, s1) = (cover_scale(w, h, view, c.zoom), cover_scale(w, h, view, zoom));
    let (cx, cy) = ((view / 2.0 - c.ox) / s0, (view / 2.0 - c.oy) / s0);
    clamp_crop(Crop { zoom, ox: view / 2.0 - cx * s1, oy: view / 2.0 - cy * s1 }, w, h, view)
}

/// FieldRow edit/remove state; one row at a time. The draft survives a failed save.
pub struct Edit {
    pub key: String,
    pub text: String,
    pub body: text_editor::Content,
    pub error: String,
    /// `mode === 'remove'`: a structured value asks before it goes.
    pub removing: bool,
}

pub enum Layer {
    Add { pick: Option<String>, name: String, value: String, error: String },
    Picture { picked: Picked, crop: Crop, busy: bool, error: String },
    RemovePhoto { error: String },
    Link(String),
    Block(String),
}

#[derive(Default)]
pub struct State {
    pub page: Option<Page>,
    /// Settings > Storage: what the last measurement found (None until it arrives).
    pub storage: Option<StorageInfo>,
    /// "Clear cache" was pressed since the page opened.
    pub cache_cleared: bool,
    /// About Tinline: the open-source licence list is expanded.
    pub licences_open: bool,
    pub history: Vec<Page>,
    pub cache: HashMap<String, Cached>,
    generations: HashMap<String, u64>,
    pub edit: Option<Edit>,
    pub layer: Option<Layer>,
    pub preview: Option<Preview>,
    preview_generation: u64,
    /// A write is running (one at a time, like the playground's synchronous saves).
    pub pending: bool,
    /// MyProfile `picError`: a refused picture, floating over the page bottom.
    pub pic_error: String,
    /// Rows showing "Saved" (key → token), Availability uses "availability".
    pub saved: HashMap<String, u64>,
    saved_seq: u64,
    /// Availability: the chosen Busy length and a refusal.
    pub busy_choice: Option<&'static str>,
    pub av_error: String,
    /// A save was accepted while offline (MyProfile `px.pending` strip).
    pub offline_saves: bool,
    drag: Option<iced::Point>,
}

struct Bridge {
    epoch: u64,
    tx: tokio::sync::mpsc::UnboundedSender<Ev>,
}
impl p2pcore::ProfileEvents for Bridge {
    fn on_profile_changed(&self, did: String) {
        let _ = self.tx.send(Ev::Profile { epoch: self.epoch, did });
    }
}

/// Value.svelte `show()` for scalars; structure is drawn by the view.
pub fn scalar(v: &Value) -> Option<String> {
    match v {
        Value::Null => Some("\u{2014}".into()),
        Value::Bool(b) => Some(if *b { "Yes" } else { "No" }.into()),
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// info.ts humanize: "work-title" / "work_title" / "workTitle" -> "Work title".
pub fn humanize(k: &str) -> String {
    let mut out = String::new();
    let mut prev_lower = false;
    for c in k.chars() {
        if matches!(c, '-' | '_' | '.') {
            if !out.ends_with(' ') {
                out.push(' ');
            }
            prev_lower = false;
            continue;
        }
        if c.is_uppercase() && prev_lower {
            out.push(' ');
        }
        prev_lower = c.is_lowercase();
        out.push(c);
    }
    let s = out.trim().to_string();
    let mut it = s.chars();
    match it.next() {
        Some(f) => f.to_uppercase().collect::<String>() + it.as_str(),
        None => k.to_string(),
    }
}
pub fn label(k: &str) -> String {
    known(k).map(|k| k.0.to_string()).unwrap_or_else(|| humanize(k))
}
pub fn reserved(k: &str) -> bool {
    matches!(k, "name" | "status" | "about" | "availability" | "picture")
}
/// info.ts extraKeys: known ones first, then the rest in stored order.
pub fn extra_keys(fields: &Map<String, Value>) -> Vec<String> {
    let mut out: Vec<String> = KNOWN.iter().map(|k| k.0).filter(|k| fields.contains_key(*k)).map(str::to_string).collect();
    out.extend(fields.keys().filter(|k| !reserved(k) && known(k).is_none()).cloned());
    out
}
/// info.ts safeUrl: http(s) only, no credentials.
pub fn safe_url(raw: &str) -> Option<String> {
    let t = raw.trim();
    let rest = t.strip_prefix("https://").or_else(|| t.strip_prefix("http://"))?;
    let host = rest.split(['/', '?', '#']).next()?;
    if host.is_empty() || host.contains('@') || t.chars().any(|c| c.is_whitespace() || c.is_control() || c == '\\') {
        return None;
    }
    Some(if rest.contains('/') { t.to_string() } else { format!("{t}/") })
}
/// info.ts normaliseLink: a bare "arjun.example" becomes https://arjun.example/.
pub fn normalise_link(raw: &str) -> Option<String> {
    let t = raw.trim();
    if t.is_empty() {
        return None;
    }
    let has_scheme = t.split_once(':').is_some_and(|(s, _)| !s.is_empty() && s.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) && s.chars().all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c)));
    safe_url(&if has_scheme { t.to_string() } else { format!("https://{t}") })
}

fn has_control(s: &str, allow_newline: bool) -> bool {
    s.chars().any(|c| {
        let bidi = matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}');
        bidi || (c.is_control() && !(allow_newline && matches!(c, '\n' | '\r' | '\t')))
    })
}
/// profile.ts checkName.
pub fn check_name(raw: &str, what: &str) -> Result<(), String> {
    let s = raw.trim();
    if s.is_empty() {
        return Err(format!("The {what} can\u{2019}t be empty."));
    }
    if has_control(s, false) {
        return Err(format!("The {what} can\u{2019}t contain control or text-direction characters."));
    }
    if s.len() > LIMIT_NAME {
        return Err(format!("That {what} is too long. Up to {LIMIT_NAME} bytes; some characters take more than one."));
    }
    Ok(())
}
/// profile.ts checkText.
pub fn check_text(s: &str, what: &str) -> Result<(), String> {
    if s.len() > LIMIT_TEXT {
        return Err(format!("That is too long for {what}: up to {LIMIT_TEXT} bytes, and some characters take more than one."));
    }
    if has_control(s, true) {
        let mut c = what.chars();
        let cap = c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default();
        return Err(format!("{cap} can\u{2019}t contain control or text-direction characters."));
    }
    Ok(())
}

/// images.ts inspectImage, on a worker: size, sniffed kind, decodability, pixel limits. The messages are the playground's.
pub fn inspect(bytes: &[u8]) -> Result<image::RgbaImage, String> {
    use image::{ImageDecoder, ImageFormat, ImageReader};
    const MAX: usize = 2 * 1024 * 1024;
    let mb = |n: usize| {
        let v = format!("{:.1}", n as f64 / (1024.0 * 1024.0));
        v.trim_end_matches(".0").to_string()
    };
    if bytes.len() > MAX {
        return Err(format!("That picture is {} MB. Pictures can be up to {} MB.", mb(bytes.len()), mb(MAX)));
    }
    let kind = match bytes {
        [0x89, 0x50, 0x4E, 0x47, ..] => ImageFormat::Png,
        [0xFF, 0xD8, 0xFF, ..] => ImageFormat::Jpeg,
        [0x52, 0x49, 0x46, 0x46, _, _, _, _, 0x57, 0x45, 0x42, 0x50, ..] => ImageFormat::WebP,
        _ => return Err("Tinline can use PNG, JPEG or WebP pictures. That file is a different kind of file.".into()),
    };
    let damaged = || "That file couldn\u{2019}t be read as a picture. It may be damaged.".to_string();
    let mut r = ImageReader::with_format(Cursor::new(bytes), kind);
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(96 * 1024 * 1024);
    r.limits(limits);
    let dec = r.into_decoder().map_err(|_| damaged())?;
    let (w, h) = dec.dimensions();
    if w > 4096 || h > 4096 {
        return Err(format!("That picture is {w} \u{d7} {h} pixels. Tinline accepts up to 4096 pixels on a side."));
    }
    if w == 0 || h == 0 {
        return Err(damaged());
    }
    image::DynamicImage::from_decoder(dec).map(|i| i.to_rgba8()).map_err(|_| damaged())
}

/// images.ts renderSquare: the stage's square, at most 512 px, as PNG.
pub fn render_square(src: &image::RgbaImage, crop: Crop) -> image::RgbaImage {
    let (w, h) = (src.width() as f32, src.height() as f32);
    let s = cover_scale(w, h, VIEW, crop.zoom);
    let side = VIEW / s;
    let out = (side.round().max(1.0) as u32).min(512);
    let (x, y) = ((-crop.ox / s).max(0.0), (-crop.oy / s).max(0.0));
    let sq = image::imageops::crop_imm(src, x.round() as u32, y.round() as u32, side.round().max(1.0) as u32, side.round().max(1.0) as u32).to_image();
    image::imageops::resize(&sq, out, out, image::imageops::FilterType::CatmullRom)
}

/// The Avatar: `object-fit: cover` in a circle, with the 1px hairline that keeps a pale photo's
/// edge (rgb(0 0 0 / .1) light, rgb(255 255 255 / .12) dark). Rendered at `px` device pixels.
pub fn circle(src: &image::RgbaImage, px: u32, dark: bool) -> Handle {
    let side = src.width().min(src.height());
    let sq = image::imageops::crop_imm(src, (src.width() - side) / 2, (src.height() - side) / 2, side, side).to_image();
    let mut img = image::imageops::resize(&sq, px, px, image::imageops::FilterType::CatmullRom);
    let r = px as f32 / 2.0;
    let ring_w = 1.0 * (px as f32 / 112.0).max(1.0);
    let ring = if dark { [255u8, 255, 255, 31] } else { [0u8, 0, 0, 26] };
    for (x, y, p) in img.enumerate_pixels_mut() {
        let d = ((x as f32 + 0.5 - r).powi(2) + (y as f32 + 0.5 - r).powi(2)).sqrt();
        // hairline inset ring (inset 0 0 0 1px), blended over the picture
        let ring_a = ((d - (r - ring_w)).clamp(0.0, 1.0)) * ring[3] as f32 / 255.0;
        for c in 0..3 {
            p.0[c] = (p.0[c] as f32 * (1.0 - ring_a) + ring[c] as f32 * ring_a).round() as u8;
        }
        p.0[3] = (p.0[3] as f32 * (r - d + 0.5).clamp(0.0, 1.0)).round() as u8;
    }
    Handle::from_rgba(px, px, img.into_raw())
}

/// Cropper stage: the picture at (ox, oy) scaled to cover, everything outside the round window
/// dimmed with rgb(10 16 14 / .5), a 1.5px rgb(255 255 255 / .85) edge, corners at --r-ctl.
pub fn stage(src: &image::RgbaImage, crop: Crop, scale: f32, surface2: [u8; 3]) -> Handle {
    let px = (VIEW * scale).round() as u32;
    let (w, h) = (src.width() as f32, src.height() as f32);
    let s = cover_scale(w, h, VIEW, crop.zoom) * scale;
    let scaled = image::imageops::resize(src, (w * s).round().max(1.0) as u32, (h * s).round().max(1.0) as u32, image::imageops::FilterType::Triangle);
    let mut out = image::RgbaImage::from_pixel(px, px, image::Rgba([surface2[0], surface2[1], surface2[2], 255]));
    image::imageops::overlay(&mut out, &scaled, (crop.ox * scale).round() as i64, (crop.oy * scale).round() as i64);
    let r = px as f32 / 2.0;
    let edge = 1.5 * scale;
    let corner = 14.0 * scale;
    for (x, y, p) in out.enumerate_pixels_mut() {
        let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
        let d = ((fx - r).powi(2) + (fy - r).powi(2)).sqrt();
        let outside = (d - r + 0.5).clamp(0.0, 1.0);
        let dim = 0.5 * outside;
        for (c, base) in [10.0f32, 16.0, 14.0].iter().enumerate() {
            p.0[c] = (p.0[c] as f32 * (1.0 - dim) + base * dim).round() as u8;
        }
        let ring = ((d - (r - edge)).clamp(0.0, 1.0) * (1.0 - outside)) * 0.85;
        for c in 0..3 {
            p.0[c] = (p.0[c] as f32 * (1.0 - ring) + 255.0 * ring).round() as u8;
        }
        // rounded stage corners (border-radius: var(--r-ctl); overflow: hidden)
        let cx = fx.clamp(corner, px as f32 - corner);
        let cy = fy.clamp(corner, px as f32 - corner);
        let cd = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
        p.0[3] = (255.0 * (corner - cd + 0.5).clamp(0.0, 1.0)).round() as u8;
    }
    Handle::from_rgba(px, px, out.into_raw())
}

fn read_profile(node: &Node, owner: &str, did: &str, scale: f32, dark: bool) -> Result<Option<Cached>, String> {
    if node.profile().is_none_or(|p| p.did != owner) {
        return Err("Account changed".into());
    }
    let Some(p) = node.account_profile(did.into()).map_err(s)? else { return Ok(None) };
    let fields: Map<String, Value> = serde_json::from_str(&p.fields_json).map_err(s)?;
    let mut c = Cached { fields, hash: p.picture_hash.clone(), ..Default::default() };
    if p.picture_hash.is_some() && !p.picture_ready {
        c.awaiting = true;
    } else if p.picture_ready {
        // A picture that fails to decode falls back to initials (Avatar `badUrl`).
        if let Ok(Some(bytes)) = node.profile_picture(did.into())
            && let Ok(img) = inspect(&bytes)
        {
            c.photo = Some(circle(&img, (112.0 * scale).ceil() as u32, dark));
        }
    }
    let current = node.account_profile(did.into()).map_err(s)?;
    if current.as_ref().is_none_or(|latest| latest.picture_hash != p.picture_hash || latest.picture_ready != p.picture_ready) {
        return Err("Profile picture changed while loading".into());
    }
    if node.profile().is_none_or(|p| p.did != owner) {
        return Err("Account changed".into());
    }
    Ok(Some(c))
}

impl App {
    pub(super) fn install_profile_events(&self) {
        self.node.set_profile_events(Arc::new(Bridge { epoch: self.chat.account_epoch, tx: INIT.get().unwrap().tx.clone() }));
    }
    pub(super) fn profile_owner(&self) -> Option<String> {
        self.me.as_ref().map(|c| c.did.clone())
    }
    pub(super) fn own_profile(&self) -> Option<&Cached> {
        self.profile_owner().and_then(|d| self.profiles.cache.get(&d))
    }
    pub(super) fn refresh_profile(&mut self, did: String) -> Task<Msg> {
        if self.demo {
            return Task::none();
        }
        let Some(owner) = self.profile_owner() else { return Task::none() };
        let epoch = self.chat.account_epoch;
        let generation = self.profiles.generations.entry(did.clone()).or_default();
        *generation += 1;
        let generation = *generation;
        let (node, o, d, scale, dark) = (self.node.clone(), owner.clone(), did.clone(), ui::scale(), self.dark);
        blocking(move || read_profile(&node, &o, &d, scale, dark), move |result| Msg::Profile(Action::Loaded { epoch, owner: owner.clone(), did: did.clone(), generation, result }))
    }
    pub(super) fn refresh_profiles(&mut self) -> Task<Msg> {
        let mut dids = self.contacts.iter().map(|c| c.did.clone()).collect::<Vec<_>>();
        if let Some(me) = self.profile_owner() {
            dids.push(me);
        }
        Task::batch(dids.into_iter().map(|did| self.refresh_profile(did)).collect::<Vec<_>>())
    }
    fn profile_completion_current(&self, epoch: u64, owner: &str) -> bool {
        epoch == self.chat.account_epoch && self.profile_owner().as_deref() == Some(owner) && (self.demo || self.node.profile().is_some_and(|p| p.did == owner))
    }
    /// One write at a time, on a worker, for the account that is selected now.
    pub(super) fn profile_write(&mut self, what: Save, f: impl FnOnce(&Node) -> Result<(), String> + Send + 'static) -> Task<Msg> {
        if !mutation_allowed(self.busy, self.profiles.pending) || self.lock != p2pcore::LockState::Unlocked {
            self.notice = Some("Wait for the current account operation to finish.".into());
            return Task::none();
        }
        let Some(owner) = self.profile_owner() else { return Task::none() };
        let epoch = self.chat.account_epoch;
        self.profiles.pending = true;
        if self.demo {
            return Task::done(Msg::Profile(Action::Saved { epoch, owner, what, result: Ok(()) }));
        }
        let node = self.node.clone();
        let o = owner.clone();
        blocking(
            move || {
                if node.profile().is_none_or(|p| p.did != o) {
                    return Err("Account changed".into());
                }
                f(&node)
            },
            move |result| Msg::Profile(Action::Saved { epoch, owner: owner.clone(), what: what.clone(), result }),
        )
    }
    fn prepare_preview(&mut self) -> Task<Msg> {
        let Some(Layer::Picture { picked, crop, .. }) = &self.profiles.layer else { return Task::none() };
        let (src, crop) = (picked.source.clone(), *crop);
        self.profiles.preview_generation += 1;
        let (generation, epoch, scale) = (self.profiles.preview_generation, self.chat.account_epoch, ui::scale());
        let t = self.t();
        let s2 = [t.surface2.r, t.surface2.g, t.surface2.b].map(|v| (v * 255.0).round() as u8);
        let dark = self.dark;
        blocking(
            move || {
                let sq = render_square(&src, crop);
                let sizes = [72.0f32, 48.0, 40.0].map(|v| circle(&sq, (v * scale).ceil() as u32, dark));
                Ok(Preview { stage: stage(&src, crop, scale, s2), sizes })
            },
            move |result| Msg::Profile(Action::Preview { epoch, generation, result }),
        )
    }
    fn mark_saved(&mut self, key: &str) -> Task<Msg> {
        self.profiles.saved_seq += 1;
        let token = self.profiles.saved_seq;
        self.profiles.saved.insert(key.to_string(), token);
        let key = key.to_string();
        Task::perform(tokio::time::sleep(Duration::from_millis(SAVED_MS)), move |_| Msg::Profile(Action::SavedOff(key.clone(), token)))
    }
    fn crop_layer(&mut self, f: impl FnOnce(Crop, f32, f32) -> Crop) -> Task<Msg> {
        if let Some(Layer::Picture { picked, crop, .. }) = self.profiles.layer.as_mut() {
            let (w, h) = (picked.source.width() as f32, picked.source.height() as f32);
            let next = f(*crop, w, h);
            if next == *crop {
                return Task::none();
            }
            *crop = next;
            return self.prepare_preview();
        }
        Task::none()
    }

    /// Reads the sizes off the main thread (the chat records are scanned).
    fn measure_storage(&self) -> Task<Msg> {
        let node = self.node.clone();
        blocking(move || {
            let usage = node.storage_usage().ok()?;
            Some(StorageInfo { usage, cache: crate::media::cache_bytes(), download: node.picture_download().unwrap_or_default() })
        }, |i| Msg::Profile(Action::StorageLoaded(i)))
    }

    pub(super) fn profile_update(&mut self, action: Action) -> Task<Msg> {
        if self.busy && matches!(action, Action::Save | Action::Remove(_) | Action::Available | Action::Busy(_) | Action::AddSave | Action::UsePhoto | Action::ConfirmRemovePhoto | Action::ConfirmBlock) {
            self.notice = Some("Wait for the current account operation to finish.".into());
            return Task::none();
        }
        match action {
            Action::Open(page) => {
                self.leave_chat();
                self.screen = Screen::Settings;
                self.profiles.edit = None;
                self.profiles.layer = None;
                self.profiles.pic_error.clear();
                if let Some(old) = self.profiles.page.replace(page.clone()) {
                    self.profiles.history.push(old);
                }
                self.refresh_accounts();
                if let Page::Native(tab) = page {
                    self.settings_tab = tab;
                    self.refresh_linked();
                }
                // The camera list is read when the Video page opens (no "Find cameras" button).
                if page == Page::Native(24) && self.remember_avail.is_none() {
                    return Task::batch([self.refresh_profiles(), self.probe_remember()]);
                }
                if page == Page::Storage {
                    self.profiles.cache_cleared = false;
                    return Task::batch([self.refresh_profiles(), self.measure_storage()]);
                }
                if page == Page::Video {
                    return Task::batch([self.refresh_profiles(), self.video_update(super::Vm::ScanCameras)]);
                }
                return self.refresh_profiles();
            }
            Action::StorageLoaded(info) => self.profiles.storage = info,
            Action::SetDownload(p) => {
                if let Some(i) = self.profiles.storage.as_mut() { i.download = p; }
                let node = self.node.clone();
                return blocking(move || node.set_picture_download(p).map_err(s), Msg::Done);
            }
            Action::ClearCache => {
                return blocking(|| { crate::media::wipe_now(); }, |_| Msg::Profile(Action::CacheCleared));
            }
            Action::CacheCleared => {
                self.profiles.cache_cleared = true;
                if let Some(i) = self.profiles.storage.as_mut() { i.cache = 0; }
            }
            Action::ToggleLicences => self.profiles.licences_open = !self.profiles.licences_open,
            Action::OpenSource => {
                return blocking(move || open::that_detached(SOURCE_URL).map_err(s), Msg::Done);
            }
            Action::Back => {
                self.profiles.edit = None;
                self.profiles.layer = None;
                self.profiles.pic_error.clear();
                self.hide_phrase();
                match self.profiles.history.pop() {
                    Some(page) => self.profiles.page = Some(page),
                    None => {
                        self.profiles.page = None;
                        self.screen = Screen::Home;
                    }
                }
            }
            Action::Edit(key) => {
                let text = if key == "alias" {
                    self.profile_person().and_then(|c| c.alias.clone()).unwrap_or_default()
                } else if key == "name" {
                    self.profile_name.clone()
                } else {
                    self.own_profile().and_then(|c| c.fields.get(&key)).and_then(scalar).unwrap_or_default()
                };
                self.profiles.edit = Some(Edit { key, body: text_editor::Content::with_text(&text), text, error: String::new(), removing: false });
            }
            Action::AskRemove(key) => {
                self.profiles.edit = Some(Edit { key, text: String::new(), body: text_editor::Content::new(), error: String::new(), removing: true });
            }
            Action::Input(v) => {
                if let Some(e) = self.profiles.edit.as_mut() {
                    e.text = v;
                }
            }
            Action::Body(a) => {
                if let Some(e) = self.profiles.edit.as_mut() {
                    e.body.perform(a);
                    e.text = e.body.text();
                }
            }
            Action::CancelEdit => {
                if !self.profiles.pending {
                    self.profiles.edit = None;
                }
            }
            Action::Save => {
                let Some(e) = self.profiles.edit.as_mut() else { return Task::none() };
                let key = e.key.clone();
                let raw = e.text.clone();
                let checked = match key.as_str() {
                    "name" => check_name(&raw, "name").map(|_| Some(raw.trim().to_string())),
                    "alias" => {
                        let v = raw.trim().to_string();
                        if v.is_empty() { Ok(None) } else { check_name(&v, "nickname").map(|_| Some(v)) }
                    }
                    "status" | "about" => {
                        let v = raw.trim().to_string();
                        if v.is_empty() { Ok(None) } else { check_text(&v, if key == "status" { "a status" } else { "about" }).map(|_| Some(v)) }
                    }
                    k if known(k).is_some_and(|k| k.1) => {
                        if raw.trim().is_empty() { Ok(None) } else { normalise_link(&raw).map(Some).ok_or_else(|| "Enter a web address that starts with http:// or https://.".to_string()) }
                    }
                    _ => {
                        let v = raw.trim().to_string();
                        if v.is_empty() { Ok(None) } else { check_text(&v, "this").map(|_| Some(v)) }
                    }
                };
                let value = match checked {
                    Ok(v) => v,
                    Err(err) => {
                        e.error = err;
                        return Task::none();
                    }
                };
                e.error.clear();
                let what = Save::Field(key.clone());
                if key == "alias" {
                    let Some(did) = self.profile_person().map(|c| c.did.clone()) else { return Task::none() };
                    return self.profile_write(what, move |n| n.rename_contact(did, value).map_err(s));
                }
                return self.profile_write(what, move |n| match (key.as_str(), value) {
                    ("name", Some(v)) => n.set_name(v).map_err(s),
                    (_, v) => n.set_profile_field(key, v.map(|v| serde_json::to_string(&v).unwrap())).map_err(s),
                });
            }
            Action::Remove(key) => {
                if key == "alias" {
                    let Some(did) = self.profile_person().map(|c| c.did.clone()) else { return Task::none() };
                    return self.profile_write(Save::Remove(key), move |n| n.rename_contact(did, None).map_err(s));
                }
                let k = key.clone();
                return self.profile_write(Save::Remove(key), move |n| n.set_profile_field(k, None).map_err(s));
            }
            Action::SavedOff(key, token) => {
                if self.profiles.saved.get(&key) == Some(&token) {
                    self.profiles.saved.remove(&key);
                }
            }
            Action::Available => {
                if self.avail.available {
                    return Task::none();
                }
                self.profiles.av_error.clear();
                return self.profile_write(Save::Availability, |n| n.set_available(true, None).map_err(s));
            }
            Action::Busy(id) => {
                if !self.demo {
                    self.profiles.av_error = availability_problem(false).unwrap().into();
                    return Task::none();
                }
                self.profiles.busy_choice = Some(id);
                self.profiles.av_error.clear();
                let until = busy_until(id);
                return self.profile_write(Save::Availability, move |n| n.set_available(false, until).map_err(s));
            }
            Action::Add => {
                self.profiles.layer = Some(Layer::Add { pick: None, name: String::new(), value: String::new(), error: String::new() });
                return operation::focus(ADD_VALUE_ID);
            }
            Action::AddPick(p) => {
                if let Some(Layer::Add { pick, error, .. }) = self.profiles.layer.as_mut() {
                    let first = if p.as_deref() == Some("") { ADD_NAME_ID } else { ADD_VALUE_ID };
                    *pick = p;
                    error.clear();
                    return operation::focus(first);
                }
            }
            Action::AddName(v) => {
                if let Some(Layer::Add { name, .. }) = self.profiles.layer.as_mut() {
                    *name = v;
                }
            }
            Action::AddValue(v) => {
                if let Some(Layer::Add { value, .. }) = self.profiles.layer.as_mut() {
                    *value = v;
                }
            }
            Action::AddSave => {
                let have: Vec<String> = self.own_profile().map(|c| c.fields.keys().cloned().collect()).unwrap_or_default();
                let Some(Layer::Add { pick: Some(pick), name, value, error }) = self.profiles.layer.as_mut() else { return Task::none() };
                let key = if pick.is_empty() { name.trim().to_lowercase().split_whitespace().collect::<Vec<_>>().join("-") } else { pick.clone() };
                if pick.is_empty() {
                    if name.trim().is_empty() {
                        *error = "Give the information a name.".into();
                        return Task::none();
                    }
                    if has_control(name, false) || name.len() > 256 {
                        *error = "That name can\u{2019}t be used. Keep it short, without control characters.".into();
                        return Task::none();
                    }
                    if reserved(&key) || known(&key).is_some() || have.contains(&key) {
                        *error = "You already have information with that name.".into();
                        return Task::none();
                    }
                }
                let mut v = value.trim().to_string();
                if v.is_empty() {
                    *error = "Add something to save.".into();
                    return Task::none();
                }
                if known(&key).is_some_and(|k| k.1) {
                    match normalise_link(value) {
                        Some(u) => v = u,
                        None => {
                            *error = "Enter a web address that starts with http:// or https://.".into();
                            return Task::none();
                        }
                    }
                }
                error.clear();
                let k = key.clone();
                return self.profile_write(Save::Add(key), move |n| n.set_profile_field(k, Some(serde_json::to_string(&v).unwrap())).map_err(s));
            }
            Action::PickPhoto => {
                let epoch = self.chat.account_epoch;
                self.profiles.pic_error.clear();
                return blocking(
                    move || {
                        let Some(path) = rfd::FileDialog::new().add_filter("Pictures", &["png", "jpg", "jpeg", "webp"]).pick_file() else { return Ok(None) };
                        let len = std::fs::metadata(&path).map_err(s)?.len() as usize;
                        if len > 2 * 1024 * 1024 {
                            // Never read a huge file just to refuse it.
                            return inspect(&vec![0; len.min(2 * 1024 * 1024 + 1)]).map(|_| None);
                        }
                        let bytes = std::fs::read(path).map_err(s)?;
                        inspect(&bytes).map(|img| Some(Picked { source: Arc::new(img) }))
                    },
                    move |r| Msg::Profile(Action::Picked(epoch, r)),
                );
            }
            Action::Picked(epoch, result) => {
                if epoch != self.chat.account_epoch {
                    return Task::none();
                }
                match result {
                    Ok(Some(picked)) => {
                        let (w, h) = (picked.source.width() as f32, picked.source.height() as f32);
                        self.profiles.layer = Some(Layer::Picture { crop: centred(w, h, VIEW, 1.0), picked, busy: false, error: String::new() });
                        self.profiles.preview = None;
                        return self.prepare_preview();
                    }
                    Err(e) => self.profiles.pic_error = e,
                    Ok(None) => {}
                }
            }
            Action::DismissPicError => self.profiles.pic_error.clear(),
            Action::Nudge(dx, dy) => return self.crop_layer(|c, w, h| clamp_crop(Crop { ox: c.ox + dx, oy: c.oy + dy, ..c }, w, h, VIEW)),
            Action::Zoom(z) => return self.crop_layer(|c, w, h| zoom_at(c, z.clamp(1.0, 4.0), w, h, VIEW)),
            Action::Recenter => return self.crop_layer(|c, w, h| centred(w, h, VIEW, c.zoom)),
            Action::DragStart => self.profiles.drag = Some(iced::Point::new(f32::NAN, f32::NAN)),
            Action::DragAt(p) => {
                if let Some(old) = self.profiles.drag.replace(p) {
                    if old.x.is_finite() {
                        let (dx, dy) = (p.x - old.x, p.y - old.y);
                        return self.crop_layer(|c, w, h| clamp_crop(Crop { ox: c.ox + dx, oy: c.oy + dy, ..c }, w, h, VIEW));
                    }
                } else {
                    self.profiles.drag = None;
                }
            }
            Action::DragEnd => self.profiles.drag = None,
            Action::UsePhoto => {
                let Some(Layer::Picture { picked, crop, busy, error }) = self.profiles.layer.as_mut() else { return Task::none() };
                if *busy {
                    return Task::none();
                }
                *busy = true;
                error.clear();
                let (src, crop) = (picked.source.clone(), *crop);
                return self.profile_write(Save::Picture, move |n| {
                    let sq = render_square(&src, crop);
                    let mut bytes = Cursor::new(Vec::new());
                    image::DynamicImage::ImageRgba8(sq).write_to(&mut bytes, image::ImageFormat::Png).map_err(|_| "Couldn\u{2019}t prepare that picture. Try a different one.".to_string())?;
                    let bytes = bytes.into_inner();
                    if bytes.len() > 1024 * 1024 {
                        return Err("Couldn\u{2019}t prepare that picture. Try a different one.".into());
                    }
                    n.replace_profile_picture(bytes).map_err(s)
                });
            }
            Action::RemovePhoto => self.profiles.layer = Some(Layer::RemovePhoto { error: String::new() }),
            Action::ConfirmRemovePhoto => return self.profile_write(Save::RemovePicture, |n| n.remove_profile_picture().map_err(s)),
            Action::Preview { epoch, generation, result } => {
                if epoch == self.chat.account_epoch && generation == self.profiles.preview_generation && matches!(self.profiles.layer, Some(Layer::Picture { .. })) {
                    match result {
                        Ok(p) => self.profiles.preview = Some(p),
                        Err(e) => {
                            if let Some(Layer::Picture { error, .. }) = self.profiles.layer.as_mut() {
                                *error = e;
                            }
                        }
                    }
                }
            }
            Action::CloseLayer => {
                if !self.profiles.pending {
                    self.profiles.layer = None;
                    self.profiles.preview = None;
                    self.profiles.preview_generation += 1;
                }
            }
            Action::AskLink(u) => {
                if let Some(u) = safe_url(&u) {
                    self.profiles.layer = Some(Layer::Link(u));
                }
            }
            Action::OpenLink => {
                if let Some(Layer::Link(u)) = self.profiles.layer.take() {
                    return blocking(move || open::that_detached(u).map_err(s), Msg::Done);
                }
            }
            Action::AskBlock(did) => self.profiles.layer = Some(Layer::Block(did)),
            Action::ConfirmBlock => {
                if let Some(Layer::Block(did)) = self.profiles.layer.take() {
                    return self.update(Msg::Block(did));
                }
            }
            Action::Loaded { epoch, owner, did, generation, result } => {
                if !self.profile_completion_current(epoch, &owner) || self.profiles.generations.get(&did) != Some(&generation) {
                    return Task::none();
                }
                match result {
                    Ok(Some(c)) => {
                        if did == owner {
                            if let Some(name) = c.fields.get("name").and_then(Value::as_str) {
                                self.profile_name = name.into();
                            }
                            self.avail = self.node.availability();
                        }
                        self.profiles.cache.insert(did, c);
                    }
                    Ok(None) => {
                        self.profiles.cache.remove(&did);
                    }
                    // Keep what is cached; a read that failed changes nothing on screen.
                    Err(_) => {}
                }
            }
            Action::Saved { epoch, owner, what, result } => {
                if !self.profile_completion_current(epoch, &owner) {
                    return Task::none();
                }
                self.profiles.pending = false;
                match result {
                    Ok(()) => {
                        if !self.status.online {
                            self.profiles.offline_saves = true;
                        }
                        let mut tasks = vec![];
                        match &what {
                            Save::Field(k) | Save::Remove(k) => {
                                self.profiles.edit = None;
                                if matches!(what, Save::Field(_)) {
                                    tasks.push(self.mark_saved(&k.clone()));
                                }
                            }
                            Save::Availability => {
                                if !self.demo {
                                    self.avail = self.node.availability();
                                }
                                tasks.push(self.mark_saved("availability"));
                            }
                            Save::Add(_) | Save::Picture | Save::RemovePicture => {
                                self.profiles.layer = None;
                                self.profiles.preview = None;
                            }
                        }
                        if !self.demo {
                            self.reload_contacts();
                        }
                        tasks.push(self.refresh_profiles());
                        return Task::batch(tasks);
                    }
                    Err(e) => {
                        let msg = if e == "Account changed" { SAVE_FAILED.to_string() } else { format!("{SAVE_FAILED} ({e})") };
                        match (&what, self.profiles.layer.as_mut(), self.profiles.edit.as_mut()) {
                            (Save::Availability, ..) => self.profiles.av_error = msg,
                            (Save::Add(_), Some(Layer::Add { error, .. }), _) => *error = msg,
                            (Save::Picture, Some(Layer::Picture { error, busy, .. }), _) => {
                                *error = msg;
                                *busy = false;
                            }
                            (Save::RemovePicture, Some(Layer::RemovePhoto { error }), _) => *error = msg,
                            (_, _, Some(edit)) => edit.error = msg,
                            _ => self.notice = Some(msg),
                        }
                    }
                }
            }
        }
        Task::none()
    }

    /// The contact whose profile page is open.
    pub(super) fn profile_person(&self) -> Option<&Contact> {
        let Some(Page::Person(did)) = &self.profiles.page else { return None };
        self.contacts.iter().chain(self.kept.iter()).find(|c| &c.did == did)
    }
}

pub const ADD_NAME_ID: &str = "add-info-name";
pub const ADD_VALUE_ID: &str = "add-info-value";

/// Availability BUSY_CHOICES `until`, in unix seconds (None: no end time).
fn busy_until(id: &str) -> Option<u64> {
    use chrono::{Local, TimeZone};
    let now = Local::now();
    let secs = now.timestamp() as u64;
    match id {
        "30m" => Some(secs + 30 * 60),
        "1h" => Some(secs + 3600),
        "4h" => Some(secs + 4 * 3600),
        "tom" => {
            let t = (now.date_naive() + chrono::Duration::days(1)).and_hms_opt(8, 0, 0)?;
            Local.from_local_datetime(&t).single().map(|d| d.timestamp() as u64)
        }
        _ => None,
    }
}

/// info.ts untilLabel: "18:00", "tomorrow 08:00", "Friday 08:00".
pub fn until_label(until_secs: u64) -> String {
    use chrono::{Local, TimeZone};
    let Some(a) = Local.timestamp_opt(until_secs as i64, 0).single() else { return String::new() };
    let today = Local::now().date_naive();
    let hm = a.format("%H:%M").to_string();
    if a.date_naive() == today {
        hm
    } else if a.date_naive() == today + chrono::Duration::days(1) {
        format!("tomorrow {hm}")
    } else {
        format!("{} {hm}", a.format("%A"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recovery_mutations_exclude_transitions_in_both_directions() {
        assert!(mutation_allowed(false, false));
        assert!(!mutation_allowed(true, false));
        assert!(!mutation_allowed(false, true));
        assert!(!mutation_allowed(true, true));
    }
    #[test]
    fn recovery_busy_is_not_core_unavailability() {
        assert_eq!(availability_problem(false), Some(BUSY_UNSUPPORTED));
        assert_eq!(availability_problem(true), None);
    }
    #[test]
    fn profile_link_safety_matches_info_ts() {
        assert_eq!(normalise_link("arjun.example/notes").as_deref(), Some("https://arjun.example/notes"));
        assert_eq!(normalise_link("arjun.example").as_deref(), Some("https://arjun.example/"));
        for s in ["javascript:alert(1)", "https://a:b@host/", "file:///tmp/a", "https://", "https://x\\y", "https://a b", "ftp://x"] {
            assert!(normalise_link(s).is_none(), "{s}");
        }
    }
    #[test]
    fn profile_humanize_and_order_match_info_ts() {
        assert_eq!(humanize("work-title"), "Work title");
        assert_eq!(humanize("workTitle"), "Work Title");
        let f: Map<String, Value> = serde_json::from_value(serde_json::json!({"zeta": 1, "website": "x", "status": "s", "pronouns": "p"})).unwrap();
        assert_eq!(extra_keys(&f), vec!["pronouns", "website", "zeta"]);
    }
    #[test]
    fn profile_crop_math_matches_images_ts() {
        let (w, h) = (800.0, 500.0);
        let c = centred(w, h, VIEW, 1.0);
        let s = cover_scale(w, h, VIEW, 1.0);
        assert!((h * s - VIEW).abs() < 1e-3 && c.oy.abs() < 1e-3 && c.ox < 0.0);
        let far = clamp_crop(Crop { ox: 999.0, oy: -999.0, zoom: 2.0 }, w, h, VIEW);
        let s2 = cover_scale(w, h, VIEW, 2.0);
        assert_eq!(far.ox, 0.0);
        assert!((far.oy - (VIEW - h * s2)).abs() < 1e-3);
        let z = zoom_at(c, 2.0, w, h, VIEW);
        assert!(((VIEW / 2.0 - z.ox) / s2 - (VIEW / 2.0 - c.ox) / s).abs() < 1e-2, "zoom keeps the centre point");
    }
    #[test]
    fn profile_inspect_refuses_with_playground_words() {
        assert!(inspect(b"not a picture").unwrap_err().contains("PNG, JPEG or WebP"));
        assert!(inspect(&vec![0; 2 * 1024 * 1024 + 1]).unwrap_err().contains("up to 2 MB"));
        let mut png = Vec::new();
        image::DynamicImage::new_rgba8(4100, 2).write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        assert!(inspect(&png).unwrap_err().contains("4100 \u{d7} 2 pixels"));
    }
    #[test]
    fn profile_render_square_is_at_most_512() {
        let src = image::RgbaImage::new(2000, 1000);
        let out = render_square(&src, centred(2000.0, 1000.0, VIEW, 1.0));
        assert_eq!((out.width(), out.height()), (512, 512));
        let small = image::RgbaImage::new(300, 200);
        let out = render_square(&small, centred(300.0, 200.0, VIEW, 1.0));
        assert_eq!(out.width(), 200);
    }
    #[test]
    fn profile_name_checks_match_profile_ts() {
        assert!(check_name("  ", "name").unwrap_err().contains("can\u{2019}t be empty"));
        assert!(check_name("a\u{202E}b", "name").is_err());
        assert!(check_name(&"é".repeat(129), "name").unwrap_err().contains("256 bytes"));
        assert!(check_text("line\nbreak", "about").is_ok());
    }
}
