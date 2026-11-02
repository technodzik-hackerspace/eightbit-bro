use std::io::Read;
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::{FrameData, RawFrame, Source};

/// Frames from an `ffmpeg` child writing a JPEG stream to stdout: the capture card's own MJPEG
/// copied without re-encoding, or a video file re-encoded to JPEG.
pub struct FfmpegSource {
    child: Child,
    stdout: ChildStdout,
    splitter: JpegSplitter,
    no: u64,
    clock: Clock,
}

enum Clock {
    /// Live: time of arrival.
    Live(Instant),
    /// Files: `no / fps`, so runs are reproducible (PRD I2).
    Synthetic(u32),
}

impl FfmpegSource {
    /// The MS2109 in its chosen mode: MJPEG 1280×720 at `fps` (60, 50 or 30; tech design §2).
    pub fn device(path: &Path, fps: u32) -> Result<FfmpegSource> {
        let fps = fps.to_string();
        let args =
            ["-f", "v4l2", "-input_format", "mjpeg", "-video_size", "1280x720", "-framerate"];
        let args = args.into_iter().chain([fps.as_str(), "-i"]).map(Into::into);
        let args: Vec<_> = args.chain([path.as_os_str().to_owned()]).collect();
        Self::spawn(&args, &["-c:v", "copy"], Clock::Live(Instant::now()))
    }

    pub fn video(path: &Path, fps: u32) -> Result<FfmpegSource> {
        let args = ["-i".into(), path.as_os_str().to_owned()];
        Self::spawn(&args, &["-c:v", "mjpeg", "-q:v", "2"], Clock::Synthetic(fps))
    }

    fn spawn(input: &[std::ffi::OsString], codec: &[&str], clock: Clock) -> Result<FfmpegSource> {
        let mut child = Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-nostdin"])
            .args(input)
            .args(codec)
            .args(["-f", "mjpeg", "pipe:1"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()
            .context("starting ffmpeg")?;
        let stdout = child.stdout.take().unwrap();
        Ok(FfmpegSource { child, stdout, splitter: JpegSplitter::default(), no: 0, clock })
    }
}

impl Source for FfmpegSource {
    fn next(&mut self) -> Result<Option<RawFrame>> {
        let mut buf = [0u8; 64 * 1024];
        loop {
            if let Some(jpeg) = self.splitter.next_frame() {
                let no = self.no;
                self.no += 1;
                let ts = match self.clock {
                    Clock::Live(start) => start.elapsed(),
                    Clock::Synthetic(fps) => Duration::from_nanos(no * 1_000_000_000 / fps as u64),
                };
                return Ok(Some(RawFrame { no, ts, data: FrameData::Jpeg(jpeg.into()) }));
            }
            let n = self.stdout.read(&mut buf).context("reading from ffmpeg")?;
            if n == 0 {
                let status = self.child.wait()?;
                return match self.clock {
                    Clock::Synthetic(_) if status.success() => Ok(None),
                    _ => bail!("ffmpeg exited: {status}"),
                };
            }
            self.splitter.push(&buf[..n]);
        }
    }
}

impl Drop for FfmpegSource {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Splits a byte stream of concatenated JPEGs at the start (FF D8) and end (FF D9) markers.
/// Entropy-coded data never contains FF D9 (byte stuffing), so the first end marker after a
/// start closes the frame.
#[derive(Default)]
pub struct JpegSplitter {
    buf: Vec<u8>,
}

impl JpegSplitter {
    pub fn push(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    pub fn next_frame(&mut self) -> Option<Vec<u8>> {
        let start = find(&self.buf, &[0xFF, 0xD8])?;
        let end = start + 2 + find(&self.buf[start + 2..], &[0xFF, 0xD9])? + 2;
        let frame = self.buf[start..end].to_vec();
        self.buf.drain(..end);
        Some(frame)
    }
}

fn find(hay: &[u8], needle: &[u8; 2]) -> Option<usize> {
    hay.windows(2).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_frames_across_reads() {
        let stream = [
            &[0x00, 0xFF, 0xD8, 1, 2][..],
            &[3, 0xFF][..],
            &[0xD9, 0xFF, 0xD8, 4, 0xFF, 0xD9, 0xFF][..],
        ];
        let mut s = JpegSplitter::default();
        let mut frames = Vec::new();
        for chunk in stream {
            s.push(chunk);
            while let Some(f) = s.next_frame() {
                frames.push(f);
            }
        }
        assert_eq!(
            frames,
            vec![vec![0xFF, 0xD8, 1, 2, 3, 0xFF, 0xD9], vec![0xFF, 0xD8, 4, 0xFF, 0xD9],]
        );
    }
}
