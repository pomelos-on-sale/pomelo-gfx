pub mod canvas;
pub mod color;
pub mod geometry;
pub mod paint;
pub mod path;
pub mod pixmap;
pub mod raster;

pub use canvas::Canvas;
pub use color::{
    blend_rgb565, blend_rgb888_onto_rgb565, rgb565_to_rgb888, rgb888_to_rgb565, Color, ColorU8,
};
pub use geometry::{Point, RRect, Radius, Rect, Size, Transform};
pub use paint::{
    FillRule, GradientStop, LineCap, LineJoin, LinearGradient, Paint, Shader, SpreadMode, Stroke,
};
pub use path::{Path, PathBuilder, PathVerb};
pub use pixmap::{Pixmap, Pixmap565, Pixmap565Mut, PixmapMut};

pub mod prelude {
    pub use crate::canvas::Canvas;
    pub use crate::color::{blend_rgb565, rgb565_to_rgb888, rgb888_to_rgb565, Color};
    pub use crate::geometry::{Point, RRect, Radius, Rect, Size, Transform};
    pub use crate::paint::{
        FillRule, GradientStop, LineCap, LineJoin, LinearGradient, Paint, Shader, SpreadMode,
        Stroke,
    };
    pub use crate::path::{Path, PathBuilder, PathVerb};
    pub use crate::pixmap::{Pixmap, Pixmap565, Pixmap565Mut, PixmapMut};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_rgb565_roundtrip() {
        let red = Color::RED;
        assert_eq!(red.to_rgb565(), 0xF800);
        let (r, g, b) = rgb565_to_rgb888(0xF800);
        assert_eq!(r, 255);
        assert_eq!(g, 0);
        assert_eq!(b, 0);

        let green = Color::GREEN;
        assert_eq!(green.to_rgb565(), 0x07E0);

        let blue = Color::BLUE;
        assert_eq!(blue.to_rgb565(), 0x001F);
    }

    #[test]
    fn test_blend_rgb565() {
        let white = 0xFFFF;
        let black = 0x0000;
        let mid = blend_rgb565(black, white, 128);
        assert!(mid > 0 && mid < white);
    }

    #[test]
    fn test_pixmap_fill_and_rect() {
        let mut pixmap = Pixmap565::new(100, 100).expect("failed to allocate");
        assert_eq!(pixmap.data().len(), 10000);
        pixmap.fill(0x0000);

        let mut canvas = Canvas::new(pixmap.as_mut());
        canvas.draw_rect(Rect::from_ltwh(10.0, 10.0, 20.0, 20.0), Color::RED);

        // Check inside rect
        let row15 = &pixmap.data()[15 * 100..16 * 100];
        assert_eq!(row15[15], 0xF800);
        // Check outside rect
        assert_eq!(row15[5], 0x0000);
    }

    #[test]
    fn test_rrect_and_circle() {
        let mut pixmap = Pixmap565::new(200, 200).unwrap();
        let mut canvas = Canvas::new(pixmap.as_mut());

        canvas.draw_rrect(
            RRect::from_rect_radius(
                Rect::from_ltwh(10.0, 10.0, 80.0, 50.0),
                Radius::circular(10.0),
            ),
            Color::BLUE,
        );

        canvas.draw_circle(Point::new(150.0, 150.0), 30.0, Color::GREEN);
        assert_eq!(pixmap.data()[150 * 200 + 150], 0x07E0);

        // Check that the corner arc of the rrect has a smooth anti-aliased edge pixel
        // that is blended between 0x0000 (black) and 0x001F (blue)
        let edge_pixel = pixmap.data()[13 * 200 + 12];
        assert!(
            edge_pixel > 0 && edge_pixel < 0x001F,
            "expected anti-aliased blend on rrect corner edge, got {edge_pixel:#06x}"
        );
    }

    #[test]
    fn test_gradient_filled_rect() {
        let mut pixmap = Pixmap565::new(100, 20).expect("failed to allocate");
        let mut canvas = Canvas::new(pixmap.as_mut());

        let shader = LinearGradient::new(
            Point::new(0.0, 0.0),
            Point::new(100.0, 0.0),
            vec![
                GradientStop::new(0.0, Color::RED),
                GradientStop::new(1.0, Color::BLUE),
            ],
            SpreadMode::Pad,
            Transform::identity(),
        )
        .expect("a gradient");

        canvas.fill_rect(
            Rect::from_ltwh(0.0, 0.0, 100.0, 20.0),
            &Paint {
                shader,
                anti_alias: true,
            },
        );

        let left = rgb565_to_rgb888(pixmap.data()[10 * 100 + 2]);
        let right = rgb565_to_rgb888(pixmap.data()[10 * 100 + 97]);

        // A red-to-blue gradient has to be red on one side and blue on the other. A solid fill
        // would make one of these assertions fail whichever end it picked.
        assert!(left.0 > left.2, "the left end is the first stop: {left:?}");
        assert!(
            right.2 > right.0,
            "the right end is the last stop: {right:?}"
        );
    }

    #[test]
    fn test_path_flatten_and_stroke() {
        let mut pb = PathBuilder::new();
        pb.move_to(0.0, 0.0);
        pb.cubic_to(10.0, 50.0, 90.0, 50.0, 100.0, 100.0);
        let path = pb.finish().unwrap();

        let mut pixmap = Pixmap565::new(120, 120).unwrap();
        let mut canvas = Canvas::new(pixmap.as_mut());
        let paint = Paint::new(Color::RED);
        let stroke = Stroke {
            width: 4.0,
            line_cap: LineCap::Round,
            ..Default::default()
        };
        canvas.stroke_path(&path, &paint, &stroke);
    }
}
