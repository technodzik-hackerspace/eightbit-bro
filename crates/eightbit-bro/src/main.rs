//! The eightbit-bro service. Takes one argument: the config file (PRD I3).

mod config;

use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use bro_capture::{DirSource, Source};
use bro_games::Perception;
use bro_heart::{ConsoleState, EventKind, Observation, reduce};
use bro_vision::NesCanvas;
use tracing::{info, warn};

use crate::config::Config;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let mut args = std::env::args_os().skip(1);
    let (Some(path), None) = (args.next(), args.next()) else {
        bail!("usage: eightbit-bro <config.toml>");
    };
    let config = Config::load(&PathBuf::from(path))?;
    run(&config)
}

fn run(config: &Config) -> Result<()> {
    let mut source = DirSource::open(&config.input.dir, config.input.fps)?;
    info!(frames = source.len(), dir = %config.input.dir.display(), "replaying folder");
    let perception = Perception::standard();

    let started = Instant::now();
    let mut state = ConsoleState::NoSignal;
    let (mut frames, mut corrupt) = (0u64, 0u64);
    while let Some(frame) = source.next().context("reading the next frame")? {
        frames += 1;
        // Corrupt frames are counted and skipped; they never change the state (PRD Q9).
        let img = match frame.decode() {
            Ok(img) => img,
            Err(err) => {
                corrupt += 1;
                warn!(frame = frame.no, %err, "corrupt frame");
                continue;
            }
        };
        let canvas = NesCanvas::sample(&img, &config.geometry, None);
        let hint = match &state {
            ConsoleState::InGame { game, .. } => Some(*game),
            _ => None,
        };
        let obs = Observation {
            frame_no: frame.no,
            seen: perception.perceive(&canvas, hint),
        };
        let (next, events) = reduce(&state, &obs);
        for event in events {
            match event.kind {
                EventKind::Changed { from, to } => {
                    info!(frame = event.frame_no, %from, %to, "state")
                }
                EventKind::UnknownScreen { state } => {
                    warn!(frame = event.frame_no, %state, "unknown screen")
                }
            }
        }
        state = next;
    }

    let secs = started.elapsed().as_secs_f64();
    info!(frames, corrupt, secs, fps = frames as f64 / secs, state = %state.path(), "done");
    Ok(())
}
