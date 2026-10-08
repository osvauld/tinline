//! In-memory chat for test-hooks builds (`P2P_CHAT_FAKE=1`, and the chat demo screens): seeded
//! conversations, delivery that turns to two ticks after a moment, downloads with progress, and
//! a "pong" to any "ping". Events go through the same channel the core's do.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use p2pcore::{
    Attachment, AttachmentKind, Chat, Contact, DayPage, DeliveryState, Error, Message, TransferState,
};
use tokio::sync::mpsc::UnboundedSender;

use crate::{ChatEv, Ev};

const MIN: u64 = 60_000;
const HOUR: u64 = 60 * MIN;
const DAY: u64 = 24 * HOUR;

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn utc_day(ms: u64) -> String {
    chrono::DateTime::from_timestamp_millis(ms as i64).map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_default()
}

fn nope(what: &str) -> Error {
    Error::Protocol(format!("fake chat: {what}"))
}

pub struct Fake {
    tx: Option<UnboundedSender<Ev>>,
    st: Mutex<State>,
}

#[derive(Default)]
struct State {
    msgs: HashMap<String, Vec<Message>>,
    unread: HashMap<String, u32>,
    seq: u64,
    asked: HashMap<String, u32>,
    names: HashMap<String, String>,
}

/// A varied waveform of 64 peaks.
pub fn wave() -> Vec<u8> {
    (0..64).map(|i| (60.0 + 150.0 * ((i as f32 * 0.45).sin() * (i as f32 * 0.13).cos()).abs()) as u8).collect()
}

fn base(id: &str, peer: &str, outgoing: bool, at: u64, text: &str) -> Message {
    Message {
        id: id.into(),
        peer_did: peer.into(),
        author_did: if outgoing { "did:me".into() } else { peer.into() },
        outgoing,
        at,
        text: text.into(),
        edited_at: None,
        deleted: false,
        reply_to: None,
        attachment: None,
        delivery: DeliveryState::Delivered,
    }
}

fn file(name: &str, size: u64, mime: &str, state: TransferState) -> Attachment {
    Attachment {
        hash: format!("h-{name}"),
        name: name.into(),
        size,
        mime: mime.into(),
        kind: AttachmentKind::File,
        duration_ms: 0,
        waveform: Vec::new(),
        state,
        transferred: 0,
    }
}

/// The conversation every contact starts with. `now` is unix ms.
pub fn seed(peer: &str, now: u64) -> Vec<Message> {
    let id = |n: u32| format!("{peer}-{n}");
    let mut v = vec![
        base(&id(1), peer, false, now - 5 * DAY, "Found a flat near the station, sending pics later"),
        base(&id(2), peer, true, now - 5 * DAY + 4 * MIN, "Nice! Which street?"),
        base(&id(3), peer, false, now - DAY - 2 * HOUR, "Are you around tomorrow to go over the lease?"),
        base(&id(4), peer, true, now - DAY - 2 * HOUR + 2 * MIN, "Yes, after 5. I\u{2019}ll call you."),
        base(&id(5), peer, false, now - 3 * HOUR, "Can you send the floor plan?"),
    ];
    let mut plan = base(&id(6), peer, true, now - 3 * HOUR + 2 * MIN, "");
    plan.attachment = Some(file("Floor plan \u{2013} 3rd floor.pdf", 2_400_000, "application/pdf", TransferState::Ready));
    let mut voice = base(&id(7), peer, false, now - 3 * HOUR + 5 * MIN, "");
    voice.attachment = Some(Attachment {
        hash: "h-voice".into(),
        name: "Voice message".into(),
        size: 40_000,
        mime: "audio/ogg".into(),
        kind: AttachmentKind::Voice,
        duration_ms: 32_000,
        waveform: wave(),
        state: TransferState::Ready,
        transferred: 0,
    });
    let mut r = base(&id(8), peer, false, now - 2 * HOUR, "Got it, the kitchen wall can go. Talk at 6?");
    r.reply_to = Some(id(5));
    let mut big = base(&id(9), peer, false, now - 90 * MIN, "");
    big.attachment = Some(file("Walkthrough.mp4", 184_000_000, "video/mp4", TransferState::Remote));
    let mut out = base(&id(10), peer, true, now - 20 * MIN, "Sure \u{2014} it\u{2019}s the one from Tuesday");
    out.edited_at = Some(now - 18 * MIN);
    let mut pending = base(&id(11), peer, true, now - 2 * MIN, "Running 10 min late, sorry");
    pending.delivery = DeliveryState::Pending;
    let mut photo = base(&id(12), peer, false, now - 110 * MIN, "The view from the balcony");
    photo.attachment = Some(file("Balcony view.jpg", 1_800_000, "image/jpeg", TransferState::Ready));
    let mut sketch = base(&id(13), peer, true, now - 100 * MIN, "");
    sketch.attachment = Some(file("Sketch.png", 640_000, "image/png", TransferState::Ready));
    let mut notes = base(&id(14), peer, false, now - 98 * MIN, "");
    notes.attachment = Some(file("Move-in checklist.txt", 1_200, "text/plain", TransferState::Ready));
    let mut exe = base(&id(15), peer, false, now - 96 * MIN, "");
    exe.attachment = Some(file("setup.exe", 4_200_000, "application/x-msdownload", TransferState::Ready));
    let mut sh = base(&id(16), peer, false, now - 95 * MIN, "");
    sh.attachment = Some(file("install.sh", 2_048, "text/x-shellscript", TransferState::Ready));
    v.extend([plan, voice, r, photo, sketch, notes, exe, sh, big, out, pending]);
    v
}

/// A PNG to look at: sky gradient with a sun and a hill (landscape), or a dusk-coloured portrait.
fn fake_picture(portrait: bool) -> Vec<u8> {
    let (w, h) = if portrait { (520u32, 700u32) } else { (900u32, 600u32) };
    let mut px = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
            let hill = 0.72 + 0.08 * (fx * 6.0).sin();
            let (cx, cy) = (0.7 * w as f32, 0.3 * h as f32);
            let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
            let c = if fy > hill {
                [40, (110.0 + 60.0 * fy) as u8, 70]
            } else if d < 0.09 * w as f32 {
                [255, 214, 120]
            } else if portrait {
                [(60.0 + 120.0 * fy) as u8, (50.0 + 60.0 * fy) as u8, (120.0 + 80.0 * fy) as u8]
            } else {
                [(90.0 + 90.0 * fy) as u8, (160.0 + 60.0 * fy) as u8, (235.0 - 30.0 * fy) as u8]
            };
            px.extend_from_slice(&c);
        }
    }
    let mut out = Vec::new();
    let mut e = png::Encoder::new(&mut out, w, h);
    e.set_color(png::ColorType::Rgb);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().and_then(|mut wr| wr.write_image_data(&px)).expect("png");
    out
}

impl Fake {
    /// `tx = None`: no events (screenshots that fill the state by hand).
    pub fn new(tx: Option<UnboundedSender<Ev>>) -> Self {
        Fake { tx, st: Mutex::default() }
    }

    fn emit(&self, e: ChatEv) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Ev::Chat(e));
        }
    }

    fn with<T>(&self, peer: &str, f: impl FnOnce(&mut State, &str) -> T) -> T {
        let mut st = self.st.lock().unwrap();
        st.msgs.entry(peer.to_string()).or_insert_with(|| seed(peer, now()));
        f(&mut st, peer)
    }

    fn chat_row(st: &State, peer: &str, name: &str) -> Chat {
        let last = st.msgs.get(peer).and_then(|v| v.last());
        Chat {
            peer_did: peer.into(),
            peer_name: name.into(),
            preview: last.map(super::preview_of).unwrap_or_default(),
            last_outgoing: last.is_some_and(|m| m.outgoing),
            last_delivery: last.map(|m| m.delivery).unwrap_or(DeliveryState::Delivered),
            last_activity: last.map(|m| m.at).unwrap_or(0),
            unread: st.unread.get(peer).copied().unwrap_or(0),
        }
    }

    pub fn chats(&self, contacts: Vec<Contact>) -> Result<Vec<Chat>, Error> {
        let mut out = Vec::new();
        for c in contacts {
            let name = c.alias.clone().filter(|a| !a.is_empty()).unwrap_or(c.name.clone());
            let did = c.did.clone();
            self.st.lock().unwrap().names.insert(did.clone(), name.clone());
            out.push(self.with(&did, |st, p| Self::chat_row(st, p, &name)));
        }
        out.sort_by(|a, b| b.last_activity.cmp(&a.last_activity));
        Ok(out)
    }

    pub fn chat_day(&self, peer: String, day: Option<String>) -> Result<DayPage, Error> {
        self.with(&peer, |st, p| {
            let all = &st.msgs[p];
            let mut days: Vec<String> = all.iter().map(|m| utc_day(m.at)).collect();
            days.dedup();
            days.sort();
            days.dedup();
            let want = day.or_else(|| days.last().cloned()).ok_or_else(|| nope("empty"))?;
            let older_day = days.iter().rev().find(|d| **d < want).cloned();
            let mut messages: Vec<Message> = all.iter().filter(|m| utc_day(m.at) == want).cloned().collect();
            messages.sort_by(|a, b| (a.at, &a.id).cmp(&(b.at, &b.id)));
            Ok(DayPage { day: want, messages, older_day })
        })
    }

    /// The first ask adds a day of older messages; later asks find nothing.
    pub fn fetch_older_history(&self, peer: String, before_day: String) -> Result<u32, Error> {
        std::thread::sleep(Duration::from_millis(600));
        self.with(&peer, |st, p| {
            let n = st.asked.entry(p.to_string()).or_insert(0);
            *n += 1;
            if *n > 1 {
                return Ok(0);
            }
            let at = chrono::NaiveDate::parse_from_str(&before_day, "%Y-%m-%d")
                .ok()
                .and_then(|d| d.and_hms_opt(12, 0, 0))
                .map(|d| d.and_utc().timestamp_millis() as u64 - 20 * DAY)
                .unwrap_or(0);
            let v = st.msgs.get_mut(p).unwrap();
            v.push(base(&format!("{p}-old1"), p, false, at, "We should find a place before the summer"));
            v.push(base(&format!("{p}-old2"), p, true, at + MIN, "Agreed. I\u{2019}ll start looking this weekend"));
            Ok(1)
        })
    }

    /// Two ticks after a moment, for a message of ours.
    fn deliver_later(self: &Arc<Self>, peer: &str, id: &str) {
        let (fake, peer, id) = (self.clone(), peer.to_string(), id.to_string());
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(1500));
            {
                let mut st = fake.st.lock().unwrap();
                if let Some(m) = st.msgs.get_mut(&peer).and_then(|v| v.iter_mut().find(|m| m.id == id)) {
                    m.delivery = DeliveryState::Delivered;
                }
            }
            fake.emit(ChatEv::Delivery(peer, id, DeliveryState::Delivered));
        });
    }

    fn push_out(self: &Arc<Self>, peer: &str, text: &str, reply: Option<String>, att: Option<Attachment>) -> Message {
        let m = self.with(peer, |st, p| {
            st.seq += 1;
            let mut m = base(&format!("{p}-new{}", st.seq), p, true, now(), text);
            m.delivery = DeliveryState::Pending;
            m.reply_to = reply;
            m.attachment = att;
            st.msgs.get_mut(p).unwrap().push(m.clone());
            m
        });
        self.deliver_later(peer, &m.id);
        m
    }

    pub fn send_text(self: &Arc<Self>, peer: String, text: String, reply_to: Option<String>) -> Result<Message, Error> {
        let pong = text.trim().eq_ignore_ascii_case("ping");
        let m = self.push_out(&peer, &text, reply_to, None);
        if pong {
            self.incoming_later(&peer, "pong");
        }
        Ok(m)
    }

    /// A message from the peer a couple of seconds from now.
    pub fn incoming_later(&self, peer: &str, text: &str) {
        let (peer, text, tx) = (peer.to_string(), text.to_string(), self.tx.clone());
        let (m, row) = self.with(&peer, |st, p| {
            st.seq += 1;
            let m = base(&format!("{p}-in{}", st.seq), p, false, now() + 2000, &text);
            st.msgs.get_mut(p).unwrap().push(m.clone());
            *st.unread.entry(p.to_string()).or_insert(0) += 1;
            let name = st.names.get(p).cloned().unwrap_or_default();
            (m, Self::chat_row(st, p, &name))
        });
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(2));
            if let Some(tx) = tx {
                let _ = tx.send(Ev::Chat(ChatEv::Added(m)));
                let _ = tx.send(Ev::Chat(ChatEv::Chat(row)));
            }
        });
    }

    pub fn edit_message(&self, peer: String, id: String, text: String) -> Result<Message, Error> {
        self.with(&peer, |st, p| {
            let m = st.msgs.get_mut(p).unwrap().iter_mut().find(|m| m.id == id && m.outgoing).ok_or_else(|| nope("no such message"))?;
            m.text = text;
            m.edited_at = Some(now());
            Ok(m.clone())
        })
    }

    pub fn delete_message(&self, peer: String, id: String) -> Result<Message, Error> {
        self.with(&peer, |st, p| {
            let m = st.msgs.get_mut(p).unwrap().iter_mut().find(|m| m.id == id && m.outgoing).ok_or_else(|| nope("no such message"))?;
            m.text.clear();
            m.deleted = true;
            m.attachment = None;
            Ok(m.clone())
        })
    }

    pub fn mark_read(&self, peer: String) -> Result<(), Error> {
        self.with(&peer, |st, p| st.unread.insert(p.to_string(), 0));
        Ok(())
    }

    pub fn send_file(self: &Arc<Self>, peer: String, path: String, mime: String) -> Result<Message, Error> {
        let md = std::fs::metadata(&path).map_err(|e| nope(&e.to_string()))?;
        let name = std::path::Path::new(&path).file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
        Ok(self.push_out(&peer, "", None, Some(file(&name, md.len(), &mime, TransferState::Ready))))
    }

    pub fn send_voice(self: &Arc<Self>, peer: String, path: String, duration_ms: u32, waveform: Vec<u8>) -> Result<Message, Error> {
        let md = std::fs::metadata(&path).map_err(|e| nope(&e.to_string()))?;
        let mut a = file("Voice message", md.len(), "audio/ogg", TransferState::Ready);
        a.kind = AttachmentKind::Voice;
        a.duration_ms = duration_ms;
        a.waveform = waveform;
        a.hash = format!("h-voice-{}", now());
        Ok(self.push_out(&peer, "", None, Some(a)))
    }

    /// Ten steps of progress, then Ready.
    pub fn download_attachment(self: &Arc<Self>, peer: String, message_id: String) -> Result<(), Error> {
        let (total, hash) = self.with(&peer, |st, p| {
            let m = st.msgs.get_mut(p).unwrap().iter_mut().find(|m| m.id == message_id).ok_or_else(|| nope("no such message"))?;
            let a = m.attachment.as_mut().ok_or_else(|| nope("no attachment"))?;
            a.state = TransferState::Downloading;
            Ok::<_, Error>((a.size, a.hash.clone()))
        })?;
        let fake = self.clone();
        std::thread::spawn(move || {
            for i in 1..=10u64 {
                std::thread::sleep(Duration::from_millis(300));
                fake.emit(ChatEv::Progress { hash: hash.clone(), done: total * i / 10, total, outgoing: false });
            }
            let m = fake.with(&peer, |st, p| {
                let m = st.msgs.get_mut(p).unwrap().iter_mut().find(|m| m.id == message_id)?;
                m.attachment.as_mut()?.state = TransferState::Ready;
                Some(m.clone())
            });
            if let Some(m) = m {
                fake.emit(ChatEv::Changed(m));
            }
        });
        Ok(())
    }

    pub fn save_attachment(&self, peer: String, message_id: String, dest_path: String) -> Result<(), Error> {
        let name = self.with(&peer, |st, p| {
            st.msgs[p].iter().find(|m| m.id == message_id).and_then(|m| m.attachment.as_ref()).map(|a| (a.name.clone(), a.state))
        });
        match name {
            Some((n, TransferState::Ready)) => {
                let bytes = if n.ends_with(".jpg") || n.ends_with(".png") {
                    fake_picture(n.ends_with(".png"))
                } else if n.ends_with(".txt") {
                    b"Move-in checklist\n\n[x] Sign the lease\n[x] Pay the deposit\n[ ] Collect the keys (Tuesday, 6 pm)\n[ ] Photograph every room\n[ ] Meter readings: gas, electricity, water\n[ ] Change the address\n".to_vec()
                } else {
                    format!("fake attachment {n}\n").into_bytes()
                };
                std::fs::write(&dest_path, bytes).map_err(|e| nope(&e.to_string()))
            }
            _ => Err(nope("not ready")),
        }
    }
}
