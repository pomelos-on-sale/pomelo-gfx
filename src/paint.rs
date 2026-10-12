use crate::color::Color;
use crate::geometry::{Point, Transform};
use palette::{FromColor, Oklab, Srgb};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineCap {
    Butt,
    Round,
    Square,
}

impl Default for LineCap {
    fn default() -> Self {
        Self::Butt
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineJoin {
    Miter,
    Round,
    Bevel,
}

impl Default for LineJoin {
    fn default() -> Self {
        Self::Miter
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillRule {
    Winding,
    EvenOdd,
}

impl Default for FillRule {
    fn default() -> Self {
        Self::Winding
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpreadMode {
    Pad,
    Reflect,
    Repeat,
}

impl Default for SpreadMode {
    fn default() -> Self {
        Self::Pad
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GradientStop {
    pub position: f32,
    pub color: Color,
    pub oklab: Oklab,
}

impl GradientStop {
    pub fn new(position: f32, color: Color) -> Self {
        let srgb = Srgb::new(color.r as f32 / 255.0, color.g as f32 / 255.0, color.b as f32 / 255.0);
        let oklab = Oklab::from_color(srgb);
        Self { position, color, oklab }
    }
}

static LINEAR_TO_SRGB_TABLE: std::sync::LazyLock<[f32; 1025]> = std::sync::LazyLock::new(|| {
    let mut table = [0.0f32; 1025];
    for i in 0..=1024 {
        let c = i as f32 / 1024.0;
        let s = if c <= 0.0031308 {
            12.92 * c
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        };
        table[i] = s.clamp(0.0, 1.0) * 255.0;
    }
    table
});

#[inline(always)]
fn linear_to_srgb_table(c: f32) -> f32 {
    let idx = (c.clamp(0.0, 1.0) * 1024.0 + 0.5) as usize;
    LINEAR_TO_SRGB_TABLE[idx]
}

/// Fast continuous Oklab to sRGB conversion (0.0..=255.0), bypassing generic libm::powf
#[inline(always)]
pub fn oklab_to_srgb_float(l_in: f32, a_in: f32, b_in: f32) -> (f32, f32, f32) {
    let l_ = l_in + 0.3963377774 * a_in + 0.2158037573 * b_in;
    let m_ = l_in - 0.1055613458 * a_in - 0.0638541728 * b_in;
    let s_ = l_in - 0.0894841775 * a_in - 1.2914855480 * b_in;

    let l = l_ * l_ * l_;
    let m = m_ * m_ * m_;
    let s = s_ * s_ * s_;

    let r_lin = 4.0767434775 * l - 3.3077115913 * m + 0.2309699292 * s;
    let g_lin = -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s;
    let b_lin = -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s;

    (
        linear_to_srgb_table(r_lin),
        linear_to_srgb_table(g_lin),
        linear_to_srgb_table(b_lin),
    )
}

#[derive(Debug, Clone, PartialEq)]
pub struct LinearGradient {
    pub start: Point,
    pub end: Point,
    pub stops: Vec<GradientStop>,
    pub spread: SpreadMode,
    pub transform: Transform,
}

impl LinearGradient {
    pub fn new(
        start: Point,
        end: Point,
        stops: Vec<GradientStop>,
        spread: SpreadMode,
        transform: Transform,
    ) -> Option<Shader<'static>> {
        if stops.is_empty() {
            None
        } else {
            Some(Shader::Linear(Self {
                start,
                end,
                stops,
                spread,
                transform,
            }))
        }
    }

    /// Evaluates the continuous floating-point RGBA (0.0..=255.0) at (x, y).
    #[inline(always)]
    pub fn eval_at(&self, x: f32, y: f32) -> (f32, f32, f32, f32) {
        let p = self.transform.map_point(Point::new(x, y));
        let dx = self.end.x - self.start.x;
        let dy = self.end.y - self.start.y;
        let len_sq = dx * dx + dy * dy;
        if len_sq <= 1e-6 {
            let c = self.stops[0].color;
            return (c.r as f32, c.g as f32, c.b as f32, c.a as f32);
        }

        let mut t = ((p.x - self.start.x) * dx + (p.y - self.start.y) * dy) / len_sq;
        if t.is_nan() || t.is_infinite() {
            let c = self.stops[0].color;
            return (c.r as f32, c.g as f32, c.b as f32, c.a as f32);
        }

        t = match self.spread {
            SpreadMode::Pad => t.clamp(0.0, 1.0),
            SpreadMode::Repeat => t.fract(),
            SpreadMode::Reflect => {
                let m = t.abs() % 2.0;
                if m > 1.0 {
                    2.0 - m
                } else {
                    m
                }
            }
        };

        if t <= self.stops[0].position {
            let c = self.stops[0].color;
            return (c.r as f32, c.g as f32, c.b as f32, c.a as f32);
        }
        let last = self.stops.len() - 1;
        if t >= self.stops[last].position {
            let c = self.stops[last].color;
            return (c.r as f32, c.g as f32, c.b as f32, c.a as f32);
        }

        if self.stops.len() == 2 && self.stops[0].position == 0.0 && self.stops[1].position == 1.0 {
            let s0 = &self.stops[0];
            let s1 = &self.stops[1];
            let factor = t;
            let inv = 1.0 - factor;
            let interp_l = s0.oklab.l * inv + s1.oklab.l * factor;
            let interp_a = s0.oklab.a * inv + s1.oklab.a * factor;
            let interp_b = s0.oklab.b * inv + s1.oklab.b * factor;
            let (r, g, b) = oklab_to_srgb_float(interp_l, interp_a, interp_b);
            return (
                r,
                g,
                b,
                s0.color.a as f32 * inv + s1.color.a as f32 * factor,
            );
        }

        for i in 0..last {
            let s0 = &self.stops[i];
            let s1 = &self.stops[i + 1];
            if t >= s0.position && t <= s1.position {
                let range = s1.position - s0.position;
                let factor = if range > 1e-5 {
                    (t - s0.position) / range
                } else {
                    0.0
                };
                let inv = 1.0 - factor;
                let interp_l = s0.oklab.l * inv + s1.oklab.l * factor;
                let interp_a = s0.oklab.a * inv + s1.oklab.a * factor;
                let interp_b = s0.oklab.b * inv + s1.oklab.b * factor;
                let (r, g, b) = oklab_to_srgb_float(interp_l, interp_a, interp_b);
                return (
                    r,
                    g,
                    b,
                    s0.color.a as f32 * inv + s1.color.a as f32 * factor,
                );
            }
        }

        let c = self.stops[0].color;
        (c.r as f32, c.g as f32, c.b as f32, c.a as f32)
    }

    /// Sample the color of the gradient at (x, y) coordinates.
    pub fn color_at(&self, x: f32, y: f32) -> Color {
        let (r, g, b, a) = self.eval_at(x, y);
        Color {
            r: (r.clamp(0.0, 255.0) + 0.5) as u8,
            g: (g.clamp(0.0, 255.0) + 0.5) as u8,
            b: (b.clamp(0.0, 255.0) + 0.5) as u8,
            a: (a.clamp(0.0, 255.0) + 0.5) as u8,
        }
    }

    /// Pre-bakes a 257-entry continuous RGBA lookup table for this gradient.
    ///
    /// Building this takes ~10 microseconds, but allows inner raster loops
    /// to query colors via table lookup rather than per-pixel color matrix operations.
    #[inline]
    pub fn bake_lut(&self) -> [(f32, f32, f32, f32); 257] {
        let mut lut = [(0.0f32, 0.0f32, 0.0f32, 0.0f32); 257];
        if self.stops.is_empty() {
            return lut;
        }
        if self.stops.len() == 1 {
            let c = self.stops[0].color;
            lut.fill((c.r as f32, c.g as f32, c.b as f32, c.a as f32));
            return lut;
        }

        let first = self.stops[0];
        let last = self.stops[self.stops.len() - 1];

        // 2-stop fast path (covers >95% of UI gradients)
        if self.stops.len() == 2 && first.position == 0.0 && last.position == 1.0 {
            let s0 = &self.stops[0];
            let s1 = &self.stops[1];
            for i in 0..=256 {
                let factor = i as f32 / 256.0;
                let inv = 1.0 - factor;
                let interp_l = s0.oklab.l * inv + s1.oklab.l * factor;
                let interp_a = s0.oklab.a * inv + s1.oklab.a * factor;
                let interp_b = s0.oklab.b * inv + s1.oklab.b * factor;
                let (r, g, b) = oklab_to_srgb_float(interp_l, interp_a, interp_b);
                lut[i] = (r, g, b, s0.color.a as f32 * inv + s1.color.a as f32 * factor);
            }
            return lut;
        }

        for i in 0..=256 {
            let t = i as f32 / 256.0;
            if t <= first.position {
                let c = first.color;
                lut[i] = (c.r as f32, c.g as f32, c.b as f32, c.a as f32);
                continue;
            }
            if t >= last.position {
                let c = last.color;
                lut[i] = (c.r as f32, c.g as f32, c.b as f32, c.a as f32);
                continue;
            }

            for j in 0..self.stops.len() - 1 {
                let s0 = &self.stops[j];
                let s1 = &self.stops[j + 1];
                if t >= s0.position && t <= s1.position {
                    let range = s1.position - s0.position;
                    let factor = if range > 1e-5 {
                        (t - s0.position) / range
                    } else {
                        0.0
                    };
                    let inv = 1.0 - factor;
                    let interp_l = s0.oklab.l * inv + s1.oklab.l * factor;
                    let interp_a = s0.oklab.a * inv + s1.oklab.a * factor;
                    let interp_b = s0.oklab.b * inv + s1.oklab.b * factor;
                    let (r, g, b) = oklab_to_srgb_float(interp_l, interp_a, interp_b);
                    lut[i] = (r, g, b, s0.color.a as f32 * inv + s1.color.a as f32 * factor);
                    break;
                }
            }
        }
        lut
    }

    /// Fast lookup evaluation using pre-baked LUT.
    #[inline(always)]
    pub fn eval_at_lut(&self, x: f32, y: f32, lut: &[(f32, f32, f32, f32); 257]) -> (f32, f32, f32, f32) {
        let p = self.transform.map_point(Point::new(x, y));
        let dx = self.end.x - self.start.x;
        let dy = self.end.y - self.start.y;
        let len_sq = dx * dx + dy * dy;
        if len_sq <= 1e-6 {
            return lut[0];
        }

        let mut t = ((p.x - self.start.x) * dx + (p.y - self.start.y) * dy) / len_sq;
        if t.is_nan() || t.is_infinite() {
            return lut[0];
        }

        t = match self.spread {
            SpreadMode::Pad => t.clamp(0.0, 1.0),
            SpreadMode::Repeat => t.fract(),
            SpreadMode::Reflect => {
                let m = t.abs() % 2.0;
                if m > 1.0 {
                    2.0 - m
                } else {
                    m
                }
            }
        };

        let idx = (t * 256.0).clamp(0.0, 256.0).round() as usize;
        lut[idx]
    }

    /// Fast dithered evaluation using pre-baked LUT.
    #[inline(always)]
    pub fn dithered_at_lut(
        &self,
        x: f32,
        y: f32,
        px: i32,
        py: i32,
        lut: &[(f32, f32, f32, f32); 257],
    ) -> (u16, u8) {
        let (r, g, b, a) = self.eval_at_lut(x, y, lut);
        let col565 = crate::color::dither_float_to_rgb565(r, g, b, px, py);
        (col565, (a.clamp(0.0, 255.0) + 0.5) as u8)
    }

    /// Sample the gradient at (x, y) coordinates with Bayer 8x8 dithering directly from
    /// continuous floating-point color, preventing 8-bit intermediate quantization banding.
    ///
    /// Returns `(rgb565, alpha)`.
    #[inline(always)]
    pub fn dithered_at(&self, x: f32, y: f32, px: i32, py: i32) -> (u16, u8) {
        let (r, g, b, a) = self.eval_at(x, y);
        let col565 = crate::color::dither_float_to_rgb565(r, g, b, px, py);
        (col565, (a.clamp(0.0, 255.0) + 0.5) as u8)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Shader<'a> {
    SolidColor(Color),
    Linear(LinearGradient),
    #[doc(hidden)]
    _Phantom(core::marker::PhantomData<&'a ()>),
}

impl<'a> From<LinearGradient> for Shader<'a> {
    fn from(g: LinearGradient) -> Self {
        Shader::Linear(g)
    }
}

impl<'a> From<Color> for Shader<'a> {
    fn from(c: Color) -> Self {
        Shader::SolidColor(c)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stroke {
    pub width: f32,
    pub miter_limit: f32,
    pub line_cap: LineCap,
    pub line_join: LineJoin,
}

impl Default for Stroke {
    fn default() -> Self {
        Self {
            width: 1.0,
            miter_limit: 4.0,
            line_cap: LineCap::default(),
            line_join: LineJoin::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// kurbo stroke interoperability
// ---------------------------------------------------------------------------

impl From<LineCap> for kurbo::Cap {
    #[inline(always)]
    fn from(c: LineCap) -> Self {
        match c {
            LineCap::Butt => kurbo::Cap::Butt,
            LineCap::Round => kurbo::Cap::Round,
            LineCap::Square => kurbo::Cap::Square,
        }
    }
}

impl From<kurbo::Cap> for LineCap {
    #[inline(always)]
    fn from(c: kurbo::Cap) -> Self {
        match c {
            kurbo::Cap::Butt => LineCap::Butt,
            kurbo::Cap::Round => LineCap::Round,
            kurbo::Cap::Square => LineCap::Square,
        }
    }
}

impl From<LineJoin> for kurbo::Join {
    #[inline(always)]
    fn from(j: LineJoin) -> Self {
        match j {
            LineJoin::Miter => kurbo::Join::Miter,
            LineJoin::Round => kurbo::Join::Round,
            LineJoin::Bevel => kurbo::Join::Bevel,
        }
    }
}

impl From<kurbo::Join> for LineJoin {
    #[inline(always)]
    fn from(j: kurbo::Join) -> Self {
        match j {
            kurbo::Join::Miter => LineJoin::Miter,
            kurbo::Join::Round => LineJoin::Round,
            kurbo::Join::Bevel => LineJoin::Bevel,
        }
    }
}

impl From<&Stroke> for kurbo::Stroke {
    #[inline(always)]
    fn from(s: &Stroke) -> Self {
        kurbo::Stroke::new(s.width as f64)
            .with_caps(s.line_cap.into())
            .with_join(s.line_join.into())
            .with_miter_limit(s.miter_limit as f64)
    }
}

impl From<Stroke> for kurbo::Stroke {
    #[inline(always)]
    fn from(s: Stroke) -> Self {
        (&s).into()
    }
}

impl From<kurbo::Stroke> for Stroke {
    #[inline(always)]
    fn from(s: kurbo::Stroke) -> Self {
        Stroke {
            width: s.width as f32,
            miter_limit: s.miter_limit as f32,
            line_cap: s.start_cap.into(),
            line_join: s.join.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Paint<'a> {
    pub shader: Shader<'a>,
    pub anti_alias: bool,
}

impl Default for Paint<'static> {
    fn default() -> Self {
        Self::new(Color::BLACK)
    }
}

impl<'a> Paint<'a> {
    #[inline(always)]
    pub fn new(color: Color) -> Self {
        Self {
            shader: Shader::SolidColor(color),
            anti_alias: true,
        }
    }

    #[inline(always)]
    pub fn set_color(&mut self, color: Color) {
        self.shader = Shader::SolidColor(color);
    }

    #[inline(always)]
    pub fn set_color_rgba8(&mut self, r: u8, g: u8, b: u8, a: u8) {
        self.shader = Shader::SolidColor(Color::from_rgba(r, g, b, a));
    }
}
