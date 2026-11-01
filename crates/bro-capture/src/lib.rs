//! Where frames come from, and the ring buffer that shares them (tech design §3.1, §3.2).

mod dir;
mod ring;

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use image::{ImageFormat, RgbImage};

pub use dir::DirSource;
pub use ring::Ring;

#[derive(Debug, Clone)]
pub struct RawFrame {
    /// Monotonically increasing from the start of the source.
    pub no: u64,
    /// Capture time: the V4L2 buffer time, or synthetic for files (PRD I2).
    pub ts: Duration,
    pub data: FrameData,
}

/// Frames stay compressed until someone needs pixels.
#[derive(Debug, Clone)]
pub enum FrameData {
    Jpeg(Arc<[u8]>),
    /// v1 captures.
    Png(Arc<[u8]>),
}

impl RawFrame {
    pub fn decode(&self) -> Result<RgbImage> {
        let (bytes, format) = match &self.data {
            FrameData::Jpeg(b) => (b, ImageFormat::Jpeg),
            FrameData::Png(b) => (b, ImageFormat::Png),
        };
        Ok(image::load_from_memory_with_format(bytes, format)?.to_rgb8())
    }
}

pub trait Source: Send {
    /// The next frame, or `None` at the end of a file source.
    fn next(&mut self) -> Result<Option<RawFrame>>;
}
