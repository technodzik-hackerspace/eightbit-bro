//! One frame through perception, the reducer, the event log and the review queue.

use std::io::Cursor;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use anyhow::Result;
use bro_capture::{FrameData, RawFrame};
use bro_games::Perception;
use bro_heart::{ConsoleState, EventKind, Observation, Seen, reduce};
use bro_store::{Offer, Store};
use bro_vision::{Geometry, NesCanvas};
use bro_web::{Live, Push};
use image::ImageFormat;
use tracing::{debug, info, warn};

pub struct Pipeline {
    perception: Perception,
    geometry: Geometry,
    state: ConsoleState,
    store: Arc<Store>,
    live: Arc<Live>,
}

impl Pipeline {
    pub fn new(geometry: Geometry, store: Arc<Store>, live: Arc<Live>) -> Pipeline {
        live.set_state(&ConsoleState::NoSignal);
        let perception = Perception::standard();
        Pipeline { perception, geometry, state: ConsoleState::NoSignal, store, live }
    }

    pub fn process(&mut self, frame: &RawFrame) {
        let started = Instant::now();
        let live = self.live.clone();
        let stats = &live.stats;
        // Corrupt frames are counted and skipped; they never change the state (PRD Q9).
        let img = match frame.decode() {
            Ok(img) => img,
            Err(err) => {
                stats.corrupt.fetch_add(1, Ordering::Relaxed);
                debug!(frame = frame.no, %err, "corrupt frame");
                return;
            }
        };
        if !self.geometry.fits(img.width(), img.height()) {
            stats.corrupt.fetch_add(1, Ordering::Relaxed);
            debug!(
                frame = frame.no,
                w = img.width(),
                h = img.height(),
                "frame smaller than the picture"
            );
            return;
        }
        let canvas = NesCanvas::sample(&img, &self.geometry, None);
        let hint = match &self.state {
            ConsoleState::InGame { game, .. } => Some(*game),
            _ => None,
        };
        let seen = self.perception.perceive(&canvas, hint);
        let unrecognized = seen == Seen::Unrecognized;
        self.observe(Observation { frame_no: frame.no, seen });
        if unrecognized && let Err(err) = self.review(frame, &canvas) {
            warn!(frame = frame.no, %err, "review queue");
        }
        stats.processed.fetch_add(1, Ordering::Relaxed);
        stats.perceive_us.fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);
    }

    pub fn observe(&mut self, obs: Observation) {
        let (next, events) = reduce(&self.state, &obs);
        for event in events {
            if let EventKind::Changed { from, to } = &event.kind {
                info!(frame = event.frame_no, %from, %to, "state");
            }
            self.record(event.frame_no, &event.kind);
        }
        if next != self.state {
            self.live.set_state(&next);
        }
        self.state = next;
    }

    fn record(&self, frame_no: u64, kind: &EventKind) {
        match self.store.insert_event(frame_no, &self.state.path(), kind) {
            Ok(row) => self.live.push(Push::Event(row)),
            Err(err) => warn!(%err, "storing event"),
        }
    }

    fn review(&self, frame: &RawFrame, canvas: &NesCanvas) -> Result<()> {
        let jpeg = match &frame.data {
            FrameData::Jpeg(bytes) => bytes.to_vec(),
            FrameData::Png(_) => encode(&frame.decode()?, ImageFormat::Jpeg)?,
        };
        let state = self.state.path();
        let offer = self.store.review_offer(canvas.dhash(), frame.no, &state, &jpeg, || {
            encode(&canvas.to_image(), ImageFormat::Png)
        })?;
        if let Offer::New(item) = offer {
            warn!(frame = frame.no, item, %state, "unknown screen");
            self.record(frame.no, &EventKind::UnknownScreen { item, state });
            self.live.push(Push::Review { id: item });
        }
        Ok(())
    }
}

fn encode(img: &image::RgbImage, format: ImageFormat) -> Result<Vec<u8>> {
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, format)?;
    Ok(out.into_inner())
}
