//! The eightbit-bro service. Takes one argument: the config file (PRD I3).

mod config;
mod pipeline;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use bro_capture::{DirSource, FfmpegSource, Ring, Source};
use bro_heart::{Observation, Seen};
use bro_store::{ReviewConfig, Store};
use bro_web::Live;
use tracing::{info, warn};

use crate::config::{Config, InputKind};
use crate::pipeline::Pipeline;

/// The ring holds this much of the capture (tech design §3.2).
const RING_SECONDS: usize = 4;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stdout()))
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let mut args = std::env::args_os().skip(1);
    let (Some(path), None) = (args.next(), args.next()) else {
        bail!("usage: eightbit-bro <config.toml>");
    };
    let (config, config_text) = Config::load(&PathBuf::from(path))?;
    let review = ReviewConfig { max_items: config.review.max_items, ..Default::default() };
    let store = Arc::new(Store::open(&config.data_dir, review)?);
    let ring = Arc::new(Ring::new(RING_SECONDS * config.input.fps as usize));

    let (live, worker) = match config.input.kind()? {
        InputKind::Device(device) => {
            let live = Arc::new(Live::new(Some(ring.clone()), device.display().to_string()));
            let pipeline = Pipeline::new(config.geometry, store.clone(), live.clone())?;
            (live.clone(), spawn_live(device.to_owned(), config.input.fps, ring, pipeline, live))
        }
        InputKind::Dir(dir) => {
            let source = DirSource::open(dir, config.input.fps)?;
            let live = Arc::new(Live::new(Some(ring.clone()), dir.display().to_string()));
            let pipeline = Pipeline::new(config.geometry, store.clone(), live.clone())?;
            (live.clone(), spawn_replay(Box::new(source), ring, pipeline, live))
        }
        InputKind::Video(video) => {
            let source = FfmpegSource::video(video, config.input.fps)?;
            let live = Arc::new(Live::new(Some(ring.clone()), video.display().to_string()));
            let pipeline = Pipeline::new(config.geometry, store.clone(), live.clone())?;
            (live.clone(), spawn_replay(Box::new(source), ring, pipeline, live))
        }
    };

    let Some(web) = config.web else {
        return worker.join().expect("pipeline thread panicked");
    };
    let ctx = bro_web::Context {
        store,
        live,
        geometry: config.geometry,
        data_dir: config.data_dir.clone(),
        config_text,
    };
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?
        .block_on(bro_web::serve(&web.listen, ctx))
}

/// Files: every frame, in order, as fast as possible; the result is deterministic.
fn spawn_replay(
    mut source: Box<dyn Source>,
    ring: Arc<Ring>,
    mut pipeline: Pipeline,
    live: Arc<Live>,
) -> thread::JoinHandle<Result<()>> {
    thread::spawn(move || {
        let started = Instant::now();
        while let Some(frame) = source.next().context("reading the next frame")? {
            live.stats.captured.fetch_add(1, Ordering::Relaxed);
            let frame = ring.push(frame);
            pipeline.process(&frame);
        }
        let frames = live.stats.processed.load(Ordering::Relaxed);
        info!(frames, secs = started.elapsed().as_secs_f64(), "replay done");
        Ok(())
    })
}

/// The capture card: capture never waits for perception (tech design §1). Perception takes
/// the newest frame and skips what it can't keep up with.
fn spawn_live(
    device: PathBuf,
    fps: u32,
    ring: Arc<Ring>,
    mut pipeline: Pipeline,
    live: Arc<Live>,
) -> thread::JoinHandle<Result<()>> {
    let (wake, woken) = mpsc::sync_channel::<()>(1);
    {
        let (ring, live) = (ring.clone(), live.clone());
        thread::Builder::new()
            .name("capture".into())
            .spawn(move || capture(&device, fps, &ring, &live, &wake))
            .expect("spawning the capture thread");
    }
    thread::spawn(move || {
        let mut last: Option<u64> = None;
        let mut last_frame_at = Instant::now();
        let mut signal = true;
        loop {
            match woken.recv_timeout(Duration::from_secs(1)) {
                Ok(()) => {
                    let Some(frame) = ring.latest() else { continue };
                    if last.is_some_and(|l| frame.no <= l) {
                        continue;
                    }
                    if let Some(l) = last {
                        live.stats.skipped.fetch_add(frame.no - l - 1, Ordering::Relaxed);
                    }
                    last = Some(frame.no);
                    last_frame_at = Instant::now();
                    signal = true;
                    pipeline.process(&frame);
                }
                // No frames for a second: the signal is gone (tech design §3.1).
                Err(RecvTimeoutError::Timeout)
                    if signal && last_frame_at.elapsed() >= Duration::from_secs(1) =>
                {
                    signal = false;
                    let frame_no = last.map_or(0, |l| l + 1);
                    pipeline.observe(Observation { frame_no, seen: Seen::SignalLost });
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => bail!("capture thread stopped"),
            }
        }
    })
}

/// Reads the device forever, re-opening it after errors (it re-enumerates, v1's bug B8).
/// Frame numbers keep increasing across re-opens.
fn capture(device: &Path, fps: u32, ring: &Ring, live: &Live, wake: &mpsc::SyncSender<()>) {
    let mut next_no = 0u64;
    let mut failures = 0u32;
    loop {
        let result = FfmpegSource::device(device, fps).and_then(|mut source| {
            while let Some(mut frame) = source.next()? {
                frame.no = next_no;
                next_no += 1;
                failures = 0;
                live.stats.captured.fetch_add(1, Ordering::Relaxed);
                ring.push(frame);
                let _ = wake.try_send(());
            }
            Ok(())
        });
        live.stats.capture_errors.fetch_add(1, Ordering::Relaxed);
        if failures.is_multiple_of(30) {
            match result {
                Ok(()) => warn!(device = %device.display(), "capture ended, re-opening"),
                Err(err) => {
                    warn!(device = %device.display(), err = format!("{err:#}"), "capture failed, retrying")
                }
            }
        }
        failures += 1;
        thread::sleep(Duration::from_secs(2));
    }
}
