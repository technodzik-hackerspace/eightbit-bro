use std::collections::HashMap;

use crate::{Geometry, NesCanvas, Rect};

/// Content differs from the background by more than this per channel.
const CONTENT_DIST: u8 = 40;
/// NES backgrounds and text are drawn in 8×8 tiles.
const TILE: u32 = 8;

/// Turns a point of interest the owner drew on a frame (capture pixels) into the canvas
/// region a probe should check (tech design §3.6): mapped to the canvas, trimmed to the
/// content inside it, and snapped outwards to the 8×8 tile grid. `None` when the drawn
/// rectangle is outside the console picture.
pub fn optimize_poi(c: &NesCanvas, geo: &Geometry, drawn: Rect) -> Option<Rect> {
    let region = geo.to_canvas(drawn);
    if region.is_empty() {
        return None;
    }
    let pixels =
        || (region.y1..region.y2).flat_map(move |y| (region.x1..region.x2).map(move |x| (x, y)));

    let bg = background(pixels().map(|(x, y)| c.rgb_at(x, y)));
    let mut content: Option<Rect> = None;
    for (x, y) in pixels().filter(|&(x, y)| dist(c.rgb_at(x, y), bg) > CONTENT_DIST) {
        let r = content.get_or_insert(Rect::new(x, y, x + 1, y + 1));
        *r = Rect::new(r.x1.min(x), r.y1.min(y), r.x2.max(x + 1), r.y2.max(y + 1));
    }
    // A uniform region is a signature too: keep it as drawn.
    let trimmed = content.unwrap_or(region);
    Some(trimmed.snap_out(TILE).intersect(&NesCanvas::full()))
}

/// The most common colour, by coarse bins so JPEG noise doesn't split it.
fn background(colors: impl Iterator<Item = [u8; 3]>) -> [u8; 3] {
    let mut bins: HashMap<[u8; 3], (u32, [u32; 3])> = HashMap::new();
    for c in colors {
        let bin = bins.entry(c.map(|v| v >> 4)).or_default();
        bin.0 += 1;
        for (s, v) in bin.1.iter_mut().zip(c) {
            *s += v as u32;
        }
    }
    let (_, (n, sum)) = bins.into_iter().max_by_key(|(k, (n, _))| (*n, *k)).unwrap();
    sum.map(|s| (s / n) as u8)
}

/// Largest per-channel difference.
fn dist(a: [u8; 3], b: [u8; 3]) -> u8 {
    (0..3).map(|i| a[i].abs_diff(b[i])).max().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Probe, fixtures};

    // The "SCORE" label of the Tetris HUD, drawn a bit loosely on the black between the grey
    // separators. Tetris draws its HUD 3 px right of the tile grid, so 5 letters span 6 tiles.
    const SCORE_DRAWN: Rect = Rect { x1: 566, y1: 48, x2: 707, y2: 72 };

    #[test]
    fn trims_and_snaps_to_tiles() {
        let (_, geo, canvas) = fixtures::tetris();
        let poi = optimize_poi(&canvas, &geo, SCORE_DRAWN).unwrap();
        assert_eq!(poi, Rect::new(104, 16, 152, 24));
        assert!(poi.area() <= geo.to_canvas(SCORE_DRAWN).snap_out(8).area());
    }

    #[test]
    fn outside_the_picture_is_none() {
        let (_, geo, canvas) = fixtures::tetris();
        assert_eq!(optimize_poi(&canvas, &geo, Rect::new(0, 0, 150, 700)), None);
    }

    #[test]
    fn uniform_region_is_kept() {
        let (_, geo, canvas) = fixtures::tetris();
        // Inside the empty middle well.
        let well = Rect::new(600, 300, 680, 400);
        assert_eq!(optimize_poi(&canvas, &geo, well).unwrap(), geo.to_canvas(well).snap_out(8));
    }

    #[test]
    fn probe_from_poi_tells_labels_apart() {
        let (_, geo, canvas) = fixtures::tetris();
        let score = optimize_poi(&canvas, &geo, SCORE_DRAWN).unwrap();
        let probe = Probe::capture(&canvas, score);
        assert_eq!(probe.mismatch(&canvas, 0), 0.0);
        // Two tile rows down is "LINES".
        assert!(probe.mismatch_at(&canvas, 16) > 0.2);
    }
}
