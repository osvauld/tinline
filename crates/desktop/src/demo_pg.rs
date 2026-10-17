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
                Contact { did: pg_did(id), name: name.to_string(), device: "pg".into(), added_at: now - 86400 * 30, alias: None, verified: *id == "arjun", lost_touch: false }
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
            last_id: Some(format!("pg-last-{id}")),
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
        // seed.ts call messages: Sana missed (in), Arjun 12:22 out, Meera couldn't connect (out).
        let rec = |id: &str, name: &str, incoming: bool, at_ms: u64, secs: u32, reason: &str, missed: bool| p2pcore::CallRecord {
            call_id: format!("pg-{id}"), peer_did: pg_did(id), peer_name: name.into(), incoming,
            started_at: at_ms / 1000, duration_secs: secs, reason: reason.into(), direct: true, missed, video: false,
        };
        self.recents = vec![
            rec("sana", "Sana Rao", true, at(9, 2, 0), 0, "no_answer", true),
            rec("arjun", "Arjun Oommen", false, at(21, 32, 1), 742, "hangup_local", false),
            rec("meera", "Meera Nair", false, at(17, 50, 1), 0, "unreachable", false),
        ];
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
            r if r.starts_with("privacy") => {
                self.screen = Screen::Settings;
                let id = match r { "privacy-blocked" => 21, "privacy-calls" => 22, "privacy-phrase" | "privacy-phrase-shown" => 23, "privacy-passphrase" => 24, _ => 7 };
                self.profiles.page = Some(profile_ui::Page::Native(id));
                self.has_pass = true;
                if r == "privacy-phrase-shown" {
                    self.revealed = Some(zeroize::Zeroizing::new("harbor velvet orbit candle meadow lunar ticket gravel salmon whisper copper anchor fabric jungle pepper quarter ripple saddle timber unveil walnut yellow zebra mosaic".into()));
                    self.pv.shown = true;
                    self.pv.shown_at = Some(std::time::Instant::now());
                }
            }
            r if r.starts_with("devices") => {
                self.screen = Screen::Settings;
                self.profiles.page = Some(profile_ui::Page::Native(1));
                // seedDevices(): this laptop and Maya's phone, online now.
                self.linked = vec![
                    p2pcore::LinkedDevice { device: "laptop".into(), label: "Work laptop".into(), this_device: true, last_seen: Some(now), removed: false },
                    p2pcore::LinkedDevice { device: "pixel".into(), label: "Pixel 8".into(), this_device: false, last_seen: Some(now), removed: false },
                ];
                match r {
                    "devices-menu" => self.dev_menu = Some("pixel".into()),
                    "devices-unlink" => { self.unlink_ask = Some("pixel".into()); self.has_pass = true; }
                    "devices-rename" => self.dev_rename = Some(("laptop".into(), "Work laptop".into())),
                    _ => {}
                }
            }
            "settings-notifications" | "settings-sounds" => {
                self.screen = Screen::Settings;
                self.profiles.page = Some(if route.ends_with("sounds") { profile_ui::Page::Sounds } else { profile_ui::Page::Notifications });
                self.devices = (vec!["Built-in Audio".into()], vec!["Built-in Audio".into()]);
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
            // `pg-chat-arjun` (with P2P_CHAT_FAKE=1 for the seed's messages): the conversation open.
            "chat-hike" | "chat-hike-waiting" | "chat-hike-call" | "chat-hike-info" | "gcall" | "gcall-strip" | "gcall-about" => {
                // seed.ts hike: nine members, Arjun's device hosts; Sam, Priya, Kavya and I post.
                let names = [("meera", "Meera Nair"), ("arjun", "Arjun Oommen"), ("sam", "Sam Rao"), ("priya", "Priya Menon"), ("leo", "Leo Thomas"), ("dev", "Dev Patel"), ("kavya", "Kavya Iyer"), ("nisha", "Nisha Rao")];
                let mut members: Vec<p2pcore::GroupMember> = names.iter().map(|(id, n)| p2pcore::GroupMember {
                    did: pg_did(id), name: n.to_string(), is_me: false, is_owner: *id == "meera", is_admin: matches!(*id, "sam" | "priya"),
                    is_host: *id == "arjun", flags: 0, joined_at: at(9, 0, 50), invited_by: None, device_ids: vec![],
                }).collect();
                members.push(p2pcore::GroupMember { did: me.clone(), name: "Maya Kurian".into(), is_me: true, is_owner: false, is_admin: true, is_host: false, flags: 0, joined_at: at(9, 0, 53), invited_by: None, device_ids: vec![] });
                let waiting = route.ends_with("waiting");
                let host = p2pcore::GroupHost { did: pg_did("arjun"), name: "Arjun Oommen".into(), device_id: "a1b2c3d4".into(), appointed_by: None, is_me: false, online: !waiting };
                let msg = |n: u32, from: &str, name: &str, at_: u64, text: &str, kind: GroupItemKind| GroupMessage {
                    id: format!("hike-{n}"), group_id: "hike".into(), kind, author_did: if from == "me" { me.clone() } else { pg_did(from) }, author_name: name.into(),
                    subject_did: pg_did(from), subject_name: name.into(), at: at_, text: text.into(), reply_to: None, outgoing: from == "me",
                    delivery: if waiting && from == "me" { GroupDelivery::Waiting } else { GroupDelivery::Delivered }, before_removal: false,
                };
                self.live.msgs = vec![
                    msg(1, "sam", "Sam Rao", at(18, 0, 8), "Trail map and the parking details are in the shared folder.", GroupItemKind::Post),
                    msg(2, "priya", "Priya Menon", at(9, 30, 5), "Anyone have a spare headlamp for Saturday? Mine died last time.", GroupItemKind::Post),
                    msg(3, "kavya", "Kavya Iyer", at(9, 2, 0), "Kavya joined, invited by Priya", GroupItemKind::Joined),
                    msg(4, "sam", "Sam Rao", at(12, 30, 0), "Saturday 7am at the trailhead.", GroupItemKind::Post),
                    msg(5, "sam", "Sam Rao", at(12, 31, 0), "Bring rain jackets, the forecast says showers after 10.", GroupItemKind::Post),
                    msg(6, "sam", "Sam Rao", at(12, 31, 0) + 1, "And snacks for the top.", GroupItemKind::Post),
                    msg(7, "me", "Maya Kurian", at(12, 33, 0), "I can bring the stove.", GroupItemKind::Post),
                    msg(8, "priya", "Priya Menon", at(12, 38, 0), "Can someone give me a lift from the station?", GroupItemKind::Post),
                    msg(9, "kavya", "Kavya Iyer", at(12, 52, 0), "First hike with you all. Is the loop fine for beginners?", GroupItemKind::Post),
                    msg(10, "sam", "Sam Rao", at(12, 55, 0), "It is. Easy grade, one steep bit near the top.", GroupItemKind::Post),
                ];
                self.live.open = Some("hike".into());
                self.live.hide_info = route != "chat-hike-info";
                self.live.unread_at_open = 3;
                // A call in Weekend hike, hosted by Arjun's laptop: Sam started it; in it (gcall*), or not (chat-hike-call).
                let call = p2pcore::GroupCall { group_id: "hike".into(), call_id: "c1".into(), host_did: pg_did("arjun"), host_device_id: Some("a1b2".into()),
                    starter_did: pg_did("sam"), started_at: at(13, 2, 0), host_gen: 0, hosted_here: false, joined: route.starts_with("gcall") };
                if route != "chat-hike" && route != "chat-hike-waiting" { self.gcall.live.insert("hike".into(), call); }
                if route.starts_with("gcall") {
                    self.gcall.gid = Some("hike".into());
                    self.gcall.call_id = Some("c1".into());
                    self.gcall.phase = super::gcall::Phase::Active;
                    self.gcall.parts = vec![me.clone(), pg_did("arjun"), pg_did("sam"), pg_did("priya")];
                    self.gcall.host = Some(pg_did("arjun"));
                    self.gcall.since = std::time::Instant::now().checked_sub(std::time::Duration::from_secs(252));
                    self.gcall.frozen = Some(252);
                    self.gcall.minimized = route == "gcall-strip";
                    self.gcall.about = route == "gcall-about";
                }
                self.live.detail = Some(p2pcore::GroupDetail {
                    id: "hike".into(), name: "Weekend hike".into(), owner_did: pg_did("meera"), my_role: GroupRole::Admin, am_host: false,
                    members, hosts: vec![host], host_offers: vec![], host_requests: vec![], host_requested_here: false, removal: None,
                    am_call_host: false, call_hosts: vec![], call_host_requests: vec![], call_host_requested_here: false,
                });
            }
            r if r.starts_with("chat-") => self.sel = Some(pg_did(r["chat-".len()..].trim_end_matches("-find").trim_end_matches("-reply"))),
            "unread" => self.shell_search.unread_only = true,
            _ => {}
        }
    }
}
