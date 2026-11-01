//! Tetris, identity only for now (deep recognition is M3).

use bro_heart::GameId;
use bro_vision::{NesCanvas, Probe, Rect};

use crate::GameRecognizer;

/// The SCORE / LINES / LEVEL labels: on every Tetris screen we know (title aside), in the
/// same place: gameplay, game select, level select (the owner's POI on review item 13).
const LABELS: Rect = Rect { x1: 106, y1: 15, x2: 148, y2: 57 };
const MAX_MISMATCH: f32 = 0.08;

pub struct Tetris {
    labels: Probe,
}

impl Tetris {
    pub fn new(reference: &NesCanvas) -> Tetris {
        Tetris { labels: Probe::capture(reference, LABELS) }
    }
}

impl GameRecognizer for Tetris {
    fn id(&self) -> GameId {
        GameId::Tetris
    }

    fn claims(&self, c: &NesCanvas) -> bool {
        self.labels.mismatch(c, 2) <= MAX_MISMATCH
    }
}
