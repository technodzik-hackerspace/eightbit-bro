//! Tetris (PRD §6.1). Every probe is a region cut from a reference frame: the owner's POIs on
//! the Pi frames, and the v1 1080p fixtures for what the Pi hasn't shown yet (pause, bonus).
//! Where both exist, an element matches if either reference does.

use bro_heart::tetris::{HANDICAPS, Layout, TetrisMode, TetrisMusic, TetrisObs, TetrisScreen};
use bro_heart::{GameId, GameObs};
use bro_vision::{Ink, NesCanvas, Probe, Rect};

use crate::GameRecognizer;

/// The SCORE / LINES / LEVEL labels: on every Tetris screen but the title, in the same place
/// (the owner's POI on review item 13).
const LABELS: Rect = Rect { x1: 106, y1: 15, x2: 148, y2: 57 };
/// Probes cut from the Pi are exact on the Pi; the v1 captures differ by ~0.11.
const LABELS_MAX_MISMATCH: f32 = 0.2;
const MAX_MISMATCH: f32 = 0.1;
const MAX_SHIFT: i32 = 2;

/// Where each probe comes from. `pi*` are the live 720p capture, `v1_*` v1's 1080p one.
pub struct References<'a> {
    /// Pi 2026-11-01 item 13: game select, 1-player layout (owner's POIs).
    pub game_select: &'a NesCanvas,
    /// Pi 2026-11-01 item 31: level select (owner's POI).
    pub level_select: &'a NesCanvas,
    /// Pi 2026-11-01 item 11: 1-player game (owner's POIs: NEXT, STATS).
    pub single: &'a NesCanvas,
    /// Pi 2026-11-02 item 5: the splash (owner's POI).
    pub title: &'a NesCanvas,
    /// Pi 2026-11-02 item 33: handicap select (owner's POI).
    pub handicap_select: &'a NesCanvas,
    /// Pi 2026-11-02 item 34: music select (owner's POI).
    pub music_select: &'a NesCanvas,
    /// Pi 2026-11-02 item 36: two-player game (owner's POI: the right NEXT).
    pub double: &'a NesCanvas,
    /// Pi 2026-11-02 items 44 and 40: GAME OVER left and right (owner's POIs).
    pub over_left: &'a NesCanvas,
    pub over_right: &'a NesCanvas,
    /// v1 `game_versus`, `game_over_solo`, `game_over_right`: the same elements at 1080p.
    pub v1_double: &'a NesCanvas,
    pub v1_over_left: &'a NesCanvas,
    pub v1_over_right: &'a NesCanvas,
    /// v1 `game_pause`, `game_bonus`, `bonus3`: not seen on the Pi yet.
    pub v1_pause: &'a NesCanvas,
    pub v1_bonus_left: &'a NesCanvas,
    pub v1_bonus_right: &'a NesCanvas,
}

/// Probes for one element; it shows if any of them matches.
struct Element(Vec<Probe>);

impl Element {
    fn shows(&self, c: &NesCanvas, max_mismatch: f32) -> bool {
        self.0.iter().any(|p| p.mismatch(c, MAX_SHIFT) <= max_mismatch)
    }
}

pub struct Tetris {
    labels: Element,
    title: Element,
    game_select: Element,
    level_select: Element,
    handicap_select: Element,
    music_select: Element,
    stats: Element,
    next_right: Element,
    pause: Element,
    over: [Element; 2],
    bonus: [Element; 2],
}

/// Arrows on the select screens: one row per option, 8 lines apart. Player 1's arrow points
/// right, left of the list; player 2's points left, right of the list.
const MODE_ARROW: Rect = Rect { x1: 78, y1: 128, x2: 90, y2: 136 };
const P1_ARROW: Rect = Rect { x1: 100, y1: 96, x2: 114, y2: 104 };
const P2_ARROW: Rect = Rect { x1: 130, y1: 96, x2: 146, y2: 104 };
const MIN_ARROW: usize = 6;

impl Tetris {
    pub fn new(r: &References) -> Tetris {
        let lit = |c: &NesCanvas, rect| Probe::capture(c, rect, Ink::Lit);
        let one = |c: &NesCanvas, rect| Element(vec![lit(c, rect)]);
        let tetris = Tetris {
            labels: one(r.game_select, LABELS),
            title: one(r.title, Rect::new(54, 72, 182, 81)),
            game_select: one(r.game_select, Rect::new(78, 112, 166, 121)),
            level_select: one(r.level_select, Rect::new(78, 80, 174, 89)),
            handicap_select: one(r.handicap_select, Rect::new(70, 80, 192, 89)),
            music_select: one(r.music_select, Rect::new(86, 80, 182, 89)),
            stats: one(r.single, Rect::new(174, 48, 215, 57)),
            next_right: Element(vec![
                lit(r.double, Rect::new(210, 16, 241, 24)),
                lit(r.v1_double, Rect::new(203, 15, 236, 23)),
            ]),
            pause: one(r.v1_pause, Rect::new(97, 63, 158, 79)),
            over: [
                Element(vec![
                    lit(r.over_left, Rect::new(31, 96, 80, 128)),
                    lit(r.v1_over_left, Rect::new(37, 95, 82, 127)),
                ]),
                Element(vec![
                    lit(r.over_right, Rect::new(175, 96, 224, 128)),
                    lit(r.v1_over_right, Rect::new(173, 95, 218, 127)),
                ]),
            ],
            bonus: [
                one(r.v1_bonus_left, Rect::new(24, 62, 93, 78)),
                one(r.v1_bonus_right, Rect::new(160, 62, 229, 78)),
            ],
        };
        // A probe of plain background would match any plain background.
        debug_assert!(tetris.probes().all(|p| p.ink_share() > 0.2), "a Tetris probe is empty");
        tetris
    }

    fn probes(&self) -> impl Iterator<Item = &Probe> {
        let single = [
            &self.labels,
            &self.title,
            &self.game_select,
            &self.level_select,
            &self.handicap_select,
            &self.music_select,
            &self.stats,
            &self.next_right,
            &self.pause,
        ];
        single.into_iter().chain(&self.over).chain(&self.bonus).flat_map(|e| &e.0)
    }

    fn shows(&self, e: &Element, c: &NesCanvas) -> bool {
        e.shows(c, MAX_MISMATCH)
    }

    pub fn observe_tetris(&self, c: &NesCanvas) -> TetrisObs {
        if self.shows(&self.title, c) {
            return TetrisObs::screen(TetrisScreen::Title);
        }
        if self.shows(&self.game_select, c) {
            let mode = arrow_row(c, MODE_ARROW, TetrisMode::ALL.len()).map(|i| TetrisMode::ALL[i]);
            return TetrisObs { mode_cursor: mode, ..TetrisObs::screen(TetrisScreen::ModeSelect) };
        }
        let arrows = |rows: usize| [P1_ARROW, P2_ARROW].map(|a| arrow_row(c, a, rows));
        if self.shows(&self.level_select, c) {
            let level_cursor = arrows(10).map(|row| row.map(|i| i as u8));
            return TetrisObs { level_cursor, ..TetrisObs::screen(TetrisScreen::LevelSelect) };
        }
        if self.shows(&self.handicap_select, c) {
            let handicap_cursor = arrows(HANDICAPS.len()).map(|row| row.map(|i| HANDICAPS[i]));
            return TetrisObs {
                handicap_cursor,
                ..TetrisObs::screen(TetrisScreen::HandicapSelect)
            };
        }
        if self.shows(&self.music_select, c) {
            let music = arrow_row(c, P1_ARROW, TetrisMusic::ALL.len()).map(|i| TetrisMusic::ALL[i]);
            return TetrisObs {
                music_cursor: music,
                ..TetrisObs::screen(TetrisScreen::MusicSelect)
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
        self.labels.shows(c, LABELS_MAX_MISMATCH) || self.shows(&self.title, c)
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
