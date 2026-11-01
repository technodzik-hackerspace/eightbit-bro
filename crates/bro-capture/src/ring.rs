use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::RawFrame;

/// The last few seconds of frames, shared without copying by capture (writer), perception,
/// the recorder and the dashboard preview. The lock is only held to push or look up.
pub struct Ring {
    frames: Mutex<VecDeque<Arc<RawFrame>>>,
    capacity: usize,
}

impl Ring {
    pub fn new(capacity: usize) -> Ring {
        Ring { frames: Mutex::new(VecDeque::with_capacity(capacity)), capacity }
    }

    /// Adds a frame, evicting the oldest when full. Frame numbers must increase.
    pub fn push(&self, frame: RawFrame) -> Arc<RawFrame> {
        let frame = Arc::new(frame);
        let mut frames = self.frames.lock().unwrap();
        if frames.len() == self.capacity {
            frames.pop_front();
        }
        frames.push_back(frame.clone());
        frame
    }

    /// Frame `no`, if it is still in the ring (and wasn't lost by capture).
    pub fn get(&self, no: u64) -> Option<Arc<RawFrame>> {
        let frames = self.frames.lock().unwrap();
        let i = frames.binary_search_by_key(&no, |f| f.no).ok()?;
        Some(frames[i].clone())
    }

    pub fn latest(&self) -> Option<Arc<RawFrame>> {
        self.frames.lock().unwrap().back().cloned()
    }

    pub fn oldest_no(&self) -> Option<u64> {
        self.frames.lock().unwrap().front().map(|f| f.no)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::FrameData;

    fn frame(no: u64) -> RawFrame {
        RawFrame { no, ts: Duration::ZERO, data: FrameData::Jpeg(Arc::from(&[][..])) }
    }

    #[test]
    fn keeps_the_newest_frames() {
        let ring = Ring::new(3);
        for no in [0, 1, 2, 4, 5] {
            ring.push(frame(no));
        }
        assert_eq!(ring.oldest_no(), Some(2));
        assert_eq!(ring.latest().unwrap().no, 5);
        assert_eq!(ring.get(4).unwrap().no, 4);
        assert!(ring.get(3).is_none(), "lost by capture");
        assert!(ring.get(1).is_none(), "evicted");
    }
}
