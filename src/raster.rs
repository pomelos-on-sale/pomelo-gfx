use crate::color::{blend_rgb565, rgb888_to_rgb565, Color};
use crate::geometry::{Point, RRect, Rect};
use crate::paint::{LineCap, Paint, Shader, Stroke};
use crate::pixmap::Pixmap565Mut;

/// Fast fill helper for 16-bit RGB565 slices.
/// Uses native memset (write_bytes) for black/zero, and 32-bit dwords for non-zero colors.
#[inline(always)]
pub fn fill_u16_slice(slice: &mut [u16], val: u16) {
    if val == 0 {
        unsafe {
            std::ptr::write_bytes(slice.as_mut_ptr() as *mut u8, 0, slice.len() * 2);
        }
        return;
    }

    let len = slice.len();
    if len == 0 {
        return;
    }

    let mut ptr = slice.as_mut_ptr();
    let mut count = len;

    if (ptr as usize & 2) != 0 && count > 0 {
        unsafe {
            *ptr = val;
            ptr = ptr.add(1);
        }
        count -= 1;
    }

    let val32 = (val as u32) | ((val as u32) << 16);
    let u32_count = count / 2;
    let ptr32 = ptr as *mut u32;

    for i in 0..u32_count {
        unsafe {
            *ptr32.add(i) = val32;
        }
    }

    if (count & 1) != 0 {
        unsafe {
            *ptr.add(count - 1) = val;
        }
    }
}

/// Fill an axis-aligned rectangle with high-speed span filling.
pub fn fill_rect(pixmap: &mut Pixmap565Mut<'_>, clip: Option<Rect>, rect: Rect, color: Color) {
    if rect.width <= 0.0 || rect.height <= 0.0 || color.a == 0 {
        return;
    }

    let bounds = match clip {
        Some(c) => match rect.intersect(&c) {
            Some(i) => i,
            None => return,
        },
        None => rect,
    };

    let pix_w = pixmap.width as i32;
    let pix_h = pixmap.height as i32;

    let x1 = (bounds.x.round() as i32).clamp(0, pix_w);
    let y1 = (bounds.y.round() as i32).clamp(0, pix_h);
    let x2 = (bounds.right().round() as i32).clamp(x1, pix_w);
    let y2 = (bounds.bottom().round() as i32).clamp(y1, pix_h);

    if x2 <= x1 || y2 <= y1 {
        return;
    }

    let col565 = color.to_rgb565();
    let is_opaque = color.a == 255;

    // Ultra-fast path: full pixmap fill using native hardware memset (0.5ms)
    if is_opaque && col565 == 0 && x1 == 0 && y1 == 0 && x2 == pix_w && y2 == pix_h {
        unsafe {
            std::ptr::write_bytes(
                pixmap.data.as_mut_ptr() as *mut u8,
                0,
                pixmap.data.len() * 2,
            );
        }
        return;
    }

    for y in y1..y2 {
        let row = pixmap.row_mut(y as u32);
        let slice = &mut row[x1 as usize..x2 as usize];
        if is_opaque {
            fill_u16_slice(slice, col565);
        } else {
            let a = color.a;
            for px in slice.iter_mut() {
                *px = blend_rgb565(*px, col565, a);
            }
        }
    }
}

/// Fill a rounded rectangle with scanline spans.
pub fn fill_rrect(pixmap: &mut Pixmap565Mut<'_>, clip: Option<Rect>, rrect: RRect, color: Color) {
    let rect = rrect.rect;
    if rect.width <= 0.0 || rect.height <= 0.0 || color.a == 0 {
        return;
    }

    let max_r = (rect.width * 0.5).min(rect.height * 0.5);
    let radius = rrect.radius.x.min(max_r);
    if radius <= 0.5 {
        fill_rect(pixmap, clip, rect, color);
        return;
    }

    let bounds = match clip {
        Some(c) => match rect.intersect(&c) {
            Some(i) => i,
            None => return,
        },
        None => rect,
    };

    let pix_w = pixmap.width as i32;
    let pix_h = pixmap.height as i32;
    let col565 = color.to_rgb565();
    let is_opaque = color.a == 255;
    let a = color.a;

    let y_start = (bounds.y.floor() as i32).clamp(0, pix_h);
    let y_end = (bounds.bottom().ceil() as i32).clamp(y_start, pix_h);

    let rx = radius;
    let ry = radius;
    let r_sq = rx * rx;

    let top_corner_limit = rect.y + ry;
    let bottom_corner_limit = rect.bottom() - ry;

    for y in y_start..y_end {
        let y_f = y as f32 + 0.5;

        let (x_min, x_max) = if y_f < top_corner_limit {
            let dy = top_corner_limit - y_f;
            if dy < ry {
                let dx = (r_sq - dy * dy).max(0.0).sqrt();
                (rect.x + rx - dx, rect.right() - rx + dx)
            } else {
                (rect.x + rx, rect.right() - rx)
            }
        } else if y_f > bottom_corner_limit {
            let dy = y_f - bottom_corner_limit;
            if dy < ry {
                let dx = (r_sq - dy * dy).max(0.0).sqrt();
                (rect.x + rx - dx, rect.right() - rx + dx)
            } else {
                (rect.x + rx, rect.right() - rx)
            }
        } else {
            (rect.x, rect.right())
        };

        let clamp_left = bounds.x.max(0.0);
        let clamp_right = bounds.right().min(pix_w as f32);

        let final_x1 = (x_min.max(clamp_left).round() as i32).clamp(0, pix_w);
        let final_x2 = (x_max.min(clamp_right).round() as i32).clamp(final_x1, pix_w);

        if final_x2 > final_x1 {
            let row = pixmap.row_mut(y as u32);
            let slice = &mut row[final_x1 as usize..final_x2 as usize];
            if is_opaque {
                fill_u16_slice(slice, col565);
            } else {
                for px in slice.iter_mut() {
                    *px = blend_rgb565(*px, col565, a);
                }
            }
        }
    }
}

/// Draw a stroked rounded rectangle border.
pub fn stroke_rrect(
    pixmap: &mut Pixmap565Mut<'_>,
    clip: Option<Rect>,
    rrect: RRect,
    color: Color,
    stroke_width: f32,
) {
    if stroke_width <= 0.0 || color.a == 0 {
        return;
    }
    let rect = rrect.rect;
    if rect.width <= stroke_width * 2.0 || rect.height <= stroke_width * 2.0 {
        fill_rrect(pixmap, clip, rrect, color);
        return;
    }

    let inner_rect = Rect {
        x: rect.x + stroke_width,
        y: rect.y + stroke_width,
        width: rect.width - stroke_width * 2.0,
        height: rect.height - stroke_width * 2.0,
    };
    let inner_radius = (rrect.radius.x - stroke_width).max(0.0);

    let bounds = match clip {
        Some(c) => match rect.intersect(&c) {
            Some(i) => i,
            None => return,
        },
        None => rect,
    };

    let pix_w = pixmap.width as i32;
    let pix_h = pixmap.height as i32;
    let col565 = color.to_rgb565();
    let is_opaque = color.a == 255;
    let a = color.a;

    let y_start = (bounds.y.floor() as i32).clamp(0, pix_h);
    let y_end = (bounds.bottom().ceil() as i32).clamp(y_start, pix_h);

    let outer_r = rrect.radius.x;
    let outer_sq = outer_r * outer_r;
    let inner_sq = inner_radius * inner_radius;

    for y in y_start..y_end {
        let y_f = y as f32 + 0.5;

        // Outer span
        let mut out_x1 = rect.x;
        let mut out_x2 = rect.right();

        if y_f < rect.y + outer_r {
            let dy = rect.y + outer_r - y_f;
            let dx = (outer_sq - dy * dy).max(0.0).sqrt();
            out_x1 = rect.x + outer_r - dx;
            out_x2 = rect.right() - outer_r + dx;
        } else if y_f > rect.bottom() - outer_r {
            let dy = y_f - (rect.bottom() - outer_r);
            let dx = (outer_sq - dy * dy).max(0.0).sqrt();
            out_x1 = rect.x + outer_r - dx;
            out_x2 = rect.right() - outer_r + dx;
        }

        // Inner span (if inside inner rect bounds)
        let in_span = if y_f >= inner_rect.y && y_f <= inner_rect.bottom() {
            let mut in_x1 = inner_rect.x;
            let mut in_x2 = inner_rect.right();

            if inner_radius > 0.0 {
                if y_f < inner_rect.y + inner_radius {
                    let dy = inner_rect.y + inner_radius - y_f;
                    let dx = (inner_sq - dy * dy).max(0.0).sqrt();
                    in_x1 = inner_rect.x + inner_radius - dx;
                    in_x2 = inner_rect.right() - inner_radius + dx;
                } else if y_f > inner_rect.bottom() - inner_radius {
                    let dy = y_f - (inner_rect.bottom() - inner_radius);
                    let dx = (inner_sq - dy * dy).max(0.0).sqrt();
                    in_x1 = inner_rect.x + inner_radius - dx;
                    in_x2 = inner_rect.right() - inner_radius + dx;
                }
            }
            Some((in_x1, in_x2))
        } else {
            None
        };

        let clamp_left = bounds.x.max(0.0);
        let clamp_right = bounds.right().min(pix_w as f32);

        let row = pixmap.row_mut(y as u32);

        match in_span {
            Some((in_x1, in_x2)) => {
                // Draw left segment [out_x1 .. in_x1]
                let lx1 = (out_x1.max(clamp_left).round() as i32).clamp(0, pix_w);
                let lx2 = (in_x1.min(clamp_right).round() as i32).clamp(lx1, pix_w);
                if lx2 > lx1 {
                    let slice = &mut row[lx1 as usize..lx2 as usize];
                    if is_opaque {
                        fill_u16_slice(slice, col565);
                    } else {
                        for px in slice.iter_mut() {
                            *px = blend_rgb565(*px, col565, a);
                        }
                    }
                }
                // Draw right segment [in_x2 .. out_x2]
                let rx1 = (in_x2.max(clamp_left).round() as i32).clamp(0, pix_w);
                let rx2 = (out_x2.min(clamp_right).round() as i32).clamp(rx1, pix_w);
                if rx2 > rx1 {
                    let slice = &mut row[rx1 as usize..rx2 as usize];
                    if is_opaque {
                        fill_u16_slice(slice, col565);
                    } else {
                        for px in slice.iter_mut() {
                            *px = blend_rgb565(*px, col565, a);
                        }
                    }
                }
            }
            None => {
                // Entire row is solid border
                let x1 = (out_x1.max(clamp_left).round() as i32).clamp(0, pix_w);
                let x2 = (out_x2.min(clamp_right).round() as i32).clamp(x1, pix_w);
                if x2 > x1 {
                    let slice = &mut row[x1 as usize..x2 as usize];
                    if is_opaque {
                        fill_u16_slice(slice, col565);
                    } else {
                        for px in slice.iter_mut() {
                            *px = blend_rgb565(*px, col565, a);
                        }
                    }
                }
            }
        }
    }
}

/// Fill a circle using scanline spans.
pub fn fill_circle(
    pixmap: &mut Pixmap565Mut<'_>,
    clip: Option<Rect>,
    center: Point,
    radius: f32,
    color: Color,
) {
    if radius <= 0.0 || color.a == 0 {
        return;
    }
    let rect = Rect::from_ltwh(
        center.x - radius,
        center.y - radius,
        radius * 2.0,
        radius * 2.0,
    );
    let bounds = match clip {
        Some(c) => match rect.intersect(&c) {
            Some(i) => i,
            None => return,
        },
        None => rect,
    };

    let pix_w = pixmap.width as i32;
    let pix_h = pixmap.height as i32;
    let col565 = color.to_rgb565();
    let is_opaque = color.a == 255;
    let a = color.a;

    let y_start = (bounds.y.floor() as i32).clamp(0, pix_h);
    let y_end = (bounds.bottom().ceil() as i32).clamp(y_start, pix_h);
    let r_sq = radius * radius;

    for y in y_start..y_end {
        let dy = (y as f32 + 0.5) - center.y;
        let dx_sq = r_sq - dy * dy;
        if dx_sq < 0.0 {
            continue;
        }
        let dx = dx_sq.sqrt();
        let x1 = ((center.x - dx).max(bounds.x).round() as i32).clamp(0, pix_w);
        let x2 = ((center.x + dx).min(bounds.right()).round() as i32).clamp(x1, pix_w);

        if x2 > x1 {
            let row = pixmap.row_mut(y as u32);
            let slice = &mut row[x1 as usize..x2 as usize];
            if is_opaque {
                slice.fill(col565);
            } else {
                for px in slice.iter_mut() {
                    *px = blend_rgb565(*px, col565, a);
                }
            }
        }
    }
}

/// Direct blit of an 8-bit alpha mask (font glyphs) with color into RGB565 buffer.
pub fn blit_mask(
    pixmap: &mut Pixmap565Mut<'_>,
    clip: Option<Rect>,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    mask: &[u8],
    color: Color,
) {
    if w == 0 || h == 0 || color.a == 0 || mask.is_empty() {
        return;
    }

    let pix_w = pixmap.width as i32;
    let pix_h = pixmap.height as i32;
    let col565 = color.to_rgb565();

    let clip_box = clip.unwrap_or(Rect::from_ltwh(0.0, 0.0, pix_w as f32, pix_h as f32));
    let clip_x1 = (clip_box.x.floor() as i32).max(0);
    let clip_y1 = (clip_box.y.floor() as i32).max(0);
    let clip_x2 = (clip_box.right().ceil() as i32).min(pix_w);
    let clip_y2 = (clip_box.bottom().ceil() as i32).min(pix_h);

    let x1 = x.max(clip_x1);
    let y1 = y.max(clip_y1);
    let x2 = (x + w as i32).min(clip_x2);
    let y2 = (y + h as i32).min(clip_y2);

    if x2 <= x1 || y2 <= y1 {
        return;
    }

    let alpha_base = color.a as u32;

    for py in y1..y2 {
        let gy = (py - y) as usize;
        let row = pixmap.row_mut(py as u32);

        for px in x1..x2 {
            let gx = (px - x) as usize;
            let mask_alpha = mask[gy * (w as usize) + gx];
            if mask_alpha == 0 {
                continue;
            }

            let effective_alpha = ((alpha_base * mask_alpha as u32) / 255) as u8;
            if effective_alpha > 0 {
                let dst = &mut row[px as usize];
                *dst = blend_rgb565(*dst, col565, effective_alpha);
            }
        }
    }
}

/// Direct slice blit of opaque 16-bit RGB565 pixels (e.g. wallpapers, background textures).
pub fn blit_image_565(
    pixmap: &mut Pixmap565Mut<'_>,
    clip: Option<Rect>,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    pixels: &[u16],
) {
    if w == 0 || h == 0 || pixels.len() < (w * h) as usize {
        return;
    }

    let pix_w = pixmap.width as i32;
    let pix_h = pixmap.height as i32;

    let clip_box = clip.unwrap_or(Rect::from_ltwh(0.0, 0.0, pix_w as f32, pix_h as f32));
    let clip_x1 = (clip_box.x.floor() as i32).max(0);
    let clip_y1 = (clip_box.y.floor() as i32).max(0);
    let clip_x2 = (clip_box.right().ceil() as i32).min(pix_w);
    let clip_y2 = (clip_box.bottom().ceil() as i32).min(pix_h);

    let x1 = x.max(clip_x1);
    let y1 = y.max(clip_y1);
    let x2 = (x + w as i32).min(clip_x2);
    let y2 = (y + h as i32).min(clip_y2);

    if x2 <= x1 || y2 <= y1 {
        return;
    }

    let copy_len = (x2 - x1) as usize;
    for py in y1..y2 {
        let sy = (py - y) as usize;
        let sx = (x1 - x) as usize;
        let src_start = sy * (w as usize) + sx;
        let src_slice = &pixels[src_start..src_start + copy_len];

        let row = pixmap.row_mut(py as u32);
        row[x1 as usize..x2 as usize].copy_from_slice(src_slice);
    }
}

/// Blit of 16-bit RGB565 pixels with an 8-bit alpha mask (e.g. app icons with squircle rounded corners).
pub fn blit_image_565_with_alpha(
    pixmap: &mut Pixmap565Mut<'_>,
    clip: Option<Rect>,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    rgb_pixels: &[u16],
    alpha_mask: &[u8],
) {
    let total = (w * h) as usize;
    if w == 0 || h == 0 || rgb_pixels.len() < total || alpha_mask.len() < total {
        return;
    }

    let pix_w = pixmap.width as i32;
    let pix_h = pixmap.height as i32;

    let clip_box = clip.unwrap_or(Rect::from_ltwh(0.0, 0.0, pix_w as f32, pix_h as f32));
    let clip_x1 = (clip_box.x.floor() as i32).max(0);
    let clip_y1 = (clip_box.y.floor() as i32).max(0);
    let clip_x2 = (clip_box.right().ceil() as i32).min(pix_w);
    let clip_y2 = (clip_box.bottom().ceil() as i32).min(pix_h);

    let x1 = x.max(clip_x1);
    let y1 = y.max(clip_y1);
    let x2 = (x + w as i32).min(clip_x2);
    let y2 = (y + h as i32).min(clip_y2);

    if x2 <= x1 || y2 <= y1 {
        return;
    }

    let half_w = w as f32 * 0.5;
    let half_h = h as f32 * 0.5;
    let corner_r = 0.225 * (w.min(h) as f32);
    let inner_w = half_w - corner_r;
    let inner_h = half_h - corner_r;

    for py in y1..y2 {
        let sy = (py - y) as usize;
        let row = pixmap.row_mut(py as u32);
        let py_dist = (sy as f32 + 0.5 - half_h).abs();
        let qy = (py_dist - inner_h).max(0.0);

        for px in x1..x2 {
            let sx = (px - x) as usize;
            let idx = sy * (w as usize) + sx;
            let mut a = alpha_mask[idx];
            if a == 0 {
                continue;
            }

            // Authentic iOS squircle boundary check
            let px_dist = (sx as f32 + 0.5 - half_w).abs();
            let qx = (px_dist - inner_w).max(0.0);
            let dist_from_corner = (qx * qx + qy * qy).sqrt();

            if dist_from_corner > corner_r {
                continue;
            }
            if dist_from_corner > corner_r - 1.25 {
                let edge_factor = ((corner_r - dist_from_corner) / 1.25).clamp(0.0, 1.0);
                a = ((a as f32) * edge_factor) as u8;
                if a == 0 {
                    continue;
                }
            }

            let src = rgb_pixels[idx];

            // White fringe suppressor for hello and counter icons:
            // Filter near-white halo pixels (R>200, G>200, B>200) within 3.0px of the outer edge
            if dist_from_corner > corner_r - 3.0 {
                let r5 = (src >> 11) & 0x1F;
                let g6 = (src >> 5) & 0x3F;
                let b5 = src & 0x1F;
                if r5 >= 25 && g6 >= 50 && b5 >= 25 {
                    continue;
                }
            }

            let dst = &mut row[px as usize];
            if a == 255 {
                *dst = src;
            } else {
                *dst = blend_rgb565(*dst, src, a);
            }
        }
    }
}

/// Blit of 16-bit RGB565 pixels with an 8-bit alpha mask, scaled to (dst_w, dst_h)
/// with authentic iOS squircle boundary antialiasing and edge white fringe removal.
pub fn blit_image_565_with_alpha_scaled(
    pixmap: &mut Pixmap565Mut<'_>,
    clip: Option<Rect>,
    x: i32,
    y: i32,
    dst_w: u32,
    dst_h: u32,
    src_w: u32,
    src_h: u32,
    rgb_pixels: &[u16],
    alpha_mask: &[u8],
) {
    let total = (src_w * src_h) as usize;
    if dst_w == 0
        || dst_h == 0
        || src_w == 0
        || src_h == 0
        || rgb_pixels.len() < total
        || alpha_mask.len() < total
    {
        return;
    }

    let pix_w = pixmap.width as i32;
    let pix_h = pixmap.height as i32;

    let clip_box = clip.unwrap_or(Rect::from_ltwh(0.0, 0.0, pix_w as f32, pix_h as f32));
    let clip_x1 = (clip_box.x.floor() as i32).max(0);
    let clip_y1 = (clip_box.y.floor() as i32).max(0);
    let clip_x2 = (clip_box.right().ceil() as i32).min(pix_w);
    let clip_y2 = (clip_box.bottom().ceil() as i32).min(pix_h);

    let x1 = x.max(clip_x1);
    let y1 = y.max(clip_y1);
    let x2 = (x + dst_w as i32).min(clip_x2);
    let y2 = (y + dst_h as i32).min(clip_y2);

    if x2 <= x1 || y2 <= y1 {
        return;
    }

    // Authentic iOS squircle mask parameters for dst_w x dst_h (corner radius ~0.225 size)
    let half_w = dst_w as f32 * 0.5;
    let half_h = dst_h as f32 * 0.5;
    let corner_r = 0.225 * (dst_w.min(dst_h) as f32);
    let inner_w = half_w - corner_r;
    let inner_h = half_h - corner_r;

    // Fixed-point 16.16 scale factors for fast coordinate mapping on ESP32-S3
    let scale_x = ((src_w as u64) << 16) / (dst_w as u64);
    let scale_y = ((src_h as u64) << 16) / (dst_h as u64);

    for py in y1..y2 {
        let dy = (py - y) as usize;
        let sy = (((dy as u64 * scale_y) >> 16) as usize).min((src_h - 1) as usize);
        let row = pixmap.row_mut(py as u32);
        let src_row_offset = sy * (src_w as usize);

        let py_dist = (dy as f32 + 0.5 - half_h).abs();
        let qy = (py_dist - inner_h).max(0.0);

        for px in x1..x2 {
            let dx = (px - x) as usize;
            let sx = (((dx as u64 * scale_x) >> 16) as usize).min((src_w - 1) as usize);
            let idx = src_row_offset + sx;

            let mut a = alpha_mask[idx];
            if a == 0 {
                continue;
            }

            // Compute exact squircle distance from corner center
            let px_dist = (dx as f32 + 0.5 - half_w).abs();
            let qx = (px_dist - inner_w).max(0.0);
            let dist_from_corner = (qx * qx + qy * qy).sqrt();

            if dist_from_corner > corner_r {
                continue;
            }
            if dist_from_corner > corner_r - 1.25 {
                let edge_factor = ((corner_r - dist_from_corner) / 1.25).clamp(0.0, 1.0);
                a = ((a as f32) * edge_factor) as u8;
                if a == 0 {
                    continue;
                }
            }

            let src = rgb_pixels[idx];

            // White fringe suppressor for hello and counter icons:
            // Filter near-white halo pixels (R>200, G>200, B>200) within 3.0px of the outer edge
            if dist_from_corner > corner_r - 3.0 {
                let r5 = (src >> 11) & 0x1F;
                let g6 = (src >> 5) & 0x3F;
                let b5 = src & 0x1F;
                if r5 >= 25 && g6 >= 50 && b5 >= 25 {
                    continue;
                }
            }

            let dst = &mut row[px as usize];
            if a == 255 {
                *dst = src;
            } else {
                *dst = blend_rgb565(*dst, src, a);
            }
        }
    }
}

/// Stroke a list of points (polyline) with thickness, caps and optional shader.
pub fn stroke_polyline(
    pixmap: &mut Pixmap565Mut<'_>,
    clip: Option<Rect>,
    points: &[Point],
    paint: &Paint,
    stroke: &Stroke,
) {
    if points.len() < 2 || stroke.width <= 0.0 {
        return;
    }

    let half_w = stroke.width * 0.5;
    let is_round_cap = stroke.line_cap == LineCap::Round;

    for i in 0..points.len() - 1 {
        let p0 = points[i];
        let p1 = points[i + 1];

        let dx = p1.x - p0.x;
        let dy = p1.y - p0.y;
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1e-4 {
            continue;
        }

        let nx = (-dy / len) * half_w;
        let ny = (dx / len) * half_w;

        // Draw quad for segment
        let q0 = Point::new(p0.x + nx, p0.y + ny);
        let q1 = Point::new(p0.x - nx, p0.y - ny);
        let q2 = Point::new(p1.x - nx, p1.y - ny);
        let q3 = Point::new(p1.x + nx, p1.y + ny);

        fill_convex_quad(pixmap, clip, q0, q1, q2, q3, paint);

        // Cap joints or line caps
        if is_round_cap {
            fill_circle_paint(pixmap, clip, p0, half_w, paint);
            if i == points.len() - 2 {
                fill_circle_paint(pixmap, clip, p1, half_w, paint);
            }
        }
    }
}

fn fill_convex_quad(
    pixmap: &mut Pixmap565Mut<'_>,
    clip: Option<Rect>,
    q0: Point,
    q1: Point,
    q2: Point,
    q3: Point,
    paint: &Paint,
) {
    let min_y = q0.y.min(q1.y).min(q2.y).min(q3.y);
    let max_y = q0.y.max(q1.y).max(q2.y).max(q3.y);
    let min_x = q0.x.min(q1.x).min(q2.x).min(q3.x);
    let max_x = q0.x.max(q1.x).max(q2.x).max(q3.x);

    let quad_rect = Rect::from_ltrb(min_x, min_y, max_x, max_y);
    let bounds = match clip {
        Some(c) => match quad_rect.intersect(&c) {
            Some(i) => i,
            None => return,
        },
        None => quad_rect,
    };

    let pix_w = pixmap.width as i32;
    let pix_h = pixmap.height as i32;

    let y_start = (bounds.y.floor() as i32).clamp(0, pix_h);
    let y_end = (bounds.bottom().ceil() as i32).clamp(y_start, pix_h);

    let edges = [(q0, q1), (q1, q2), (q2, q3), (q3, q0)];

    for y in y_start..y_end {
        let y_f = y as f32 + 0.5;
        let mut x_inters = [0.0f32; 4];
        let mut count = 0;

        for (a, b) in edges {
            if (a.y <= y_f && b.y > y_f) || (b.y <= y_f && a.y > y_f) {
                let dy = b.y - a.y;
                if dy.abs() > 1e-5 {
                    let t = (y_f - a.y) / dy;
                    if count < 4 {
                        x_inters[count] = a.x + t * (b.x - a.x);
                        count += 1;
                    }
                }
            }
        }

        if count >= 2 {
            let mut x_min = x_inters[0].min(x_inters[1]);
            let mut x_max = x_inters[0].max(x_inters[1]);
            for i in 2..count {
                x_min = x_min.min(x_inters[i]);
                x_max = x_max.max(x_inters[i]);
            }

            let x1 = (x_min.max(bounds.x).round() as i32).clamp(0, pix_w);
            let x2 = (x_max.min(bounds.right()).round() as i32).clamp(x1, pix_w);

            if x2 > x1 {
                let row = pixmap.row_mut(y as u32);
                match &paint.shader {
                    Shader::Linear(grad) => {
                        for px in x1..x2 {
                            let col = grad.color_at(px as f32, y_f);
                            let col565 = col.to_rgb565();
                            if col.a == 255 {
                                row[px as usize] = col565;
                            } else if col.a > 0 {
                                row[px as usize] = blend_rgb565(row[px as usize], col565, col.a);
                            }
                        }
                    }
                    Shader::SolidColor(color) => {
                        let col565 = color.to_rgb565();
                        let a = color.a;
                        let slice = &mut row[x1 as usize..x2 as usize];
                        if a == 255 {
                            slice.fill(col565);
                        } else if a > 0 {
                            for px in slice.iter_mut() {
                                *px = blend_rgb565(*px, col565, a);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

fn fill_circle_paint(
    pixmap: &mut Pixmap565Mut<'_>,
    clip: Option<Rect>,
    center: Point,
    radius: f32,
    paint: &Paint,
) {
    match &paint.shader {
        Shader::Linear(grad) => {
            let col = grad.color_at(center.x, center.y);
            fill_circle(pixmap, clip, center, radius, col);
        }
        Shader::SolidColor(color) => {
            fill_circle(pixmap, clip, center, radius, *color);
        }
        _ => {}
    }
}

/// Bayer 8x8 matrix dithering gradient to eliminate color banding on RGB565.
pub fn fill_dithered_horizontal_gradient(
    pixmap: &mut Pixmap565Mut<'_>,
    clip: Option<Rect>,
    c0: (u8, u8, u8),
    c1: (u8, u8, u8),
) {
    /// The classic Bayer 8x8 ordered-dither matrix: the raw thresholds, each of
    /// 0..=63 appearing exactly once. Kept as integers so the pattern is readable
    /// and so the compiler does not constant-fold a term like `32.0 / 64.0 - 0.5`
    /// into `0.5 - 0.5` and trip `clippy::eq_op`.
    static BAYER8: [u8; 64] = [
        0, 32, 8, 40, 2, 34, 10, 42, 48, 16, 56, 24, 50, 18, 58, 26, 12, 44, 4, 36, 14, 46, 6, 38,
        60, 28, 52, 20, 62, 30, 54, 22, 3, 35, 11, 43, 1, 33, 9, 41, 51, 19, 59, 27, 49, 17, 57,
        25, 15, 47, 7, 39, 13, 45, 5, 37, 63, 31, 55, 23, 61, 29, 53, 21,
    ];

    let pix_w = pixmap.width as usize;
    let pix_h = pixmap.height as usize;

    let (x1, y1, x2, y2) = match clip {
        Some(c) => (
            (c.x.max(0.0).round() as usize).min(pix_w),
            (c.y.max(0.0).round() as usize).min(pix_h),
            (c.right().min(pix_w as f32).round() as usize).min(pix_w),
            (c.bottom().min(pix_h as f32).round() as usize).min(pix_h),
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
        let by = (y & 7) << 3;
        let row = pixmap.row_mut(y as u32);
        for x in x1..x2 {
            let bx = x & 7;
            // The threshold centred on zero, scaled to a few steps of 8-bit colour,
            // so the dither breaks up the banding a 565 gradient would show.
            let d = (BAYER8[by + bx] as f32 / 64.0 - 0.5) * 6.0;
            let t = x as f32 * inv_w;
            let r = (r0 + dr * t + d + 0.5).clamp(0.0, 255.0) as u8;
            let g = (g0 + dg * t + d + 0.5).clamp(0.0, 255.0) as u8;
            let b = (b0 + db * t + d + 0.5).clamp(0.0, 255.0) as u8;
            row[x] = rgb888_to_rgb565(r, g, b);
        }
    }
}
