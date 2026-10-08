use crate::color::{blend_rgb565, Color};
use crate::geometry::{Point, Rect};
use crate::paint::{Paint, Shader};
use crate::pixmap::Pixmap565Mut;
use super::rect::fill_u16_slice;

/// Signed distance field subpixel coverage for a circle.
#[inline(always)]
pub fn circle_coverage(px: f32, py: f32, center: Point, radius: f32) -> f32 {
    let dx = px - center.x;
    let dy = py - center.y;
    let dist = (dx * dx + dy * dy).sqrt();
    let sd = dist - radius;
    (0.5 - sd).clamp(0.0, 1.0)
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

    let clip_x1 = (bounds.x.floor() as i32).clamp(0, pix_w);
    let clip_x2 = (bounds.right().ceil() as i32).clamp(clip_x1, pix_w);
    let y_start = (bounds.y.floor() as i32).clamp(0, pix_h);
    let y_end = (bounds.bottom().ceil() as i32).clamp(y_start, pix_h);

    if clip_x2 <= clip_x1 || y_end <= y_start {
        return;
    }

    let r_outer = radius + 0.5;
    let r_solid = (radius - 0.5).max(0.0);
    let r_outer_sq = r_outer * r_outer;
    let r_solid_sq = r_solid * r_solid;

    for y in y_start..y_end {
        let y_f = y as f32 + 0.5;
        let dy = (y_f - center.y).abs();
        if dy >= r_outer {
            continue;
        }

        let dx_outer = (r_outer_sq - dy * dy).max(0.0).sqrt();
        let x_min = center.x - dx_outer;
        let x_max = center.x + dx_outer;

        let (x_solid_min, x_solid_max) = if dy < r_solid {
            let dx_solid = (r_solid_sq - dy * dy).max(0.0).sqrt();
            (center.x - dx_solid, center.x + dx_solid)
        } else {
            (center.x + 1.0, center.x - 1.0)
        };

        let x_start = (x_min.floor() as i32).clamp(clip_x1, clip_x2);
        let x_end = (x_max.ceil() as i32).clamp(x_start, clip_x2);

        let solid_start = (x_solid_min.ceil() as i32).clamp(x_start, x_end);
        let solid_end = (x_solid_max.floor() as i32).clamp(x_start, x_end);

        let row = pixmap.row_mut(y as u32);

        if solid_start < solid_end {
            for x in x_start..solid_start {
                let px = x as f32 + 0.5;
                let cov = circle_coverage(px, y_f, center, radius);
                if cov > 0.0 {
                    let eff_a = (cov * a as f32).round() as u8;
                    if eff_a > 0 {
                        row[x as usize] = blend_rgb565(row[x as usize], col565, eff_a);
                    }
                }
            }

            let slice = &mut row[solid_start as usize..solid_end as usize];
            if is_opaque {
                fill_u16_slice(slice, col565);
            } else {
                for px in slice.iter_mut() {
                    *px = blend_rgb565(*px, col565, a);
                }
            }

            for x in solid_end..x_end {
                let px = x as f32 + 0.5;
                let cov = circle_coverage(px, y_f, center, radius);
                if cov > 0.0 {
                    let eff_a = (cov * a as f32).round() as u8;
                    if eff_a > 0 {
                        row[x as usize] = blend_rgb565(row[x as usize], col565, eff_a);
                    }
                }
            }
        } else {
            for x in x_start..x_end {
                let px = x as f32 + 0.5;
                let cov = circle_coverage(px, y_f, center, radius);
                if cov > 0.0 {
                    let eff_a = (cov * a as f32).round() as u8;
                    if eff_a > 0 {
                        row[x as usize] = blend_rgb565(row[x as usize], col565, eff_a);
                    }
                }
            }
        }
    }
}


pub(crate) fn fill_circle_paint(
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

