//! Cross-platform fallback implementation of batch span operations using
//! fixed-point arithmetic, loop unrolling, and precomputed color components
//! (inspired by DSP fixed-point vector math).

#[inline(always)]
pub fn blend_span_rgb565(dst: &mut [u16], src: u16, alpha: u8) {
    if alpha == 0 || dst.is_empty() {
        return;
    }
    if alpha == 255 {
        fill_span_rgb565(dst, src);
        return;
    }

    let a = alpha as u32;
    let inv_a = 255 - a;
    let s = src as u32;
    let r_s = ((s >> 11) & 0x1F) * a;
    let g_s = ((s >> 5) & 0x3F) * a;
    let b_s = (s & 0x1F) * a;

    let len = dst.len();
    let mut i = 0;

    // 4-pixel unrolled batch loop
    while i + 4 <= len {
        let chunk = &mut dst[i..i + 4];
        for px in chunk.iter_mut() {
            let d = *px as u32;
            let r_d = (d >> 11) & 0x1F;
            let g_d = (d >> 5) & 0x3F;
            let b_d = d & 0x1F;

            let r = crate::color::div255_round(r_s + r_d * inv_a) as u16;
            let g = crate::color::div255_round(g_s + g_d * inv_a) as u16;
            let b = crate::color::div255_round(b_s + b_d * inv_a) as u16;

            *px = (r << 11) | (g << 5) | b;
        }
        i += 4;
    }

    while i < len {
        let d = dst[i] as u32;
        let r_d = (d >> 11) & 0x1F;
        let g_d = (d >> 5) & 0x3F;
        let b_d = d & 0x1F;

        let r = crate::color::div255_round(r_s + r_d * inv_a) as u16;
        let g = crate::color::div255_round(g_s + g_d * inv_a) as u16;
        let b = crate::color::div255_round(b_s + b_d * inv_a) as u16;

        dst[i] = (r << 11) | (g << 5) | b;
        i += 1;
    }
}

#[inline(always)]
pub fn blit_mask_span(dst: &mut [u16], mask: &[u8], src: u16, base_alpha: u8) {
    let len = dst.len().min(mask.len());
    if len == 0 || base_alpha == 0 {
        return;
    }

    let s = src as u32;
    let s_r = (s >> 11) & 0x1F;
    let s_g = (s >> 5) & 0x3F;
    let s_b = s & 0x1F;
    let alpha_base = base_alpha as u32;

    let mut i = 0;
    while i + 4 <= len {
        let m0 = mask[i];
        let m1 = mask[i + 1];
        let m2 = mask[i + 2];
        let m3 = mask[i + 3];

        // Fast skip 4 consecutive empty mask pixels
        if (m0 | m1 | m2 | m3) == 0 {
            i += 4;
            continue;
        }

        for j in 0..4 {
            let m = mask[i + j];
            if m == 0 {
                continue;
            }
            let a = if base_alpha == 255 {
                m as u32
            } else {
                crate::color::div255_fast(alpha_base * m as u32)
            };
            if a == 0 {
                continue;
            }
            if a == 255 {
                dst[i + j] = src;
                continue;
            }

            let inv_a = 255 - a;
            let d = dst[i + j] as u32;
            let r_d = (d >> 11) & 0x1F;
            let g_d = (d >> 5) & 0x3F;
            let b_d = d & 0x1F;

            let r = crate::color::div255_round(s_r * a + r_d * inv_a) as u16;
            let g = crate::color::div255_round(s_g * a + g_d * inv_a) as u16;
            let b = crate::color::div255_round(s_b * a + b_d * inv_a) as u16;

            dst[i + j] = (r << 11) | (g << 5) | b;
        }
        i += 4;
    }

    while i < len {
        let m = mask[i];
        if m > 0 {
            let a = if base_alpha == 255 {
                m as u32
            } else {
                crate::color::div255_fast(alpha_base * m as u32)
            };
            if a == 255 {
                dst[i] = src;
            } else if a > 0 {
                let inv_a = 255 - a;
                let d = dst[i] as u32;
                let r_d = (d >> 11) & 0x1F;
                let g_d = (d >> 5) & 0x3F;
                let b_d = d & 0x1F;

                let r = crate::color::div255_round(s_r * a + r_d * inv_a) as u16;
                let g = crate::color::div255_round(s_g * a + g_d * inv_a) as u16;
                let b = crate::color::div255_round(s_b * a + b_d * inv_a) as u16;

                dst[i] = (r << 11) | (g << 5) | b;
            }
        }
        i += 1;
    }
}

#[inline(always)]
pub fn fill_span_rgb565(dst: &mut [u16], color: u16) {
    crate::raster::rect::fill_u16_slice(dst, color);
}
