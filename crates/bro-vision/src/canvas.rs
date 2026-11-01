use image::{Rgb, RgbImage};

use crate::{Geometry, Palette, Rect};

pub const W: usize = 256;
pub const H: usize = 240;
/// Palette index of a pixel that matched no palette colour well enough (JPEG ringing at
/// colour edges). Matchers ignore it.
pub const UNSURE: u8 = 0xFF;

/// The frame mapped back to the console's 256×240 pixels (tech design §3.3).
#[derive(Clone)]
pub struct NesCanvas {
    /// Palette index per pixel, `UNSURE` when unsure or when sampled without a palette.
    pub px: Box<[u8]>,
    /// The sampled colours.
    pub rgb: Box<[[u8; 3]]>,
}

impl NesCanvas {
    /// Samples the centre of every NES pixel. Without a palette only `rgb` is filled.
    pub fn sample(img: &RgbImage, geo: &Geometry, palette: Option<&Palette>) -> Self {
        let mut rgb = vec![[0u8; 3]; W * H].into_boxed_slice();
        for ny in 0..H as u32 {
            for nx in 0..W as u32 {
                let (x, y) = geo.sample_point(nx, ny);
                rgb[ny as usize * W + nx as usize] = img.get_pixel(x, y).0;
            }
        }
        let px = match palette {
            Some(p) => rgb.iter().map(|c| p.quantize(*c)).collect(),
            None => vec![UNSURE; W * H].into_boxed_slice(),
        };
        NesCanvas { px, rgb }
    }

    pub fn rgb_at(&self, x: u32, y: u32) -> [u8; 3] {
        self.rgb[y as usize * W + x as usize]
    }

    pub fn px_at(&self, x: u32, y: u32) -> u8 {
        self.px[y as usize * W + x as usize]
    }

    /// Almost every pixel is black (PRD R4).
    pub fn is_black(&self) -> bool {
        let dark = self
            .rgb
            .iter()
            .filter(|c| c.iter().all(|&v| v < 48))
            .count();
        dark * 100 >= self.rgb.len() * 98
    }

    pub fn to_image(&self) -> RgbImage {
        RgbImage::from_fn(W as u32, H as u32, |x, y| Rgb(self.rgb_at(x, y)))
    }

    pub fn full() -> Rect {
        Rect::new(0, 0, W as u32, H as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;

    #[test]
    fn black_frame_is_black() {
        let img = RgbImage::from_pixel(1280, 720, Rgb([8, 6, 10]));
        assert!(NesCanvas::sample(&img, &Geometry::MS2109_720P, None).is_black());
    }

    #[test]
    fn tetris_is_not_black() {
        let (_, _, canvas) = fixtures::tetris_1p();
        assert!(!canvas.is_black());
    }
}
