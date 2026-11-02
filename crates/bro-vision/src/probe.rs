use serde::{Deserialize, Serialize};

use crate::{NesCanvas, Rect};

/// Which pixels count as drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ink {
    /// Luma above 150: white and yellow text on a coloured background (the menu).
    Bright,
    /// Any channel above 100: anything on a black background (Tetris), including red and
    /// dim blue text.
    Lit,
    /// Battle City's dark grey (63, 63, 63) ±22, neutral: its frame, the eagle, the flag.
    Grey,
}

impl Ink {
    pub fn at(self, c: &NesCanvas, x: u32, y: u32) -> bool {
        match self {
            Ink::Bright => c.bright_at(x, y),
            Ink::Lit => c.rgb_at(x, y).iter().any(|&v| v > 100),
            Ink::Grey => {
                let px = c.rgb_at(x, y);
                let (lo, hi) = (*px.iter().min().unwrap(), *px.iter().max().unwrap());
                lo >= 41 && hi <= 85 && hi - lo < 16
            }
        }
    }
}

/// A region that must look like it did on a reference frame: which pixels are drawn.
/// Shapes survive JPEG and the capture's sub-pixel shifts much better than colours.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Probe {
    pub rect: Rect,
    pub ink: Ink,
    /// `ink.at` inside `rect`, row by row.
    pub mask: Vec<bool>,
}

impl Probe {
    pub fn capture(c: &NesCanvas, rect: Rect, ink: Ink) -> Probe {
        let rect = rect.intersect(&NesCanvas::full());
        let mask = (rect.y1..rect.y2)
            .flat_map(|y| (rect.x1..rect.x2).map(move |x| (x, y)))
            .map(|(x, y)| ink.at(c, x, y))
            .collect();
        Probe { rect, ink, mask }
    }

    /// Share of drawn pixels in the reference: a probe of empty background would match any
    /// empty background.
    pub fn ink_share(&self) -> f32 {
        self.mask.iter().filter(|&&m| m).count() as f32 / self.mask.len().max(1) as f32
    }

    /// Share of differing pixels with the probe moved by `(dx, dy)` (0..1; 1 off the canvas).
    pub fn mismatch_at(&self, c: &NesCanvas, dx: i32, dy: i32) -> f32 {
        let r = &self.rect;
        let (x1, y1) = (r.x1 as i32 + dx, r.y1 as i32 + dy);
        let (x2, y2) = (r.x2 as i32 + dx, r.y2 as i32 + dy);
        if x1 < 0 || y1 < 0 || x2 > crate::W as i32 || y2 > crate::H as i32 || self.mask.is_empty()
        {
            return 1.0;
        }
        let mut want = self.mask.iter();
        let mut differ = 0usize;
        for y in y1 as u32..y2 as u32 {
            for x in x1 as u32..x2 as u32 {
                differ += (*want.next().unwrap() != self.ink.at(c, x, y)) as usize;
            }
        }
        differ as f32 / self.mask.len() as f32
    }

    /// The best mismatch over shifts of up to `max_shift` pixels in each direction: the
    /// picture moves by a pixel or a line between screens and between captures.
    pub fn mismatch(&self, c: &NesCanvas, max_shift: i32) -> f32 {
        let shifts = -max_shift..=max_shift;
        shifts
            .clone()
            .flat_map(|dy| shifts.clone().map(move |dx| (dx, dy)))
            .map(|(dx, dy)| self.mismatch_at(c, dx, dy))
            .fold(1.0, f32::min)
    }
}
