//! `Tallest`: shows one of several (static) elements but reserves the size of the largest, so swapping
//! which one is shown never moves anything around it (the no-layout-shift rule). Every variant is laid out
//! with the same limits on every pass, so the reservation holds at any window width or text scale.
//! Only the shown variant is drawn or operated on; the variants are meant to be plain text (no input).

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, Tree};
use iced::advanced::{Widget, mouse, renderer};
use iced::{Element, Length, Rectangle, Size};

pub struct Tallest<'a, Message, Theme> {
    variants: Vec<Element<'a, Message, Theme, iced::Renderer>>,
    shown: usize,
}

/// Show `variants[shown]` while reserving the largest variant's size.
pub fn tallest<'a, Message, Theme>(
    variants: Vec<Element<'a, Message, Theme, iced::Renderer>>,
    shown: usize,
) -> Tallest<'a, Message, Theme> {
    let shown = shown.min(variants.len().saturating_sub(1));
    Tallest { variants, shown }
}

impl<Message, Theme> Widget<Message, Theme, iced::Renderer> for Tallest<'_, Message, Theme> {
    fn children(&self) -> Vec<Tree> {
        self.variants.iter().map(Tree::new).collect()
    }
    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&self.variants);
    }
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let nodes: Vec<layout::Node> = self
            .variants
            .iter_mut()
            .zip(tree.children.iter_mut())
            .map(|(v, t)| v.as_widget_mut().layout(t, renderer, limits))
            .collect();
        let size = nodes.iter().fold(Size::ZERO, |m, n| {
            Size::new(m.width.max(n.size().width), m.height.max(n.size().height))
        });
        layout::Node::with_children(limits.resolve(Length::Fill, Length::Shrink, size), nodes)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        op: &mut dyn widget::Operation,
    ) {
        if let (Some(v), Some(l)) = (
            self.variants.get_mut(self.shown),
            layout.children().nth(self.shown),
        ) {
            v.as_widget_mut()
                .operate(&mut tree.children[self.shown], l, renderer, op);
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        if let (Some(v), Some(l)) = (
            self.variants.get(self.shown),
            layout.children().nth(self.shown),
        ) {
            v.as_widget().draw(
                &tree.children[self.shown],
                renderer,
                theme,
                style,
                l,
                cursor,
                viewport,
            );
        }
    }
}

impl<'a, Message: 'a, Theme: 'a> From<Tallest<'a, Message, Theme>>
    for Element<'a, Message, Theme, iced::Renderer>
{
    fn from(t: Tallest<'a, Message, Theme>) -> Self {
        Element::new(t)
    }
}
