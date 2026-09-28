#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    #[inline(always)]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    #[inline(always)]
    pub fn from_xy(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    #[inline(always)]
    pub fn distance(&self, other: Point) -> f32 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        (dx * dx + dy * dy).sqrt()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Size {
    pub const ZERO: Self = Self {
        width: 0.0,
        height: 0.0,
    };

    #[inline(always)]
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    };

    #[inline(always)]
    pub const fn from_xywh(x: f32, y: f32, width: f32, height: f32) -> Option<Self> {
        if width < 0.0 || height < 0.0 {
            None
        } else {
            Some(Self {
                x,
                y,
                width,
                height,
            })
        }
    }

    #[inline(always)]
    pub const fn from_ltwh(left: f32, top: f32, width: f32, height: f32) -> Self {
        Self {
            x: left,
            y: top,
            width: if width < 0.0 { 0.0 } else { width },
            height: if height < 0.0 { 0.0 } else { height },
        }
    }

    #[inline(always)]
    pub fn from_ltrb(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        let w = (right - left).max(0.0);
        let h = (bottom - top).max(0.0);
        Self {
            x: left,
            y: top,
            width: w,
            height: h,
        }
    }

    #[inline(always)]
    pub fn left(&self) -> f32 {
        self.x
    }

    #[inline(always)]
    pub fn top(&self) -> f32 {
        self.y
    }

    #[inline(always)]
    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    #[inline(always)]
    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    #[inline(always)]
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x < self.right() && p.y >= self.y && p.y < self.bottom()
    }

    #[inline(always)]
    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        let left = self.x.max(other.x);
        let top = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());

        if right > left && bottom > top {
            Some(Rect {
                x: left,
                y: top,
                width: right - left,
                height: bottom - top,
            })
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Radius {
    pub x: f32,
    pub y: f32,
}

impl Radius {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    #[inline(always)]
    pub const fn circular(radius: f32) -> Self {
        Self {
            x: radius,
            y: radius,
        }
    }

    #[inline(always)]
    pub const fn elliptical(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RRect {
    pub rect: Rect,
    pub radius: Radius,
}

impl RRect {
    #[inline(always)]
    pub const fn from_rect_radius(rect: Rect, radius: Radius) -> Self {
        Self { rect, radius }
    }

    #[inline(always)]
    pub const fn from_rect_xy(rect: Rect, rx: f32, ry: f32) -> Self {
        Self {
            rect,
            radius: Radius { x: rx, y: ry },
        }
    }
}

/// 2D Affine Transform matrix:
/// [ sx  kx  tx ]
/// [ ky  sy  ty ]
/// [  0   0   1 ]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub sx: f32,
    pub ky: f32,
    pub kx: f32,
    pub sy: f32,
    pub tx: f32,
    pub ty: f32,
}

impl Default for Transform {
    fn default() -> Self {
        Self::identity()
    }
}

impl Transform {
    #[inline(always)]
    pub const fn identity() -> Self {
        Self {
            sx: 1.0,
            ky: 0.0,
            kx: 0.0,
            sy: 1.0,
            tx: 0.0,
            ty: 0.0,
        }
    }

    #[inline(always)]
    pub const fn from_row(sx: f32, ky: f32, kx: f32, sy: f32, tx: f32, ty: f32) -> Self {
        Self {
            sx,
            ky,
            kx,
            sy,
            tx,
            ty,
        }
    }

    #[inline(always)]
    pub const fn from_translate(tx: f32, ty: f32) -> Self {
        Self {
            sx: 1.0,
            ky: 0.0,
            kx: 0.0,
            sy: 1.0,
            tx,
            ty,
        }
    }

    #[inline(always)]
    pub const fn from_scale(sx: f32, sy: f32) -> Self {
        Self {
            sx,
            ky: 0.0,
            kx: 0.0,
            sy,
            tx: 0.0,
            ty: 0.0,
        }
    }

    #[inline(always)]
    pub fn post_translate(&self, dx: f32, dy: f32) -> Self {
        Self {
            sx: self.sx,
            ky: self.ky,
            kx: self.kx,
            sy: self.sy,
            tx: self.tx + dx,
            ty: self.ty + dy,
        }
    }

    #[inline(always)]
    pub fn post_scale(&self, sx: f32, sy: f32) -> Self {
        Self {
            sx: self.sx * sx,
            ky: self.ky * sy,
            kx: self.kx * sx,
            sy: self.sy * sy,
            tx: self.tx * sx,
            ty: self.ty * sy,
        }
    }

    #[inline(always)]
    pub fn map_point(&self, p: Point) -> Point {
        Point {
            x: self.sx * p.x + self.kx * p.y + self.tx,
            y: self.ky * p.x + self.sy * p.y + self.ty,
        }
    }

    /// Map an axis-aligned rectangle.
    ///
    /// Skew is ignored, which is the model both rasterizers draw under (they
    /// only ever apply scale and translate).
    #[inline(always)]
    pub fn map_rect(&self, rect: Rect) -> Rect {
        Rect {
            x: self.sx * rect.x + self.tx,
            y: self.sy * rect.y + self.ty,
            width: self.sx * rect.width,
            height: self.sy * rect.height,
        }
    }

    /// Map a rounded rectangle: the box through [`Self::map_rect`], the corner
    /// radii by the scale factors.
    #[inline(always)]
    pub fn map_rrect(&self, rrect: RRect) -> RRect {
        RRect {
            rect: self.map_rect(rrect.rect),
            radius: Radius {
                x: self.sx * rrect.radius.x,
                y: self.sy * rrect.radius.y,
            },
        }
    }
}
