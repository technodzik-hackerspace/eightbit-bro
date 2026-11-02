//! From captured frames to the NES canvas, and the matching primitives recognizers use
//! (tech design §3.3, §3.6).

mod canvas;
mod geometry;
mod poi;
mod probe;

pub use canvas::{H, NesCanvas, W, hamming, luma};
pub use geometry::{Geometry, Rect};
pub use poi::optimize_poi;
pub use probe::{Ink, Probe};

#[cfg(test)]
pub(crate) mod fixtures {
    use image::RgbImage;

    use crate::{Geometry, NesCanvas};

    /// Tetris 1P demo, as the MS2109 captures it at 720p.
    pub fn tetris() -> (RgbImage, Geometry, NesCanvas) {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/frames/ms2109_720p_tetris_demo.jpg"
        );
        let img = image::open(path).unwrap().to_rgb8();
        let geo = Geometry::MS2109_720P;
        let canvas = NesCanvas::sample(&img, &geo);
        (img, geo, canvas)
    }
}
