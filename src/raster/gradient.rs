use crate::color::dither_float_to_rgb565;
use crate::geometry::Rect;
use crate::pixmap::Pixmap565Mut;

/// Bayer 8x8 matrix dithering gradient to eliminate color banding on RGB565.
pub fn fill_dithered_horizontal_gradient(
    pixmap: &mut Pixmap565Mut<'_>,
    clip: Option<Rect>,
    c0: (u8, u8, u8),
    c1: (u8, u8, u8),
) {
    let pix_w = pixmap.width as usize;
    let pix_h = pixmap.height as usize;

    let (x1, y1, x2, y2) = match clip {
        Some(c) => (
            (c.x.max(0.0).floor() as usize).min(pix_w),
            (c.y.max(0.0).floor() as usize).min(pix_h),
            (c.right().min(pix_w as f32).ceil() as usize).min(pix_w),
            (c.bottom().min(pix_h as f32).ceil() as usize).min(pix_h),
        ),
        None => (0, 0, pix_w, pix_h),
    };

    let inv_w = if pix_w > 1 {
        1.0 / (pix_w - 1) as f32
    } else {
        0.0
    };
    let (r0, g0, b0) = (c0.0 as f32, c0.1 as f32, c0.2 as f32);
    let (dr, dg, db) = (c1.0 as f32 - r0, c1.1 as f32 - g0, c1.2 as f32 - b0);

    for y in y1..y2 {
        let row = pixmap.row_mut(y as u32);
        for x in x1..x2 {
            let t = x as f32 * inv_w;
            let r = r0 + dr * t;
            let g = g0 + dg * t;
            let b = b0 + db * t;
            row[x] = dither_float_to_rgb565(r, g, b, x as i32, y as i32);
        }
    }
}
