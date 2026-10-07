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
            "onboarding" => {
                self.screen = Screen::Onboarding;
                self.name_in = "Maya".into();
            }
            "settings" | "settings-audio" | "settings-security" => {
                self.screen = Screen::Settings;
                self.settings_tab = match name {
                    "settings-audio" => 2,
                    "settings-security" => 3,
                    _ => 1,
                };
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
            _ => {}
        }
        self.contacts = contacts;
    }
}

#[cfg(not(feature = "test-hooks"))]
impl App {
    pub(super) fn load_demo(&mut self, _name: &str) {}
}
