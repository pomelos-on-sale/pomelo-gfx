use crate::color::blend_rgb565;
use crate::geometry::{Point, Rect};
use crate::paint::{LineCap, Paint, Shader, Stroke};
use crate::pixmap::Pixmap565Mut;
use super::circle::fill_circle_paint;

/// Stroke a list of points (polyline) with thickness, caps and optional shader.
pub fn stroke_polyline(
    pixmap: &mut Pixmap565Mut<'_>,
    clip: Option<Rect>,
    points: &[Point],
    paint: &Paint,
    stroke: &Stroke,
) {
    if points.is_empty() || stroke.width <= 0.0 {
        return;
    }

    if points.len() == 1 {
        if stroke.line_cap == LineCap::Round {
            fill_circle_paint(pixmap, clip, points[0], stroke.width * 0.5, paint);
        }
        return;
    }

    let radius = stroke.width * 0.5;
    let r_outer = radius + 0.5;
    let r_solid = (radius - 0.5).max(0.0);
    let r_outer_sq = r_outer * r_outer;
    let r_solid_sq = r_solid * r_solid;

    #[derive(Clone, Copy)]
    struct Segment {
        p0: Point,
        dx: f32,
        dy: f32,
        inv_len_sq: f32,
        min_x: f32,
        max_x: f32,
        min_y: f32,
        max_y: f32,
    }

    let mut segments = Vec::with_capacity(points.len().saturating_sub(1));
    let mut total_min_x = f32::INFINITY;
    let mut total_max_x = f32::NEG_INFINITY;
    let mut total_min_y = f32::INFINITY;
    let mut total_max_y = f32::NEG_INFINITY;

    for i in 0..points.len() - 1 {
        let p0 = points[i];
        let p1 = points[i + 1];

        let dx = p1.x - p0.x;
        let dy = p1.y - p0.y;
        let len_sq = dx * dx + dy * dy;
        if len_sq < 1e-6 {
            continue;
        }

        let inv_len_sq = 1.0 / len_sq;
        let min_x = p0.x.min(p1.x) - r_outer;
        let max_x = p0.x.max(p1.x) + r_outer;
        let min_y = p0.y.min(p1.y) - r_outer;
        let max_y = p0.y.max(p1.y) + r_outer;

        if min_x < total_min_x {
            total_min_x = min_x;
        }
        if max_x > total_max_x {
            total_max_x = max_x;
        }
        if min_y < total_min_y {
            total_min_y = min_y;
        }
        if max_y > total_max_y {
            total_max_y = max_y;
        }

        segments.push(Segment {
            p0,
            dx,
            dy,
            inv_len_sq,
            min_x,
            max_x,
            min_y,
            max_y,
        });
    }

    if segments.is_empty() {
        if stroke.line_cap == LineCap::Round {
            fill_circle_paint(pixmap, clip, points[0], radius, paint);
        }
        return;
    }

    let total_rect = Rect::from_ltrb(total_min_x, total_min_y, total_max_x, total_max_y);
    let bounds = match clip {
        Some(c) => match total_rect.intersect(&c) {
            Some(i) => i,
            None => return,
        },
        None => total_rect,
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

    let is_round_cap = stroke.line_cap == LineCap::Round;
    let col565_opt = match &paint.shader {
        Shader::SolidColor(color) => Some((color.to_rgb565(), color.a)),
        _ => None,
    };

    for y in y_start..y_end {
        let y_f = y as f32 + 0.5;

        let mut active = [0usize; 64];
        let mut active_count = 0;
        for (idx, s) in segments.iter().enumerate() {
            if y_f >= s.min_y && y_f <= s.max_y {
                if active_count < active.len() {
                    active[active_count] = idx;
                    active_count += 1;
                }
            }
        }
        if active_count == 0 {
            continue;
        }

        let mut intervals = [(0i32, 0i32); 64];
        for i in 0..active_count {
            let s = &segments[active[i]];
            let x1 = (s.min_x.floor() as i32).clamp(clip_x1, clip_x2);
            let x2 = (s.max_x.ceil() as i32).clamp(x1, clip_x2);
            intervals[i] = (x1, x2);
        }

        let mut merged = [(0i32, 0i32); 64];
        let mut merged_count = 0;

        if active_count == 1 {
            if intervals[0].1 > intervals[0].0 {
                merged[0] = intervals[0];
                merged_count = 1;
            }
        } else {
            for i in 1..active_count {
                let key = intervals[i];
                let mut j = i;
                while j > 0 && intervals[j - 1].0 > key.0 {
                    intervals[j] = intervals[j - 1];
                    j -= 1;
                }
                intervals[j] = key;
            }

            let mut cur = intervals[0];
            for i in 1..active_count {
                let next = intervals[i];
                if next.0 <= cur.1 {
                    if next.1 > cur.1 {
                        cur.1 = next.1;
                    }
                } else {
                    if cur.1 > cur.0 {
                        merged[merged_count] = cur;
                        merged_count += 1;
                    }
                    cur = next;
                }
            }
            if cur.1 > cur.0 {
                merged[merged_count] = cur;
                merged_count += 1;
            }
        }

        if merged_count == 0 {
            continue;
        }

        let row = pixmap.row_mut(y as u32);

        for m in 0..merged_count {
            let (x_start, x_end) = merged[m];
            for x in x_start..x_end {
                let px = x as f32 + 0.5;

                let mut min_d_sq = f32::INFINITY;
                for i in 0..active_count {
                    let seg_idx = active[i];
                    let s = &segments[seg_idx];
                    if px < s.min_x || px > s.max_x {
                        continue;
                    }

                    let u_x = px - s.p0.x;
                    let u_y = y_f - s.p0.y;
                    let t = (u_x * s.dx + u_y * s.dy) * s.inv_len_sq;

                    if !is_round_cap && (t < 0.0 || t > 1.0) {
                        continue;
                    }

                    let t_c = t.clamp(0.0, 1.0);
                    let cx = s.p0.x + t_c * s.dx;
                    let cy = s.p0.y + t_c * s.dy;
                    let d_sq = (px - cx) * (px - cx) + (y_f - cy) * (y_f - cy);
                    if d_sq < min_d_sq {
                        min_d_sq = d_sq;
                        if min_d_sq <= r_solid_sq {
                            break;
                        }
                    }
                }

                if min_d_sq >= r_outer_sq {
                    continue;
                }

                let cov = if min_d_sq <= r_solid_sq {
                    1.0
                } else {
                    r_outer - min_d_sq.sqrt()
                };

                if cov > 0.0 {
                    if let Some((col565, a)) = col565_opt {
                        let eff_a = (cov * a as f32).round() as u8;
                        if eff_a == 255 {
                            row[x as usize] = col565;
                        } else if eff_a > 0 {
                            row[x as usize] = blend_rgb565(row[x as usize], col565, eff_a);
                        }
                    } else if let Shader::Linear(grad) = &paint.shader {
                        let (col565, a) = grad.dithered_at(px, y_f, x, y);
                        let eff_a = (cov * a as f32).round() as u8;
                        if eff_a == 255 {
                            row[x as usize] = col565;
                        } else if eff_a > 0 {
                            row[x as usize] = blend_rgb565(row[x as usize], col565, eff_a);
                        }
                    }
                }
            }
        }
    }
}
