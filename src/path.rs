use crate::geometry::{Point, Rect};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathVerb {
    MoveTo(Point),
    LineTo(Point),
    QuadTo(Point, Point),
    CubicTo(Point, Point, Point),
    Close,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Path {
    pub(crate) verbs: Vec<PathVerb>,
}

impl Path {
    pub fn new() -> Self {
        Self { verbs: Vec::new() }
    }

    pub fn is_empty(&self) -> bool {
        self.verbs.is_empty()
    }

    /// Flatten this path into one or more polylines of line segments.
    pub fn flatten(&self, tolerance: f32) -> Vec<Vec<Point>> {
        let mut polylines = Vec::new();
        let mut current_poly = Vec::new();
        let mut start_pt = Point::ZERO;
        let mut current_pt = Point::ZERO;
        let tol_sq = (tolerance.max(0.1)).powi(2);

        for verb in &self.verbs {
            match *verb {
                PathVerb::MoveTo(p) => {
                    if !current_poly.is_empty() {
                        polylines.push(std::mem::take(&mut current_poly));
                    }
                    start_pt = p;
                    current_pt = p;
                    current_poly.push(p);
                }
                PathVerb::LineTo(p) => {
                    current_pt = p;
                    current_poly.push(p);
                }
                PathVerb::QuadTo(p1, p2) => {
                    subdivide_quad(current_pt, p1, p2, tol_sq, 0, &mut current_poly);
                    current_pt = p2;
                }
                PathVerb::CubicTo(p1, p2, p3) => {
                    subdivide_cubic(current_pt, p1, p2, p3, tol_sq, 0, &mut current_poly);
                    current_pt = p3;
                }
                PathVerb::Close => {
                    if current_pt != start_pt {
                        current_poly.push(start_pt);
                        current_pt = start_pt;
                    }
                    if !current_poly.is_empty() {
                        polylines.push(std::mem::take(&mut current_poly));
                    }
                }
            }
        }

        if !current_poly.is_empty() {
            polylines.push(current_poly);
        }

        polylines
    }
}

fn subdivide_quad(
    p0: Point,
    p1: Point,
    p2: Point,
    tol_sq: f32,
    depth: usize,
    out: &mut Vec<Point>,
) {
    if depth > 8 || point_line_dist_sq(p1, p0, p2) <= tol_sq {
        out.push(p2);
        return;
    }
    let m01 = midpoint(p0, p1);
    let m12 = midpoint(p1, p2);
    let m012 = midpoint(m01, m12);

    subdivide_quad(p0, m01, m012, tol_sq, depth + 1, out);
    subdivide_quad(m012, m12, p2, tol_sq, depth + 1, out);
}

fn subdivide_cubic(
    p0: Point,
    p1: Point,
    p2: Point,
    p3: Point,
    tol_sq: f32,
    depth: usize,
    out: &mut Vec<Point>,
) {
    let d1 = point_line_dist_sq(p1, p0, p3);
    let d2 = point_line_dist_sq(p2, p0, p3);
    if depth > 10 || (d1 <= tol_sq && d2 <= tol_sq) {
        out.push(p3);
        return;
    }

    let m01 = midpoint(p0, p1);
    let m12 = midpoint(p1, p2);
    let m23 = midpoint(p2, p3);

    let m012 = midpoint(m01, m12);
    let m123 = midpoint(m12, m23);

    let m0123 = midpoint(m012, m123);

    subdivide_cubic(p0, m01, m012, m0123, tol_sq, depth + 1, out);
    subdivide_cubic(m0123, m123, m23, p3, tol_sq, depth + 1, out);
}

#[inline(always)]
fn midpoint(a: Point, b: Point) -> Point {
    Point {
        x: (a.x + b.x) * 0.5,
        y: (a.y + b.y) * 0.5,
    }
}

#[inline(always)]
fn point_line_dist_sq(p: Point, a: Point, b: Point) -> f32 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let len_sq = dx * dx + dy * dy;
    if len_sq < 1e-6 {
        let px = p.x - a.x;
        let py = p.y - a.y;
        return px * px + py * py;
    }
    let cross = (p.x - a.x) * dy - (p.y - a.y) * dx;
    (cross * cross) / len_sq
}

#[derive(Debug, Clone, Default)]
pub struct PathBuilder {
    verbs: Vec<PathVerb>,
}

impl PathBuilder {
    pub fn new() -> Self {
        Self { verbs: Vec::new() }
    }

    pub fn move_to(&mut self, x: f32, y: f32) {
        self.verbs.push(PathVerb::MoveTo(Point::new(x, y)));
    }

    pub fn line_to(&mut self, x: f32, y: f32) {
        self.verbs.push(PathVerb::LineTo(Point::new(x, y)));
    }

    pub fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.verbs
            .push(PathVerb::QuadTo(Point::new(x1, y1), Point::new(x, y)));
    }

    pub fn cubic_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.verbs.push(PathVerb::CubicTo(
            Point::new(x1, y1),
            Point::new(x2, y2),
            Point::new(x, y),
        ));
    }

    pub fn close(&mut self) {
        self.verbs.push(PathVerb::Close);
    }

    pub fn push_rect(&mut self, rect: Rect) {
        self.move_to(rect.x, rect.y);
        self.line_to(rect.right(), rect.y);
        self.line_to(rect.right(), rect.bottom());
        self.line_to(rect.x, rect.bottom());
        self.close();
    }

    pub fn push_circle(&mut self, cx: f32, cy: f32, radius: f32) {
        let k = radius * 0.55228475;
        self.move_to(cx + radius, cy);
        self.cubic_to(cx + radius, cy + k, cx + k, cy + radius, cx, cy + radius);
        self.cubic_to(cx - k, cy + radius, cx - radius, cy + k, cx - radius, cy);
        self.cubic_to(cx - radius, cy - k, cx - k, cy - radius, cx, cy - radius);
        self.cubic_to(cx + k, cy - radius, cx + radius, cy - k, cx + radius, cy);
        self.close();
    }

    pub fn finish(self) -> Option<Path> {
        if self.verbs.is_empty() {
            None
        } else {
            Some(Path { verbs: self.verbs })
        }
    }
}
