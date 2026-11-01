use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};

use crate::{FrameData, RawFrame, Source};

/// A folder of `*.jpg` / `*.jpeg` / `*.png` frames, in file-name order, with timestamps
/// `no / fps` so runs are reproducible.
pub struct DirSource {
    files: Vec<(PathBuf, bool)>,
    pos: usize,
    fps: u32,
}

impl DirSource {
    pub fn open(dir: &Path, fps: u32) -> Result<DirSource> {
        let mut files = Vec::new();
        for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
            let path = entry?.path();
            let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
            match ext.as_deref() {
                Some("jpg" | "jpeg") => files.push((path, false)),
                Some("png") => files.push((path, true)),
                _ => {}
            }
        }
        files.sort();
        Ok(DirSource { files, pos: 0, fps })
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

impl Source for DirSource {
    fn next(&mut self) -> Result<Option<RawFrame>> {
        let Some((path, png)) = self.files.get(self.pos) else {
            return Ok(None);
        };
        let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let no = self.pos as u64;
        self.pos += 1;
        let data = if *png { FrameData::Png(bytes.into()) } else { FrameData::Jpeg(bytes.into()) };
        let ts = Duration::from_nanos(no * 1_000_000_000 / self.fps as u64);
        Ok(Some(RawFrame { no, ts, data }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_frames_in_name_order_with_synthetic_time() {
        let dir = std::env::temp_dir().join(format!("bro-dir-source-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        for name in ["b.png", "a.jpg", "notes.txt", "c.JPEG"] {
            fs::write(dir.join(name), name).unwrap();
        }
        let mut src = DirSource::open(&dir, 60).unwrap();
        assert_eq!(src.len(), 3);

        let mut seen = Vec::new();
        while let Some(f) = src.next().unwrap() {
            let bytes = match &f.data {
                FrameData::Jpeg(b) | FrameData::Png(b) => b.clone(),
            };
            seen.push((f.no, f.ts.as_nanos(), String::from_utf8(bytes.to_vec()).unwrap()));
        }
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(
            seen,
            vec![
                (0, 0, "a.jpg".into()),
                (1, 16_666_666, "b.png".into()),
                (2, 33_333_333, "c.JPEG".into()),
            ]
        );
    }
}
