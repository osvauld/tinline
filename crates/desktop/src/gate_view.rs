//! The gate card for the screens around sign-up (link, unlinked, key lost, set passphrase, ...): the same frame as
//! the ported sign-up (Gate.svelte desktop): 600 wide, up to 640 tall, radius 24, --surface-raised with the rim; the
//! brand row; title 28/34 bold and the lead 15/22 muted; the content scrolls inside the card. Their actions stay in
//! their existing security flows.
use super::*;
use iced::widget::column;

pub(super) fn frame<'a>(t: Tok, title: &str, sub: &str, content: El<'a>) -> El<'a> {
    let brand = container(row![ui::logo(24.0, t), semi("Tinline", 17.0, t.ink).line_height(lh(24.0))].spacing(8).align_y(Alignment::Center).height(44))
        .padding(Padding { top: 12.0, right: 22.0, bottom: 0.0, left: 68.0 }).height(56).width(Fill);
    let mut head = column![bold(title.to_string(), 28.0, t.ink).line_height(lh(34.0))].spacing(16);
    if !sub.is_empty() {
        head = head.push(tx(sub.to_string(), 15.0, t.ink2).line_height(lh(22.0)));
    }
    let body = scroll(t, container(column![head, content].spacing(16)).padding(Padding { top: 8.0, right: 40.0, bottom: 32.0, left: 40.0 }).width(Fill)).height(Fill);
    let inner = container(column![brand, body].width(Fill).height(Fill)).padding(1);
    let card = ui::raised(container(inner).width(Fill).height(Fill), t, ui::Depth::Sheet, ui::outlined(t.raised, t.line, 24.0));
    let framed = container(card).width(Fill).max_width(600).height(Fill).max_height(640);
    container(framed).padding(24).width(Fill).height(Fill).center_x(Fill).center_y(Fill).style(ui::plain(t.canvas, 0.0)).into()
}
