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
    /// Panics unless `geo.fits(img.width(), img.height())`.
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
        let dark = self.rgb.iter().filter(|c| c.iter().all(|&v| v < 48)).count();
        dark * 100 >= self.rgb.len() * 98
    }

    /// Perceptual hash (dHash): the canvas shrunk to 9×8 brightness cells, one bit per pair of
    /// neighbours. Near-identical frames differ in a few bits (see `hamming`).
    pub fn dhash(&self) -> u64 {
        let mut cells = [0u32; 9 * 8];
        for y in 0..H {
            for x in 0..W {
                let [r, g, b] = self.rgb[y * W + x];
                cells[y * 8 / H * 9 + x * 9 / W] += r as u32 * 3 + g as u32 * 6 + b as u32;
            }
        }
        let mut hash = 0u64;
        for row in cells.chunks(9) {
            for pair in row.windows(2) {
                hash = hash << 1 | (pair[0] < pair[1]) as u64;
            }
        }
        hash
    }

    pub fn to_image(&self) -> RgbImage {
        RgbImage::from_fn(W as u32, H as u32, |x, y| Rgb(self.rgb_at(x, y)))
    }

    pub fn full() -> Rect {
        Rect::new(0, 0, W as u32, H as u32)
    }
}

/// Number of differing bits between two hashes.
pub fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
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
    fn dhash_tells_frames_apart_but_not_noise() {
        let (img, geo, canvas) = fixtures::tetris_1p();
        let mut noisy = img.clone();
        for (i, p) in noisy.pixels_mut().enumerate() {
            p.0[0] = p.0[0].saturating_add((i % 7) as u8);
        }
        let noisy = NesCanvas::sample(&noisy, &geo, None);
        assert!(hamming(canvas.dhash(), noisy.dhash()) <= 2);

        let mut gradient = RgbImage::new(896, 720);
        for (x, _, p) in gradient.enumerate_pixels_mut() {
            *p = Rgb([(x / 4) as u8, 0, 0]);
        }
        let other = NesCanvas::sample(&gradient, &geo, None);
        assert!(hamming(canvas.dhash(), other.dhash()) > 16);
    }

    #[test]
    fn tetris_is_not_black() {
        let (_, _, canvas) = fixtures::tetris_1p();
        assert!(!canvas.is_black());
    }
}
