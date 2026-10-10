use crate::color::blend_rgb565;
use crate::geometry::{Point, Rect};
use crate::paint::{Paint, Shader};
use crate::pixmap::Pixmap565Mut;

/// Fills a convex quadrilateral, sampling `paint`'s shader per pixel.
///
/// This is the shader-aware filler: a solid colour comes out solid, and a gradient comes out as
/// the gradient. The corners may be given in either winding. It is `pub` because `Canvas` needs
/// it for a rectangle whose paint is not a colour -- `raster::fill_rect` takes a `Color` and has
/// the cheaper axis-aligned path.
pub fn fill_convex_quad(
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

    let clip_x1 = (bounds.x.floor() as i32).clamp(0, pix_w);
    let clip_x2 = (bounds.right().ceil() as i32).clamp(clip_x1, pix_w);
    let y_start = (bounds.y.floor() as i32).clamp(0, pix_h);
    let y_end = (bounds.bottom().ceil() as i32).clamp(y_start, pix_h);

    if clip_x2 <= clip_x1 || y_end <= y_start {
        return;
    }

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

            if x_max <= x_min {
                continue;
            }

            let left_pixel = x_min.floor() as i32;
            let solid_start = x_min.ceil() as i32;
            let solid_end = x_max.floor() as i32;
            let right_pixel = x_max.floor() as i32;

            let row = pixmap.row_mut(y as u32);

            match &paint.shader {
                Shader::Linear(grad) => {
                    if !paint.anti_alias {
                        let s_start = (x_min.round() as i32).clamp(clip_x1, clip_x2) as usize;
                        let s_end = (x_max.round() as i32).clamp(clip_x1, clip_x2) as usize;
                        for px in s_start..s_end {
                            let (col565, a) = grad.dithered_at(px as f32 + 0.5, y_f, px as i32, y);
                            if a == 255 {
                                row[px] = col565;
                            } else if a > 0 {
                                row[px] = blend_rgb565(row[px], col565, a);
                            }
                        }
                    } else if solid_start < solid_end {
                        // Left subpixel anti-aliased edge
                        if left_pixel >= clip_x1 && left_pixel < clip_x2 && left_pixel < solid_start {
                            let cov = 1.0 - (x_min - left_pixel as f32);
                            let (col565, a) = grad.dithered_at(left_pixel as f32 + 0.5, y_f, left_pixel, y);
                            let eff_a = (cov * a as f32).round() as u8;
                            if eff_a > 0 {
                                row[left_pixel as usize] = blend_rgb565(row[left_pixel as usize], col565, eff_a);
                            }
                        }

                        // Solid middle
                        let s_start = solid_start.clamp(clip_x1, clip_x2) as usize;
                        let s_end = solid_end.clamp(clip_x1, clip_x2) as usize;
                        for px in s_start..s_end {
                            let (col565, a) = grad.dithered_at(px as f32 + 0.5, y_f, px as i32, y);
                            if a == 255 {
                                row[px] = col565;
                            } else if a > 0 {
                                row[px] = blend_rgb565(row[px], col565, a);
                            }
                        }

                        // Right subpixel anti-aliased edge
                        if right_pixel >= clip_x1 && right_pixel < clip_x2 && right_pixel >= solid_end {
                            let cov = x_max - right_pixel as f32;
                            let (col565, a) = grad.dithered_at(right_pixel as f32 + 0.5, y_f, right_pixel, y);
                            let eff_a = (cov * a as f32).round() as u8;
                            if eff_a > 0 {
                                row[right_pixel as usize] = blend_rgb565(row[right_pixel as usize], col565, eff_a);
                            }
                        }
                    } else {
                        let start = left_pixel.clamp(clip_x1, clip_x2);
                        let end = (x_max.ceil() as i32).clamp(start, clip_x2);
                        for px in start..end {
                            let p_left = px as f32;
                            let p_right = p_left + 1.0;
                            let span_l = p_left.max(x_min);
                            let span_r = p_right.min(x_max);
                            let cov = (span_r - span_l).clamp(0.0, 1.0);
                            if cov > 0.0 {
                                let (col565, a) = grad.dithered_at(px as f32 + 0.5, y_f, px, y);
                                let eff_a = (cov * a as f32).round() as u8;
                                if eff_a > 0 {
                                    row[px as usize] = blend_rgb565(row[px as usize], col565, eff_a);
                                }
                            }
                        }
                    }
                }
                Shader::SolidColor(color) => {
                    let col565 = color.to_rgb565();
                    let a = color.a;

                    if !paint.anti_alias {
                        let s_start = (x_min.round() as i32).clamp(clip_x1, clip_x2) as usize;
                        let s_end = (x_max.round() as i32).clamp(clip_x1, clip_x2) as usize;
                        if s_end > s_start {
                            let slice = &mut row[s_start..s_end];
                            if a == 255 {
                                crate::arch::fill_span_rgb565(slice, col565);
                            } else {
                                crate::arch::blend_span_rgb565(slice, col565, a);
                            }
                        }
                    } else if solid_start < solid_end {
                        // Left subpixel anti-aliased edge
                        if left_pixel >= clip_x1 && left_pixel < clip_x2 && left_pixel < solid_start {
                            let cov = 1.0 - (x_min - left_pixel as f32);
                            let eff_a = (cov * a as f32).round() as u8;
                            if eff_a > 0 {
                                row[left_pixel as usize] = blend_rgb565(row[left_pixel as usize], col565, eff_a);
                            }
                        }

                        // Solid middle
                        let s_start = solid_start.clamp(clip_x1, clip_x2) as usize;
                        let s_end = solid_end.clamp(clip_x1, clip_x2) as usize;
                        if s_end > s_start {
                            let slice = &mut row[s_start..s_end];
                            if a == 255 {
                                crate::arch::fill_span_rgb565(slice, col565);
                            } else {
                                crate::arch::blend_span_rgb565(slice, col565, a);
                            }
                        }

                        // Right subpixel anti-aliased edge
                        if right_pixel >= clip_x1 && right_pixel < clip_x2 && right_pixel >= solid_end {
                            let cov = x_max - right_pixel as f32;
                            let eff_a = (cov * a as f32).round() as u8;
                            if eff_a > 0 {
                                row[right_pixel as usize] = blend_rgb565(row[right_pixel as usize], col565, eff_a);
                            }
                        }
                    } else {
                        let start = left_pixel.clamp(clip_x1, clip_x2);
                        let end = (x_max.ceil() as i32).clamp(start, clip_x2);
                        for px in start..end {
                            let p_left = px as f32;
                            let p_right = p_left + 1.0;
                            let span_l = p_left.max(x_min);
                            let span_r = p_right.min(x_max);
                            let cov = (span_r - span_l).clamp(0.0, 1.0);
                            if cov > 0.0 {
                                let eff_a = (cov * a as f32).round() as u8;
                                if eff_a > 0 {
                                    row[px as usize] = blend_rgb565(row[px as usize], col565, eff_a);
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

/// Fills a single triangle, sampling `paint`'s shader or color per pixel.
#[inline(always)]
pub fn fill_triangle(
    pixmap: &mut Pixmap565Mut<'_>,
    clip: Option<Rect>,
    p0: Point,
    p1: Point,
    p2: Point,
    paint: &Paint,
) {
    fill_convex_quad(pixmap, clip, p0, p1, p2, p2, paint);
}

