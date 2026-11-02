//! Games recognized by a table of screens: each screen is one or more probes that must all
//! match, and flags are probes checked on a given screen. The game is claimed when its
//! identity probes or any of its screens match. Battle City and Dr. Mario (PRD §6.2) are
//! tables, from the owner's POIs (2026-11-02).

use bro_heart::{GameId, GameObs, ScreenObs};
use bro_vision::{Ink, NesCanvas, Probe, Rect};

use crate::GameRecognizer;

const MAX_SHIFT: i32 = 2;

struct Check {
    probe: Probe,
    max_mismatch: f32,
}

impl Check {
    fn new(reference: &NesCanvas, rect: Rect, ink: Ink, max_mismatch: f32) -> Check {
        let probe = Probe::capture(reference, rect, ink);
        // A probe of plain background would match any plain background.
        debug_assert!(probe.ink_share() > 0.2, "empty probe at {rect:?}");
        Check { probe, max_mismatch }
    }

    fn shows(&self, c: &NesCanvas) -> bool {
        self.probe.mismatch(c, MAX_SHIFT) <= self.max_mismatch
    }
}

struct Screen {
    name: &'static str,
    all: Vec<Check>,
    flags: Vec<(&'static str, Check)>,
}

pub struct ScreenTable {
    id: GameId,
    identity: Vec<Check>,
    screens: Vec<Screen>,
}

impl ScreenTable {
    fn screen(&self, c: &NesCanvas) -> Option<&Screen> {
        self.screens.iter().find(|s| s.all.iter().all(|check| check.shows(c)))
    }
}

impl GameRecognizer for ScreenTable {
    fn id(&self) -> GameId {
        self.id
    }

    fn claims(&self, c: &NesCanvas) -> bool {
        self.identity.iter().any(|check| check.shows(c)) || self.screen(c).is_some()
    }

    fn observe(&self, c: &NesCanvas) -> GameObs {
        let screen = self.screen(c);
        let flags = screen
            .map(|s| {
                s.flags
                    .iter()
                    .filter(|(_, check)| check.shows(c))
                    .map(|(name, _)| name.to_string())
                    .collect()
            })
            .unwrap_or_default();
        GameObs::Screen(ScreenObs { screen: screen.map(|s| s.name.to_owned()), flags })
    }
}

/// Reference frames: Pi 2026-11-02 review items.
pub struct BattleCityRefs<'a> {
    /// Item 46: the title, settled (it scrolls up first).
    pub title: &'a NesCanvas,
    /// Item 60: a game, eagle intact.
    pub playing: &'a NesCanvas,
    /// Item 95: the base destroyed.
    pub lost: &'a NesCanvas,
    /// Item 101: the score screen.
    pub score: &'a NesCanvas,
}

pub fn battle_city(r: &BattleCityRefs) -> ScreenTable {
    // The dark grey frame around the playfield: every in-game screen, stage curtain included.
    let frame = || Check::new(r.playing, Rect::new(2, 20, 14, 220), Ink::Grey, 0.05);
    // The eagle at the bottom centre (owner's POIs), told apart by its grey shape.
    let base = Rect::new(104, 208, 128, 232);
    ScreenTable {
        id: GameId::BattleCity,
        identity: vec![frame()],
        screens: vec![
            Screen {
                name: "Title",
                all: vec![Check::new(r.title, Rect::new(24, 40, 216, 80), Ink::Lit, 0.1)],
                flags: vec![],
            },
            Screen {
                name: "Score",
                all: vec![Check::new(r.score, Rect::new(64, 16, 128, 32), Ink::Lit, 0.1)],
                flags: vec![],
            },
            Screen {
                name: "Base destroyed",
                all: vec![frame(), Check::new(r.lost, base, Ink::Grey, 0.05)],
                flags: vec![],
            },
            Screen {
                name: "Playing",
                all: vec![frame(), Check::new(r.playing, base, Ink::Grey, 0.05)],
                flags: vec![],
            },
        ],
    }
}

/// Reference frames: Pi 2026-11-02 review items.
pub struct DrMarioRefs<'a> {
    /// Item 103: the menu (1 / 2 player game).
    pub menu: &'a NesCanvas,
    /// Item 106: 1-player settings.
    pub settings: &'a NesCanvas,
    /// Item 110: a 1-player game.
    pub single: &'a NesCanvas,
    /// Item 114: a 1-player game lost.
    pub single_over: &'a NesCanvas,
    /// Items 120, 122, 125: a 2-player game; P1 game over; P2 game over.
    pub double: &'a NesCanvas,
    pub p1_over: &'a NesCanvas,
    pub p2_over: &'a NesCanvas,
}

pub fn dr_mario(r: &DrMarioRefs) -> ScreenTable {
    let lit = |c, rect| Check::new(c, rect, Ink::Lit, 0.1);
    ScreenTable {
        id: GameId::DrMario,
        identity: vec![],
        screens: vec![
            Screen {
                name: "Menu",
                all: vec![lit(r.menu, Rect::new(88, 160, 184, 208))],
                flags: vec![],
            },
            Screen {
                name: "Settings",
                all: vec![lit(r.settings, Rect::new(72, 32, 192, 56))],
                flags: vec![],
            },
            Screen {
                name: "Playing 1P",
                all: vec![
                    // The TOP / SCORE clipboard: bright, unlike the checkerboard behind it.
                    Check::new(r.single, Rect::new(24, 24, 88, 72), Ink::Bright, 0.1),
                    lit(r.single, Rect::new(88, 208, 168, 240)),
                ],
                flags: vec![("game over", lit(r.single_over, Rect::new(96, 96, 160, 152)))],
            },
            Screen {
                name: "Playing 2P",
                all: vec![lit(r.double, Rect::new(16, 208, 232, 240))],
                flags: vec![
                    ("P1 over", lit(r.p1_over, Rect::new(40, 152, 80, 184))),
                    ("P2 over", lit(r.p2_over, Rect::new(168, 152, 208, 184))),
                ],
            },
        ],
    }
}
