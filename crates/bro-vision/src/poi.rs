use std::collections::HashMap;

use crate::palette::dist;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Palette, Probe, fixtures};

    // On the fixture (896×720, 3.5 × 3 capture px per NES pixel) the "SCORE" label is canvas
    // rows 16..24, x 107..147: the crop isn't tile-calibrated yet, so the letters sit ~3 px
    // right of the tile grid. Drawn a bit loosely around the text, on the black.
    const SCORE_DRAWN: Rect = Rect {
        x1: 365,
        y1: 48,
        x2: 516,
        y2: 72,
    };

    #[test]
    fn trims_and_snaps_to_tiles() {
        let (_, geo, canvas) = fixtures::tetris_1p();
        let poi = optimize_poi(&canvas, &geo, SCORE_DRAWN).unwrap();
        let drawn = geo.to_canvas(SCORE_DRAWN);
        assert_eq!(
            poi.x1 % 8 + poi.y1 % 8 + poi.x2 % 8 + poi.y2 % 8,
            0,
            "{poi:?}"
        );
        assert!(
            poi.area() <= drawn.snap_out(8).area(),
            "{poi:?} vs drawn {drawn:?}"
        );
        // One tile row high; 5 letters span 6 tiles while the crop is off the grid.
        assert_eq!(poi, Rect::new(104, 16, 152, 24));
    }

    #[test]
    fn outside_the_picture_is_none() {
        let (_, _, canvas) = fixtures::tetris_1p();
        let geo = Geometry::MS2109_720P;
        assert_eq!(optimize_poi(&canvas, &geo, Rect::new(0, 0, 150, 700)), None);
    }

    #[test]
    fn uniform_region_is_kept() {
        let (_, geo, canvas) = fixtures::tetris_1p();
        // Inside the empty middle well.
        let poi = optimize_poi(&canvas, &geo, Rect::new(400, 300, 480, 400)).unwrap();
        assert_eq!(
            poi,
            geo.to_canvas(Rect::new(400, 300, 480, 400)).snap_out(8)
        );
    }

    #[test]
    fn probe_from_poi_tells_labels_apart() {
        let (img, geo, canvas) = fixtures::tetris_1p();
        let palette = Palette::learn(canvas.rgb.iter(), 20, 40);
        let canvas = NesCanvas::sample(&img, &geo, Some(&palette));

        let score = optimize_poi(&canvas, &geo, SCORE_DRAWN).unwrap();
        let probe = Probe::capture(&canvas, score);
        assert!(probe.matches(&canvas, 0.1));

        // The same probe one tile row down sits on "LINES".
        let lines = Rect {
            y1: score.y1 + 16,
            y2: score.y2 + 16,
            ..score
        };
        let moved = Probe {
            rect: lines,
            px: probe.px.clone(),
        };
        assert!(!moved.matches(&canvas, 0.1));
    }
}
