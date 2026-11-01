use serde::{Deserialize, Serialize};

use crate::{NesCanvas, Rect, UNSURE};

/// A region of the canvas that must look exactly like it did on a reference frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Probe {
    pub rect: Rect,
    /// Palette indices inside `rect`, row by row.
    pub px: Vec<u8>,
}

impl Probe {
    pub fn capture(c: &NesCanvas, rect: Rect) -> Probe {
        let rect = rect.intersect(&NesCanvas::full());
        let px = (rect.y1..rect.y2)
            .flat_map(|y| (rect.x1..rect.x2).map(move |x| (x, y)))
            .map(|(x, y)| c.px_at(x, y))
            .collect();
        Probe { rect, px }
    }

    /// Every pixel that is sure on both sides matches, and at most `max_unsure` (0..1) of
    /// the pixels are unsure (PRD R1: 100% or nothing).
    pub fn matches(&self, c: &NesCanvas, max_unsure: f32) -> bool {
        let r = &self.rect;
        let mut unsure = 0usize;
        let mut want = self.px.iter();
        for y in r.y1..r.y2 {
            for x in r.x1..r.x2 {
                let (want, got) = (*want.next().unwrap(), c.px_at(x, y));
                if want == UNSURE || got == UNSURE {
                    unsure += 1;
                } else if want != got {
                    return false;
                }
            }
        }
        unsure as f32 <= max_unsure * self.px.len() as f32
    }
}
