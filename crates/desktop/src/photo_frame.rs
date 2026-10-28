//! Keep photo bubbles at the image's width, even when a caption or timestamp fills its row.
use iced::advanced::renderer;
use iced::widget::container::{Catalog, Container};
use iced::Padding;

pub fn constrain<Message, Theme: Catalog, Renderer: renderer::Renderer>(
    frame: Container<'_, Message, Theme, Renderer>,
    photo_width: Option<f32>,
    padding: Padding,
) -> Container<'_, Message, Theme, Renderer> {
    match photo_width {
        Some(width) => frame.width(width + padding.left + padding.right),
        None => frame,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::{image, layout, widget::Tree, Widget};
    use iced::widget::{column, container, Space};
    use iced::{Length, Size, Theme};

    // Iced's renderer for `()` exists only with debug assertions. This layout-only
    // renderer keeps the regression runnable in optimized test profiles too.
    struct LayoutOnly;
    impl renderer::Renderer for LayoutOnly {
        fn start_layer(&mut self, _: iced::Rectangle) { unreachable!("layout must not draw") }
        fn end_layer(&mut self) { unreachable!("layout must not draw") }
        fn start_transformation(&mut self, _: iced::Transformation) { unreachable!("layout must not draw") }
        fn end_transformation(&mut self) { unreachable!("layout must not draw") }
        fn reset(&mut self, _: iced::Rectangle) { unreachable!("layout must not draw") }
        fn fill_quad(&mut self, _: renderer::Quad, _: impl Into<iced::Background>) { unreachable!("layout must not draw") }
        fn allocate_image(&mut self, _: &image::Handle, _: impl FnOnce(Result<image::Allocation, image::Error>) + Send + 'static) {
            unreachable!("layout must not allocate images")
        }
    }

    #[test]
    fn photo_and_timestamp_stay_inside_portrait_landscape_and_square_bubbles() {
        for (width, height) in [(191.0, 340.0), (300.0, 180.0), (300.0, 300.0)] {
            // Group photos and direct photos use different padding; both must stay compact.
            for padding in [Padding::from([8, 13]), Padding { top: 3.0, right: 3.0, bottom: 6.0, left: 3.0 }] {
                for caption in [false, true] {
                    let mut body = column![Space::new().width(width).height(height)];
                    if caption {
                        body = body.push(Space::new().width(Length::Fill).height(42));
                    }
                    let body = body.push(container(Space::new().width(50).height(12)).align_right(Length::Fill));
                    let frame: Container<'_, (), Theme, LayoutOnly> = container(body).padding(padding).max_width(560);
                    let mut frame = constrain(frame, Some(width), padding);
                    let mut tree = Tree::new(&frame as &dyn Widget<(), Theme, LayoutOnly>);
                    let node = frame.layout(&mut tree, &LayoutOnly, &layout::Limits::new(Size::ZERO, Size::new(900.0, 900.0)));
                    assert_eq!(node.size().width, width + padding.left + padding.right);
                    let image = &node.children()[0].children()[0];
                    assert_eq!(image.size().width, width);
                    let stamp = node.children()[0].children().last().unwrap();
                    assert_eq!(stamp.size().width, width);
                }
            }
        }
    }
}
