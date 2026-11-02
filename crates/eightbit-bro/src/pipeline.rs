//! One frame through perception, the reducer, the event log and the review queue.

use std::io::Cursor;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use anyhow::Result;
use bro_capture::{Decoder, FrameData, RawFrame};
use bro_games::{MenuRead, MenuRow, Perception};
use bro_heart::{ConsoleState, EventKind, Observation, Title, reduce};
use bro_store::{Offer, Store};
use bro_vision::{Geometry, NesCanvas};
use bro_web::{Live, Push};
use image::ImageFormat;
use tracing::{debug, info, warn};

pub struct Pipeline {
    decoder: Decoder,
    perception: Perception,
    state: ConsoleState,
    store: Arc<Store>,
    live: Arc<Live>,
    /// The last complete menu page logged, so each page is logged once per visit.
    logged_page: Option<u8>,
}

impl Pipeline {
    pub fn new(geometry: Geometry, store: Arc<Store>, live: Arc<Live>) -> Result<Pipeline> {
        live.set_state(&ConsoleState::NoSignal);
        Ok(Pipeline {
            decoder: Decoder::new()?,
            perception: Perception::new(geometry)?,
            state: ConsoleState::NoSignal,
            store,
            live,
            logged_page: None,
        })
    }

    pub fn process(&mut self, frame: &RawFrame) {
        let started = Instant::now();
        let live = self.live.clone();
        let stats = &live.stats;
        let hint = match &self.state {
            ConsoleState::InGame { game, .. } => Some(*game),
            _ => None,
        };
        // Corrupt frames are counted and skipped; they never change the state (PRD Q9).
        let sight = match self.perception.see(&mut self.decoder, frame, hint) {
            Ok(sight) => sight,
            Err(err) => {
                stats.corrupt.fetch_add(1, Ordering::Relaxed);
                debug!(frame = frame.no, %err, "corrupt frame");
                return;
            }
        };
        if let Some(read) = &sight.menu {
            self.log_menu_page(read);
        }
        let review = sight.seen.needs_review();
        self.observe(Observation { frame_no: frame.no, seen: sight.seen });
        if review && let Err(err) = self.review(frame, &sight.canvas) {
            warn!(frame = frame.no, %err, "review queue");
        }
        stats.processed.fetch_add(1, Ordering::Relaxed);
        stats.perceive_us.fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);
    }

    /// Logs each complete menu page once, with the games the catalog doesn't know yet, so
    /// scrolling through the menu completes `bro_heart::TITLES`.
    fn log_menu_page(&mut self, read: &MenuRead) {
        let Some(page) = read.complete_page() else { return };
        if self.logged_page == Some(page) {
            return;
        }
        self.logged_page = Some(page);
        let rows: Vec<MenuRow> = (0..read.text.len()).filter_map(|i| read.row(i)).collect();
        let unknown: Vec<String> = rows
            .iter()
            .filter(|r| Title::from_number(r.number).is_none_or(|t| t.name() != r.name))
            .map(|r| format!("{}.{}", r.number, r.name))
            .collect();
        info!(page, unknown = unknown.join(" | "), "menu page");
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
