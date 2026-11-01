use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameStats {
    /// Frames offered to the timeline outside pauses.
    pub captured: u64,
    /// Output frames.
    pub output: u64,
    /// Output frames that repeat the previous one because no new frame arrived (gaps).
    pub duplicated: u64,
    /// Frames not used because a newer one won the tick (every other one at 60 → 30 fps).
    pub unused: u64,
    /// Frames cut out as paused.
    pub paused: u64,
}

/// Turns captured frames into a constant-frame-rate output (tech design §4.2): each output
/// tick takes the newest frame at or before it, repeating the previous one over gaps. Paused
/// frames are cut out and the time after a pause is shifted back by its length, so a pause
/// leaves neither duplicates nor a jump.
pub struct Timeline {
    fps: u64,
    t0: Option<Duration>,
    tick: u64,
    /// Newest frame for the current tick, and whether it was output already.
    current: Option<(u64, bool)>,
    pause_started: Option<Duration>,
    shift: Duration,
    pub stats: FrameStats,
}

impl Timeline {
    pub fn new(fps: u32) -> Timeline {
        Timeline {
            fps: fps as u64,
            t0: None,
            tick: 0,
            current: None,
            pause_started: None,
            shift: Duration::ZERO,
            stats: FrameStats::default(),
        }
    }

    /// Offers a captured frame; returns the frame numbers to output, in order.
    pub fn push(&mut self, no: u64, ts: Duration, paused: bool) -> Vec<u64> {
        if paused {
            self.pause_started.get_or_insert(ts);
            self.stats.paused += 1;
            return Vec::new();
        }
        if let Some(start) = self.pause_started.take() {
            self.shift += ts.saturating_sub(start);
        }
        let ts = ts.saturating_sub(self.shift);
        let t0 = *self.t0.get_or_insert(ts);
        self.stats.captured += 1;

        let mut out = Vec::new();
        while ts > self.tick_time(t0) {
            self.emit(&mut out);
            self.tick += 1;
        }
        if let Some((_, false)) = self.current {
            self.stats.unused += 1;
        }
        self.current = Some((no, false));
        out
    }

    /// Outputs the last frame, if it wasn't yet.
    pub fn finish(&mut self) -> Vec<u64> {
        let mut out = Vec::new();
        if let Some((_, false)) = self.current {
            self.emit(&mut out);
        }
        out
    }

    fn tick_time(&self, t0: Duration) -> Duration {
        t0 + Duration::from_nanos(self.tick * 1_000_000_000 / self.fps)
    }

    fn emit(&mut self, out: &mut Vec<u64>) {
        let Some((no, used)) = &mut self.current else {
            return;
        };
        if *used {
            self.stats.duplicated += 1;
        }
        *used = true;
        self.stats.output += 1;
        out.push(*no);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at_60(no: u64) -> Duration {
        Duration::from_nanos(no * 1_000_000_000 / 60)
    }

    fn run(frames: impl IntoIterator<Item = (u64, Duration, bool)>) -> (Vec<u64>, FrameStats) {
        let mut t = Timeline::new(30);
        let mut out: Vec<u64> = frames
            .into_iter()
            .flat_map(|(no, ts, p)| t.push(no, ts, p))
            .collect();
        out.extend(t.finish());
        (out, t.stats)
    }

    #[test]
    fn sixty_to_thirty_takes_every_other_frame() {
        let (out, stats) = run((0..600).map(|n| (n, at_60(n), false)));
        assert_eq!(&out[..4], &[0, 2, 4, 6]);
        assert_eq!(out[299], 598);
        assert_eq!(stats.duplicated, 0);
        assert_eq!(stats.output + stats.unused, 600);
    }

    #[test]
    fn jitter_does_not_duplicate() {
        // ±2 ms of jitter on every frame.
        let jitter = |n: u64| {
            if n.is_multiple_of(2) {
                at_60(n) + Duration::from_millis(2)
            } else {
                at_60(n).saturating_sub(Duration::from_millis(2))
            }
        };
        let (out, stats) = run((0..600).map(|n| (n, jitter(n), false)));
        assert_eq!(stats.duplicated, 0);
        assert!((300..=301).contains(&out.len()), "{}", out.len());
    }

    #[test]
    fn capture_gap_repeats_the_last_frame() {
        let (out, stats) = run((0..120)
            .filter(|n| !(40..60).contains(n))
            .map(|n| (n, at_60(n), false)));
        assert_eq!(out.len(), 61, "real time is kept");
        assert_eq!(stats.duplicated, 9);
        assert_eq!(
            &out[19..31],
            &[38, 39, 39, 39, 39, 39, 39, 39, 39, 39, 39, 60]
        );
    }

    #[test]
    fn pause_is_cut_out_without_duplicates() {
        let (out, stats) = run((0..240).map(|n| (n, at_60(n), (60..180).contains(&n))));
        assert_eq!(stats.paused, 120);
        assert_eq!(stats.duplicated, 0);
        assert_eq!(out.len(), 61, "like 120 continuous frames");
        assert!(
            out.contains(&58) && out.contains(&180) && !out.iter().any(|n| (60..180).contains(n))
        );
    }
}
