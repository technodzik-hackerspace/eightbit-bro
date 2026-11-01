use serde::{Deserialize, Serialize};

use crate::{NesCanvas, Rect};

/// A region that must look like it did on a reference frame: which pixels are bright.
/// Brightness survives JPEG and the capture's sub-line shifts much better than colour.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Probe {
    pub rect: Rect,
    /// `bright_at` inside `rect`, row by row.
    pub bright: Vec<bool>,
}

impl Probe {
    pub fn capture(c: &NesCanvas, rect: Rect) -> Probe {
        let rect = rect.intersect(&NesCanvas::full());
        let bright = (rect.y1..rect.y2)
            .flat_map(|y| (rect.x1..rect.x2).map(move |x| (x, y)))
            .map(|(x, y)| c.bright_at(x, y))
            .collect();
        Probe { rect, bright }
    }

    /// Share of differing pixels with the probe moved `dy` lines (0..1; 1 if off the canvas).
    pub fn mismatch_at(&self, c: &NesCanvas, dy: i32) -> f32 {
        let r = &self.rect;
        let (y1, y2) = (r.y1 as i32 + dy, r.y2 as i32 + dy);
        if y1 < 0 || y2 > crate::H as i32 || self.bright.is_empty() {
            return 1.0;
        }
        let mut want = self.bright.iter();
        let mut differ = 0usize;
        for y in y1 as u32..y2 as u32 {
            for x in r.x1..r.x2 {
                differ += (*want.next().unwrap() != c.bright_at(x, y)) as usize;
            }
        }
        differ as f32 / self.bright.len() as f32
    }

    /// The best mismatch over vertical shifts of up to `max_dy` lines: some screens scroll
    /// by a line between frames.
    pub fn mismatch(&self, c: &NesCanvas, max_dy: i32) -> f32 {
        (-max_dy..=max_dy).map(|dy| self.mismatch_at(c, dy)).fold(1.0, f32::min)
    }
}
