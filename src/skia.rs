//! A [`tiny_skia`] rasterizer: the same drawing surface as [`crate::Canvas`],
//! but rendering into premultiplied RGBA8888 with real antialiasing.
//!
//! # Why this exists
//!
//! The hand-written rasterizer in [`crate::raster`] writes RGB565 directly and
//! gives hard, aliased edges (see `issues-and-todo/zh/260925-02-rendering-anti-aliasing.md`).
//! `tiny-skia` is a pure-CPU port of a Skia subset: no GPU, no `build.rs`, and
//! its SIMD path is x86-only, so it falls back to scalar code on the panel's
//! LX7 cores. The price is the pixel format — `tiny-skia` only draws into
//! premultiplied RGBA8888 — so this backend owns an RGBA surface and converts
//! to RGB565 ([`Surface::to_rgb565`]) on the way to the display.
//!
//! # Design
//!
//! [`Surface`] *owns* the RGBA pixmap and the clip mask; [`Canvas`] is a short
//! lived view over it that mirrors the drawing API of [`crate::Canvas`] one to
//! one. The transform and clip bookkeeping is the same scale+translate model
//! the RGB565 canvas uses, so a caller can swap backends without changing how
//! it draws.
//!
//! Two deliberate differences from the RGB565 rasterizer:
//!
//! - [`Canvas::fill_path`] is a real fill (with the requested fill rule); the
//!   RGB565 one only strokes the flattened outline.
//! - [`Canvas::fill_dithered_horizontal_gradient`] is a plain linear gradient
//!   shader, so the RGB565 dithering is not reproduced — banding is a property
//!   of the 16-bit *output*, not of the gradient.

use crate::color::Color;
use crate::geometry::{Point, RRect, Rect, Transform};
use crate::paint::{FillRule, LineCap, LineJoin, Paint, Shader, Stroke};
use crate::path::{Path, PathVerb};

use tiny_skia::{
    BlendMode, FilterQuality, Mask, Paint as SkiaPaint, PathBuilder as SkiaPathBuilder, Pixmap,
    PixmapPaint, PixmapRef, SpreadMode as SkiaSpreadMode, Stroke as SkiaStroke,
    Transform as SkiaTransform,
};

/// Per-operation timing for the skia backend, behind the `profile` feature.
///
/// A single frame is a few hundred drawing calls, so "which call is slow" cannot
/// be answered by reading the code — guessing got this wrong by an order of
/// magnitude once already. Each entry point records its own cost, and the two
/// halves of an image blit are recorded separately, because the RGB565 -> RGBA
/// staging loop and tiny-skia's composite are completely different animals.
#[cfg(feature = "profile")]
pub mod profile {
    use std::cell::RefCell;
    use std::time::{Duration, Instant};

    #[derive(Clone, Copy)]
    pub struct Row {
        pub calls: u32,
        /// Pixels covered, so a table can tell "many small calls" from one huge one.
        pub units: u64,
        pub time: Duration,
    }

    impl Row {
        const ZERO: Row = Row {
            calls: 0,
            units: 0,
            time: Duration::ZERO,
        };
    }

    pub const DRAW_RECT: usize = 0;
    pub const DRAW_RRECT: usize = 1;
    pub const DRAW_RRECT_STROKE: usize = 2;
    pub const DRAW_CIRCLE: usize = 3;
    pub const FILL_PATH: usize = 4;
    pub const STROKE_PATH: usize = 5;
    pub const FILL_GRADIENT: usize = 6;
    pub const BLIT_MASK: usize = 7;
    pub const BLIT_IMAGE: usize = 8;
    pub const BLIT_IMAGE_ALPHA: usize = 9;
    pub const BLIT_IMAGE_SCALED: usize = 10;
    pub const CLIP_MASK: usize = 11;
    pub const CLEAR: usize = 12;
    pub const COPY_PIXELS: usize = 13;
    /// Nested inside the blit rows above.
    pub const STAGE_EXPAND: usize = 14;
    pub const STAGE_COMPOSITE: usize = 15;
    pub const COUNT: usize = 16;

    const CATEGORIES: [&str; COUNT] = [
        "draw_rect",
        "draw_rrect",
        "draw_rrect_stroke",
        "draw_circle",
        "fill_path",
        "stroke_path",
        "fill_gradient",
        "blit_mask",
        "blit_image_565",
        "blit_image_565_alpha",
        "blit_image_565_scaled",
        "clip_mask_rebuild",
        "clear",
        "copy_pixels_from",
        "  (of which) expand",
        "  (of which) composite",
    ];

    thread_local! {
        static ROWS: RefCell<[Row; COUNT]> = RefCell::new([Row::ZERO; COUNT]);
    }

    /// Times one call; records on drop, so an early return still counts.
    pub struct Timer {
        slot: usize,
        units: u64,
        started: Instant,
    }

    impl Timer {
        #[inline(always)]
        pub fn start(slot: usize, units: u64) -> Self {
            Self {
                slot,
                units,
                started: Instant::now(),
            }
        }
    }

    impl Drop for Timer {
        fn drop(&mut self) {
            let elapsed = self.started.elapsed();
            ROWS.with(|rows| {
                let mut rows = rows.borrow_mut();
                let row = &mut rows[self.slot];
                row.calls += 1;
                row.units += self.units;
                row.time += elapsed;
            });
        }
    }

    /// Pixels in a box, for the `units` column.
    #[inline(always)]
    pub fn area(width: f32, height: f32) -> u64 {
        (width.max(0.0) as u64) * (height.max(0.0) as u64)
    }

    pub fn reset() {
        ROWS.with(|rows| *rows.borrow_mut() = [Row::ZERO; COUNT]);
    }

    /// One line per category, slowest first.
    pub fn report() -> String {
        ROWS.with(|rows| {
            let rows = rows.borrow();
            let mut order: Vec<usize> = (0..COUNT).collect();
            order.sort_by_key(|&slot| std::cmp::Reverse(rows[slot].time));

            let mut out = String::from("---- skia ops (slowest first) ----\n");
            for slot in order {
                let row = rows[slot];
                if row.calls == 0 {
                    continue;
                }
                out.push_str(&format!(
                    "{:<24} {:>5} calls {:>9} px {:>8.1}ms\n",
                    CATEGORIES[slot],
                    row.calls,
                    row.units,
                    row.time.as_secs_f64() * 1000.0,
                ));
            }
            out
        })
    }
}

/// Circle-to-Bézier constant: the control point offset that approximates a
/// quarter arc with a cubic. Same value every vector library uses.
const KAPPA: f32 = 0.552_284_8;

/// An off-screen premultiplied RGBA8888 surface, plus the clip mask tiny-skia
/// needs for anything that is not an axis-aligned rectangle.
pub struct Surface {
    pixmap: Pixmap,
    clip_mask: Mask,
    /// The clip currently baked into `clip_mask`, so a redraw that does not
    /// change the clip does not pay for rebuilding the mask.
    clip: Option<Rect>,
    /// Staging buffer for mask and RGB565 image blits (premultiplied RGBA).
    /// Reused across calls: a per-glyph allocation would dominate the frame.
    scratch: Vec<u8>,
}

impl Surface {
    /// Allocate a surface. Returns `None` for a zero-sized or absurdly large
    /// request (the same contract as [`crate::Pixmap565::new`]).
    pub fn new(width: u32, height: u32) -> Option<Self> {
        Some(Self {
            pixmap: Pixmap::new(width, height)?,
            clip_mask: Mask::new(width, height)?,
            clip: None,
            scratch: Vec::new(),
        })
    }

    #[inline(always)]
    pub fn width(&self) -> u32 {
        self.pixmap.width()
    }

    #[inline(always)]
    pub fn height(&self) -> u32 {
        self.pixmap.height()
    }

    /// Resize, discarding the contents (the RGBA equivalent of
    /// [`crate::Pixmap565::new`] on a resize).
    pub fn resize(&mut self, width: u32, height: u32) -> bool {
        let Some(pixmap) = Pixmap::new(width, height) else {
            return false;
        };
        let Some(clip_mask) = Mask::new(width, height) else {
            return false;
        };
        self.pixmap = pixmap;
        self.clip_mask = clip_mask;
        self.clip = None;
        true
    }

    /// A drawing view over this surface.
    pub fn canvas(&mut self) -> Canvas<'_> {
        Canvas::new(self)
    }

    /// Fill the whole surface with `color` (premultiplied, replacing whatever
    /// was there — Blender's `Source`).
    pub fn clear(&mut self, color: Color) {
        self.pixmap.fill(skia_color(color));
    }

    /// Set (or clear) the clip rectangle. Rebuilds the coverage mask only when
    /// the rectangle actually changes.
    fn set_clip(&mut self, clip: Option<Rect>) {
        if self.clip == clip {
            return;
        }
        #[cfg(feature = "profile")]
        let _t = profile::Timer::start(
            profile::CLIP_MASK,
            (self.width() as u64) * (self.height() as u64),
        );
        self.clip_mask.clear();
        if let Some(rect) = clip.and_then(to_skia_rect) {
            let path = SkiaPathBuilder::from_rect(rect);
            self.clip_mask.fill_path(
                &path,
                tiny_skia::FillRule::Winding,
                true,
                SkiaTransform::identity(),
            );
        }
        self.clip = clip;
    }

    /// Convert the whole surface to RGB565, row-major.
    ///
    /// The pixels are premultiplied, which is exactly the right thing for a
    /// panel with no alpha channel: the stored RGB *is* the colour composited
    /// over black.
    pub fn to_rgb565(&self, dst: &mut [u16]) -> usize {
        let data = self.pixmap.data();
        let count = (data.len() / 4).min(dst.len());
        for (i, out) in dst.iter_mut().take(count).enumerate() {
            let bytes = &data[i * 4..i * 4 + 4];
            *out = rgb565_of(bytes[0], bytes[1], bytes[2]);
        }
        count
    }

    /// Convert one rectangle of the surface to RGB565 (for a dirty-region or
    /// banded flush). Returns the number of pixels written — 0 when the
    /// rectangle is empty, out of bounds, or `dst` is too small.
    pub fn region_to_rgb565(&self, rect: Rect, dst: &mut [u16]) -> usize {
        let width = self.width() as i32;
        let height = self.height() as i32;

        let x1 = (rect.x.floor() as i32).clamp(0, width);
        let y1 = (rect.y.floor() as i32).clamp(0, height);
        let x2 = (rect.right().ceil() as i32).clamp(x1, width);
        let y2 = (rect.bottom().ceil() as i32).clamp(y1, height);

        let rect_w = (x2 - x1) as usize;
        let rect_h = (y2 - y1) as usize;
        let total = rect_w * rect_h;
        if total == 0 || dst.len() < total {
            return 0;
        }

        let data = self.pixmap.data();
        let stride = width as usize;
        for y in y1..y2 {
            let src_row = y as usize * stride + x1 as usize;
            let dst_row = (y - y1) as usize * rect_w;
            for x in 0..rect_w {
                let i = (src_row + x) * 4;
                dst[dst_row + x] = rgb565_of(data[i], data[i + 1], data[i + 2]);
            }
        }
        total
    }

    /// The raw premultiplied RGBA buffer (`R, G, B, A` bytes), for tests and
    /// for hosts that want to inspect a frame.
    #[inline(always)]
    pub fn rgba(&self) -> &[u8] {
        self.pixmap.data()
    }
}

/// A drawing view over a [`Surface`], mirroring [`crate::Canvas`].
pub struct Canvas<'a> {
    surface: &'a mut Surface,
    transform: Transform,
    transform_stack: Vec<Transform>,
    clip_stack: Vec<Option<Rect>>,
    current_clip: Option<Rect>,
}

impl<'a> Canvas<'a> {
    pub fn new(surface: &'a mut Surface) -> Self {
        Self {
            surface,
            transform: Transform::identity(),
            transform_stack: Vec::new(),
            clip_stack: Vec::new(),
            current_clip: None,
        }
    }

    #[inline(always)]
    pub fn width(&self) -> u32 {
        self.surface.width()
    }

    #[inline(always)]
    pub fn height(&self) -> u32 {
        self.surface.height()
    }

    pub fn save(&mut self) {
        self.transform_stack.push(self.transform);
        self.clip_stack.push(self.current_clip);
    }

    pub fn restore(&mut self) {
        if let Some(tf) = self.transform_stack.pop() {
            self.transform = tf;
        }
        if let Some(clip) = self.clip_stack.pop() {
            self.current_clip = clip;
        }
    }

    #[inline(always)]
    pub fn current_clip(&self) -> Option<Rect> {
        self.current_clip
    }

    pub fn translate(&mut self, dx: f32, dy: f32) {
        self.transform = self.transform.post_translate(dx, dy);
    }

    pub fn scale(&mut self, sx: f32, sy: f32) {
        self.transform = self.transform.post_scale(sx, sy);
    }

    pub fn clip_rect(&mut self, rect: Rect) {
        let mapped = self.transform.map_rect(rect);
        self.current_clip = match self.current_clip {
            Some(prev) => prev.intersect(&mapped),
            None => Some(mapped),
        };
    }

    pub fn clear(&mut self, color: Color) {
        #[cfg(feature = "profile")]
        let _t = profile::Timer::start(
            profile::CLEAR,
            profile::area(self.width() as f32, self.height() as f32),
        );
        match self.current_clip {
            // Nothing clipped: replace the whole surface.
            None => self.surface.clear(color),
            Some(clip) => {
                // Replacing inside a clip means "erase everything first", so the
                // clipped area is exactly `color` and not a blend of the two.
                self.raster_replace(clip, color);
            }
        }
    }

    /// Overwrite `rect` with `color`, ignoring what was underneath.
    fn raster_replace(&mut self, rect: Rect, color: Color) {
        let clip = self.current_clip;
        self.sync_clip();
        let surface = &mut *self.surface;
        let mut pixmap = surface.pixmap.as_mut();
        let mask = clip.is_some().then_some(&surface.clip_mask);
        let mut paint = SkiaPaint::default();
        paint.set_color_rgba8(color.r, color.g, color.b, color.a);
        paint.blend_mode = BlendMode::Source;
        if let Some(sk_rect) = to_skia_rect(rect) {
            pixmap.fill_rect(sk_rect, &paint, SkiaTransform::identity(), mask);
        }
    }

    /// Copy raw RGB565 pixels over the surface (top-left aligned).
    pub fn copy_pixels_from(&mut self, src: &[u16]) {
        #[cfg(feature = "profile")]
        let _t = profile::Timer::start(
            profile::COPY_PIXELS,
            profile::area(self.width() as f32, self.height() as f32),
        );
        let width = self.width();
        if width == 0 {
            return;
        }
        let rows = (src.len() / width as usize) as u32;
        if rows == 0 {
            return;
        }
        self.blit_565(0, 0, width, rows, src, None, false);
    }

    pub fn draw_rect(&mut self, rect: Rect, color: Color) {
        let rect = self.transform.map_rect(rect);
        #[cfg(feature = "profile")]
        let _t = profile::Timer::start(profile::DRAW_RECT, profile::area(rect.width, rect.height));
        let clip = self.current_clip;
        self.sync_clip();
        let surface = &mut *self.surface;
        let mut pixmap = surface.pixmap.as_mut();
        let mask = clip.is_some().then_some(&surface.clip_mask);
        let mut paint = SkiaPaint::default();
        paint.set_color_rgba8(color.r, color.g, color.b, color.a);
        if let Some(sk_rect) = to_skia_rect(rect) {
            pixmap.fill_rect(sk_rect, &paint, SkiaTransform::identity(), mask);
        }
    }

    pub fn draw_rrect(&mut self, rrect: RRect, color: Color) {
        let rrect = self.transform.map_rrect(rrect);
        #[cfg(feature = "profile")]
        let _t = profile::Timer::start(
            profile::DRAW_RRECT,
            profile::area(rrect.rect.width, rrect.rect.height),
        );
        if let Some(path) = rrect_path(rrect) {
            self.fill(&path, color, FillRule::Winding);
        }
    }

    pub fn draw_rrect_stroke(&mut self, rrect: RRect, color: Color, stroke_width: f32) {
        let rrect = self.transform.map_rrect(rrect);
        #[cfg(feature = "profile")]
        let _t = profile::Timer::start(
            profile::DRAW_RRECT_STROKE,
            profile::area(rrect.rect.width, rrect.rect.height),
        );
        let Some(path) = rrect_path(rrect) else {
            return;
        };
        let clip = self.current_clip;
        self.sync_clip();
        let surface = &mut *self.surface;
        let mut pixmap = surface.pixmap.as_mut();
        let mask = clip.is_some().then_some(&surface.clip_mask);
        let mut paint = SkiaPaint::default();
        paint.set_color_rgba8(color.r, color.g, color.b, color.a);
        paint.anti_alias = true;
        let stroke = SkiaStroke {
            width: stroke_width * self.transform.sx,
            ..Default::default()
        };
        pixmap.stroke_path(&path, &paint, &stroke, SkiaTransform::identity(), mask);
    }

    pub fn draw_circle(&mut self, center: Point, radius: f32, color: Color) {
        let center = self.transform.map_point(center);
        let radius = radius * self.transform.sx;
        #[cfg(feature = "profile")]
        let _t = profile::Timer::start(
            profile::DRAW_CIRCLE,
            (radius.max(0.0) * radius.max(0.0) * 3.14159) as u64,
        );
        let Some(path) = SkiaPathBuilder::from_circle(center.x, center.y, radius) else {
            return;
        };
        self.fill(&path, color, FillRule::Winding);
    }

    /// Blit an 8-bit coverage mask (a glyph or a mono icon) tinted with `color`.
    pub fn blit_mask(&mut self, x: i32, y: i32, w: u32, h: u32, mask: &[u8], color: Color) {
        if w == 0 || h == 0 || mask.len() < (w as usize) * (h as usize) {
            return;
        }
        #[cfg(feature = "profile")]
        let _t = profile::Timer::start(profile::BLIT_MASK, (w as u64) * (h as u64));
        let tx = x + self.transform.tx.round() as i32;
        let ty = y + self.transform.ty.round() as i32;

        let scratch = &mut self.surface.scratch;
        scratch.clear();
        scratch.reserve((w as usize) * (h as usize) * 4);
        for &coverage in mask.iter().take((w as usize) * (h as usize)) {
            push_premultiplied(scratch, color.r, color.g, color.b, coverage);
        }
        self.draw_staged(tx, ty, w, h, FilterQuality::Nearest, false);
    }

    pub fn blit_image_565(&mut self, x: i32, y: i32, w: u32, h: u32, pixels: &[u16]) {
        #[cfg(feature = "profile")]
        let _t = profile::Timer::start(profile::BLIT_IMAGE, (w as u64) * (h as u64));
        self.blit_565(x, y, w, h, pixels, None, false);
    }

    pub fn blit_image_565_with_alpha(
        &mut self,
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        rgb: &[u16],
        alpha: &[u8],
    ) {
        #[cfg(feature = "profile")]
        let _t = profile::Timer::start(profile::BLIT_IMAGE_ALPHA, (w as u64) * (h as u64));
        self.blit_565(x, y, w, h, rgb, Some(alpha), false);
    }

    /// Blit an RGB565 source scaled into a `dst_w` x `dst_h` destination box.
    #[allow(clippy::too_many_arguments)]
    pub fn blit_image_565_with_alpha_scaled(
        &mut self,
        x: i32,
        y: i32,
        dst_w: u32,
        dst_h: u32,
        src_w: u32,
        src_h: u32,
        rgb: &[u16],
        alpha: &[u8],
    ) {
        if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 {
            return;
        }
        #[cfg(feature = "profile")]
        let _t = profile::Timer::start(
            profile::BLIT_IMAGE_SCALED,
            (dst_w as u64) * (dst_h as u64),
        );
        let scratch = &mut self.surface.scratch;
        expand_565(rgb, Some(alpha), src_w, src_h, scratch);
        self.draw_staged_scaled(x, y, dst_w, dst_h, src_w, src_h);
    }

    #[allow(clippy::too_many_arguments)]
    fn blit_565(
        &mut self,
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        pixels: &[u16],
        alpha: Option<&[u8]>,
        replace: bool,
    ) {
        if w == 0 || h == 0 {
            return;
        }
        let scratch = &mut self.surface.scratch;
        expand_565(pixels, alpha, w, h, scratch);
        let tx = x + self.transform.tx.round() as i32;
        let ty = y + self.transform.ty.round() as i32;
        self.draw_staged(tx, ty, w, h, FilterQuality::Nearest, replace);
    }

    /// Composite the staging buffer (already packed `w * h * 4` RGBA bytes).
    fn draw_staged(&mut self, x: i32, y: i32, w: u32, h: u32, quality: FilterQuality, replace: bool) {
        #[cfg(feature = "profile")]
        let _t = profile::Timer::start(profile::STAGE_COMPOSITE, (w as u64) * (h as u64));
        let length = (w as usize) * (h as usize) * 4;
        if self.surface.scratch.len() < length {
            return;
        }
        let clip = self.current_clip;
        self.sync_clip();

        let surface = &mut *self.surface;
        let scratch: &[u8] = &surface.scratch;
        let Some(source) = PixmapRef::from_bytes(&scratch[..length], w, h) else {
            return;
        };
        let paint = PixmapPaint {
            opacity: 1.0,
            blend_mode: if replace {
                BlendMode::Source
            } else {
                BlendMode::SourceOver
            },
            quality,
        };
        let mut pixmap = surface.pixmap.as_mut();
        let mask = clip.is_some().then_some(&surface.clip_mask);
        pixmap.draw_pixmap(x, y, source, &paint, SkiaTransform::identity(), mask);
    }

    /// Composite the staging buffer scaled from `src_w` x `src_h` into a
    /// `dst_w` x `dst_h` box whose top-left corner is `(x, y)`.
    fn draw_staged_scaled(&mut self, x: i32, y: i32, dst_w: u32, dst_h: u32, src_w: u32, src_h: u32) {
        #[cfg(feature = "profile")]
        let _t = profile::Timer::start(
            profile::STAGE_COMPOSITE,
            (dst_w as u64) * (dst_h as u64),
        );
        let length = (src_w as usize) * (src_h as usize) * 4;
        if self.surface.scratch.len() < length {
            return;
        }
        let clip = self.current_clip;
        self.sync_clip();

        let sx = dst_w as f32 / src_w as f32;
        let sy = dst_h as f32 / src_h as f32;

        let surface = &mut *self.surface;
        let scratch: &[u8] = &surface.scratch;
        let Some(source) = PixmapRef::from_bytes(&scratch[..length], src_w, src_h) else {
            return;
        };
        let paint = PixmapPaint {
            opacity: 1.0,
            blend_mode: BlendMode::SourceOver,
            quality: FilterQuality::Bilinear,
        };
        let mut pixmap = surface.pixmap.as_mut();
        let mask = clip.is_some().then_some(&surface.clip_mask);
        // `draw_pixmap` anchors the source at its own origin, so the box offset
        // has to be folded into the transform, not the `x`/`y` arguments.
        let transform = SkiaTransform::from_translate(x as f32, y as f32).pre_scale(sx, sy);
        pixmap.draw_pixmap(0, 0, source, &paint, transform, mask);
    }

    /// Stroke a path. The path is mapped into device space here rather than by
    /// tiny-skia, so the stroke width scales exactly like the RGB565
    /// rasterizer's does (and not twice).
    pub fn stroke_path(&mut self, path: &Path, paint: &Paint, stroke: &Stroke) {
        #[cfg(feature = "profile")]
        let _t = profile::Timer::start(profile::STROKE_PATH, path.verbs.len() as u64);
        let Some(path) = self.sk_path(path) else {
            return;
        };
        let clip = self.current_clip;
        self.sync_clip();

        let surface = &mut *self.surface;
        let mut pixmap = surface.pixmap.as_mut();
        let mask = clip.is_some().then_some(&surface.clip_mask);
        let skia_paint = skia_paint(paint);
        let skia_stroke = SkiaStroke {
            width: stroke.width * self.transform.sx,
            miter_limit: stroke.miter_limit,
            line_cap: match stroke.line_cap {
                LineCap::Butt => tiny_skia::LineCap::Butt,
                LineCap::Round => tiny_skia::LineCap::Round,
                LineCap::Square => tiny_skia::LineCap::Square,
            },
            line_join: match stroke.line_join {
                LineJoin::Miter => tiny_skia::LineJoin::Miter,
                LineJoin::Round => tiny_skia::LineJoin::Round,
                LineJoin::Bevel => tiny_skia::LineJoin::Bevel,
            },
            dash: None,
        };
        pixmap.stroke_path(&path, &skia_paint, &skia_stroke, SkiaTransform::identity(), mask);
    }

    /// Fill a path — a real fill, honouring `fill_rule`.
    pub fn fill_path(&mut self, path: &Path, paint: &Paint, fill_rule: FillRule) {
        #[cfg(feature = "profile")]
        let _t = profile::Timer::start(profile::FILL_PATH, path.verbs.len() as u64);
        let Some(path) = self.sk_path(path) else {
            return;
        };
        let clip = self.current_clip;
        self.sync_clip();

        let surface = &mut *self.surface;
        let mut pixmap = surface.pixmap.as_mut();
        let mask = clip.is_some().then_some(&surface.clip_mask);
        let skia_paint = skia_paint(paint);
        pixmap.fill_path(
            &path,
            &skia_paint,
            match fill_rule {
                FillRule::Winding => tiny_skia::FillRule::Winding,
                FillRule::EvenOdd => tiny_skia::FillRule::EvenOdd,
            },
            SkiaTransform::identity(),
            mask,
        );
    }

    /// Fill the whole surface with a horizontal gradient between two colours.
    pub fn fill_dithered_horizontal_gradient(&mut self, c0: (u8, u8, u8), c1: (u8, u8, u8)) {
        #[cfg(feature = "profile")]
        let _t = profile::Timer::start(
            profile::FILL_GRADIENT,
            profile::area(self.width() as f32, self.height() as f32),
        );
        let end = self.width() as f32;
        let stops = vec![
            tiny_skia::GradientStop::new(0.0, srgb(c0.0, c0.1, c0.2)),
            tiny_skia::GradientStop::new(1.0, srgb(c1.0, c1.1, c1.2)),
        ];
        let Some(shader) = tiny_skia::LinearGradient::new(
            tiny_skia::Point::from_xy(0.0, 0.0),
            tiny_skia::Point::from_xy(end, 0.0),
            stops,
            SkiaSpreadMode::Pad,
            SkiaTransform::identity(),
        ) else {
            return;
        };

        let clip = self.current_clip;
        self.sync_clip();
        let (w, h) = (self.width() as f32, self.height() as f32);
        let surface = &mut *self.surface;
        let mut pixmap = surface.pixmap.as_mut();
        let mask = clip.is_some().then_some(&surface.clip_mask);
        let paint = SkiaPaint {
            shader,
            ..Default::default()
        };
        if let Some(rect) = tiny_skia::Rect::from_xywh(0.0, 0.0, w, h) {
            pixmap.fill_rect(rect, &paint, SkiaTransform::identity(), mask);
        }
    }

    /// Build a tiny-skia path from ours, mapped into device space.
    fn sk_path(&self, path: &Path) -> Option<tiny_skia::Path> {
        if path.is_empty() {
            return None;
        }
        let mut builder = SkiaPathBuilder::new();
        for verb in &path.verbs {
            match *verb {
                PathVerb::MoveTo(p) => {
                    let p = self.transform.map_point(p);
                    builder.move_to(p.x, p.y);
                }
                PathVerb::LineTo(p) => {
                    let p = self.transform.map_point(p);
                    builder.line_to(p.x, p.y);
                }
                PathVerb::QuadTo(c, p) => {
                    let c = self.transform.map_point(c);
                    let p = self.transform.map_point(p);
                    builder.quad_to(c.x, c.y, p.x, p.y);
                }
                PathVerb::CubicTo(c1, c2, p) => {
                    let c1 = self.transform.map_point(c1);
                    let c2 = self.transform.map_point(c2);
                    let p = self.transform.map_point(p);
                    builder.cubic_to(c1.x, c1.y, c2.x, c2.y, p.x, p.y);
                }
                PathVerb::Close => builder.close(),
            }
        }
        builder.finish()
    }

    /// Fill an already-device-space path with a solid colour.
    fn fill(&mut self, path: &tiny_skia::Path, color: Color, fill_rule: FillRule) {
        let clip = self.current_clip;
        self.sync_clip();
        let surface = &mut *self.surface;
        let mut pixmap = surface.pixmap.as_mut();
        let mask = clip.is_some().then_some(&surface.clip_mask);
        let mut paint = SkiaPaint::default();
        paint.set_color_rgba8(color.r, color.g, color.b, color.a);
        paint.anti_alias = true;
        pixmap.fill_path(
            path,
            &paint,
            match fill_rule {
                FillRule::Winding => tiny_skia::FillRule::Winding,
                FillRule::EvenOdd => tiny_skia::FillRule::EvenOdd,
            },
            SkiaTransform::identity(),
            mask,
        );
    }

    /// Bring the surface's coverage mask in line with `current_clip`.
    fn sync_clip(&mut self) {
        self.surface.set_clip(self.current_clip);
    }
}

/// A rounded-rectangle outline, corners approximated by one cubic each.
fn rrect_path(rrect: RRect) -> Option<tiny_skia::Path> {
    let Rect { x, y, width, height } = rrect.rect;
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let rx = rrect.radius.x.min(width * 0.5).max(0.0);
    let ry = rrect.radius.y.min(height * 0.5).max(0.0);

    let mut pb = SkiaPathBuilder::new();
    pb.move_to(x + rx, y);
    pb.line_to(x + width - rx, y);
    pb.cubic_to(
        x + width - rx + KAPPA * rx,
        y,
        x + width,
        y + ry - KAPPA * ry,
        x + width,
        y + ry,
    );
    pb.line_to(x + width, y + height - ry);
    pb.cubic_to(
        x + width,
        y + height - ry + KAPPA * ry,
        x + width - rx + KAPPA * rx,
        y + height,
        x + width - rx,
        y + height,
    );
    pb.line_to(x + rx, y + height);
    pb.cubic_to(
        x + rx - KAPPA * rx,
        y + height,
        x,
        y + height - ry + KAPPA * ry,
        x,
        y + height - ry,
    );
    pb.line_to(x, y + ry);
    pb.cubic_to(x, y + ry - KAPPA * ry, x + rx - KAPPA * rx, y, x + rx, y);
    pb.close();
    pb.finish()
}

fn skia_paint(paint: &Paint) -> SkiaPaint<'static> {
    let mut out = SkiaPaint {
        anti_alias: paint.anti_alias,
        ..Default::default()
    };
    match &paint.shader {
        Shader::SolidColor(color) => {
            out.set_color_rgba8(color.r, color.g, color.b, color.a);
        }
        Shader::Linear(gradient) => {
            let stops = gradient
                .stops
                .iter()
                .map(|stop| {
                    tiny_skia::GradientStop::new(
                        stop.position,
                        srgb(stop.color.r, stop.color.g, stop.color.b),
                    )
                })
                .collect();
            if let Some(shader) = tiny_skia::LinearGradient::new(
                tiny_skia::Point::from_xy(gradient.start.x, gradient.start.y),
                tiny_skia::Point::from_xy(gradient.end.x, gradient.end.y),
                stops,
                match gradient.spread {
                    crate::paint::SpreadMode::Pad => SkiaSpreadMode::Pad,
                    crate::paint::SpreadMode::Reflect => SkiaSpreadMode::Reflect,
                    crate::paint::SpreadMode::Repeat => SkiaSpreadMode::Repeat,
                },
                SkiaTransform::identity(),
            ) {
                out.shader = shader;
            }
        }
        Shader::_Phantom(_) => out.set_color_rgba8(0, 0, 0, 0),
    }
    out
}

fn to_skia_rect(rect: Rect) -> Option<tiny_skia::Rect> {
    tiny_skia::Rect::from_xywh(rect.x, rect.y, rect.width, rect.height)
}

#[inline(always)]
fn srgb(r: u8, g: u8, b: u8) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(r, g, b, 255)
}

#[inline(always)]
fn skia_color(color: Color) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(color.r, color.g, color.b, color.a)
}

/// Expand an RGB565 image (optionally with an 8-bit coverage mask) into
/// premultiplied RGBA bytes.
fn expand_565(pixels: &[u16], alpha: Option<&[u8]>, w: u32, h: u32, dst: &mut Vec<u8>) {
    #[cfg(feature = "profile")]
    let _t = profile::Timer::start(profile::STAGE_EXPAND, (w as u64) * (h as u64));
    dst.clear();
    let count = (w as usize) * (h as usize);
    dst.reserve(count * 4);
    for (i, &px) in pixels.iter().take(count).enumerate() {
        let (r5, g6, b5) = (px >> 11, (px >> 5) & 0x3F, px & 0x1F);
        let r = ((r5 << 3) | (r5 >> 2)) as u8;
        let g = ((g6 << 2) | (g6 >> 4)) as u8;
        let b = ((b5 << 3) | (b5 >> 2)) as u8;
        let a = alpha.map_or(255u8, |m| m.get(i).copied().unwrap_or(0));
        push_premultiplied(dst, r, g, b, a);
    }
}

/// Append one premultiplied RGBA pixel. Components are floored so the
/// `rgb <= alpha` invariant tiny-skia relies on always holds.
#[inline(always)]
fn push_premultiplied(dst: &mut Vec<u8>, r: u8, g: u8, b: u8, a: u8) {
    let scale = |c: u8| ((c as u32 * a as u32) / 255) as u8;
    dst.push(scale(r));
    dst.push(scale(g));
    dst.push(scale(b));
    dst.push(a);
}

#[inline(always)]
fn rgb565_of(r: u8, g: u8, b: u8) -> u16 {
    ((r as u16 >> 3) << 11) | ((g as u16 >> 2) << 5) | (b as u16 >> 3)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one helper the RGB565 canvas has and a `Transform` should too: it
    /// lives on `Transform` in the parent module, so the tests exercise the
    /// mapping used by every draw call.
    fn rect_of(surface: &Surface, x: u32, y: u32) -> (u8, u8, u8, u8) {
        let i = ((y * surface.width() + x) * 4) as usize;
        let d = surface.rgba();
        (d[i], d[i + 1], d[i + 2], d[i + 3])
    }

    #[test]
    fn solid_rect_covers_exactly_its_pixels() {
        let mut surface = Surface::new(64, 64).unwrap();
        surface.canvas().draw_rect(
            Rect::from_ltwh(10.0, 10.0, 20.0, 20.0),
            Color::from_rgb(255, 0, 0),
        );

        assert_eq!(rect_of(&surface, 15, 15), (255, 0, 0, 255));
        // Integer-aligned edges are not antialiased: the neighbours stay clear.
        assert_eq!(rect_of(&surface, 29, 15), (255, 0, 0, 255));
        assert_eq!(rect_of(&surface, 30, 15).3, 0);
        assert_eq!(rect_of(&surface, 9, 15).3, 0);
    }

    #[test]
    fn rounded_rect_is_antialiased() {
        let mut surface = Surface::new(96, 96).unwrap();
        surface.canvas().draw_rrect(
            RRect::from_rect_xy(Rect::from_ltwh(8.0, 8.0, 80.0, 80.0), 40.0, 40.0),
            Color::from_rgb(255, 255, 255),
        );

        // The corner is outside the rounded shape, the centre is inside.
        assert_eq!(rect_of(&surface, 9, 9).3, 0, "corner must stay clear");
        assert_eq!(rect_of(&surface, 48, 48), (255, 255, 255, 255));

        // Walking the diagonal across the corner arc must produce partial
        // coverage somewhere — that is the whole point of this backend.
        let partial = (0..40).any(|d| {
            let (r, _, _, a) = rect_of(&surface, 8 + d, 8 + d);
            a > 0 && a < 255 && r > 0
        });
        assert!(partial, "no partially covered pixels on the arc");
    }

    #[test]
    fn clip_confines_drawing() {
        let mut surface = Surface::new(64, 64).unwrap();
        let mut canvas = surface.canvas();
        canvas.clip_rect(Rect::from_ltwh(0.0, 0.0, 32.0, 64.0));
        canvas.draw_rect(Rect::from_ltwh(0.0, 0.0, 64.0, 64.0), Color::from_rgb(0, 255, 0));
        drop(canvas);

        assert_eq!(rect_of(&surface, 10, 32), (0, 255, 0, 255));
        assert_eq!(rect_of(&surface, 40, 32).3, 0, "clipped half must be untouched");
    }

    #[test]
    fn save_restore_undoes_clip_and_transform() {
        let mut surface = Surface::new(64, 64).unwrap();
        let mut canvas = surface.canvas();
        canvas.save();
        canvas.clip_rect(Rect::from_ltwh(0.0, 0.0, 8.0, 8.0));
        canvas.translate(32.0, 0.0);
        canvas.restore();

        assert_eq!(canvas.current_clip(), None);
        canvas.draw_rect(Rect::from_ltwh(0.0, 0.0, 4.0, 4.0), Color::from_rgb(255, 0, 0));
        assert_eq!(rect_of(&surface, 1, 1), (255, 0, 0, 255));
    }

    #[test]
    fn blit_mask_tints_and_covers() {
        let mut surface = Surface::new(16, 16).unwrap();
        // A 2x2 mask: opaque, half, clear, opaque.
        let mask = [255u8, 128, 0, 255];
        surface.canvas().blit_mask(
            4,
            4,
            2,
            2,
            &mask,
            Color::from_rgb(255, 255, 255),
        );

        assert_eq!(rect_of(&surface, 4, 4), (255, 255, 255, 255));
        assert_eq!(rect_of(&surface, 5, 4).3, 128);
        assert_eq!(rect_of(&surface, 4, 5).3, 0);
        assert_eq!(rect_of(&surface, 5, 5), (255, 255, 255, 255));
    }

    #[test]
    fn scaled_blit_fills_its_destination_box() {
        let mut surface = Surface::new(64, 64).unwrap();
        let src = [0xF800u16; 16];
        let alpha = [255u8; 16];
        surface.canvas().blit_image_565_with_alpha_scaled(
            8, 8, 32, 32, 4, 4, &src, &alpha,
        );

        // Inside the box: red. Just outside: untouched.
        assert_eq!(rect_of(&surface, 8, 8), (255, 0, 0, 255));
        assert_eq!(rect_of(&surface, 39, 39), (255, 0, 0, 255));
        assert_eq!(rect_of(&surface, 40, 40).3, 0, "must not spill past the box");
        assert_eq!(rect_of(&surface, 7, 20).3, 0, "must not start before the box");
    }

    #[test]
    fn rgb565_output_round_trips() {
        let mut surface = Surface::new(8, 8).unwrap();
        surface
            .canvas()
            .draw_rect(Rect::from_ltwh(0.0, 0.0, 8.0, 8.0), Color::from_rgb(255, 0, 0));

        let mut out = [0u16; 64];
        assert_eq!(surface.to_rgb565(&mut out), 64);
        assert_eq!(out[0], 0xF800);
    }

    #[test]
    fn region_output_matches_the_full_frame() {
        let mut surface = Surface::new(16, 16).unwrap();
        let mut canvas = surface.canvas();
        canvas.draw_rect(Rect::from_ltwh(0.0, 0.0, 16.0, 16.0), Color::from_rgb(0, 0, 255));
        canvas.draw_rect(Rect::from_ltwh(4.0, 4.0, 4.0, 4.0), Color::from_rgb(0, 255, 0));
        drop(canvas);

        let mut full = [0u16; 256];
        surface.to_rgb565(&mut full);

        let mut region = [0u16; 16];
        assert_eq!(
            surface.region_to_rgb565(Rect::from_ltwh(4.0, 4.0, 4.0, 4.0), &mut region),
            16
        );
        for y in 0..4 {
            for x in 0..4 {
                assert_eq!(region[y * 4 + x], full[(y + 4) * 16 + (x + 4)]);
            }
        }
    }

    /// Writes a PNG of a small UI-ish scene so the antialiasing can be judged by
    /// eye. Kept out of the repository's asset dirs, like every other test that
    /// produces a file.
    #[test]
    fn dump_reference_frame() {
        let mut surface = Surface::new(240, 240).unwrap();
        surface.clear(Color::from_rgb(16, 18, 24));

        {
            let mut canvas = surface.canvas();
            canvas.fill_dithered_horizontal_gradient((30, 40, 80), (90, 30, 70));
            canvas.draw_rrect(
                RRect::from_rect_xy(Rect::from_ltwh(20.0, 24.0, 200.0, 92.0), 22.0, 22.0),
                Color::from_rgba(255, 255, 255, 36),
            );
            canvas.draw_rrect_stroke(
                RRect::from_rect_xy(Rect::from_ltwh(20.0, 24.0, 200.0, 92.0), 22.0, 22.0),
                Color::from_rgba(255, 255, 255, 120),
                2.0,
            );
            canvas.draw_circle(Point::new(172.0, 70.0), 26.0, Color::from_rgb(255, 190, 60));
            canvas.draw_rrect_stroke(
                RRect::from_rect_xy(Rect::from_ltwh(36.5, 140.5, 80.5, 40.5), 20.0, 20.0),
                Color::from_rgb(120, 220, 255),
                1.0,
            );
            canvas.draw_rect(Rect::from_ltwh(140.0, 152.0, 40.0, 2.0), Color::from_rgb(255, 255, 255));
        }

        let path = std::env::temp_dir().join("tiny_gfx_skia_reference.png");
        surface
            .pixmap
            .as_ref()
            .save_png(&path)
            .expect("write reference frame");
        println!("reference frame: {}", path.display());
    }
}
