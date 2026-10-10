pub mod arch;
pub mod canvas;
pub mod color;
pub mod geometry;
pub mod paint;
pub mod path;
pub mod pixmap;
pub mod raster;

pub use canvas::Canvas;
pub use color::{
    blend_rgb565, blend_rgb888_onto_rgb565, dither_float_to_rgb565, dither_rgb888_to_rgb565,
    rgb565_to_rgb888, rgb888_to_rgb565, Color, ColorU8, BAYER8, DITHER_OFFSETS,
};
pub use geometry::{Point, RRect, Radius, Rect, Size, Transform};
pub use paint::{
    FillRule, GradientStop, LineCap, LineJoin, LinearGradient, Paint, Shader, SpreadMode, Stroke,
};
pub use path::{Path, PathBuilder, PathVerb};
pub use pixmap::{Pixmap, Pixmap565, Pixmap565Mut, PixmapMut};

pub mod prelude {
    pub use crate::canvas::Canvas;
    pub use crate::color::{
        blend_rgb565, dither_float_to_rgb565, dither_rgb888_to_rgb565, rgb565_to_rgb888,
        rgb888_to_rgb565, Color, BAYER8, DITHER_OFFSETS,
    };
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

        // Verify that top-facing arc (row 10) has smooth multi-pixel anti-aliasing gradient
        // across the transition band rather than an abrupt 1-pixel step
        let aa_count_row10 = (15..20)
            .filter(|&x| {
                let px = pixmap.data()[10 * 200 + x];
                px > 0 && px < 0x001F
            })
            .count();
        assert!(
            aa_count_row10 >= 2,
            "expected at least 2 anti-aliased transition pixels on top corner arc, got {aa_count_row10}"
        );

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

        // Verify that anti-aliased transition pixels exist along the stroke
        let red565 = Color::RED.to_rgb565();
        let aa_pixels = pixmap
            .data()
            .iter()
            .filter(|&&px| px > 0 && px < red565)
            .count();
        assert!(
            aa_pixels > 0,
            "expected anti-aliased pixels along the stroked path, got {aa_pixels}"
        );
    }

    #[test]
    fn test_horizontal_and_vertical_stroke_aa() {
        let red565 = Color::RED.to_rgb565();
        let stroke = Stroke {
            width: 4.0,
            line_cap: LineCap::Round,
            ..Default::default()
        };
        let paint = Paint::new(Color::RED);

        // 1. Horizontal stroke: top and bottom edges must have anti-aliased pixels across the span
        let mut pixmap_h = Pixmap565::new(100, 50).unwrap();
        let mut canvas_h = Canvas::new(pixmap_h.as_mut());
        let mut pb_h = PathBuilder::new();
        pb_h.move_to(20.0, 25.3);
        pb_h.line_to(80.0, 25.3);
        let path_h = pb_h.finish().unwrap();
        canvas_h.stroke_path(&path_h, &paint, &stroke);

        let aa_h = pixmap_h
            .data()
            .iter()
            .filter(|&&px| px > 0 && px < red565)
            .count();
        assert!(
            aa_h >= 100,
            "expected at least 100 anti-aliased pixels along the horizontal stroke edges, got {aa_h}"
        );

        // 2. Vertical stroke: left and right edges must have anti-aliased pixels across the span
        let mut pixmap_v = Pixmap565::new(50, 100).unwrap();
        let mut canvas_v = Canvas::new(pixmap_v.as_mut());
        let mut pb_v = PathBuilder::new();
        pb_v.move_to(25.3, 20.0);
        pb_v.line_to(25.3, 80.0);
        let path_v = pb_v.finish().unwrap();
        canvas_v.stroke_path(&path_v, &paint, &stroke);

        let aa_v = pixmap_v
            .data()
            .iter()
            .filter(|&&px| px > 0 && px < red565)
            .count();
        assert!(
            aa_v >= 100,
            "expected at least 100 anti-aliased pixels along the vertical stroke edges, got {aa_v}"
        );
    }

    #[test]
    fn test_kurbo_interop() {
        // 1. Point / Rect conversions
        let pt = Point::new(12.5, 34.75);
        let kpt: kurbo::Point = pt.into();
        assert_eq!(Point::from(kpt), pt);

        let rect = Rect::from_ltwh(10.0, 20.0, 100.0, 50.0);
        let krect: kurbo::Rect = rect.into();
        assert_eq!(Rect::from(krect), rect);

        // 2. Stroke conversion
        let stroke = Stroke {
            width: 3.5,
            miter_limit: 8.0,
            line_cap: LineCap::Round,
            line_join: LineJoin::Bevel,
        };
        let kstroke: kurbo::Stroke = stroke.clone().into();
        let back_stroke: Stroke = kstroke.into();
        assert_eq!(stroke, back_stroke);

        // 3. Circle to Path and bounds
        let circle = kurbo::Circle::new((50.0, 50.0), 20.0);
        let path = Path::from(circle);
        assert!(!path.is_empty());
        let bounds = path.bounds();
        assert!((bounds.x - 30.0).abs() < 1e-2);
        assert!((bounds.y - 30.0).abs() < 1e-2);
        assert!((bounds.width - 40.0).abs() < 1e-2);
        assert!((bounds.height - 40.0).abs() < 1e-2);

        // 4. PathBuilder push_shape / push_rrect
        let mut pb = PathBuilder::new();
        pb.push_rrect(RRect::from_rect_xy(rect, 5.0, 5.0));
        let path_rrect = pb.finish().unwrap();
        assert!(!path_rrect.is_empty());
        let polylines = path_rrect.flatten(0.1);
        assert!(!polylines.is_empty());
    }

    #[test]
    fn test_transform_robustness_and_gradient_nan() {
        use crate::paint::{GradientStop, LinearGradient, SpreadMode};

        // 1. Negative scaling / reflection must produce well-formed Rect
        let t_neg = Transform::from_scale(-1.5, -2.0);
        let r = Rect::from_ltwh(10.0, 20.0, 30.0, 40.0);
        let mapped = t_neg.map_rect(r);
        assert!(mapped.width >= 0.0);
        assert!(mapped.height >= 0.0);
        assert_eq!(mapped.width, 45.0);
        assert_eq!(mapped.height, 80.0);
        assert_eq!(mapped.x, -60.0);
        assert_eq!(mapped.y, -120.0);

        // 2. LinearGradient NaN / Inf coordinates handling
        let grad = LinearGradient {
            start: Point::new(0.0, 0.0),
            end: Point::new(100.0, 0.0),
            stops: vec![
                GradientStop::new(0.0, Color::RED),
                GradientStop::new(1.0, Color::BLUE),
            ],
            spread: SpreadMode::Pad,
            transform: Transform::identity(),
        };
        let c_nan = grad.color_at(f32::NAN, 0.0);
        assert_eq!(c_nan, Color::RED);

        let c_inf = grad.color_at(f32::INFINITY, 0.0);
        assert_eq!(c_inf, Color::RED);
    }

    #[test]
    fn test_bayer_dithering() {
        // 1. Verify matrix has all 64 values 0..=63
        let mut seen = [false; 64];
        for &val in BAYER8.iter() {
            assert!((val as usize) < 64);
            assert!(!seen[val as usize]);
            seen[val as usize] = true;
        }

        // 2. Uniform exact multiple (e.g. 16, 32, 48) has zero noise
        let exact_rgb = rgb888_to_rgb565(16, 32, 48);
        for y in 0..8 {
            for x in 0..8 {
                let d = dither_rgb888_to_rgb565(16, 32, 48, x, y);
                assert_eq!(d, exact_rgb);
            }
        }

        // 3. Fraction rounding: r=17 (16 + 1/8). Exactly 8 out of 64 pixels should round up.
        let mut count_up = 0;
        for y in 0..8 {
            for x in 0..8 {
                let d = dither_rgb888_to_rgb565(17, 32, 48, x, y);
                let r5 = (d >> 11) & 0x1F;
                if r5 == 3 {
                    count_up += 1;
                } else {
                    assert_eq!(r5, 2);
                }
            }
        }
        assert_eq!(count_up, 8); // 8 / 64 = 1/8

        // 4. Pure black (0) and pure white (255)
        for y in 0..8 {
            for x in 0..8 {
                assert_eq!(dither_rgb888_to_rgb565(0, 0, 0, x, y), 0x0000);
                assert_eq!(dither_rgb888_to_rgb565(255, 255, 255, x, y), 0xFFFF);
            }
        }

        // 5. Negative coordinates wrap properly and do not panic
        let d_neg = dither_rgb888_to_rgb565(100, 100, 100, -1, -5);
        let d_pos = dither_rgb888_to_rgb565(100, 100, 100, 7, 3);
        assert_eq!(d_neg, d_pos);

        // 6. Continuous float dithering: fractional step of 1/64 produces exactly 1 pixel round up
        let mut count_float_1 = 0;
        for y in 0..8 {
            for x in 0..8 {
                // r = 16.0 + 0.125 (0.125 is 1/64th of 8.0, so exactly 1 quantum step in 64-level Bayer)
                let d = dither_float_to_rgb565(16.0 + 0.125, 32.0, 48.0, x, y);
                let r5 = (d >> 11) & 0x1F;
                if r5 == 3 {
                    count_float_1 += 1;
                }
            }
        }
        assert_eq!(count_float_1, 1);
    }

    #[test]
    fn test_oklab_gradient_perceptual_uniformity() {
        use crate::paint::{GradientStop, LinearGradient, SpreadMode};

        let grad = LinearGradient {
            start: Point::new(0.0, 0.0),
            end: Point::new(100.0, 0.0),
            stops: vec![
                GradientStop::new(0.0, Color::BLACK),
                GradientStop::new(1.0, Color::WHITE),
            ],
            spread: SpreadMode::Pad,
            transform: Transform::identity(),
        };

        // Midpoint in Oklab between black (L=0) and white (L=1) is L=0.5.
        // In standard sRGB, L=0.5 corresponds to roughly ~54 (0.214 in linear, ~0.5 in perceptual lightness),
        // whereas naive sRGB linear interpolation would produce 128 (which is perceptually much too bright ~73% lightness).
        let mid = grad.color_at(50.0, 0.0);
        assert!(mid.r > 0 && mid.r < 255);
        assert_eq!(mid.r, mid.g);
        assert_eq!(mid.g, mid.b);
        // In Oklab, midpoint L=0.5 corresponds to linear ~0.125, which through sRGB transfer function yields 99,
        // whereas naive sRGB interpolation would yield 128 (perceptually ~73% lightness, washed out).
        assert_eq!(mid.r, 99);
        assert_eq!(mid.g, 99);
        assert_eq!(mid.b, 99);
    }

    #[test]
    fn test_qoi_decode_and_blit() {
        // Create a 2x2 test image in RGBA:
        // (0,0): Red (255, 0, 0, 255)
        // (1,0): Green (0, 255, 0, 255)
        // (0,1): Blue (0, 0, 255, 255)
        // (1,1): Half-alpha White (255, 255, 255, 128)
        let pixels: [u8; 16] = [
            255, 0, 0, 255,
            0, 255, 0, 255,
            0, 0, 255, 255,
            255, 255, 255, 128,
        ];
        let qoi_bytes = qoi::encode_to_vec(&pixels, 2, 2).expect("encoding 2x2 QOI image");

        // 1. Test decode_qoi_to_rgb565
        let (w, h, rgb565, alpha) =
            raster::decode_qoi_to_rgb565(&qoi_bytes).expect("decode QOI to rgb565");
        assert_eq!(w, 2);
        assert_eq!(h, 2);
        assert_eq!(rgb565.len(), 4);
        assert_eq!(rgb565[0], Color::RED.to_rgb565());
        assert_eq!(rgb565[1], Color::GREEN.to_rgb565());
        assert_eq!(rgb565[2], Color::BLUE.to_rgb565());
        assert_eq!(alpha.as_ref().unwrap()[3], 128);

        // 2. Test Canvas::blit_qoi
        let mut buf = [0u16; 16]; // 4x4 canvas initialized to black
        let pixmap = Pixmap565Mut::new(&mut buf, 4, 4);
        let mut canvas = Canvas::new(pixmap);
        canvas.blit_qoi(1, 1, &qoi_bytes).expect("blit_qoi succeeded");

        // Pixel at (1, 1) should be RED
        assert_eq!(buf[1 * 4 + 1], Color::RED.to_rgb565());
        // Pixel at (2, 1) should be GREEN
        assert_eq!(buf[1 * 4 + 2], Color::GREEN.to_rgb565());
        // Pixel at (1, 2) should be BLUE
        assert_eq!(buf[2 * 4 + 1], Color::BLUE.to_rgb565());
        // Pixel at (2, 2) should be blended half-white over black
        let expected_half_white = crate::color::blend_rgb565(0x0000, 0xFFFF, 128);
        assert_eq!(buf[2 * 4 + 2], expected_half_white);
        // Pixel at (0, 0) should remain untouched (black)
        assert_eq!(buf[0], 0);
    }

    #[test]
    fn test_arch_span_operations() {
        // 1. Test blend_span_rgb565
        let mut row = [0x0000u16; 16]; // black
        crate::arch::blend_span_rgb565(&mut row, Color::WHITE.to_rgb565(), 128);
        let expected = crate::color::blend_rgb565(0x0000, 0xFFFF, 128);
        for &px in row.iter() {
            assert_eq!(px, expected);
        }

        // 2. Test blit_mask_span
        let mut dst = [0x0000u16; 8];
        let mask = [0u8, 64, 128, 192, 255, 0, 0, 255];
        crate::arch::blit_mask_span(&mut dst, &mask, Color::RED.to_rgb565(), 255);

        assert_eq!(dst[0], 0);
        assert_eq!(dst[1], crate::color::blend_rgb565(0, Color::RED.to_rgb565(), 64));
        assert_eq!(dst[2], crate::color::blend_rgb565(0, Color::RED.to_rgb565(), 128));
        assert_eq!(dst[3], crate::color::blend_rgb565(0, Color::RED.to_rgb565(), 192));
        assert_eq!(dst[4], Color::RED.to_rgb565());
        assert_eq!(dst[5], 0);
        assert_eq!(dst[6], 0);
        assert_eq!(dst[7], Color::RED.to_rgb565());
    }

    #[test]
    fn test_lyon_concave_path_tessellation_and_fill() {
        // Build a U-shaped concave polygon:
        // (0,0) -> (30,0) -> (30,30) -> (20,30) -> (20,10) -> (10,10) -> (10,30) -> (0,30) -> close
        let mut pb = PathBuilder::new();
        pb.move_to(0.0, 0.0);
        pb.line_to(30.0, 0.0);
        pb.line_to(30.0, 30.0);
        pb.line_to(20.0, 30.0);
        pb.line_to(20.0, 10.0);
        pb.line_to(10.0, 10.0);
        pb.line_to(10.0, 30.0);
        pb.line_to(0.0, 30.0);
        pb.close();
        let path = pb.finish().expect("concave path");

        // 1. Tessellate must produce triangles
        let buffers = path
            .tessellate(FillRule::Winding, 0.1)
            .expect("tessellate succeeded");
        assert!(!buffers.vertices.is_empty());
        assert!(!buffers.indices.is_empty());
        assert_eq!(buffers.indices.len() % 3, 0);

        // 2. Draw onto a 40x40 Pixmap
        let mut pixmap = Pixmap565::new(40, 40).expect("pixmap allocated");
        let mut canvas = Canvas::new(pixmap.as_mut());
        let paint = Paint {
            shader: Shader::SolidColor(Color::WHITE),
            anti_alias: false,
        };
        canvas.fill_path(&path, &paint, FillRule::Winding);

        // Points inside the U branches should be WHITE:
        // (15, 5) - in the top horizontal bar (x in 0..30, y in 0..10)
        assert_eq!(pixmap.data()[5 * 40 + 15], Color::WHITE.to_rgb565());
        // (5, 20) - in the left vertical branch (x in 0..10, y in 10..30)
        assert_eq!(pixmap.data()[20 * 40 + 5], Color::WHITE.to_rgb565());
        // (25, 20) - in the right vertical branch (x in 20..30, y in 10..30)
        assert_eq!(pixmap.data()[20 * 40 + 25], Color::WHITE.to_rgb565());

        // Point inside the concave notch (15, 20) should remain empty (BLACK = 0)
        assert_eq!(pixmap.data()[20 * 40 + 15], 0);
    }
}
