use crate::color::Color;
use crate::geometry::Rect;
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

    let x1 = (bounds.x.floor() as i32).clamp(0, pix_w);
    let y1 = (bounds.y.floor() as i32).clamp(0, pix_h);
    let x2 = (bounds.right().ceil() as i32).clamp(x1, pix_w);
    let y2 = (bounds.bottom().ceil() as i32).clamp(y1, pix_h);

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
            crate::arch::fill_span_rgb565(slice, col565);
        } else {
            crate::arch::blend_span_rgb565(slice, col565, color.a);
        }
    }
}

