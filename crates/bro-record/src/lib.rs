//! Recording (tech design §4). The ffmpeg encoder comes next; the timeline decides which
//! captured frame goes into each output frame.

mod timeline;

pub use timeline::{FrameStats, Timeline};
