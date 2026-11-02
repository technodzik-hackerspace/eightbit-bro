//! Tetris (PRD §6.1). Every probe is a region cut from a reference frame: the owner's POIs on
//! the Pi frames, and the v1 fixtures for what the Pi hasn't seen yet (two players, pause,
//! game over, bonus).

use bro_heart::GameId;
use bro_heart::GameObs;
use bro_heart::tetris::{Layout, TetrisMode, TetrisObs, TetrisScreen};
use bro_vision::{Ink, NesCanvas, Probe, Rect};

use crate::GameRecognizer;

/// The SCORE / LINES / LEVEL labels: on every Tetris screen we know, in the same place
/// (the owner's POI on review item 13).
const LABELS: Rect = Rect { x1: 106, y1: 15, x2: 148, y2: 57 };
/// Probes cut from the Pi are exact on the Pi; the v1 captures differ by ~0.11.
const LABELS_MAX_MISMATCH: f32 = 0.2;
const MAX_MISMATCH: f32 = 0.1;
const MAX_SHIFT: i32 = 2;

/// Where each probe comes from.
pub struct References<'a> {
    /// Pi item 13: game select, 1-player layout (owner's POIs).
    pub game_select: &'a NesCanvas,
    /// Pi item 31: level select (owner's POI).
    pub level_select: &'a NesCanvas,
    /// Pi item 11: 1-player game (owner's POIs: NEXT, STATS).
    pub single: &'a NesCanvas,
    /// v1 `game_versus`: two-player game.
    pub double: &'a NesCanvas,
    /// v1 `game_pause`.
    pub pause: &'a NesCanvas,
    /// v1 `game_over_solo`: GAME OVER in the left well.
    pub over_left: &'a NesCanvas,
    /// v1 `game_over_right`: GAME OVER in the right well.
    pub over_right: &'a NesCanvas,
    /// v1 `game_bonus`: BONUS in the left well.
    pub bonus_left: &'a NesCanvas,
    /// v1 `bonus3`: BONUS in the right well.
    pub bonus_right: &'a NesCanvas,
}

pub struct Tetris {
    labels: Probe,
    game_select: Probe,
    level_select: Probe,
    stats: Probe,
    next_right: Probe,
    pause: Probe,
    over: [Probe; 2],
    bonus: [Probe; 2],
}

/// The arrow on game select: one row per mode, 8 lines apart.
const MODE_ARROW: Rect = Rect { x1: 78, y1: 128, x2: 90, y2: 136 };
/// The arrow on level select: one row per level 0–9.
const LEVEL_ARROW: Rect = Rect { x1: 100, y1: 96, x2: 114, y2: 104 };
const MIN_ARROW: usize = 6;

impl Tetris {
    pub fn new(r: &References) -> Tetris {
        let lit = |c: &NesCanvas, rect| Probe::capture(c, rect, Ink::Lit);
        let tetris = Tetris {
            labels: lit(r.game_select, LABELS),
            game_select: lit(r.game_select, Rect::new(78, 112, 166, 121)),
            level_select: lit(r.level_select, Rect::new(78, 80, 174, 89)),
            stats: lit(r.single, Rect::new(174, 48, 215, 57)),
            next_right: lit(r.double, Rect::new(203, 15, 236, 23)),
            pause: lit(r.pause, Rect::new(97, 63, 158, 79)),
            over: [
                lit(r.over_left, Rect::new(37, 95, 82, 127)),
                lit(r.over_right, Rect::new(173, 95, 218, 127)),
            ],
            bonus: [
                lit(r.bonus_left, Rect::new(24, 62, 93, 78)),
                lit(r.bonus_right, Rect::new(160, 62, 229, 78)),
            ],
        };
        // A probe of plain background would match any plain background.
        debug_assert!(tetris.probes().all(|p| p.ink_share() > 0.2), "a Tetris probe is empty");
        tetris
    }

    fn probes(&self) -> impl Iterator<Item = &Probe> {
        [
            &self.labels,
            &self.game_select,
            &self.level_select,
            &self.stats,
            &self.next_right,
            &self.pause,
        ]
        .into_iter()
        .chain(&self.over)
        .chain(&self.bonus)
    }

    fn shows(&self, probe: &Probe, c: &NesCanvas) -> bool {
        probe.mismatch(c, MAX_SHIFT) <= MAX_MISMATCH
    }

    pub fn observe_tetris(&self, c: &NesCanvas) -> TetrisObs {
        if self.shows(&self.game_select, c) {
            let mode = arrow_row(c, MODE_ARROW, TetrisMode::ALL.len()).map(|i| TetrisMode::ALL[i]);
            return TetrisObs { mode_cursor: mode, ..TetrisObs::screen(TetrisScreen::ModeSelect) };
        }
        if self.shows(&self.level_select, c) {
            let level = arrow_row(c, LEVEL_ARROW, 10).map(|i| i as u8);
            return TetrisObs {
                level_cursor: level,
                ..TetrisObs::screen(TetrisScreen::LevelSelect)
            };
        }
        let layout = if self.shows(&self.stats, c) {
            Some(Layout::Single)
        } else if self.shows(&self.next_right, c) {
            Some(Layout::Double)
        } else {
            None
        };
        if layout.is_none() {
            return TetrisObs::screen(TetrisScreen::Unknown);
        }
        TetrisObs {
            layout,
            paused: self.shows(&self.pause, c),
            game_over: [0, 1].map(|i| self.shows(&self.over[i], c)),
            bonus: [0, 1].map(|i| self.shows(&self.bonus[i], c)),
            ..TetrisObs::screen(TetrisScreen::Playing)
        }
    }
}

impl GameRecognizer for Tetris {
    fn id(&self) -> GameId {
        GameId::Tetris
    }

    fn claims(&self, c: &NesCanvas) -> bool {
        self.labels.mismatch(c, MAX_SHIFT) <= LABELS_MAX_MISMATCH
    }

    fn observe(&self, c: &NesCanvas) -> GameObs {
        GameObs::Tetris(self.observe_tetris(c))
    }
}

/// The row (of `rows`, 8 lines apart from `first`) with the most lit pixels, if enough.
fn arrow_row(c: &NesCanvas, first: Rect, rows: usize) -> Option<usize> {
    let lit = |i: usize| {
        let y0 = first.y1 + 8 * i as u32;
        (y0..y0 + first.height())
            .flat_map(|y| (first.x1..first.x2).map(move |x| (x, y)))
            .filter(|&(x, y)| Ink::Lit.at(c, x, y))
            .count()
    };
    let (row, most) = (0..rows).map(|i| (i, lit(i))).max_by_key(|&(i, n)| (n, rows - i))?;
    (most >= MIN_ARROW).then_some(row)
}
