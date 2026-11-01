use anyhow::Result;
use image::RgbImage;
use turbojpeg::{Decompressor, Image, PixelFormat, ScalingFactor};

use crate::{FrameData, RawFrame};

/// Decodes frames for perception. JPEGs are decoded at ½ scale with libjpeg-turbo's DCT
/// scaling: a quarter of the pixels, still 1.75 × 1.5 px per NES pixel at 720p
/// (tech design §3.3).
pub struct Decoder {
    tj: Decompressor,
}

impl Decoder {
    pub fn new() -> Result<Decoder> {
        let mut tj = Decompressor::new()?;
        tj.set_scaling_factor(ScalingFactor::ONE_HALF)?;
        Ok(Decoder { tj })
    }

    /// The pixels and the factor they are scaled down by (2 for JPEG, 1 for PNG).
    pub fn decode(&mut self, frame: &RawFrame) -> Result<(RgbImage, u32)> {
        let FrameData::Jpeg(jpeg) = &frame.data else {
            return Ok((frame.decode()?, 1));
        };
        let header = self.tj.read_header(jpeg)?.scaled(ScalingFactor::ONE_HALF);
        let (width, height) = (header.width, header.height);
        let mut pixels = vec![0u8; 3 * width * height];
        let image = Image {
            pixels: &mut pixels[..],
            width,
            pitch: 3 * width,
            height,
            format: PixelFormat::RGB,
        };
        self.tj.decompress(jpeg, image)?;
        let img = RgbImage::from_raw(width as u32, height as u32, pixels).expect("buffer size");
        Ok((img, 2))
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use bro_vision::{Geometry, NesCanvas, hamming};

    use super::*;

    #[test]
    fn half_scale_canvas_matches_full_decode() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/frames/ms2109_720p_tetris_demo.jpg"
        );
        let jpeg = std::fs::read(path).unwrap();
        let frame = RawFrame { no: 0, ts: Duration::ZERO, data: FrameData::Jpeg(jpeg.into()) };
        let geo = Geometry::MS2109_720P;

        let full = NesCanvas::sample(&frame.decode().unwrap(), &geo, None);
        let mut decoder = Decoder::new().unwrap();
        let started = Instant::now();
        let (img, factor) = decoder.decode(&frame).unwrap();
        eprintln!("½-scale decode: {:?}", started.elapsed());
        assert_eq!((img.width(), img.height(), factor), (640, 360, 2));
        let half = NesCanvas::sample(&img, &geo.scaled_down(factor), None);

        assert!(hamming(full.dhash(), half.dhash()) <= 2);
        let close = full
            .rgb
            .iter()
            .zip(half.rgb.iter())
            .filter(|(a, b)| a.iter().zip(b.iter()).all(|(x, y)| x.abs_diff(*y) <= 40));
        // Both decodes blur colour edges (4:2:2 chroma), differently: ~94% agree on this
        // busy screen, the rest are edge pixels.
        assert!(close.count() * 100 >= full.rgb.len() * 90);
    }
}
