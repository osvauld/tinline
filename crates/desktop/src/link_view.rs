//! Device-linking screens, laid out after docs/design/DesktopLinkDevice.dc.html and
//! AndroidLinkedDevices.dc.html.

use super::*;
use iced::widget::{column, row};
use crate::app::link::{failure_text, Role, Step};

const OS: &str = match std::env::consts::OS.as_bytes() {
    b"linux" => "Linux",
    b"macos" => "macOS",
    b"windows" => "Windows",
    _ => "Desktop",
};

fn mmss(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

impl App {
    pub(super) fn link_screen(&self, t: Tok) -> El<'_> {
        let l = &self.link;
        let existing = l.role == Role::Existing;
        let peer = if l.peer.is_empty() { "the other device".to_string() } else { l.peer.clone() };
        match &l.step {
            Step::Name => {
                let col = column![
                    field(
                        t,
                        "Device name",
                        text_input("Work desktop", &self.device_in).on_input(Msg::DeviceIn).on_submit(Msg::LinkNameGo),
                    ),
                    tx("Only your own devices see this name.", 12.0, t.ink2),
                    self.notice_bar(t),
                    self.wide(if self.busy { "Working..." } else { "Continue" }, Kind::Primary, t, (!self.busy).then_some(Msg::LinkNameGo)),
                    self.wide("Back", Kind::Ghost, t, Some(Msg::LinkCancel)),
                ]
                .spacing(12);
                self.shell(t, "Link this computer to your account", "Name this computer, then scan its code with your phone.", col.into())
            }
            Step::Qr => self.link_qr_screen(t, existing),
            Step::Connecting => {
                let col = column![
                    tx("Connecting to the other device\u{2026}", 15.0, t.ink2),
                    self.notice_bar(t),
                    self.wide("Cancel", Kind::Quiet, t, Some(Msg::LinkCancel)),
                ]
                .spacing(14);
                self.shell(t, "Connecting", "This takes a few seconds.", col.into())
            }
            Step::Code if !existing => {
                let col = column![
                    self.big_code(t, &l.code),
                    tx("Code doesn\u{2019}t match? Cancel here and on the other device.", 13.0, t.ink2),
                    self.wide("Cancel", Kind::Quiet, t, Some(Msg::LinkCancel)),
                ]
                .spacing(16);
                self.shell(
                    t,
                    "Check the code",
                    &format!("Connected to {peer}. It should show the same code. Approve there to finish."),
                    col.into(),
                )
            }
            Step::Code => {
                let mut col = column![
                    self.big_code(t, &l.code),
                    tx("Different code, or you didn\u{2019}t start this? Cancel. Someone may be trying to copy your account.", 13.0, t.ink2),
                    banner(
                        t,
                        "A linked device becomes you",
                        "It can call, chat and see your recovery phrase. Only link devices you own.",
                        None,
                        false,
                    ),
                ]
                .spacing(14);
                if self.has_pass {
                    col = col.push(self.pass_field(t, "Your passphrase", &l.pass, Msg::LinkPassIn, Some(Msg::LinkApprove)));
                } else {
                    // The desktop has no device authentication to ask for, so the screen asks.
                    col = col.push(tx(
                        "This account has no passphrase, and this computer can\u{2019}t ask for a screen lock. By continuing you confirm you started this and the codes match.",
                        13.0,
                        t.ink,
                    ));
                }
                col = col.push(self.notice_bar(t)).push(self.wide(
                    if self.busy { "Linking..." } else { "Codes match, link it" },
                    Kind::Primary,
                    t,
                    (!self.busy && (!self.has_pass || !l.pass.is_empty())).then_some(Msg::LinkApprove),
                ));
                col = col.push(self.wide("Cancel", Kind::Quiet, t, Some(Msg::LinkCancel)));
                self.shell(t, &format!("Link \u{201c}{peer}\u{201d}?"), "Make sure the other device shows this same code.", col.into())
            }
            Step::Approving => {
                let col = column![
                    tx("Sending your account over\u{2026}", 15.0, t.ink2),
                    self.wide("Cancel", Kind::Quiet, t, Some(Msg::LinkCancel)),
                ]
                .spacing(14);
                self.shell(t, &format!("Linking {peer}"), "Keep both devices open until this finishes.", col.into())
            }
            Step::Done => {
                let col = column![
                    tx(
                        "It rings for your calls now. Contacts and call history sync whenever both devices are online.",
                        14.0,
                        t.ink2,
                    ),
                    self.wide("Done", Kind::Primary, t, Some(Msg::LinkDoneOk)),
                ]
                .spacing(16);
                self.shell(t, &format!("Linked {peer}"), "Your account is on the other device.", col.into())
            }
            Step::Failed(reason) => {
                let retry = if reason == "locked" { None } else { Some(Msg::LinkRegen) };
                let col = column![
                    banner(t, "Not linked", failure_text(reason), None, true),
                    self.wide("Try again", Kind::Primary, t, retry.filter(|_| !self.busy)),
                    self.wide(if existing { "Back to Settings" } else { "Back" }, Kind::Quiet, t, Some(Msg::LinkCancel)),
                ]
                .spacing(14);
                self.shell(t, "Linking stopped", "Nothing was changed on either device.", col.into())
            }
        }
    }

    fn big_code<'a>(&self, t: Tok, code: &str) -> El<'a> {
        container(mono(code.to_string(), 44.0, t.ink))
            .padding([18, 24])
            .width(Fill)
            .center_x(Fill)
            .style(ui::outlined(t.surface2, t.line, 14.0))
            .into()
    }

    fn link_qr_screen(&self, t: Tok, existing: bool) -> El<'_> {
        let l = &self.link;
        let left = l.remaining();
        let live = l.qr.is_some() && left > 0;
        let mut col = column![].spacing(14).align_x(Alignment::Center);
        if live {
            let q = l.qr.as_ref().unwrap();
            col = col
                .push(container(canvas(super::QrView(q)).width(Length::Fixed(220.0)).height(Length::Fixed(220.0))).padding(6).style(ui::plain(Color::WHITE, 10.0)))
                .push(tx(format!("Expires in {} \u{b7} works once", mmss(left)), 13.0, t.ink2));
        } else if self.busy {
            col = col.push(tx("Making a code\u{2026}", 15.0, t.ink2));
        } else {
            col = col.push(tx(
                if l.qr.is_some() { "This code expired." } else { "There is no code yet." },
                15.0,
                t.ink2,
            ));
            col = col.push(pill(t, Kind::Primary, Some(Icon::RotateCw), "Make a new code", Some(Msg::LinkRegen)));
        }
        let step = |n: &'static str, body: El<'static>| -> El<'static> {
            row![
                container(semi(n, 13.0, t.on_primary_c)).width(24).height(24).center_x(24).center_y(24).style(ui::plain(t.primary_c, 12.0)),
                body,
            ]
            .spacing(12)
            .align_y(Alignment::Start)
            .into()
        };
        let steps: El = if existing {
            column![
                step("1", tx("On the new device, install Tinline and choose Link to an existing account.", 14.0, t.ink).into()),
                step("2", tx("It shows a QR code. Tap Scan instead and point it at this screen.", 14.0, t.ink).into()),
                step("3", tx("Check the codes match, then approve here.", 14.0, t.ink).into()),
            ]
            .spacing(10)
            .into()
        } else {
            column![
                step("1", tx("On your phone, open Tinline \u{203a} Settings \u{203a} Link a device.", 14.0, t.ink).into()),
                step("2", tx("Tap Scan QR code and point it at this screen.", 14.0, t.ink).into()),
                step("3", tx("Check the codes match and approve on your phone.", 14.0, t.ink).into()),
            ]
            .spacing(10)
            .into()
        };
        let mut all = column![col, steps].spacing(18);
        all = all.push(self.notice_bar(t));
        if l.paste_open {
            all = all.push(
                column![
                    field(
                        t,
                        "Link code from the other device",
                        text_input("OSVL1:\u{2026}", &l.paste).on_input(Msg::LinkPasteIn).on_submit(Msg::LinkPasteGo),
                    ),
                    row![
                        pill(t, Kind::Primary, None, if self.busy { "Connecting..." } else { "Connect" }, (!self.busy).then_some(Msg::LinkPasteGo)),
                        pill(t, Kind::Ghost, None, "Hide", Some(Msg::LinkPasteToggle)),
                    ]
                    .spacing(8),
                ]
                .spacing(10),
            );
        } else {
            all = all.push(self.wide("Paste a link code instead", Kind::Quiet, t, Some(Msg::LinkPasteToggle)));
        }
        all = all.push(self.wide(if existing { "Cancel" } else { "Back" }, Kind::Ghost, t, Some(Msg::LinkCancel)));
        let (title, sub) = if existing {
            ("Link a device", "Show this to the device you want to add. It becomes you, so only link your own.")
        } else {
            ("Link this computer to your account", "Scan this code from a device that already has your account.")
        };
        self.shell(t, title, sub, all.into())
    }

    /// New device linked: optional passphrase, then the S1 commit (key saved, then identity).
    pub(super) fn link_secure_screen(&self, t: Tok) -> El<'_> {
        let who = if self.link.peer.is_empty() { "your account".to_string() } else { self.link.peer.clone() };
        let col = if self.keyring_ok {
            column![
                self.new_pass_field(t, "Passphrase (optional)", Msg::LinkSecureGo),
                tx(
                    "A passphrase encrypts your account on this computer. Optional \u{2014} leave it empty and Tinline opens straight to your contacts. You can add one later in Settings.",
                    13.0,
                    t.ink2,
                ),
            ]
        } else {
            column![
                self.new_pass_field(t, "Passphrase", Msg::LinkSecureGo),
                tx(crate::app::NO_KEYRING, 13.0, t.ink),
                tx("You type it each time Tinline starts.", 13.0, t.ink2),
            ]
        }
        .spacing(12)
        .push(self.notice_bar(t))
        .push(self.wide(if self.busy { "Saving..." } else { "Continue" }, Kind::Primary, t, (!self.busy).then_some(Msg::LinkSecureGo)));
        self.shell(t, &format!("Linked to {who}"), "Protect the account on this computer before it is saved.", col.into())
    }

    pub(super) fn link_sync_screen(&self, t: Tok) -> El<'_> {
        let who = if self.link.peer.is_empty() { "your account".to_string() } else { self.link.peer.clone() };
        let count = |n: usize, what: &str| -> El<'_> {
            row![semi(what.to_string(), 15.0, t.ink), Space::new().width(Fill), mono(n.to_string(), 15.0, t.ink)].into()
        };
        let col = column![
            container(
                column![
                    count(self.contacts.len(), "Contacts"),
                    count(self.recents.len(), "Calls in history"),
                ]
                .spacing(10)
            )
            .padding(16)
            .width(Fill)
            .style(ui::outlined(t.surface2, t.line, 12.0)),
            tx(
                "Keep both devices online. If one drops off, it picks up where it left off next time they\u{2019}re both online.",
                13.0,
                t.ink2,
            ),
            self.wide("Start using Tinline", Kind::Primary, t, Some(Msg::LinkSyncDone)),
            tx("Sync keeps going in the background.", 12.0, t.ink2),
        ]
        .spacing(14);
        self.shell(t, &format!("Linked to {who}"), "Bringing your contacts and history over\u{2026}", col.into())
    }

    pub(super) fn unlinked_screen(&self, t: Tok) -> El<'_> {
        let (name, by_self) = self.unlinked.as_ref().map(|u| (u.name.clone(), u.by_self)).unwrap_or_default();
        let name = if name.is_empty() { "Your account".to_string() } else { name };
        let (title, body) = if by_self {
            (
                "This computer was unlinked",
                format!("{name} was removed from this computer. Its contacts, chats and calls are deleted here. Your other devices are not affected."),
            )
        } else {
            (
                "This computer was unlinked",
                format!("{name} was removed from this computer by another of your devices. Its contacts, chats and calls are deleted here. Your other devices are not affected."),
            )
        };
        let col = column![
            self.wide("OK", Kind::Primary, t, Some(Msg::UnlinkedOk)),
            self.wide("Link it again", Kind::Quiet, t, (!self.busy).then_some(Msg::NewLink)),
            self.other_accounts(t),
        ]
        .spacing(12);
        self.shell(t, title, &body, col.into())
    }

    /// Settings > Linked devices.
    pub(super) fn linked_devices_page(&self, t: Tok) -> El<'_> {
        let mut c = column![row![
            tx("Calls ring on all of these. People you call never see these names.", 14.0, t.ink2).width(Fill),
            pill(t, Kind::Primary, Some(Icon::ExternalLink), "Link a device", (!self.busy).then_some(Msg::LinkStartExisting)),
        ]
        .spacing(12)
        .align_y(Alignment::Center)]
        .spacing(14);
        let now = super::now_secs();
        let others = self.linked.iter().any(|d| !d.this_device);
        for d in &self.linked {
            let name = if d.label.is_empty() { "Unnamed device".to_string() } else { d.label.clone() };
            let sub = if d.this_device {
                format!("This computer \u{b7} {OS}")
            } else {
                match d.last_seen {
                    Some(ts) => format!("Synced {}", super::ago(now, ts)),
                    None => "Not synced yet".to_string(),
                }
            };
            let editing = self.dev_rename.as_ref().filter(|(dev, _)| *dev == d.device);
            let top: El = if let Some((_, text)) = editing {
                row![
                    text_input("Device name", text).on_input(Msg::DevRenameIn).on_submit(Msg::DevRenameSave).padding(11).size(15).style(ui::input_style(t)),
                    pill(t, Kind::Tonal, None, "Save", Some(Msg::DevRenameSave)),
                    pill(t, Kind::Ghost, None, "Cancel", Some(Msg::DevRenameCancel)),
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .into()
            } else {
                let mut r = row![column![semi(name.clone(), 15.0, t.ink), tx(sub, 13.0, t.ink2)].spacing(2).width(Fill)]
                    .spacing(8)
                    .align_y(Alignment::Center);
                r = r.push(pill(t, Kind::Quiet, Some(Icon::Pencil), "Rename", Some(Msg::DevRenameStart(d.device.clone()))));
                if !d.this_device || others {
                    r = r.push(pill(t, Kind::Danger, None, "Unlink", Some(Msg::UnlinkAsk(d.device.clone()))));
                }
                r.into()
            };
            let mut card = column![top].spacing(12);
            if self.unlink_ask.as_deref() == Some(d.device.as_str()) {
                card = card.push(self.unlink_confirm(t, &name, d.this_device));
            }
            c = c.push(container(card).padding(14).width(Fill).style(ui::outlined(t.surface2, t.line, 12.0)));
        }
        c.push(tx(
            "Devices sync directly whenever two are online at once. Unlinking stops sync and ringing; it can\u{2019}t lock a lost device remotely.",
            13.0,
            t.ink2,
        ))
        .into()
    }

    fn unlink_confirm(&self, t: Tok, name: &str, this: bool) -> El<'_> {
        let (title, body) = if this {
            (
                "Unlink this computer?".to_string(),
                format!("{} is removed from this computer: its contacts, chats and call history here are deleted. Your other devices are not affected.", self.profile_name),
            )
        } else {
            (
                format!("Unlink {name}?"),
                "It stops syncing and stops ringing for your calls. If it\u{2019}s online, it removes your account from itself.".to_string(),
            )
        };
        let mut c = column![bold(title, 16.0, t.ink), tx(body, 13.0, t.ink2)].spacing(10);
        if !this {
            c = c.push(banner(
                t,
                "Lost or stolen?",
                "Unlinking can\u{2019}t lock it from here. Anyone who can unlock it can still use your account. The only full cut-off is a new account.",
                None,
                false,
            ));
        }
        if self.has_pass {
            c = c.push(self.pass_field(t, "Your passphrase", &self.link.pass, Msg::LinkPassIn, Some(Msg::UnlinkGo)));
        } else {
            c = c.push(tx("This account has no passphrase, so confirming here is the only check.", 12.0, t.ink2));
        }
        c.push(
            row![
                pill(t, Kind::Danger, Some(Icon::Trash), if self.busy { "Unlinking..." } else { "Unlink" }, (!self.busy && (!self.has_pass || !self.link.pass.is_empty())).then_some(Msg::UnlinkGo)),
                pill(t, Kind::Ghost, None, "Cancel", Some(Msg::UnlinkCancel)),
            ]
            .spacing(8),
        )
        .into()
    }
}
