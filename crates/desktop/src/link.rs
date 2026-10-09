//! Device linking: state and updates (the screens are in `link_view.rs`). The desktop has no
//! camera, so it always shows a QR (and accepts a pasted `OSVL1:` code); the other device scans.
//! Design: docs/design/device-linking.md, boards DesktopLinkDevice / AndroidLinkedDevices.

use super::*;
use crate::LinkEv;

/// Which side of the link this computer is.
#[derive(PartialEq, Clone, Copy, Debug)]
pub enum Role {
    /// A new device joining an account that lives elsewhere.
    New,
    /// This computer holds the account and adds another device to it.
    Existing,
}

#[derive(PartialEq, Clone, Debug)]
pub enum Step {
    /// New device: name this computer first.
    Name,
    Qr,
    /// A pasted code was submitted; dialling.
    Connecting,
    /// Both connected and proved the QR: compare the 6-digit code.
    Code,
    /// Existing device: approved, waiting for the grant to land.
    Approving,
    /// Existing device: the other one is linked.
    Done,
    /// The link ended without success (the core's reason).
    Failed(String),
}

pub struct LinkState {
    pub role: Role,
    pub step: Step,
    pub qr: Option<Qr>,
    pub started: Instant,
    /// How long a QR lives (core default 5 minutes; a test knob shortens it).
    pub ttl: u64,
    pub code: String,
    pub peer: String,
    pub paste_open: bool,
    pub paste: String,
    /// The account passphrase typed to approve a link or an unlink.
    pub pass: Zeroizing<String>,
}

impl LinkState {
    pub fn new(role: Role) -> Self {
        LinkState {
            role,
            step: if role == Role::New { Step::Name } else { Step::Qr },
            qr: None,
            started: Instant::now(),
            ttl: crate::test_env("P2P_LINK_TTL_SECS").and_then(|v| v.parse().ok()).unwrap_or(300),
            code: String::new(),
            peer: String::new(),
            paste_open: false,
            paste: String::new(),
            pass: Zeroizing::default(),
        }
    }

    /// Seconds before the QR stops working.
    pub fn remaining(&self) -> u64 {
        self.ttl.saturating_sub(self.started.elapsed().as_secs())
    }
}

/// Shown after this computer was removed from its account.
pub struct UnlinkedInfo {
    pub name: String,
    /// This computer's own user did it (Settings), not another device.
    pub by_self: bool,
}

/// Plain words for the core's `on_link_failed` reasons.
pub fn failure_text(reason: &str) -> &'static str {
    match reason {
        "expired" => "The code ran out of time. A code works for 5 minutes, once.",
        "cancelled" => "The link was cancelled on the other device.",
        "rejected" => "The other device didn\u{2019}t accept the link.",
        "bad_proof" => "That code doesn\u{2019}t belong to this link, or it was already used. Make a new code and try again.",
        "mismatch" => "The two devices don\u{2019}t belong to the same account, so nothing was linked.",
        "timeout" => "Nobody approved in time. Nothing was linked.",
        "net" => "Couldn\u{2019}t reach the other device. Check that both are online and try again.",
        "locked" => "Tinline is locked. Unlock it and try again.",
        "busy" => "Another link is already in progress. Finish or cancel it first.",
        "bad_grant" => "What the other device sent wasn\u{2019}t valid, so nothing was linked.",
        "account_exists" => "That account is already on this computer.",
        _ => "The link didn\u{2019}t work. Nothing was changed.",
    }
}

impl App {
    pub(super) fn refresh_linked(&mut self) {
        if self.demo {
            return;
        }
        self.linked = self.node.linked_devices().into_iter().filter(|d| !d.removed).collect();
    }

    /// Leaves the link screens for where the user came from.
    fn leave_link(&mut self) -> Task<Msg> {
        let node = self.node.clone();
        let cancel = blocking(move || node.link_cancel(), |_| Msg::Chat(Cm::Nop));
        self.link = LinkState::new(self.link.role);
        self.notice = None;
        self.busy = false;
        match self.link.role {
            Role::Existing => {
                self.screen = Screen::Settings;
                self.settings_tab = 1;
            }
            Role::New => self.screen = Screen::Onboarding,
        }
        cancel
    }

    fn show_qr_task(&mut self) -> Task<Msg> {
        self.busy = true;
        self.notice = None;
        self.link.qr = None;
        self.link.code.clear();
        let node = self.node.clone();
        match self.link.role {
            Role::Existing => blocking(move || node.link_show_qr().map_err(friendly), Msg::LinkQr),
            Role::New => {
                let label = flow::valid_label(&self.device_in).unwrap_or_else(flow::suggest_label);
                blocking(move || node.link_new_show_qr(label).map_err(friendly), Msg::LinkQr)
            }
        }
    }

    fn handle_unlinked(&mut self, did: &str, by_self: bool) {
        keystore::clear(keystore::system(), &INIT.get().unwrap().data, did);
        let name = std::mem::take(&mut self.profile_name);
        self.unlinked = Some(UnlinkedInfo { name, by_self });
        self.call = None;
        self.link = LinkState::new(Role::New);
        self.link_joined = false;
        self.reset_account_state();
        self.screen = Screen::Unlinked;
    }

    pub(super) fn link_event(&mut self, ev: LinkEv) -> Task<Msg> {
        match ev {
            LinkEv::Code(code, peer) if self.screen == Screen::Link => {
                self.link.code = code;
                self.link.peer = peer;
                self.link.step = Step::Code;
                self.busy = false;
                self.notice = None;
            }
            LinkEv::Done(_, name) if self.screen == Screen::Link => {
                self.refresh_linked();
                match self.link.role {
                    Role::Existing => self.link.step = Step::Done,
                    Role::New => {
                        // The account is open in memory, not on disk yet: protect it, then commit.
                        self.link_joined = true;
                        self.link.peer = name;
                        self.busy = false;
                        self.notice = None;
                        self.pass_in.zeroize();
                        self.refresh_pass();
                        self.refresh_identity();
                        self.device_label = self.node.device_label().unwrap_or_default();
                        self.screen = Screen::LinkSecure;
                    }
                }
            }
            LinkEv::Failed(reason) if self.screen == Screen::Link => {
                // Cancelling our own unscanned QR reports "cancelled"; only a connected peer
                // can mean it.
                if reason == "cancelled" && matches!(self.link.step, Step::Qr | Step::Name) {
                    return Task::none();
                }
                self.busy = false;
                self.link.step = Step::Failed(reason);
            }
            LinkEv::Devices => self.refresh_linked(),
            LinkEv::History => {
                self.refresh_history();
            }
            LinkEv::Unlinked(did) => self.handle_unlinked(&did, false),
            _ => {}
        }
        Task::none()
    }

    pub(super) fn link_update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::LinkStartNew => {
                self.link = LinkState::new(Role::New);
                if self.device_in.trim().is_empty() {
                    self.device_in = flow::suggest_label();
                }
                self.notice = None;
                self.screen = Screen::Link;
                return self.probe_keyring();
            }
            Msg::LinkStartExisting => {
                if self.busy {
                    return Task::none();
                }
                if !self.status.started {
                    self.notice = Some("Tinline is still starting. Try again in a moment.".into());
                    return Task::none();
                }
                self.link = LinkState::new(Role::Existing);
                self.screen = Screen::Link;
                return self.show_qr_task();
            }
            Msg::NewLink => {
                if self.busy {
                    return Task::none();
                }
                if self.call.is_some() {
                    self.notice = Some(friendly(Error::InCall));
                    return Task::none();
                }
                self.busy = true;
                self.acct_menu = false;
                self.notice = None;
                self.return_to = keystore::current_did(&self.node);
                let node = self.node.clone();
                return blocking(move || node.begin_new_account().map_err(friendly), Msg::NewLinkBegun);
            }
            Msg::NewLinkBegun(r) => {
                self.busy = false;
                match r {
                    Ok(()) => {
                        self.reset_account_state();
                        self.unlinked = None;
                        return self.link_update(Msg::LinkStartNew);
                    }
                    Err(e) => {
                        self.return_to = None;
                        self.notice = Some(e);
                    }
                }
            }
            Msg::LinkNameGo => {
                if self.busy {
                    return Task::none();
                }
                let Some(label) = flow::valid_label(&self.device_in) else {
                    self.notice = Some("Give this computer a name of 1 to 64 characters.".into());
                    return Task::none();
                };
                self.device_in = label;
                return self.show_qr_task();
            }
            Msg::LinkQr(r) => {
                self.busy = false;
                match r {
                    Ok(text) => {
                        crate::tlog!("LINK_QR {text}");
                        self.link.qr = view::qr_of(&text);
                        self.link.started = Instant::now();
                        self.link.step = Step::Qr;
                        self.notice = None;
                    }
                    Err(e) => {
                        self.link.step = Step::Qr;
                        self.notice = Some(e);
                    }
                }
            }
            Msg::LinkRegen => {
                if self.busy {
                    return Task::none();
                }
                let node = self.node.clone();
                let cancel = blocking(move || node.link_cancel(), |_| Msg::Chat(Cm::Nop));
                self.link.paste_open = false;
                return Task::batch([cancel, self.show_qr_task()]);
            }
            Msg::LinkPasteToggle => {
                self.link.paste_open = !self.link.paste_open;
                self.notice = None;
            }
            Msg::LinkPasteIn(v) => self.link.paste = v,
            Msg::LinkPasteGo => {
                let text = self.link.paste.trim().to_string();
                if self.busy || text.is_empty() {
                    return Task::none();
                }
                if !text.starts_with("OSVL1:") {
                    self.notice = Some("That isn\u{2019}t a link code. It starts with OSVL1:".into());
                    return Task::none();
                }
                self.busy = true;
                self.notice = None;
                let node = self.node.clone();
                let role = self.link.role;
                let label = flow::valid_label(&self.device_in).unwrap_or_else(flow::suggest_label);
                // The scanning side first drops the QR it is showing: one link at a time.
                return blocking(
                    move || {
                        node.link_cancel();
                        match role {
                            Role::Existing => node.link_scan(text),
                            Role::New => node.link_new_scan(text, label),
                        }
                        .map_err(friendly)
                    },
                    Msg::LinkScanned,
                );
            }
            Msg::LinkScanned(r) => {
                self.busy = false;
                match r {
                    Ok(()) => {
                        self.link.paste.clear();
                        self.link.paste_open = false;
                        self.link.qr = None;
                        self.link.step = Step::Connecting;
                    }
                    Err(e) => {
                        // Our own QR was dropped to scan; offer a new one.
                        self.link.qr = None;
                        self.link.step = Step::Qr;
                        self.notice = Some(e);
                    }
                }
            }
            Msg::LinkPassIn(v) => self.link.pass = Zeroizing::new(v),
            Msg::LinkApprove => {
                if self.busy || (self.has_pass && self.link.pass.is_empty()) {
                    return Task::none();
                }
                self.busy = true;
                self.notice = None;
                let pass = self.has_pass.then(|| take_secret(&mut self.link.pass));
                let node = self.node.clone();
                return blocking(move || node.link_approve(pass).map_err(friendly), Msg::LinkApproved);
            }
            Msg::LinkApproved(r) => {
                self.busy = false;
                match r {
                    Ok(()) => self.link.step = Step::Approving,
                    // A wrong passphrase keeps the link open to retry.
                    Err(e) => self.notice = Some(e),
                }
            }
            Msg::LinkCancel => return self.leave_link(),
            Msg::LinkDoneOk => {
                self.link = LinkState::new(Role::Existing);
                self.screen = Screen::Settings;
                self.settings_tab = 1;
                self.notice = None;
            }
            Msg::LinkSecureGo => {
                if self.busy {
                    return Task::none();
                }
                if self.pass_in.is_empty() && !self.keyring_ok {
                    self.notice = Some(NO_KEYRING.into());
                    return Task::none();
                }
                self.busy = true;
                self.notice = None;
                let pass = take_secret(&mut self.pass_in);
                let (node, data) = (self.node.clone(), INIT.get().unwrap().data.clone());
                return blocking(move || flow::finish_link(keystore::system(), &data, &node, Some(pass)), Msg::Finished);
            }
            Msg::LinkSyncDone => {
                self.screen = Screen::Home;
            }
            // ---- Settings > Linked devices ----
            Msg::DevRenameStart(dev) => {
                let cur = self.linked.iter().find(|d| d.device == dev).map(|d| d.label.clone()).unwrap_or_default();
                self.unlink_ask = None;
                self.dev_rename = Some((dev, cur));
            }
            Msg::DevRenameIn(v) => {
                if let Some(r) = self.dev_rename.as_mut() {
                    r.1 = v;
                }
            }
            Msg::DevRenameCancel => self.dev_rename = None,
            Msg::DevRenameSave => {
                let Some((dev, text)) = self.dev_rename.clone() else { return Task::none() };
                let Some(label) = flow::valid_label(&text) else {
                    self.notice = Some("A device name is 1 to 64 characters.".into());
                    return Task::none();
                };
                self.dev_rename = None;
                let node = self.node.clone();
                return blocking(move || node.rename_device(dev, label).map_err(friendly), Msg::DevDone);
            }
            Msg::DevDone(r) => {
                self.refresh_linked();
                self.device_label = self.node.device_label().unwrap_or_default();
                self.device_edit = self.device_label.clone();
                if let Err(e) = r {
                    self.notice = Some(e);
                }
            }
            Msg::UnlinkAsk(dev) => {
                self.dev_rename = None;
                self.link.pass.zeroize();
                self.notice = None;
                self.unlink_ask = Some(dev);
            }
            Msg::UnlinkCancel => {
                self.unlink_ask = None;
                self.link.pass.zeroize();
                self.notice = None;
            }
            Msg::UnlinkGo => {
                let Some(dev) = self.unlink_ask.clone() else { return Task::none() };
                if self.busy || (self.has_pass && self.link.pass.is_empty()) {
                    return Task::none();
                }
                if self.call.is_some() {
                    self.notice = Some(friendly(Error::InCall));
                    return Task::none();
                }
                self.busy = true;
                self.notice = None;
                let this = self.linked.iter().any(|d| d.device == dev && d.this_device);
                let pass = self.has_pass.then(|| take_secret(&mut self.link.pass));
                let node = self.node.clone();
                let did = keystore::current_did(&self.node).unwrap_or_default();
                return blocking(
                    move || node.unlink_device(dev, pass).map(|_| (this, did)).map_err(friendly),
                    Msg::Unlinked,
                );
            }
            Msg::Unlinked(r) => {
                self.busy = false;
                match r {
                    Ok((true, did)) => self.handle_unlinked(&did, true),
                    Ok((false, _)) => {
                        self.unlink_ask = None;
                        self.refresh_linked();
                        self.notice = Some("Device unlinked".into());
                    }
                    // Wrong passphrase etc.: the confirm stays open.
                    Err(e) => self.notice = Some(e),
                }
            }
            Msg::UnlinkedOk => {
                self.unlinked = None;
                self.accounts = self.node.accounts();
                if let Some(a) = self.accounts.first() {
                    return self.update(Msg::SwitchNow(a.did.clone()));
                }
                self.restore = false;
                self.name_in.clear();
                self.screen = Screen::Onboarding;
                return self.probe_keyring();
            }
            _ => {}
        }
        Task::none()
    }
}
