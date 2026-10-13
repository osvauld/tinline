//! `P2P_DEMO=pg[-<route>]`: the playground seed (docs/design/playground/src/lib/model/seed.ts) as
//! native state, for paired Svelte/native screenshots. Test-hooks builds only; nothing reaches the
//! node. Times are today's local date at the seed's hh:mm, so stamps read the same as the
//! playground's fixed clock (Mon 12 Oct 2026 13:10).

use super::*;
use p2pcore::{Chat, DeliveryState, GroupDelivery, GroupItemKind, GroupMessage, GroupRole, GroupSummary};
use serde_json::json;

pub(super) const PG_PEOPLE: [(&str, &str, usize); 8] = [
    ("arjun", "Arjun Oommen", 5),
    ("meera", "Meera Nair", 4),
    ("dev", "Dev Patel", 1),
    ("sana", "Sana Rao", 1),
    ("sam", "Sam Rao", 3),
    ("priya", "Priya Menon", 2),
    ("amma", "Amma", 3),
    ("ravi", "Ravi Menon", 0),
];

pub(super) fn pg_did(id: &str) -> String {
    format!("did:key:z6Mkpg{id}")
}

/// Today (or `days_ago`) at hh:mm local, in unix ms.
fn at(h: u32, m: u32, days_ago: i64) -> u64 {
    use chrono::{Local, TimeZone};
    let d = Local::now().date_naive() - chrono::Duration::days(days_ago);
    let t = d.and_hms_opt(h, m, 0).unwrap();
    Local.from_local_datetime(&t).single().map(|x| x.timestamp_millis() as u64).unwrap_or(0)
}

fn fields(v: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
    let mut m = v.as_object().cloned().unwrap_or_default();
    m.entry("availability").or_insert(json!({ "available": true }));
    m
}

impl App {
    pub(super) fn load_pg(&mut self, route: &str) {
        self.demo = true;
        self.lock = LockState::Unlocked;
        self.status = NodeStatus { started: true, online: true, ..self.status.clone() };
        self.avail = Availability { available: true, until: None };
        self.profile_name = "Maya Kurian".into();
        self.groups.enable(false, "Maya Kurian");
        let me = "did:key:z6MkpgMaya".to_string();
        self.me = Some(self_contact(&me));
        self.accounts = vec![AccountSummary { did: me.clone(), name: "Maya Kurian".into(), current: true, has_passphrase: false }];
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        self.contacts = PG_PEOPLE
            .iter()
            .map(|(id, name, hue)| {
                ui::pin_hue(&pg_did(id), *hue);
                Contact { did: pg_did(id), name: name.to_string(), device: "pg".into(), added_at: now - 86400 * 30, alias: None, verified: *id == "arjun" }
            })
            .collect();
        let cache = |v: serde_json::Value| profile_ui::Cached { fields: fields(v), ..Default::default() };
        let pc = &mut self.profiles.cache;
        pc.insert(me.clone(), cache(json!({"name": "Maya Kurian", "status": "Slow mornings, fast evenings", "about": "Designing calm software. Ask me about trains.", "pronouns": "she/her"})));
        pc.insert(pg_did("arjun"), cache(json!({"name": "Arjun Oommen", "status": "Working from the café", "about": "Architect by day, cyclist at weekends. Happiest with a sketchbook and a long ride.", "pronouns": "he/him", "location": "Kochi", "website": "https://arjun.example/notes"})));
        pc.insert(pg_did("meera"), cache(json!({"name": "Meera Nair", "status": "Back online on Thursday", "about": "Teaches maths. Collects maps.", "location": "Bengaluru", "availability": {"available": false, "until": at(18, 0, 0)}})));
        pc.insert(pg_did("dev"), cache(json!({"name": "Dev Patel", "status": "Thursday lunch?"})));
        pc.insert(pg_did("sana"), cache(json!({"name": "Sana Rao", "status": "Draft review this week", "about": "Editor. Reads on trains."})));
        pc.insert(pg_did("sam"), cache(json!({"name": "Sam Rao", "status": "Trailhead at 7", "location": "Munnar"})));
        pc.insert(pg_did("priya"), cache(json!({"name": "Priya Menon", "status": "Studio day", "about": "Interiors and a lot of tea.", "pronouns": "she/her",
            "work": {"title": "Interior architect", "studio": {"name": "Tin & Lime", "city": "Kochi"}, "tools": ["Rhino", "Figma", "Pencil"]}})));
        pc.insert(pg_did("amma"), cache(json!({"name": "Amma", "status": "Call me after 6"})));
        let chat = |id: &str, preview: &str, out: bool, delivery: DeliveryState, at: u64, unread: u32| Chat {
            peer_did: pg_did(id),
            peer_name: PG_PEOPLE.iter().find(|p| p.0 == id).map(|p| p.1).unwrap_or(id).into(),
            preview: preview.into(),
            last_outgoing: out,
            last_delivery: delivery,
            last_activity: at,
            unread,
        };
        self.chat.chats = vec![
            chat("arjun", "I have the landlord\u{2019}s numbers now", false, DeliveryState::Delivered, at(12, 41, 0), 2),
            chat("dev", "Thursday works. The place near the station?", false, DeliveryState::Delivered, at(11, 5, 0), 0),
            chat("sana", "Missed call", false, DeliveryState::Delivered, at(9, 2, 0), 1),
            chat("meera", "Voice call", true, DeliveryState::Delivered, at(17, 50, 1), 0),
        ];
        self.chat.demo_draft(&pg_did("dev"), "Lunch Thursday?");
        let gmsg = |gid: &str, from: &str, name: &str, at: u64, text: &str| GroupMessage {
            id: format!("{gid}-last"),
            group_id: gid.into(),
            kind: GroupItemKind::Post,
            author_did: pg_did(from),
            author_name: name.into(),
            subject_did: pg_did(from),
            subject_name: name.into(),
            at,
            text: text.into(),
            reply_to: None,
            outgoing: false,
            delivery: GroupDelivery::Delivered,
            before_removal: false,
        };
        let group = |id: &str, name: &str, role: GroupRole, n: u32, last: GroupMessage, unread: u32| GroupSummary {
            id: id.into(),
            name: name.into(),
            my_role: role,
            member_count: n,
            host_count: 1,
            hosted_here: false,
            host_reachable: true,
            last: Some(last),
            unread,
        };
        self.live.me = me.clone();
        self.live.ready = true;
        self.live.list = vec![
            group("hike", "Weekend hike", GroupRole::Admin, 9, gmsg("hike", "sam", "Sam Rao", at(12, 55, 0), "It is. Easy grade, one steep bit near the top."), 3),
            group("family", "Family", GroupRole::Owner, 5, gmsg("family", "amma", "Amma", at(21, 10, 1), "Lunch is at 1 on Sunday"), 4),
            group("book", "Book club", GroupRole::Removed, 0, gmsg("book", "sam", "Sam Rao", at(14, 2, 14), "You were removed from the group"), 0),
        ];
        // seed.ts: everyone allows calls both ways except Dev (messages only); Ravi is blocked.
        let all: Vec<String> = PG_PEOPLE.iter().map(|p| pg_did(p.0)).collect();
        let callable: Vec<String> = all.iter().filter(|d| !d.ends_with("dev")).cloned().collect();
        let _ = &all;
        self.blocks.demo(&callable, &callable, &[pg_did("ravi")]);
        self.screen = Screen::Home;
        self.chat.tab = chat::SideTab::Chats;
        match route {
            "groups" => self.chat.tab = chat::SideTab::Groups,
            "calls" => self.chat.tab = chat::SideTab::Calls,
            "contacts" => self.chat.tab = chat::SideTab::Contacts,
            "settings" => {
                self.screen = Screen::Settings;
                self.profiles.page = Some(profile_ui::Page::Settings);
            }
            "profile" => {
                self.screen = Screen::Settings;
                self.profiles.page = Some(profile_ui::Page::Me);
            }
            r if r.starts_with("contact-") => {
                self.chat.tab = chat::SideTab::Contacts;
                self.screen = Screen::Settings;
                self.profiles.page = Some(profile_ui::Page::Person(pg_did(&r["contact-".len()..])));
            }
            "newchat" => self.shell_search.new_chat = Some(String::new()),
            "unread" => self.shell_search.unread_only = true,
            _ => {}
        }
    }
}
