//! A wrapper that paints a smooth-cornered surface (geom.rs) behind its content.
//!
//! The surface is the widget's own bounds, so it always follows the content's size in the same
//! layout pass (no frame of lag). Events, focus, text selection and overlays go straight to the
//! content. The outline is rasterised with tiny-skia at device pixels (like the icons in ui.rs) and
//! kept as an image handle in the widget state, so a bubble is rasterised once and again only when its
//! size, radii, colours or the window scale change; a small global cache shares identical shapes.
//! (iced's canvas meshes need MSAA, and with the 10-bit surface format wgpu picks here the 2-bit
//! alpha turns edge pixels bright; a raster has exact anti-aliasing on every backend.) Content is
//! drawn in its own layer so it stays above the surface (iced draws a layer's quads, then meshes,
//! then images, then text). Bubbles outside the viewport draw nothing.

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, tree, Tree};
use iced::advanced::image::{self as img, Renderer as _};
use iced::advanced::{mouse, overlay, renderer, Clipboard, Renderer as _, Shell, Widget};
use iced::{Color, Element, Event, Length, Rectangle, Size, Vector};
use resvg::tiny_skia as sk;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use crate::geom::{self, Radii};

pub struct Shaped<'a, Message, Theme> {
    content: Element<'a, Message, Theme, iced::Renderer>,
    radii: Radii,
    smoothing: f32,
    fill: Color,
    /// Hairline inside the edge: (colour, width).
    stroke: Option<(Color, f32)>,
}

pub fn shaped<'a, Message, Theme>(
    content: impl Into<Element<'a, Message, Theme, iced::Renderer>>,
    radii: Radii,
    fill: Color,
) -> Shaped<'a, Message, Theme> {
    Shaped { content: content.into(), radii, smoothing: geom::SMOOTH, fill, stroke: None }
}

impl<'a, Message, Theme> Shaped<'a, Message, Theme> {
    pub fn stroke(mut self, color: Color, width: f32) -> Self {
        self.stroke = Some((color, width));
        self
    }
}

type Key = (u32, u32, [u32; 4], u32, [u32; 4], Option<([u32; 4], u32)>);

#[derive(Default)]
struct State {
    cached: std::cell::RefCell<Option<(Key, img::Handle)>>,
}

fn bits(c: Color) -> [u32; 4] {
    [c.r.to_bits(), c.g.to_bits(), c.b.to_bits(), c.a.to_bits()]
}

fn paint(c: Color) -> sk::Paint<'static> {
    let mut p = sk::Paint::default();
    p.set_color(sk::Color::from_rgba(c.r, c.g, c.b, c.a).unwrap_or(sk::Color::TRANSPARENT));
    p.anti_alias = true;
    p
}

fn sk_path(segs: &[geom::Seg], s: f32) -> Option<sk::Path> {
    let mut b = sk::PathBuilder::new();
    for seg in segs {
        match *seg {
            geom::Seg::Move(p) => b.move_to(p.x * s, p.y * s),
            geom::Seg::Line(p) => b.line_to(p.x * s, p.y * s),
            geom::Seg::Cubic(a, c, e) => b.cubic_to(a.x * s, a.y * s, c.x * s, c.y * s, e.x * s, e.y * s),
            geom::Seg::Close => b.close(),
        }
    }
    b.finish()
}

/// The surface as straight-alpha RGBA, `wpx` x `hpx` device pixels for a `w` x `h` logical box.
fn rasterise(wpx: u32, hpx: u32, scale: f32, radii: Radii, smoothing: f32, fill: Color, stroke: Option<(Color, f32)>) -> img::Handle {
    let blank = || img::Handle::from_rgba(1, 1, vec![0, 0, 0, 0]);
    let Some(mut pm) = sk::Pixmap::new(wpx, hpx) else { return blank() };
    let (w, h) = (wpx as f32 / scale, hpx as f32 / scale);
    if let Some(p) = sk_path(&geom::outline(w, h, radii, smoothing, 0.0, 0.0), scale) {
        pm.fill_path(&p, &paint(fill), sk::FillRule::Winding, sk::Transform::identity(), None);
    }
    if let Some((color, width)) = stroke
        && let Some(p) = sk_path(&geom::inset_outline(w, h, radii, smoothing, width / 2.0), scale)
    {
        let st = sk::Stroke { width: width * scale, ..Default::default() };
        pm.stroke_path(&p, &paint(color), &st, sk::Transform::identity(), None);
    }
    let mut out = Vec::with_capacity(pm.data().len());
    for p in pm.pixels() {
        let c = p.demultiply();
        out.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    img::Handle::from_rgba(wpx, hpx, out)
}

/// Identical shapes (same size, radii, colours) share one raster; the map is dropped when it grows
/// past a few hundred entries (only bubbles on screen draw, so it refills with the visible ones).
fn shared(key: Key, make: impl FnOnce() -> img::Handle) -> img::Handle {
    static CACHE: OnceLock<Mutex<HashMap<Key, img::Handle>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let mut map = cache.lock().unwrap();
    if let Some(h) = map.get(&key) {
        return h.clone();
    }
    if map.len() > 400 {
        map.clear();
    }
    let h = make();
    map.insert(key, h.clone());
    h
}

impl<Message, Theme> Widget<Message, Theme, iced::Renderer> for Shaped<'_, Message, Theme> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }
    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }
    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &iced::Renderer, limits: &layout::Limits) -> layout::Node {
        let child = self.content.as_widget_mut().layout(&mut tree.children[0], renderer, limits);
        layout::Node::with_children(child.size(), vec![child])
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &iced::Renderer, op: &mut dyn widget::Operation) {
        self.content.as_widget_mut().operate(&mut tree.children[0], layout.children().next().unwrap(), renderer, op);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout.children().next().unwrap(),
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(&tree.children[0], layout.children().next().unwrap(), cursor, viewport, renderer)
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
        let bounds = layout.bounds();
        let Some(visible) = bounds.intersection(viewport) else { return };
        let state = tree.state.downcast_ref::<State>();
        // Device-pixel size and position, so the edge pixels are exact and the image is not resampled.
        let scale = crate::ui::scale();
        let (wpx, hpx) = ((bounds.width * scale).round().max(1.0) as u32, (bounds.height * scale).round().max(1.0) as u32);
        let key: Key = (wpx, hpx, self.radii.map(f32::to_bits), scale.to_bits(), bits(self.fill), self.stroke.map(|(c, w)| (bits(c), w.to_bits())));
        let handle = {
            let mut c = state.cached.borrow_mut();
            match &*c {
                Some((k, h)) if *k == key => h.clone(),
                _ => {
                    let h = shared(key, || rasterise(wpx, hpx, scale, self.radii, self.smoothing, self.fill, self.stroke));
                    *c = Some((key, h.clone()));
                    h
                }
            }
        };
        let at = Rectangle {
            x: (bounds.x * scale).round() / scale,
            y: (bounds.y * scale).round() / scale,
            width: wpx as f32 / scale,
            height: hpx as f32 / scale,
        };
        renderer.draw_image(img::Image::new(handle).filter_method(img::FilterMethod::Nearest).snap(true), at, visible);
        renderer.with_layer(visible, |r| {
            self.content.as_widget().draw(
                &tree.children[0],
                r,
                theme,
                style,
                layout.children().next().unwrap(),
                cursor,
                viewport,
            );
        });
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(&mut tree.children[0], layout.children().next().unwrap(), renderer, viewport, translation)
    }
}

impl<'a, Message: 'a, Theme: 'a> From<Shaped<'a, Message, Theme>> for Element<'a, Message, Theme, iced::Renderer> {
    fn from(s: Shaped<'a, Message, Theme>) -> Self {
        Element::new(s)
    }
}

