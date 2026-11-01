use serde::{Deserialize, Serialize};

use crate::canvas::{H, W};

/// A half-open rectangle: `x1 <= x < x2`, `y1 <= y < y2`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rect {
    pub x1: u32,
    pub y1: u32,
    pub x2: u32,
    pub y2: u32,
}

impl Rect {
    /// Normalizes the corners, so a rectangle drawn from any corner works.
    pub fn new(x1: u32, y1: u32, x2: u32, y2: u32) -> Self {
        Rect { x1: x1.min(x2), y1: y1.min(y2), x2: x1.max(x2), y2: y1.max(y2) }
    }

    pub fn width(&self) -> u32 {
        self.x2 - self.x1
    }

    pub fn height(&self) -> u32 {
        self.y2 - self.y1
    }

    pub fn area(&self) -> u32 {
        self.width() * self.height()
    }

    pub fn is_empty(&self) -> bool {
        self.area() == 0
    }

    pub fn intersect(&self, other: &Rect) -> Rect {
        let x1 = self.x1.max(other.x1);
        let y1 = self.y1.max(other.y1);
        Rect { x1, y1, x2: self.x2.min(other.x2).max(x1), y2: self.y2.min(other.y2).max(y1) }
    }

    pub fn contains(&self, other: &Rect) -> bool {
        self.x1 <= other.x1 && self.y1 <= other.y1 && other.x2 <= self.x2 && other.y2 <= self.y2
    }

    /// Grows the rectangle outwards to multiples of `n`.
    pub fn snap_out(&self, n: u32) -> Rect {
        Rect {
            x1: self.x1 / n * n,
            y1: self.y1 / n * n,
            x2: self.x2.div_ceil(n) * n,
            y2: self.y2.div_ceil(n) * n,
        }
    }
}

/// Where the console picture is in the captured frame (tech design §2: at 720p it is
/// pillarboxed at x = 192..1088).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Geometry {
    pub picture: Rect,
}

impl Geometry {
    pub const MS2109_720P: Geometry =
        Geometry { picture: Rect { x1: 192, y1: 0, x2: 1088, y2: 720 } };

    /// The same picture in a frame decoded at `1/factor` scale.
    pub fn scaled_down(&self, factor: u32) -> Geometry {
        let p = &self.picture;
        Geometry { picture: Rect::new(p.x1 / factor, p.y1 / factor, p.x2 / factor, p.y2 / factor) }
    }

    /// Whether a frame of this size contains the whole picture.
    pub fn fits(&self, width: u32, height: u32) -> bool {
        self.picture.x2 <= width && self.picture.y2 <= height && !self.picture.is_empty()
    }

    /// The capture pixel at the centre of NES pixel `(nx, ny)`.
    pub fn sample_point(&self, nx: u32, ny: u32) -> (u32, u32) {
        let p = &self.picture;
        (
            p.x1 + (2 * nx + 1) * p.width() / (2 * W as u32),
            p.y1 + (2 * ny + 1) * p.height() / (2 * H as u32),
        )
    }

    /// Capture rectangle → canvas rectangle, rounded outwards and clipped to the picture.
    pub fn to_canvas(&self, r: Rect) -> Rect {
        let p = &self.picture;
        let r = r.intersect(p);
        let (w, h) = (p.width(), p.height());
        let (cw, ch) = (W as u32, H as u32);
        Rect {
            x1: (r.x1 - p.x1) * cw / w,
            y1: (r.y1 - p.y1) * ch / h,
            x2: ((r.x2 - p.x1) * cw).div_ceil(w),
            y2: ((r.y2 - p.y1) * ch).div_ceil(h),
        }
    }

    /// Canvas rectangle → capture rectangle covering it, for drawing over the frame.
    pub fn to_capture(&self, r: Rect) -> Rect {
        let p = &self.picture;
        let (w, h) = (p.width(), p.height());
        let (cw, ch) = (W as u32, H as u32);
        Rect {
            x1: p.x1 + r.x1 * w / cw,
            y1: p.y1 + r.y1 * h / ch,
            x2: p.x1 + (r.x2 * w).div_ceil(cw),
            y2: p.y1 + (r.y2 * h).div_ceil(ch),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_from_any_corner() {
        assert_eq!(Rect::new(10, 20, 1, 2), Rect::new(1, 2, 10, 20));
    }

    #[test]
    fn snap_out_to_tiles() {
        assert_eq!(Rect::new(9, 15, 17, 16).snap_out(8), Rect::new(8, 8, 24, 16));
        assert_eq!(Rect::new(8, 8, 16, 16).snap_out(8), Rect::new(8, 8, 16, 16));
    }

    #[test]
    fn ms2109_720p_maps_three_lines_per_nes_line() {
        let g = Geometry::MS2109_720P;
        assert_eq!(g.sample_point(0, 0), (193, 1));
        assert_eq!(g.sample_point(255, 239), (1086, 718));
        assert_eq!(g.to_canvas(Rect::new(192, 0, 1088, 720)), Rect::new(0, 0, 256, 240));
        // Outside the picture (pillarbox) is clipped away.
        assert_eq!(g.to_canvas(Rect::new(0, 0, 199, 6)), Rect::new(0, 0, 2, 2));
    }

    #[test]
    fn canvas_to_capture_covers_the_canvas_rect() {
        let g = Geometry::MS2109_720P;
        let c = Rect::new(8, 16, 24, 32);
        let cap = g.to_capture(c);
        assert_eq!(cap, Rect::new(220, 48, 276, 96));
        assert_eq!(g.to_canvas(cap), c);
    }
}
