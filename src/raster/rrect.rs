use crate::color::{blend_rgb565, Color};
use crate::geometry::{RRect, Rect};
use crate::pixmap::Pixmap565Mut;
use super::rect::{fill_rect, fill_u16_slice};

/// Signed distance field (SDF) based subpixel coverage calculation for a rounded rectangle.
/// Returns coverage in [0.0, 1.0] for pixel center (px, py).
#[inline(always)]
pub fn rrect_coverage(px: f32, py: f32, rect: &Rect, rx: f32, ry: f32) -> f32 {
    let cx_left = rect.x + rx;
    let cx_right = rect.right() - rx;
    let cy_top = rect.y + ry;
    let cy_bottom = rect.bottom() - ry;

    let in_x_left = px < cx_left;
    let in_x_right = px > cx_right;
    let in_y_top = py < cy_top;
    let in_y_bottom = py > cy_bottom;

    let qx = if in_x_left {
        cx_left - px
    } else if in_x_right {
        px - cx_right
    } else {
        0.0
    };

    let qy = if in_y_top {
        cy_top - py
    } else if in_y_bottom {
        py - cy_bottom
    } else {
        0.0
    };

    let in_x_corner = in_x_left || in_x_right;
    let in_y_corner = in_y_top || in_y_bottom;

    let sd = if in_x_corner && in_y_corner {
        (qx * qx + qy * qy).sqrt() - rx
    } else if in_x_corner {
        qx - rx
    } else if in_y_corner {
        qy - ry
    } else {
        let dist_to_left = px - rect.x;
        let dist_to_right = rect.right() - px;
        let dist_to_top = py - rect.y;
        let dist_to_bottom = rect.bottom() - py;
        let min_d = dist_to_left.min(dist_to_right).min(dist_to_top).min(dist_to_bottom);
        -min_d
    };

    (0.5 - sd).clamp(0.0, 1.0)
}

/// Signed distance field subpixel coverage for a stroked rounded rectangle.
#[inline(always)]
pub fn stroke_rrect_coverage(
    px: f32,
    py: f32,
    rect: &Rect,
    outer_r: f32,
    inner_rect: &Rect,
    inner_r: f32,
) -> f32 {
    let cov_outer = rrect_coverage(px, py, rect, outer_r, outer_r);
    if cov_outer <= 0.0 {
        return 0.0;
    }
    let cov_inner = if px >= inner_rect.x - 0.5
        && px <= inner_rect.right() + 0.5
        && py >= inner_rect.y - 0.5
        && py <= inner_rect.bottom() + 0.5
    {
        rrect_coverage(px, py, inner_rect, inner_r, inner_r)
    } else {
        0.0
    };
    (cov_outer - cov_inner).clamp(0.0, 1.0)
}


/// Fill a rounded rectangle with scanline spans and subpixel anti-aliasing.
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
    let r_outer = rx + 0.5;
    let r_solid = (rx - 0.5).max(0.0);
    let r_outer_sq = r_outer * r_outer;
    let r_solid_sq = r_solid * r_solid;

    let cx_left = rect.x + rx;
    let cx_right = rect.right() - rx;
    let cy_top = rect.y + ry;
    let cy_bottom = rect.bottom() - ry;

    let clamp_left = bounds.x.max(0.0);
    let clamp_right = bounds.right().min(pix_w as f32);
    let clip_x1 = (clamp_left.floor() as i32).clamp(0, pix_w);
    let clip_x2 = (clamp_right.ceil() as i32).clamp(clip_x1, pix_w);

    for y in y_start..y_end {
        let y_f = y as f32 + 0.5;

        if y_f < rect.y || y_f > rect.bottom() {
            continue;
        }

        let (x_min, x_max) = if y_f < cy_top {
            let dy = cy_top - y_f;
            if dy < r_outer {
                let dx = (r_outer_sq - dy * dy).max(0.0).sqrt();
                (
                    (cx_left - dx).max(rect.x),
                    (cx_right + dx).min(rect.right()),
                )
            } else {
                (cx_left, cx_right)
            }
        } else if y_f > cy_bottom {
            let dy = y_f - cy_bottom;
            if dy < r_outer {
                let dx = (r_outer_sq - dy * dy).max(0.0).sqrt();
                (
                    (cx_left - dx).max(rect.x),
                    (cx_right + dx).min(rect.right()),
                )
            } else {
                (cx_left, cx_right)
            }
        } else {
            (rect.x, rect.right())
        };

        if x_max <= x_min {
            continue;
        }

        let (x_solid_min, x_solid_max) = if y_f < rect.y + 0.5 || y_f > rect.bottom() - 0.5 {
            (rect.right(), rect.x)
        } else if y_f < cy_top {
            let dy = cy_top - y_f;
            if dy < r_solid {
                let dx = (r_solid_sq - dy * dy).max(0.0).sqrt();
                (cx_left - dx, cx_right + dx)
            } else {
                (cx_left, cx_right)
            }
        } else if y_f > cy_bottom {
            let dy = y_f - cy_bottom;
            if dy < r_solid {
                let dx = (r_solid_sq - dy * dy).max(0.0).sqrt();
                (cx_left - dx, cx_right + dx)
            } else {
                (cx_left, cx_right)
            }
        } else {
            (rect.x + 0.5, rect.right() - 0.5)
        };

        let x_start = (x_min.floor() as i32).clamp(clip_x1, clip_x2);
        let x_end = (x_max.ceil() as i32).clamp(x_start, clip_x2);

        let solid_start = (x_solid_min.ceil() as i32).clamp(x_start, x_end);
        let solid_end = (x_solid_max.floor() as i32).clamp(x_start, x_end);

        let row = pixmap.row_mut(y as u32);

        if solid_start < solid_end {
            // 1. Left subpixel anti-aliased edge
            for x in x_start..solid_start {
                let px = x as f32 + 0.5;
                let cov = rrect_coverage(px, y_f, &rect, rx, ry);
                if cov > 0.0 {
                    if cov >= 1.0 && is_opaque {
                        row[x as usize] = col565;
                    } else {
                        let eff_a = (cov * a as f32).round() as u8;
                        if eff_a > 0 {
                            row[x as usize] = blend_rgb565(row[x as usize], col565, eff_a);
                        }
                    }
                }
            }

            // 2. High-speed solid middle span
            let slice = &mut row[solid_start as usize..solid_end as usize];
            if is_opaque {
                fill_u16_slice(slice, col565);
            } else {
                for px in slice.iter_mut() {
                    *px = blend_rgb565(*px, col565, a);
                }
            }

            // 3. Right subpixel anti-aliased edge
            for x in solid_end..x_end {
                let px = x as f32 + 0.5;
                let cov = rrect_coverage(px, y_f, &rect, rx, ry);
                if cov > 0.0 {
                    if cov >= 1.0 && is_opaque {
                        row[x as usize] = col565;
                    } else {
                        let eff_a = (cov * a as f32).round() as u8;
                        if eff_a > 0 {
                            row[x as usize] = blend_rgb565(row[x as usize], col565, eff_a);
                        }
                    }
                }
            }
        } else {
            for x in x_start..x_end {
                let px = x as f32 + 0.5;
                let cov = rrect_coverage(px, y_f, &rect, rx, ry);
                if cov > 0.0 {
                    if cov >= 1.0 && is_opaque {
                        row[x as usize] = col565;
                    } else {
                        let eff_a = (cov * a as f32).round() as u8;
                        if eff_a > 0 {
                            row[x as usize] = blend_rgb565(row[x as usize], col565, eff_a);
                        }
                    }
                }
            }
        }
    }
}

/// Draw a stroked rounded rectangle border with subpixel anti-aliasing.
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
    let r_outer = outer_r + 0.5;
    let r_outer_sq = r_outer * r_outer;
    let inner_sq = inner_radius * inner_radius;

    let clamp_left = bounds.x.max(0.0);
    let clamp_right = bounds.right().min(pix_w as f32);
    let clip_x1 = (clamp_left.floor() as i32).clamp(0, pix_w);
    let clip_x2 = (clamp_right.ceil() as i32).clamp(clip_x1, pix_w);

    for y in y_start..y_end {
        let y_f = y as f32 + 0.5;

        if y_f < rect.y || y_f > rect.bottom() {
            continue;
        }

        // Outer span
        let mut out_x1 = rect.x;
        let mut out_x2 = rect.right();

        if y_f < rect.y + outer_r {
            let dy = rect.y + outer_r - y_f;
            if dy < r_outer {
                let dx = (r_outer_sq - dy * dy).max(0.0).sqrt();
                out_x1 = (rect.x + outer_r - dx).max(rect.x);
                out_x2 = (rect.right() - outer_r + dx).min(rect.right());
            } else {
                out_x1 = rect.x + outer_r;
                out_x2 = rect.right() - outer_r;
            }
        } else if y_f > rect.bottom() - outer_r {
            let dy = y_f - (rect.bottom() - outer_r);
            if dy < r_outer {
                let dx = (r_outer_sq - dy * dy).max(0.0).sqrt();
                out_x1 = (rect.x + outer_r - dx).max(rect.x);
                out_x2 = (rect.right() - outer_r + dx).min(rect.right());
            } else {
                out_x1 = rect.x + outer_r;
                out_x2 = rect.right() - outer_r;
            }
        }

        // Inner span (if inside inner rect bounds)
        let in_span = if y_f >= inner_rect.y && y_f <= inner_rect.bottom() {
            let mut in_x1 = inner_rect.x;
            let mut in_x2 = inner_rect.right();

            if inner_radius > 0.0 {
                if y_f < inner_rect.y + inner_radius {
                    let dy = inner_rect.y + inner_radius - y_f;
                    if dy < inner_radius {
                        let dx = (inner_sq - dy * dy).max(0.0).sqrt();
                        in_x1 = inner_rect.x + inner_radius - dx;
                        in_x2 = inner_rect.right() - inner_radius + dx;
                    } else {
                        in_x1 = inner_rect.x + inner_radius;
                        in_x2 = inner_rect.right() - inner_radius;
                    }
                } else if y_f > inner_rect.bottom() - inner_radius {
                    let dy = y_f - (inner_rect.bottom() - inner_radius);
                    if dy < inner_radius {
                        let dx = (inner_sq - dy * dy).max(0.0).sqrt();
                        in_x1 = inner_rect.x + inner_radius - dx;
                        in_x2 = inner_rect.right() - inner_radius + dx;
                    } else {
                        in_x1 = inner_rect.x + inner_radius;
                        in_x2 = inner_rect.right() - inner_radius;
                    }
                }
            }
            if in_x2 > in_x1 {
                Some((in_x1, in_x2))
            } else {
                None
            }
        } else {
            None
        };

        let row = pixmap.row_mut(y as u32);

        match in_span {
            Some((in_x1, in_x2)) => {
                // Left stroke segment [out_x1 .. in_x1]
                let lx1 = (out_x1.floor() as i32).clamp(clip_x1, clip_x2);
                let lx2 = (in_x1.ceil() as i32).clamp(lx1, clip_x2);
                for x in lx1..lx2 {
                    let px = x as f32 + 0.5;
                    let cov = stroke_rrect_coverage(px, y_f, &rect, outer_r, &inner_rect, inner_radius);
                    if cov > 0.0 {
                        if cov >= 1.0 && is_opaque {
                            row[x as usize] = col565;
                        } else {
                            let eff_a = (cov * a as f32).round() as u8;
                            if eff_a > 0 {
                                row[x as usize] = blend_rgb565(row[x as usize], col565, eff_a);
                            }
                        }
                    }
                }

                // Right stroke segment [in_x2 .. out_x2]
                let rx1 = (in_x2.floor() as i32).clamp(clip_x1, clip_x2);
                let rx2 = (out_x2.ceil() as i32).clamp(rx1, clip_x2);
                for x in rx1..rx2 {
                    let px = x as f32 + 0.5;
                    let cov = stroke_rrect_coverage(px, y_f, &rect, outer_r, &inner_rect, inner_radius);
                    if cov > 0.0 {
                        if cov >= 1.0 && is_opaque {
                            row[x as usize] = col565;
                        } else {
                            let eff_a = (cov * a as f32).round() as u8;
                            if eff_a > 0 {
                                row[x as usize] = blend_rgb565(row[x as usize], col565, eff_a);
                            }
                        }
                    }
                }
            }
            None => {
                // Entire row is border band (top or bottom cap)
                let x_start = (out_x1.floor() as i32).clamp(clip_x1, clip_x2);
                let x_end = (out_x2.ceil() as i32).clamp(x_start, clip_x2);

                let mid_left = ((rect.x + outer_r).ceil() as i32).clamp(x_start, x_end);
                let mid_right = ((rect.right() - outer_r).floor() as i32).clamp(mid_left, x_end);

                // Left corner curve
                for x in x_start..mid_left {
                    let px = x as f32 + 0.5;
                    let cov = stroke_rrect_coverage(px, y_f, &rect, outer_r, &inner_rect, inner_radius);
                    if cov > 0.0 {
                        if cov >= 1.0 && is_opaque {
                            row[x as usize] = col565;
                        } else {
                            let eff_a = (cov * a as f32).round() as u8;
                            if eff_a > 0 {
                                row[x as usize] = blend_rgb565(row[x as usize], col565, eff_a);
                            }
                        }
                    }
                }

                // Middle flat horizontal segment: coverage is constant along x
                if mid_right > mid_left {
                    let sample_px = (mid_left as f32 + mid_right as f32) * 0.5;
                    let cov = stroke_rrect_coverage(sample_px, y_f, &rect, outer_r, &inner_rect, inner_radius);
                    if cov >= 1.0 && is_opaque {
                        row[mid_left as usize..mid_right as usize].fill(col565);
                    } else if cov > 0.0 {
                        let eff_a = (cov * a as f32).round() as u8;
                        if eff_a > 0 {
                            for x in mid_left..mid_right {
                                row[x as usize] = blend_rgb565(row[x as usize], col565, eff_a);
                            }
                        }
                    }
                }

                // Right corner curve
                for x in mid_right..x_end {
                    let px = x as f32 + 0.5;
                    let cov = stroke_rrect_coverage(px, y_f, &rect, outer_r, &inner_rect, inner_radius);
                    if cov > 0.0 {
                        if cov >= 1.0 && is_opaque {
                            row[x as usize] = col565;
                        } else {
                            let eff_a = (cov * a as f32).round() as u8;
                            if eff_a > 0 {
                                row[x as usize] = blend_rgb565(row[x as usize], col565, eff_a);
                            }
                        }
                    }
                }
            }
        }
    }
}

