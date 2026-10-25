//! Device-linking screens, laid out after docs/design/DesktopLinkDevice.dc.html and
//! AndroidLinkedDevices.dc.html.

use super::*;
use iced::widget::{column, row};
use crate::app::link::{failure_text, Role, Step};

fn os_name() -> &'static str {
    match std::env::consts::OS {
        "linux" => crate::i18n::tr_str("link_linux"),
        "macos" => "macOS",
        "windows" => crate::i18n::tr_str("link_windows"),
        _ => crate::i18n::tr_str("link_desktop"),
    }
}

impl App {
    pub(super) fn link_screen(&self, t: Tok) -> El<'_> {
        let l = &self.link;
        let existing = l.role == Role::Existing;
        // A named device or not: each sentence has a named and an unnamed form.
        let peer = l.peer.clone();
        let named = !peer.is_empty();
        let pick = |named_key: &str, other_key: &str| -> String {
            if named { crate::i18n::tr_args(named_key, &[("name", &peer)]) } else { crate::i18n::tr(other_key) }
        };
        match &l.step {
            Step::Name => {
                let col = column![
                    field(
                        t,
                        crate::i18n::tr_str("common_device_name"),
                        text_input(crate::i18n::tr_str("common_work_desktop"), &self.device_in).on_input(Msg::DeviceIn).on_submit(Msg::LinkNameGo),
                    ),
                    tx(crate::i18n::tr_str("common_you_can_rename_it"), 13.0, t.ink2),
                    self.notice_bar(t),
                    self.wide(if self.busy { crate::i18n::tr_str("common_working") } else { crate::i18n::tr_str("common_continue") }, Kind::Primary, t, (!self.busy).then_some(Msg::LinkNameGo)),
                    self.wide(crate::i18n::tr_str("common_back"), Kind::Ghost, t, Some(Msg::LinkCancel)),
                ]
                .spacing(12);
                self.shell(t, crate::i18n::tr_str("common_name_this_computer"), crate::i18n::tr_str("common_so_you_can_tell"), col.into())
            }
            Step::Qr => self.link_qr_screen(t, existing),
            Step::Connecting => {
                let col = column![
                    tx(crate::i18n::tr_str("link_connecting_to_the_other"), 15.0, t.ink2),
                    self.notice_bar(t),
                    self.wide(crate::i18n::tr_str("common_cancel"), Kind::Quiet, t, Some(Msg::LinkCancel)),
                ]
                .spacing(14);
                self.shell(t, crate::i18n::tr_str("common_connecting"), crate::i18n::tr_str("link_this_takes_a_few"), col.into())
            }
            // Link.svelte `scanned`: the big code, the warning, waiting for approval there.
            Step::Code if !existing => {
                let warn = container(tx(crate::i18n::tr_str("link_different_code_or_you"), 13.0, t.warn_ink).line_height(lh(19.0)))
                    .padding([12, 14]).width(Fill).style(ui::plain(t.warn, 14.0));
                let col = column![
                    container(text(l.code.clone()).size(44).font(ui::MONO).color(t.ink).line_height(lh(52.0))).center_x(Fill).padding([8, 0]),
                    warn,
                    container(row![container(Space::new()).width(8).height(8).style(ui::plain(t.accent, 4.0)), tx(crate::i18n::tr_str("link_waiting_for_approval"), 13.0, t.ink2)].spacing(10).align_y(Alignment::Center)).center_x(Fill),
                    self.wide(crate::i18n::tr_str("common_cancel"), Kind::Quiet, t, Some(Msg::LinkCancel)),
                ]
                .spacing(16);
                self.shell(t, crate::i18n::tr_str("link_same_code_on_both"), &pick("link_check_that_shows_this", "link_check_that_other_shows_this"), col.into())
            }
            Step::Code => {
                let mut col = column![
                    self.big_code(t, &l.code),
                    tx(crate::i18n::tr_str("link_different_code_or_you_didnt"), 13.0, t.ink2),
                    banner(
                        t,
                        crate::i18n::tr_str("link_a_linked_device_becomes"),
                        crate::i18n::tr_str("link_it_can_call_chat"),
                        None,
                        false,
                    ),
                ]
                .spacing(14);
                if self.has_pass {
                    col = col.push(self.pass_field(t, crate::i18n::tr_str("link_your_passphrase"), &l.pass, Msg::LinkPassIn, Some(Msg::LinkApprove)));
                } else {
                    // The desktop has no device authentication to ask for, so the screen asks.
                    col = col.push(tx(
                        crate::i18n::tr_str("link_this_account_has_no"),
                        13.0,
                        t.ink,
                    ));
                }
                col = col.push(self.notice_bar(t)).push(self.wide(
                    if self.busy { crate::i18n::tr_str("link_linking_2") } else { crate::i18n::tr_str("link_codes_match_link_it") },
                    Kind::Primary,
                    t,
                    (!self.busy && (!self.has_pass || !l.pass.is_empty())).then_some(Msg::LinkApprove),
                ));
                col = col.push(self.wide(crate::i18n::tr_str("common_cancel"), Kind::Quiet, t, Some(Msg::LinkCancel)));
                self.shell(t, &pick("link_link", "link_link_other"), crate::i18n::tr_str("link_make_sure_the_other"), col.into())
            }
            Step::Approving if !existing => {
                let col = column![container(tx("\u{2026}", 28.0, t.accent)).center_x(Fill).padding([40, 0])];
                self.shell(t, crate::i18n::tr_str("link_linking_2"), crate::i18n::tr_str("link_receiving_your_account_keep"), col.into())
            }
            Step::Approving => {
                let col = column![
                    tx(crate::i18n::tr_str("link_sending_your_account_over"), 15.0, t.ink2),
                    self.wide(crate::i18n::tr_str("common_cancel"), Kind::Quiet, t, Some(Msg::LinkCancel)),
                ]
                .spacing(14);
                self.shell(t, &pick("link_linking", "link_linking_other"), crate::i18n::tr_str("link_keep_both_devices_open"), col.into())
            }
            Step::Done => {
                let col = column![
                    tx(
                        crate::i18n::tr_str("link_it_rings_for_your"),
                        14.0,
                        t.ink2,
                    ),
                    self.wide(crate::i18n::tr_str("common_done"), Kind::Primary, t, Some(Msg::LinkDoneOk)),
                ]
                .spacing(16);
                self.shell(t, &pick("link_linked", "link_linked_other"), crate::i18n::tr_str("link_your_account_is_on"), col.into())
            }
            Step::Failed(reason) if !existing => {
                let err = container(tx(failure_text(reason), 13.0, t.danger).line_height(lh(19.0))).padding([12, 14]).width(Fill).style(ui::plain(t.danger_bg, 14.0));
                let retry = if reason == "locked" { None } else { Some(Msg::LinkRegen) };
                let col = column![
                    err,
                    self.wide(crate::i18n::tr_str("common_try_again"), Kind::Primary, t, retry.filter(|_| !self.busy)),
                    self.wide(crate::i18n::tr_str("link_restore_with_recovery_phrase"), Kind::Ghost, t, (!self.busy).then_some(Msg::GoRestore)),
                ]
                .spacing(14);
                self.shell(t, crate::i18n::tr_str("link_couldnt_link"), "", col.into())
            }
            Step::Failed(reason) => {
                let retry = if reason == "locked" { None } else { Some(Msg::LinkRegen) };
                let col = column![
                    banner(t, crate::i18n::tr_str("link_not_linked"), &failure_text(reason), None, true),
                    self.wide(crate::i18n::tr_str("common_try_again"), Kind::Primary, t, retry.filter(|_| !self.busy)),
                    self.wide(if existing { crate::i18n::tr_str("link_back_to_settings") } else { crate::i18n::tr_str("common_back") }, Kind::Quiet, t, Some(Msg::LinkCancel)),
                ]
                .spacing(14);
                self.shell(t, crate::i18n::tr_str("link_linking_stopped"), crate::i18n::tr_str("link_nothing_was_changed_on"), col.into())
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
        let mut col = column![].spacing(14).align_x(Alignment::Center).width(Fill);
        if live {
            let q = l.qr.as_ref().unwrap();
            // Link.svelte `.qr`: 256 on a computer (a link ticket is dense; the phone camera reads it from arm's length), white,
            // 12 padding, radius 20, a --line border.
            col = col
                .push(container(canvas(super::QrView(q)).width(Length::Fixed(256.0)).height(Length::Fixed(256.0))).padding(12)
                    .style(move |_: &Theme| container::Style { background: Some(Color::WHITE.into()), border: Border { color: t.line, width: 1.0, radius: 20.0.into() }, ..Default::default() }))
                .push(tx(crate::i18n::tr_args("link_expires_in_works_once", &[("value", &(present::time::clock(left as i64)).to_string())]), 13.0, t.ink2));
            if !existing {
                col = col.push(container(tx(crate::i18n::tr_str("link_anyone_who_can_see"), 13.0, t.ink2).line_height(lh(18.0)).align_x(Alignment::Center)).max_width(320));
            }
        } else if self.busy {
            col = col.push(tx(crate::i18n::tr_str("link_making_a_code"), 15.0, t.ink2));
        } else {
            col = col.push(tx(
                if l.qr.is_some() { crate::i18n::tr_str("link_this_code_expired") } else { crate::i18n::tr_str("link_there_is_no_code") },
                15.0,
                t.ink2,
            ));
            col = col.push(pill(t, Kind::Primary, Some(Icon::RotateCw), crate::i18n::tr_str("link_make_a_new_code"), Some(Msg::LinkRegen)));
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
                step("1", tx(crate::i18n::tr_str("link_on_the_new_device"), 14.0, t.ink).into()),
                step("2", tx(crate::i18n::tr_str("link_it_shows_a_qr"), 14.0, t.ink).into()),
                step("3", tx(crate::i18n::tr_str("link_check_the_codes_match_then"), 14.0, t.ink).into()),
            ]
            .spacing(10)
            .into()
        } else {
            column![
                step("1", tx(crate::i18n::tr_str("link_on_your_phone_open"), 14.0, t.ink).into()),
                step("2", tx(crate::i18n::tr_str("link_tap_scan_qr_code"), 14.0, t.ink).into()),
                step("3", tx(crate::i18n::tr_str("link_check_the_codes_match"), 14.0, t.ink).into()),
            ]
            .spacing(10)
            .into()
        };
        // The new device: no numbered steps (the lead says it); the existing device keeps its three.
        let mut all = if existing { column![col, steps].spacing(18) } else { column![col].spacing(18) };
        all = all.push(self.notice_bar(t));
        if l.paste_open {
            all = all.push(
                column![
                    field(
                        t,
                        crate::i18n::tr_str("link_link_code_from_the"),
                        text_input("OSVL1:\u{2026}", &l.paste).on_input(Msg::LinkPasteIn).on_submit(Msg::LinkPasteGo),
                    ),
                    row![
                        pill(t, Kind::Primary, None, if self.busy { crate::i18n::tr_str("link_connecting") } else { crate::i18n::tr_str("link_connect") }, (!self.busy).then_some(Msg::LinkPasteGo)),
                        pill(t, Kind::Ghost, None, crate::i18n::tr_str("common_hide"), Some(Msg::LinkPasteToggle)),
                    ]
                    .spacing(8),
                ]
                .spacing(10),
            );
        } else {
            all = all.push(self.wide(crate::i18n::tr_str("link_paste_a_link_code"), Kind::Quiet, t, Some(Msg::LinkPasteToggle)));
        }
        all = all.push(self.wide(if existing { crate::i18n::tr_str("common_cancel") } else { crate::i18n::tr_str("common_back") }, Kind::Ghost, t, Some(Msg::LinkCancel)));
        if !existing && live {
            all = all.push(self.wide(crate::i18n::tr_str("common_new_code"), Kind::Ghost, t, (!self.busy).then_some(Msg::LinkRegen)));
        }
        let (title, sub) = if existing {
            (crate::i18n::tr_str("link_link_a_device"), crate::i18n::tr_str("link_show_this_to_the"))
        } else {
            (crate::i18n::tr_str("link_link_to_your_account"), crate::i18n::tr_str("link_on_a_device_that"))
        };
        self.shell(t, title, sub, all.into())
    }

    /// New device linked: optional passphrase, then the S1 commit (key saved, then identity).
    pub(super) fn link_secure_screen(&self, t: Tok) -> El<'_> {
        let who = self.link.peer.clone();
        let linked_to = if who.is_empty() { crate::i18n::tr("link_linked_to_account") } else { crate::i18n::tr_args("link_linked_to", &[("name", &who)]) };
        let col = if self.keyring_ok {
            column![
                self.new_pass_field(t, crate::i18n::tr_str("link_passphrase_optional"), Msg::LinkSecureGo),
                tx(
                    crate::i18n::tr_str("link_a_passphrase_encrypts_your"),
                    13.0,
                    t.ink2,
                ),
            ]
        } else {
            column![
                self.new_pass_field(t, crate::i18n::tr_str("common_passphrase"), Msg::LinkSecureGo),
                tx(crate::app::no_keyring(), 13.0, t.ink),
                tx(crate::i18n::tr_str("link_you_type_it_each"), 13.0, t.ink2),
            ]
        }
        .spacing(12)
        .push(self.notice_bar(t))
        .push(self.wide(if self.busy { crate::i18n::tr_str("common_saving") } else { crate::i18n::tr_str("common_continue") }, Kind::Primary, t, (!self.busy).then_some(Msg::LinkSecureGo)));
        self.shell(t, &linked_to, crate::i18n::tr_str("link_protect_the_account_on"), col.into())
    }

    pub(super) fn link_sync_screen(&self, t: Tok) -> El<'_> {
        let who = self.link.peer.clone();
        let linked_to = if who.is_empty() { crate::i18n::tr("link_linked_to_account") } else { crate::i18n::tr_args("link_linked_to", &[("name", &who)]) };
        let count = |n: usize, what: &str| -> El<'_> {
            row![semi(what.to_string(), 15.0, t.ink), Space::new().width(Fill), mono(n.to_string(), 15.0, t.ink)].into()
        };
        let col = column![
            container(
                column![
                    count(self.contacts.len(), crate::i18n::tr_str("common_contacts")),
                    count(self.recents.len(), crate::i18n::tr_str("link_calls_in_history")),
                ]
                .spacing(10)
            )
            .padding(16)
            .width(Fill)
            .style(ui::outlined(t.surface2, t.line, 12.0)),
            tx(
                crate::i18n::tr_str("link_keep_both_devices_online"),
                13.0,
                t.ink2,
            ),
            self.wide(crate::i18n::tr_str("link_start_using_tinline"), Kind::Primary, t, Some(Msg::LinkSyncDone)),
            tx(crate::i18n::tr_str("link_sync_keeps_going_in"), 12.0, t.ink2),
        ]
        .spacing(14);
        self.shell(t, &linked_to, crate::i18n::tr_str("link_bringing_your_contacts_and"), col.into())
    }

    pub(super) fn unlinked_screen(&self, t: Tok) -> El<'_> {
        let (name, by_self) = self.unlinked.as_ref().map(|u| (u.name.clone(), u.by_self)).unwrap_or_default();
        let name = if name.is_empty() { crate::i18n::tr_str("common_your_account").to_string() } else { name };
        let (title, body) = if by_self {
            (
                crate::i18n::tr_str("link_this_computer_was_unlinked"),
                crate::i18n::tr_args("link_was_removed_from_this_computer", &[("name", &(name).to_string())]),
            )
        } else {
            (
                crate::i18n::tr_str("link_this_computer_was_unlinked"),
                crate::i18n::tr_args("link_was_removed_from_this", &[("name", &(name).to_string())]),
            )
        };
        let col = column![
            self.wide(crate::i18n::tr_str("common_ok"), Kind::Primary, t, Some(Msg::UnlinkedOk)),
            self.wide(crate::i18n::tr_str("link_link_it_again"), Kind::Quiet, t, (!self.busy).then_some(Msg::NewLink)),
            self.other_accounts(t),
        ]
        .spacing(12);
        self.shell(t, title, &body, col.into())
    }

    /// Settings > Linked devices.
    pub(super) fn linked_devices_page(&self, t: Tok) -> El<'_> {
        let mut c = column![row![
            tx(crate::i18n::tr_str("link_calls_ring_on_all"), 14.0, t.ink2).width(Fill),
            pill(t, Kind::Primary, Some(Icon::ExternalLink), crate::i18n::tr_str("link_link_a_device"), (!self.busy).then_some(Msg::LinkStartExisting)),
        ]
        .spacing(12)
        .align_y(Alignment::Center)]
        .spacing(14);
        let now = super::now_secs();
        let others = self.linked.iter().any(|d| !d.this_device);
        for d in &self.linked {
            let name = if d.label.is_empty() { crate::i18n::tr_str("common_unnamed_device").to_string() } else { d.label.clone() };
            let sub = if d.this_device {
                crate::i18n::tr_args("link_this_computer_os", &[("os", os_name())])
            } else {
                match d.last_seen {
                    Some(ts) => crate::i18n::tr_args("link_synced", &[("value", &(super::ago(now, ts)).to_string())]),
                    None => crate::i18n::tr_str("common_not_synced_yet").to_string(),
                }
            };
            let editing = self.dev_rename.as_ref().filter(|(dev, _)| *dev == d.device);
            let top: El = if let Some((_, text)) = editing {
                row![
                    text_input(crate::i18n::tr_str("common_device_name"), text).on_input(Msg::DevRenameIn).on_submit(Msg::DevRenameSave).padding(11).size(15).style(ui::input_style(t)),
                    pill(t, Kind::Tonal, None, crate::i18n::tr_str("common_save"), Some(Msg::DevRenameSave)),
                    pill(t, Kind::Ghost, None, crate::i18n::tr_str("common_cancel"), Some(Msg::DevRenameCancel)),
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .into()
            } else {
                let mut r = row![column![semi(name.clone(), 15.0, t.ink), tx(sub, 13.0, t.ink2)].spacing(2).width(Fill)]
                    .spacing(8)
                    .align_y(Alignment::Center);
                r = r.push(pill(t, Kind::Quiet, Some(Icon::Pencil), crate::i18n::tr_str("common_rename"), Some(Msg::DevRenameStart(d.device.clone()))));
                if !d.this_device || others {
                    r = r.push(pill(t, Kind::Danger, None, crate::i18n::tr_str("common_unlink"), Some(Msg::UnlinkAsk(d.device.clone()))));
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
            crate::i18n::tr_str("link_devices_sync_directly_whenever"),
            13.0,
            t.ink2,
        ))
        .into()
    }

    fn unlink_confirm(&self, t: Tok, name: &str, this: bool) -> El<'_> {
        let (title, body) = if this {
            (
                crate::i18n::tr_str("link_unlink_this_computer").to_string(),
                crate::i18n::tr_args("link_is_removed_from_this", &[("profile_name", &(self.profile_name).to_string())]),
            )
        } else {
            (
                crate::i18n::tr_args("common_unlink_2", &[("name", &(name).to_string())]),
                crate::i18n::tr_str("link_it_stops_syncing_and").to_string(),
            )
        };
        let mut c = column![bold(title, 16.0, t.ink), tx(body, 13.0, t.ink2)].spacing(10);
        if !this {
            c = c.push(banner(
                t,
                crate::i18n::tr_str("link_lost_or_stolen"),
                crate::i18n::tr_str("link_unlinking_cant_lock_it"),
                None,
                false,
            ));
        }
        if self.has_pass {
            c = c.push(self.pass_field(t, crate::i18n::tr_str("link_your_passphrase"), &self.link.pass, Msg::LinkPassIn, Some(Msg::UnlinkGo)));
        } else {
            c = c.push(tx(crate::i18n::tr_str("link_this_account_has_no_passphrase"), 12.0, t.ink2));
        }
        c.push(
            row![
                pill(t, Kind::Danger, Some(Icon::Trash), if self.busy { crate::i18n::tr_str("link_unlinking") } else { crate::i18n::tr_str("common_unlink") }, (!self.busy && (!self.has_pass || !self.link.pass.is_empty())).then_some(Msg::UnlinkGo)),
                pill(t, Kind::Ghost, None, crate::i18n::tr_str("common_cancel"), Some(Msg::UnlinkCancel)),
            ]
            .spacing(8),
        )
        .into()
    }
}
