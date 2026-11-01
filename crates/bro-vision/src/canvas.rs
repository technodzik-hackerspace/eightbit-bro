use image::{Rgb, RgbImage};

use crate::{Geometry, Rect};

pub const W: usize = 256;
pub const H: usize = 240;

/// A run of fully black lines at least this long makes a frame "blanked" (see `is_blanked`).
const BLANK_LINES: usize = 48;

/// The frame mapped back to the console's 256×240 pixels (tech design §3.3).
#[derive(Clone)]
pub struct NesCanvas {
    /// The colour sampled at the centre of each console pixel, row by row.
    pub rgb: Box<[[u8; 3]]>,
}

impl NesCanvas {
    /// Samples the centre of every console pixel. Panics unless
    /// `geo.fits(img.width(), img.height())`.
    pub fn sample(img: &RgbImage, geo: &Geometry) -> Self {
        let mut rgb = vec![[0u8; 3]; W * H].into_boxed_slice();
        for ny in 0..H as u32 {
            for nx in 0..W as u32 {
                let (x, y) = geo.sample_point(nx, ny);
                rgb[ny as usize * W + nx as usize] = img.get_pixel(x, y).0;
            }
        }
        NesCanvas { rgb }
    }

    pub fn rgb_at(&self, x: u32, y: u32) -> [u8; 3] {
        self.rgb[y as usize * W + x as usize]
    }

    /// Brightness (BT.601 luma). JPEG keeps it at full resolution, unlike colour (4:2:2), so
    /// shapes are matched on it.
    pub fn luma_at(&self, x: u32, y: u32) -> u8 {
        luma(self.rgb_at(x, y))
    }

    /// Bright pixel: text, outlines. The mask probes and the menu font compare these.
    pub fn bright_at(&self, x: u32, y: u32) -> bool {
        self.luma_at(x, y) > BRIGHT
    }

    /// Almost every pixel is black (PRD R4).
    pub fn is_black(&self) -> bool {
        let dark = self.rgb.iter().filter(|c| is_dark(c)).count();
        dark * 100 >= self.rgb.len() * 98
    }

    /// Black, or partly black: a long run of fully black lines. Games switch rendering off
    /// while they load the next screen, and a frame caught while rendering switches on or off
    /// partway down shows part picture, part black. Treated like black frames (PRD R4).
    pub fn is_blanked(&self) -> bool {
        let mut run = 0;
        for line in self.rgb.chunks(W) {
            run = if line.iter().all(is_dark) { run + 1 } else { 0 };
            if run >= BLANK_LINES {
                return true;
            }
        }
        self.is_black()
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

/// Luma above this is "bright".
const BRIGHT: u8 = 150;

pub fn luma([r, g, b]: [u8; 3]) -> u8 {
    ((r as u32 * 299 + g as u32 * 587 + b as u32 * 114) / 1000) as u8
}

fn is_dark(c: &[u8; 3]) -> bool {
    c.iter().all(|&v| v < 48)
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
        let canvas = NesCanvas::sample(&img, &Geometry::MS2109_720P);
        assert!(canvas.is_black() && canvas.is_blanked());
    }

    #[test]
    fn half_black_frame_is_blanked_not_black() {
        let (mut img, geo, _) = fixtures::tetris();
        for y in 0..300 {
            for x in 0..1280 {
                img.put_pixel(x, y, Rgb([4, 4, 4]));
            }
        }
        let canvas = NesCanvas::sample(&img, &geo);
        assert!(canvas.is_blanked() && !canvas.is_black());
    }

    #[test]
    fn tetris_is_neither() {
        let (_, _, canvas) = fixtures::tetris();
        assert!(!canvas.is_black() && !canvas.is_blanked());
    }

    #[test]
    fn dhash_tells_frames_apart_but_not_noise() {
        let (img, geo, canvas) = fixtures::tetris();
        let mut noisy = img.clone();
        for (i, p) in noisy.pixels_mut().enumerate() {
            p.0[0] = p.0[0].saturating_add((i % 7) as u8);
        }
        let noise = hamming(canvas.dhash(), NesCanvas::sample(&noisy, &geo).dhash());
        // Within the review queue's "same screen" distance.
        assert!(noise <= 6, "{noise}");

        let mut gradient = RgbImage::new(1280, 720);
        for (x, _, p) in gradient.enumerate_pixels_mut() {
            *p = Rgb([(x / 5) as u8, 0, 0]);
        }
        let other = hamming(canvas.dhash(), NesCanvas::sample(&gradient, &geo).dhash());
        assert!(other > 16, "{other}");
    }
}
