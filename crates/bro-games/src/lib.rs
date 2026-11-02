//! Perception: what is on one frame (tech design §3.4). Frames are classified on a half-scale
//! canvas; the menu is then read on the full-resolution one. Adding a game is a recognizer
//! module registered in `Perception::new` (PRD §6.3).

mod font;
mod menu;
mod screens;
mod tetris;

use anyhow::{Result, bail};
use bro_capture::{Decoder, FrameData, RawFrame};
use bro_heart::{GameId, GameObs, MenuState, Seen};
use bro_vision::{Geometry, NesCanvas};

pub use menu::{Menu, MenuRead, MenuRow};
pub use screens::{BattleCityRefs, DrMarioRefs, ScreenTable, battle_city, dr_mario};
pub use tetris::{References, Tetris};

pub trait GameRecognizer: Send + Sync {
    fn id(&self) -> GameId;
    /// Patterns that are always visible in this game. Must be exclusive: on every fixture at
    /// most one recognizer claims the frame.
    fn claims(&self, c: &NesCanvas) -> bool;
    /// What else the frame shows, for games with deep recognition.
    fn observe(&self, _c: &NesCanvas) -> GameObs {
        GameObs::None
    }
}

/// What a half-scale canvas shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Claim {
    Black,
    Menu,
    Game(GameId),
    Unrecognized,
}

/// One perceived frame.
pub struct Sight {
    pub seen: Seen,
    /// The half-scale canvas everything was classified on.
    pub canvas: NesCanvas,
    /// The menu text, on menu frames.
    pub menu: Option<MenuRead>,
}

/// Reference frames the probes are cut from, decoded like live frames: from the Pi (720p)…
macro_rules! pi {
    ($n:literal) => {
        include_bytes!(concat!("../../../fixtures/frames/pi-2026-11-01/", $n, ".jpg"))
    };
}
macro_rules! pi2 {
    ($n:literal) => {
        include_bytes!(concat!("../../../fixtures/frames/pi-2026-11-02/", $n, ".jpg"))
    };
}
/// …and from v1's 1080p captures, for screens the Pi hasn't shown yet.
macro_rules! v1 {
    ($name:literal) => {
        include_bytes!(concat!("../../../fixtures/frames/v1-1080p/", $name, ".jpg"))
    };
}

/// Where the picture is in v1's 1080p captures (fitted to pixel edges like the 720p one).
pub const V1_1080P: Geometry =
    Geometry { picture: bro_vision::Rect { x1: 238, y1: 3, x2: 1683, y2: 1080 } };

pub struct Perception {
    geometry: Geometry,
    menu: Menu,
    games: Vec<Box<dyn GameRecognizer>>,
    /// The last menu read and the list area it was read from: the menu is mostly still, so
    /// the full-resolution decode only runs when the list changes.
    last_menu: Option<(u64, MenuRead, MenuState)>,
}

impl Perception {
    /// All recognizers. Battle City, Super Mario, Dr. Mario and Jewelry come with their
    /// fixtures.
    pub fn new(geometry: Geometry) -> Result<Perception> {
        let mut decoder = Decoder::new()?;
        let mut reference = |jpeg: &[u8], geometry: &Geometry| -> Result<NesCanvas> {
            let frame =
                RawFrame { no: 0, ts: Default::default(), data: FrameData::Jpeg(jpeg.into()) };
            half_canvas(&mut decoder, geometry, &frame)
        };
        // Each reference is decoded with its own capture's geometry.
        let pi = Geometry::MS2109_720P;
        let menu = Menu::new(&reference(pi!("23"), &pi)?);
        let tetris = Tetris::new(&References {
            game_select: &reference(pi!("13"), &pi)?,
            level_select: &reference(pi!("31"), &pi)?,
            single: &reference(pi!("11"), &pi)?,
            title: &reference(pi2!("5"), &pi)?,
            handicap_select: &reference(pi2!("33"), &pi)?,
            music_select: &reference(pi2!("34"), &pi)?,
            double: &reference(pi2!("36"), &pi)?,
            pause: &reference(pi2!("133"), &pi)?,
            bonus: &reference(pi2!("134"), &pi)?,
            bonus_right: &reference(pi2!("135"), &pi)?,
            coop: &reference(pi2!("155"), &pi)?,
            coop_over: &reference(pi2!("156"), &pi)?,
            over_left: &reference(pi2!("44"), &pi)?,
            over_right: &reference(pi2!("40"), &pi)?,
            v1_double: &reference(v1!("game_versus"), &V1_1080P)?,
            v1_over_left: &reference(v1!("game_over_solo"), &V1_1080P)?,
            v1_over_right: &reference(v1!("game_over_right"), &V1_1080P)?,
            v1_pause: &reference(v1!("game_pause"), &V1_1080P)?,
            v1_bonus_left: &reference(v1!("game_bonus"), &V1_1080P)?,
            v1_bonus_right: &reference(v1!("bonus3"), &V1_1080P)?,
        });
        let battle_city = battle_city(&BattleCityRefs {
            title: &reference(pi2!("46"), &pi)?,
            playing: &reference(pi2!("60"), &pi)?,
            lost: &reference(pi2!("95"), &pi)?,
            score: &reference(pi2!("101"), &pi)?,
        });
        let dr_mario = dr_mario(&DrMarioRefs {
            menu: &reference(pi2!("103"), &pi)?,
            settings: &reference(pi2!("106"), &pi)?,
            single: &reference(pi2!("110"), &pi)?,
            single_over: &reference(pi2!("114"), &pi)?,
            stage_clear: &reference(pi2!("149"), &pi)?,
            double: &reference(pi2!("120"), &pi)?,
            p1_over: &reference(pi2!("122"), &pi)?,
            p2_over: &reference(pi2!("125"), &pi)?,
        });
        let games: Vec<Box<dyn GameRecognizer>> =
            vec![Box::new(tetris), Box::new(battle_city), Box::new(dr_mario)];
        Ok(Perception::with_games(geometry, menu, games))
    }

    pub fn with_games(geometry: Geometry, menu: Menu, games: Vec<Box<dyn GameRecognizer>>) -> Self {
        Perception { geometry, menu, games, last_menu: None }
    }

    /// `hint` (usually the current game) is tried first. It changes speed, never the result.
    pub fn classify(&self, c: &NesCanvas, hint: Option<GameId>) -> Claim {
        // Positive recognition first (PRD R5): some screens are mostly black (the Tetris
        // splash), and a blanked transition frame is never claimed.
        if self.menu.claims(c) {
            return Claim::Menu;
        }
        if let Some(g) = self.claimant(c, hint) {
            return Claim::Game(g.id());
        }
        if c.is_blanked() { Claim::Black } else { Claim::Unrecognized }
    }

    fn claimant(&self, c: &NesCanvas, hint: Option<GameId>) -> Option<&dyn GameRecognizer> {
        let hinted = self.games.iter().filter(|g| Some(g.id()) == hint);
        let others = self.games.iter().filter(|g| Some(g.id()) != hint);
        hinted.chain(others).find(|g| g.claims(c)).map(|g| g.as_ref())
    }

    pub fn see(
        &mut self,
        decoder: &mut Decoder,
        frame: &RawFrame,
        hint: Option<GameId>,
    ) -> Result<Sight> {
        let canvas = half_canvas(decoder, &self.geometry, frame)?;
        let (seen, menu) = match self.classify(&canvas, hint) {
            Claim::Black => (Seen::Black, None),
            Claim::Game(game) => {
                let obs = self
                    .claimant(&canvas, Some(game))
                    .map_or(GameObs::None, |g| g.observe(&canvas));
                (Seen::Game(game, obs), None)
            }
            Claim::Unrecognized => (Seen::Unrecognized, None),
            Claim::Menu => {
                let key = list_key(&canvas);
                let cached = self.last_menu.as_ref().filter(|(k, _, _)| *k == key);
                let (read, state) = match cached {
                    Some((_, read, state)) => (read.clone(), state.clone()),
                    None => {
                        let full = NesCanvas::sample(&decoder.decode_full(frame)?, &self.geometry);
                        let read = self.menu.read(&full);
                        let state = read.state(self.last_menu.as_ref().map(|(_, _, s)| s));
                        (read, state)
                    }
                };
                self.last_menu = Some((key, read.clone(), state.clone()));
                (Seen::Menu(state), Some(read))
            }
        };
        Ok(Sight { seen, canvas, menu })
    }
}

fn half_canvas(decoder: &mut Decoder, geometry: &Geometry, frame: &RawFrame) -> Result<NesCanvas> {
    let (img, factor) = decoder.decode(frame)?;
    let geometry = geometry.scaled_down(factor);
    if !geometry.fits(img.width(), img.height()) {
        bail!("frame {}×{} is smaller than the picture", img.width(), img.height());
    }
    Ok(NesCanvas::sample(&img, &geometry))
}

/// Fingerprint of the menu's list area (text and arrow) on the half-scale canvas.
fn list_key(c: &NesCanvas) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for y in 24..200 {
        for x in 0..232 {
            let [r, g, b] = c.rgb_at(x, y);
            let ink = c.bright_at(x, y) as u64 | ((r > 170 && g < 120 && b < 120) as u64) << 1;
            hash = (hash ^ ink).wrapping_mul(0x100_0000_01b3);
        }
    }
    hash
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use bro_heart::tetris::{Layout, TetrisMode, TetrisMusic, TetrisScreen};
    use serde::Deserialize;

    use super::*;

    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/frames");

    #[derive(Deserialize)]
    struct Expected {
        frame: Vec<ExpectedFrame>,
    }

    #[derive(Deserialize)]
    struct ExpectedFrame {
        file: String,
        seen: String,
        // Menu.
        cursor: Option<u8>,
        rows: Option<Vec<String>>,
        // Tetris.
        screen: Option<TetrisScreen>,
        layout: Option<Layout>,
        mode: Option<TetrisMode>,
        /// Per player, -1 for no arrow (TOML has no null).
        level: Option<[i16; 2]>,
        handicap: Option<[i16; 2]>,
        music: Option<TetrisMusic>,
        // Screen-table games: the screen name ("?" for recognized but unknown) and flags.
        game_screen: Option<String>,
        flags: Option<Vec<String>>,
        paused: Option<bool>,
        bonus: Option<[bool; 2]>,
        game_over: Option<[bool; 2]>,
    }

    fn frame(path: &Path) -> RawFrame {
        let jpeg = std::fs::read(path).unwrap();
        RawFrame { no: 0, ts: Default::default(), data: FrameData::Jpeg(jpeg.into()) }
    }

    fn check<T: PartialEq + std::fmt::Debug>(
        failures: &mut Vec<String>,
        file: &str,
        what: &str,
        got: T,
        want: Option<T>,
    ) {
        if let Some(want) = want.filter(|w| *w != got) {
            failures.push(format!("{file}: {what} {got:?}, want {want:?}"));
        }
    }

    /// Perceives every frame of a fixture folder on its own (PRD R3) and compares with its
    /// `expected.toml`.
    fn check_fixtures(folder: &str, geometry: Geometry) {
        let dir = Path::new(FIXTURES).join(folder);
        let expected: Expected =
            toml::from_str(&std::fs::read_to_string(dir.join("expected.toml")).unwrap()).unwrap();
        let mut decoder = Decoder::new().unwrap();
        let mut failures = Vec::new();
        for want in &expected.frame {
            let mut perception = Perception::new(geometry).unwrap();
            let sight = perception.see(&mut decoder, &frame(&dir.join(&want.file)), None).unwrap();
            let seen = match &sight.seen {
                Seen::Black => "black",
                Seen::Menu(_) => "menu",
                Seen::Game(GameId::Tetris, _) => "tetris",
                Seen::Game(GameId::BattleCity, _) => "battle city",
                Seen::Game(GameId::DrMario, _) => "dr mario",
                Seen::Game(..) => "other game",
                Seen::Unrecognized => "unrecognized",
                Seen::SignalLost => "signal lost",
            };
            if seen != want.seen {
                failures.push(format!("{}: seen {seen}, want {}", want.file, want.seen));
                continue;
            }
            let file = want.file.as_str();
            if let (Some(rows), Some(read)) = (&want.rows, &sight.menu) {
                for (i, (got, want_row)) in read.text.iter().zip(rows).enumerate() {
                    check(&mut failures, file, &format!("row {i}"), got, Some(want_row));
                }
                check(&mut failures, file, "cursor", read.cursor, Some(want.cursor));
            }
            if let Seen::Game(_, GameObs::Screen(o)) = &sight.seen {
                let screen = o.screen.clone().unwrap_or_else(|| "?".into());
                check(&mut failures, file, "game screen", screen, want.game_screen.clone());
                check(&mut failures, file, "flags", o.flags.clone(), want.flags.clone());
            }
            if let Seen::Game(_, GameObs::Tetris(t)) = &sight.seen {
                check(&mut failures, file, "screen", t.screen, want.screen);
                check(&mut failures, file, "layout", t.layout, want.layout.map(Some));
                check(&mut failures, file, "mode", t.mode_cursor, want.mode.map(Some));
                let per_player = |v: [i16; 2]| v.map(|x| u8::try_from(x).ok());
                check(&mut failures, file, "level", t.level_cursor, want.level.map(per_player));
                check(
                    &mut failures,
                    file,
                    "handicap",
                    t.handicap_cursor,
                    want.handicap.map(per_player),
                );
                check(&mut failures, file, "music", t.music_cursor, want.music.map(Some));
                check(&mut failures, file, "paused", t.paused, want.paused);
                check(&mut failures, file, "bonus", t.bonus, want.bonus);
                check(&mut failures, file, "game over", t.game_over, want.game_over);
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// Every frame the Pi queued on 2026-11-01.
    #[test]
    fn pi_frames() {
        check_fixtures("pi-2026-11-01", Geometry::MS2109_720P);
    }

    /// The frames the Pi queued on 2026-11-02, and the owner captured.
    #[test]
    fn pi_frames_2() {
        check_fixtures("pi-2026-11-02", Geometry::MS2109_720P);
    }

    /// v1's 1080p fixtures with v1's assertions.
    #[test]
    fn v1_frames() {
        check_fixtures("v1-1080p", V1_1080P);
    }

    /// On every fixture frame at most one recognizer claims the frame (tech design §3.4).
    #[test]
    fn no_frame_is_claimed_twice() {
        let mut decoder = Decoder::new().unwrap();
        let mut failures = Vec::new();
        for (folder, geometry) in [
            ("pi-2026-11-01", Geometry::MS2109_720P),
            ("pi-2026-11-02", Geometry::MS2109_720P),
            ("v1-1080p", V1_1080P),
        ] {
            let perception = Perception::new(geometry).unwrap();
            let mut files: Vec<_> = std::fs::read_dir(Path::new(FIXTURES).join(folder))
                .unwrap()
                .map(|e| e.unwrap().path())
                .filter(|p| p.extension().is_some_and(|e| e == "jpg"))
                .collect();
            files.sort();
            for path in files {
                let canvas = half_canvas(&mut decoder, &geometry, &frame(&path)).unwrap();
                let mut claims: Vec<String> = perception
                    .games
                    .iter()
                    .filter(|g| g.claims(&canvas))
                    .map(|g| g.id().to_string())
                    .collect();
                if perception.menu.claims(&canvas) {
                    claims.push("Menu".into());
                }
                if claims.len() > 1 {
                    failures.push(format!("{}: {claims:?}", path.display()));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn complete_menu_page_gives_the_full_state() {
        let mut perception = Perception::new(Geometry::MS2109_720P).unwrap();
        let mut decoder = Decoder::new().unwrap();
        let path = Path::new(FIXTURES).join("pi-2026-11-01/23.jpg");
        let sight = perception.see(&mut decoder, &frame(&path), None).unwrap();
        let read = sight.menu.unwrap();
        assert_eq!(read.complete_page(), Some(12));
        assert_eq!(
            sight.seen,
            Seen::Menu(MenuState {
                page: 12,
                cursor: 0,
                highlighted: Some(bro_heart::Title::RaidOnBungelingBay)
            })
        );
    }

    #[test]
    fn live_tetris_demo_is_tetris() {
        let mut perception = Perception::new(Geometry::MS2109_720P).unwrap();
        let mut decoder = Decoder::new().unwrap();
        let path = Path::new(FIXTURES).join("ms2109_720p_tetris_demo.jpg");
        let sight = perception.see(&mut decoder, &frame(&path), None).unwrap();
        assert!(matches!(sight.seen, Seen::Game(GameId::Tetris, _)));
    }

    /// Claims frames whose top-left pixel has this red value.
    struct Fake(GameId, u8);

    impl GameRecognizer for Fake {
        fn id(&self) -> GameId {
            self.0
        }
        fn claims(&self, c: &NesCanvas) -> bool {
            c.rgb_at(0, 0)[0] == self.1
        }
    }

    fn canvas(red: u8) -> NesCanvas {
        let img = image::RgbImage::from_pixel(256, 240, image::Rgb([red, 200, 200]));
        NesCanvas::sample(&img, &Geometry { picture: NesCanvas::full() })
    }

    #[test]
    fn hint_never_changes_the_result() {
        // A menu that never matches these canvases: pure blue background.
        let blue = image::RgbImage::from_pixel(256, 240, image::Rgb([0, 0, 255]));
        let menu = Menu::new(&NesCanvas::sample(&blue, &Geometry { picture: NesCanvas::full() }));
        let p = Perception::with_games(
            Geometry::MS2109_720P,
            menu,
            vec![Box::new(Fake(GameId::Tetris, 10)), Box::new(Fake(GameId::Jewelry, 20))],
        );
        for red in [10, 20, 30] {
            let plain = p.classify(&canvas(red), None);
            for hint in [GameId::Tetris, GameId::Jewelry, GameId::Other] {
                assert_eq!(p.classify(&canvas(red), Some(hint)), plain);
            }
        }
        assert_eq!(p.classify(&canvas(20), None), Claim::Game(GameId::Jewelry));
        assert_eq!(p.classify(&canvas(30), None), Claim::Unrecognized);
    }
}
