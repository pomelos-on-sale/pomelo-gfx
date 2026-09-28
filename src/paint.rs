use crate::color::Color;
use crate::geometry::{Point, Transform};

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

#[derive(Debug, Clone, PartialEq)]
pub struct GradientStop {
    pub position: f32,
    pub color: Color,
}

impl GradientStop {
    pub fn new(position: f32, color: Color) -> Self {
        Self { position, color }
    }
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

    /// Sample the color of the gradient at (x, y) coordinates.
    pub fn color_at(&self, x: f32, y: f32) -> Color {
        let p = self.transform.map_point(Point::new(x, y));
        let dx = self.end.x - self.start.x;
        let dy = self.end.y - self.start.y;
        let len_sq = dx * dx + dy * dy;
        if len_sq <= 1e-6 {
            return self.stops[0].color;
        }

        let mut t = ((p.x - self.start.x) * dx + (p.y - self.start.y) * dy) / len_sq;

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
            return self.stops[0].color;
        }
        let last = self.stops.len() - 1;
        if t >= self.stops[last].position {
            return self.stops[last].color;
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
                return Color {
                    r: (s0.color.r as f32 * inv + s1.color.r as f32 * factor + 0.5) as u8,
                    g: (s0.color.g as f32 * inv + s1.color.g as f32 * factor + 0.5) as u8,
                    b: (s0.color.b as f32 * inv + s1.color.b as f32 * factor + 0.5) as u8,
                    a: (s0.color.a as f32 * inv + s1.color.a as f32 * factor + 0.5) as u8,
                };
            }
        }

        self.stops[0].color
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
