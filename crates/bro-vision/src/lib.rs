//! From captured frames to the NES canvas, and the matching primitives recognizers use
//! (tech design §3.3, §3.6).

mod canvas;
mod geometry;
mod palette;
mod poi;
mod probe;

pub use canvas::{H, NesCanvas, UNSURE, W};
pub use geometry::{Geometry, Rect};
pub use palette::Palette;
pub use poi::optimize_poi;
pub use probe::Probe;

#[cfg(test)]
pub(crate) mod fixtures {
    use image::RgbImage;

    use crate::{Geometry, NesCanvas, Rect};

    /// Tetris 1P demo from the MS2109 at 720p, already cropped to the console picture.
    pub fn tetris_1p() -> (RgbImage, Geometry, NesCanvas) {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/frames/tetris_1p_demo_crop_896x720.png"
        );
        let img = image::open(path).unwrap().to_rgb8();
        let geo = Geometry {
            picture: Rect::new(0, 0, 896, 720),
        };
        let canvas = NesCanvas::sample(&img, &geo, None);
        (img, geo, canvas)
    }
}
