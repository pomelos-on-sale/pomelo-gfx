use crate::color::{blend_rgb565, Color};
use crate::geometry::Rect;
use crate::pixmap::Pixmap565Mut;

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

    let span_len = (x2 - x1) as usize;
    let gx_start = (x1 - x) as usize;

    for py in y1..y2 {
        let gy = (py - y) as usize;
        let row = pixmap.row_mut(py as u32);
        let dst_slice = &mut row[x1 as usize..x2 as usize];

        let mask_start = gy * (w as usize) + gx_start;
        let mask_slice = &mask[mask_start..mask_start + span_len];

        crate::arch::blit_mask_span(dst_slice, mask_slice, col565, color.a);
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

    // The squircle mask and its anti-aliasing are fully baked into alpha_mask at
    // build time (4× supersampled). This function only needs to alpha-blend pixels.
    for py in y1..y2 {
        let sy = (py - y) as usize;
        let row = pixmap.row_mut(py as u32);

        for px in x1..x2 {
            let sx = (px - x) as usize;
            let idx = sy * (w as usize) + sx;
            let a = alpha_mask[idx];
            if a == 0 {
                continue;
            }

            let src = rgb_pixels[idx];
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

    // The squircle mask and its anti-aliasing are fully baked into alpha_mask at
    // build time (4× supersampled). This function only needs to scale + alpha-blend.
    //
    // Fixed-point 16.16 scale factors for fast coordinate mapping on ESP32-S3.
    let scale_x = ((src_w as u64) << 16) / (dst_w as u64);
    let scale_y = ((src_h as u64) << 16) / (dst_h as u64);

    for py in y1..y2 {
        let dy = (py - y) as usize;
        let sy = (((dy as u64 * scale_y) >> 16) as usize).min((src_h - 1) as usize);
        let row = pixmap.row_mut(py as u32);
        let src_row_offset = sy * (src_w as usize);

        for px in x1..x2 {
            let dx = (px - x) as usize;
            let sx = (((dx as u64 * scale_x) >> 16) as usize).min((src_w - 1) as usize);
            let idx = src_row_offset + sx;

            let a = alpha_mask[idx];
            if a == 0 {
                continue;
            }

            let src = rgb_pixels[idx];
            let dst = &mut row[px as usize];
            if a == 255 {
                *dst = src;
            } else {
                *dst = blend_rgb565(*dst, src, a);
            }
        }
    }
}

/// Direct blit of a QOI image (RGB or RGBA) straight into the RGB565 pixmap.
/// Decodes on-the-fly and writes pixels with clipping and alpha blending.
pub fn blit_qoi(
    pixmap: &mut Pixmap565Mut<'_>,
    clip: Option<Rect>,
    x: i32,
    y: i32,
    qoi_bytes: &[u8],
) -> Result<(u32, u32), qoi::Error> {
    let (header, decoded) = qoi::decode_to_vec(qoi_bytes)?;
    let w = header.width;
    let h = header.height;
    if w == 0 || h == 0 {
        return Ok((w, h));
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
        return Ok((w, h));
    }

    let channels = header.channels.as_u8() as usize;
    match header.channels {
        qoi::Channels::Rgb => {
            for py in y1..y2 {
                let sy = (py - y) as usize;
                let row = pixmap.row_mut(py as u32);
                for px in x1..x2 {
                    let sx = (px - x) as usize;
                    let idx = (sy * w as usize + sx) * channels;
                    let r = decoded[idx];
                    let g = decoded[idx + 1];
                    let b = decoded[idx + 2];
                    row[px as usize] = crate::color::rgb888_to_rgb565(r, g, b);
                }
            }
        }
        qoi::Channels::Rgba => {
            for py in y1..y2 {
                let sy = (py - y) as usize;
                let row = pixmap.row_mut(py as u32);
                for px in x1..x2 {
                    let sx = (px - x) as usize;
                    let idx = (sy * w as usize + sx) * channels;
                    let a = decoded[idx + 3];
                    if a == 0 {
                        continue;
                    }
                    let r = decoded[idx];
                    let g = decoded[idx + 1];
                    let b = decoded[idx + 2];
                    let col565 = crate::color::rgb888_to_rgb565(r, g, b);
                    if a == 255 {
                        row[px as usize] = col565;
                    } else {
                        let dst = &mut row[px as usize];
                        *dst = blend_rgb565(*dst, col565, a);
                    }
                }
            }
        }
    }

    Ok((w, h))
}

/// Decodes QOI bytes into an RGB565 pixel buffer and an optional 8-bit alpha mask (if RGBA with alpha).
pub fn decode_qoi_to_rgb565(
    qoi_bytes: &[u8],
) -> Result<(u32, u32, Vec<u16>, Option<Vec<u8>>), qoi::Error> {
    let (header, decoded) = qoi::decode_to_vec(qoi_bytes)?;
    let w = header.width;
    let h = header.height;
    let count = (w * h) as usize;
    let mut rgb565 = Vec::with_capacity(count);
    let channels = header.channels.as_u8() as usize;

    match header.channels {
        qoi::Channels::Rgb => {
            for i in 0..count {
                let idx = i * channels;
                rgb565.push(crate::color::rgb888_to_rgb565(
                    decoded[idx],
                    decoded[idx + 1],
                    decoded[idx + 2],
                ));
            }
            Ok((w, h, rgb565, None))
        }
        qoi::Channels::Rgba => {
            let mut alpha = Vec::with_capacity(count);
            let mut has_non_opaque = false;
            for i in 0..count {
                let idx = i * channels;
                let a = decoded[idx + 3];
                if a < 255 {
                    has_non_opaque = true;
                }
                alpha.push(a);
                rgb565.push(crate::color::rgb888_to_rgb565(
                    decoded[idx],
                    decoded[idx + 1],
                    decoded[idx + 2],
                ));
            }
            Ok((w, h, rgb565, if has_non_opaque { Some(alpha) } else { None }))
        }
    }
}

