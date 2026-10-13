//! Desktop gate frame only. Setup/unlock/link actions remain in their existing security flows.
use super::*;
use iced::widget::column;
use crate::shaped::shaped;

pub(super) fn frame<'a>(t: Tok, title: &str, sub: &str, content: El<'a>) -> El<'a> {
    let brand = row![ui::logo(24.0, t), semi("Tinline", 17.0, t.ink)]
        .spacing(8).align_y(Alignment::Center).height(44);
    let head = column![bold(title.to_string(), 28.0, t.ink), tx(sub.to_string(), 15.0, t.ink2)]
        .spacing(16);
    let card = shaped(
        container(column![brand, head, content].spacing(24)).padding([32, 40]).width(600),
        [24.0; 4], t.surface,
    ).stroke(t.line, 1.0);
    // Shrink the viewport when it fits; scroll instead of clipping at small window heights.
    container(scroll(t, container(card).center_x(Fill)).height(Length::Shrink))
        .padding(24).center_x(Fill).center_y(Fill)
        .style(ui::plain(t.canvas, 0.0)).into()
}
