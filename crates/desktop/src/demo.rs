//! Fake data for design screenshots (`P2P_DEMO=<screen>`, test-hooks builds only). Nothing here
//! touches the node, and it is compiled out of normal builds.

use super::*;

#[cfg(feature = "test-hooks")]
impl App {
    pub(super) fn load_demo(&mut self, name: &str) {
        use p2pcore::{CallRecord, CallStats};
        self.demo = true;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let contact = |did: &str, name: &str, alias: Option<&str>, verified: bool| Contact {
            did: format!("did:key:z6Mk{did}"),
            name: name.into(),
            device: "ab12cd34ef56".into(),
            added_at: now - 86400 * 30,
            alias: alias.map(str::to_string),
            verified,
        };
        let rec = |c: &str, name: &str, incoming: bool, ago: u64, secs: u32, reason: &str, direct: bool, missed: bool| CallRecord {
            call_id: format!("{c}{ago}"),
            peer_did: format!("did:key:z6Mk{c}"),
            peer_name: name.into(),
            incoming,
            started_at: now - ago,
            duration_secs: secs,
            reason: reason.into(),
            direct,
            missed,
        };
        self.profile_name = "Maya Fernandes".into();
        self.device_label = "Work desktop".into();
        self.device_edit = self.device_label.clone();
        self.accounts = vec![
            AccountSummary { did: "did:key:z6MkMaya".into(), name: "Maya Fernandes".into(), current: true, has_passphrase: false },
            AccountSummary { did: "did:key:z6MkWork".into(), name: "Maya \u{b7} Work".into(), current: false, has_passphrase: true },
        ];
        self.name_edit = self.profile_name.clone();
        self.lock = LockState::Unlocked;
        self.status = NodeStatus { started: true, online: true, ..self.status.clone() };
        self.avail = Availability { available: true, until: None };
        self.ticket = Some("OSVC2:MFRGGZDFMZTWQ2LKNNWG23TPOBYXE43UOV3HO2LUNBUGC5DBMJZGK6DPNFXGOIDBNZSXIZLNMRQWG2LTOQQGI33OEBZGS3TFEB2GQZJAMZXXEIDMN5ZGKIDPNZSXIYLSMVRWK3THEBQWY2LTNFYGKZLSORSXE5BAONSWY3DPEBTW63TFOJQWO3DFMQQHEZLTEBXXEZJAONUW4ZZAN5YGK4TSEA".to_string());
        self.qr = view::qr_of(self.ticket.as_deref().unwrap());
        let mut contacts = vec![
            contact("Arjun", "Arjun Oommen", None, true),
            contact("Dad", "Dad", None, false),
            contact("Joanna", "Joanna Ostrowska", None, false),
            contact("Jonas", "Jonas Kr\u{fc}ger", None, false),
            contact("Lena", "Lena Park", Some("Lena (work)"), false),
            contact("Rosa", "Rosa Silva", None, false),
        ];
        self.recents = vec![
            rec("Arjun", "Arjun Oommen", true, 240, 1120, "hangup_remote", true, false),
            rec("Rosa", "Rosa Silva", true, 3 * 3600, 0, "cancelled", false, true),
            rec("Jonas", "Jonas Kr\u{fc}ger", false, 26 * 3600, 190, "hangup_local", false, false),
            rec("Lena", "Lena Park", false, 50 * 3600, 0, "unreachable", false, false),
            rec("Arjun", "Arjun Oommen", false, 28 * 3600, 3120, "hangup_local", true, false),
            rec("Arjun", "Arjun Oommen", true, 6 * 86400, 2520, "hangup_remote", true, false),
        ];
        self.sel = None;
        let arjun = "did:key:z6MkArjun".to_string();
        let info = |incoming: bool| CallInfo {
            call_id: "demo".into(),
            peer_did: arjun.clone(),
            peer_name: "Arjun Oommen".into(),
            incoming,
        };
        let stats = |direct: bool, secs: u32, rtt: u32, reconnecting: bool| CallStats {
            call_id: "demo".into(),
            state: CallState::Active,
            secs,
            direct,
            rtt_ms: rtt,
            sent: 1000,
            received: 990,
            lost: 4,
            recovered: 3,
            concealed: 1,
            buffered_ms: 40,
            rx_freq_hz: 0.0,
            rx_rms: 0.0,
            reconnecting,
        };
        self.screen = Screen::Home;
        match name {
            "home" => {}
            "empty" => {
                contacts.clear();
                self.recents.clear();
            }
            "contact" | "verify" | "rename" | "remove" => {
                self.sel = Some(arjun.clone());
                self.detail_calls = self.recents.iter().filter(|r| r.peer_did == arjun).cloned().collect();
                self.detail = match name {
                    "verify" => Detail::Verify,
                    "rename" => Detail::Rename,
                    "remove" => Detail::ConfirmRemove,
                    _ => Detail::View,
                };
                self.safety = Some("41203 88127 05519 73360 29841 66012 90475 13398 57206 84431 20987 36654".into());
            }
            "offer" | "empty-offer" => {
                if name == "empty-offer" {
                    contacts.clear();
                    self.recents.clear();
                }
                self.offer = Some(p2pcore::CardPeek { ticket: "OSVC2:x".into(), name: "Priya Nair".into(), did: "did:key:z6MkPriya".into(), known: false });
            }
            "avail" => self.avail_open = true,
            "offline" => self.status.online = false,
            "unavailable" => {
                self.avail = Availability { available: false, until: None };
                self.sel = Some(arjun.clone());
                self.detail_calls = self.recents.iter().filter(|r| r.peer_did == arjun).cloned().collect();
            }
            "add" => self.screen = Screen::AddContact,
            "add-paste" => {
                self.screen = Screen::AddContact;
                self.add_in = "OSVC2:eyJuIjoiQXJqdW4iLCJrIjoiejZNa3ZWUXFwYkxzTW1ZbkVSN3pkTXNUdnJENHMi".into();
            }
            "add-done" => {
                self.screen = Screen::AddContact;
                self.add_alias = "Arjun".into();
                self.add_phase = AddPhase::Added(contacts[0].clone());
            }
            "unlock" => self.screen = Screen::Unlock,
            "unlock-wrong" => {
                self.screen = Screen::Unlock;
                self.notice = Some("That passphrase is not right. Try again.".into());
            }
            "accounts-menu" | "switch-confirm" => {
                self.acct_menu = true;
                if name == "switch-confirm" {
                    self.switch_ask = Some(self.accounts[1].did.clone());
                }
            }
            "name-device" => {
                self.screen = Screen::NameDevice;
                self.device_in = "Work desktop".into();
            }
            "key-failed" => {
                self.screen = Screen::KeyFailed;
                self.key_fail = Some("no secure keyring is available".into());
            }
            "onboarding-nokeyring" => {
                self.screen = Screen::Onboarding;
                self.name_in = "Maya".into();
                self.keyring_ok = false;
            }
            "onboarding" => {
                self.screen = Screen::Onboarding;
                self.name_in = "Maya".into();
            }
            "settings" | "settings-audio" | "settings-security" | "settings-account" | "settings-devices" | "settings-legacy-key" => {
                self.screen = Screen::Settings;
                self.settings_tab = match name {
                    "settings-account" => 0,
                    "settings-devices" => 1,
                    "settings-audio" => 3,
                    "settings-security" | "settings-legacy-key" => 4,
                    _ => 2,
                };
                if name == "settings-legacy-key" {
                    self.file_key = true;
                    self.key_home = "Your key is kept in a plain file in Tinline\u{2019}s data folder. Anyone who copies that folder can open your account. Add a passphrase to protect it; the file is then deleted.";
                    self.has_pass = false;
                }
                self.devices = (
                    vec!["Built-in Audio Analog Stereo".into(), "USB Headset".into()],
                    vec!["Built-in Audio Analog Stereo".into(), "USB Headset".into()],
                );
                self.settings.input = Some("USB Headset".into());
            }
            "calling" => {
                self.call = Some(CallView { info: info(false), state: CallState::Dialing, answered: false, stats: None });
            }
            "incoming" => {
                self.call = Some(CallView { info: info(true), state: CallState::Ringing, answered: false, stats: None });
            }
            "incall" | "incall-relay" | "reconnecting" => {
                let direct = name != "incall-relay";
                self.call = Some(CallView {
                    info: info(true),
                    state: CallState::Active,
                    answered: true,
                    stats: Some(stats(direct, 252, if direct { 38 } else { 140 }, name == "reconnecting")),
                });
            }
            "ended" | "unreachable" => {
                let unreachable = name == "unreachable";
                let who = if unreachable { "Lena (work)" } else { "Arjun Oommen" };
                self.ended = Some(EndedView {
                    reason: if unreachable { "unreachable" } else { "hangup_remote" }.into(),
                    did: arjun.clone(),
                    name: who.into(),
                    text: reason::text(if unreachable { "unreachable" } else { "hangup_remote" }, who).unwrap(),
                    direct: true,
                    secs: if unreachable { 0 } else { 1124 },
                    at: Instant::now() + Duration::from_secs(3600),
                });
            }
            n if n.starts_with("chat") => {
                use chat::chat_fake::Fake;
                use p2pcore::TransferState;
                let f = Arc::new(Fake::new(Some(INIT.get().unwrap().tx.clone())));
                self.chat.fake = Some(f.clone());
                self.chat.tab = chat::SideTab::Chats;
                let mut rows = f.chats(contacts.clone()).unwrap_or_default();
                for r in rows.iter_mut() {
                    r.unread = match r.peer_did.as_str() {
                        "did:key:z6MkRosa" => 2,
                        "did:key:z6MkJonas" => 1,
                        _ => 0,
                    };
                }
                self.chat.chats = rows;
                let peer = if matches!(n, "chat-offline" | "chat-drop") { "did:key:z6MkJonas".to_string() } else { arjun.clone() };
                self.sel = Some(peer.clone());
                self.chat.peer = Some(peer.clone());
                let page = f.chat_day(peer.clone(), None).unwrap();
                let mut msgs = page.messages;
                if let Some(prev) = page.older_day.and_then(|d| f.chat_day(peer.clone(), Some(d)).ok()) {
                    // Show yesterday as well; whatever is older waits behind "Load earlier".
                    self.chat.older = prev.older_day;
                    let mut m = prev.messages;
                    m.append(&mut msgs);
                    msgs = m;
                }
                self.chat.msgs = msgs;
                let id = |n: u32| format!("{peer}-{n}");
                match n {
                    "chat-offline" | "chat-drop" => {
                        self.status.online = false;
                        self.chat.drop_hover = n == "chat-drop";
                    }
                    "chat-menu" => self.chat.menu = Some(id(10)),
                    "chat-hover" => self.chat.hover = Some(id(8)),
                    "chat-reply" => {
                        self.chat.reply = Some(id(5));
                        self.chat.input = iced::widget::text_editor::Content::with_text("Sure \u{2014} it\u{2019}s the one from Tuesday");
                    }
                    "chat-edit" => {
                        self.chat.editing = Some(id(10));
                        self.chat.input = iced::widget::text_editor::Content::with_text("Sure \u{2014} it\u{2019}s the one from Tuesday");
                    }
                    "chat-files" => {
                        if let Some(a) = self.chat.msgs.iter_mut().find(|m| m.id == id(9)).and_then(|m| m.attachment.as_mut()) {
                            a.state = TransferState::Downloading;
                            self.chat.progress.insert(a.hash.clone(), (62_000_000, 184_000_000));
                        }
                    }
                    "chat-failed" => {
                        if let Some(a) = self.chat.msgs.iter_mut().find(|m| m.id == id(9)).and_then(|m| m.attachment.as_mut()) {
                            a.state = TransferState::Failed;
                        }
                    }
                    "chat-empty" => {
                        self.chat.msgs.clear();
                        self.chat.older = None;
                    }
                    "chat-list" => {
                        self.sel = None;
                        self.chat.peer = None;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        self.contacts = contacts;
    }
}

#[cfg(not(feature = "test-hooks"))]
impl App {
    pub(super) fn load_demo(&mut self, _name: &str) {}
}
