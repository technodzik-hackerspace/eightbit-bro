//! Perception: what is on one frame (tech design §3.4). Frames are classified on a half-scale
//! canvas; the menu is then read on the full-resolution one. Adding a game is a recognizer
//! module registered in `Perception::new` (PRD §6.3).

mod font;
mod menu;
mod tetris;

use anyhow::{Result, bail};
use bro_capture::{Decoder, FrameData, RawFrame};
use bro_heart::{GameId, MenuState, Seen};
use bro_vision::{Geometry, NesCanvas};

pub use menu::{Menu, MenuRead, MenuRow};
pub use tetris::Tetris;

pub trait GameRecognizer: Send + Sync {
    fn id(&self) -> GameId;
    /// Patterns that are always visible in this game. Must be exclusive: on every fixture at
    /// most one recognizer claims the frame.
    fn claims(&self, c: &NesCanvas) -> bool;
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

/// Reference frames the probes are cut from, decoded like live frames.
const MENU_REFERENCE: &[u8] = include_bytes!("../../../fixtures/frames/pi-2026-11-01/23.jpg");
const TETRIS_REFERENCE: &[u8] = include_bytes!("../../../fixtures/frames/pi-2026-11-01/13.jpg");

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
        let mut reference = |jpeg: &[u8]| -> Result<NesCanvas> {
            let frame =
                RawFrame { no: 0, ts: Default::default(), data: FrameData::Jpeg(jpeg.into()) };
            half_canvas(&mut decoder, &geometry, &frame)
        };
        let menu = Menu::new(&reference(MENU_REFERENCE)?);
        let tetris = Tetris::new(&reference(TETRIS_REFERENCE)?);
        Ok(Perception::with_games(geometry, menu, vec![Box::new(tetris)]))
    }

    pub fn with_games(geometry: Geometry, menu: Menu, games: Vec<Box<dyn GameRecognizer>>) -> Self {
        Perception { geometry, menu, games, last_menu: None }
    }

    /// `hint` (usually the current game) is tried first. It changes speed, never the result.
    pub fn classify(&self, c: &NesCanvas, hint: Option<GameId>) -> Claim {
        if c.is_blanked() {
            return Claim::Black;
        }
        if self.menu.claims(c) {
            return Claim::Menu;
        }
        let hinted = self.games.iter().filter(|g| Some(g.id()) == hint);
        let others = self.games.iter().filter(|g| Some(g.id()) != hint);
        match hinted.chain(others).find(|g| g.claims(c)) {
            Some(g) => Claim::Game(g.id()),
            None => Claim::Unrecognized,
        }
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
            Claim::Game(game) => (Seen::Game(game), None),
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
        cursor: Option<u8>,
        rows: Option<Vec<String>>,
    }

    fn frame(path: &Path) -> RawFrame {
        let jpeg = std::fs::read(path).unwrap();
        RawFrame { no: 0, ts: Default::default(), data: FrameData::Jpeg(jpeg.into()) }
    }

    /// Every frame the Pi queued on 2026-11-01, each perceived on its own (PRD R3).
    #[test]
    fn pi_frames_are_seen_and_menus_read() {
        let dir = Path::new(FIXTURES).join("pi-2026-11-01");
        let expected: Expected =
            toml::from_str(&std::fs::read_to_string(dir.join("expected.toml")).unwrap()).unwrap();
        let mut decoder = Decoder::new().unwrap();
        let mut failures = Vec::new();
        for want in &expected.frame {
            let mut perception = Perception::new(Geometry::MS2109_720P).unwrap();
            let sight = perception.see(&mut decoder, &frame(&dir.join(&want.file)), None).unwrap();
            let seen = match &sight.seen {
                Seen::Black => "black",
                Seen::Menu(_) => "menu",
                Seen::Game(GameId::Tetris) => "tetris",
                Seen::Game(_) => "other game",
                Seen::Unrecognized => "unrecognized",
                Seen::SignalLost => "signal lost",
            };
            if seen != want.seen {
                failures.push(format!("{}: seen {seen}, want {}", want.file, want.seen));
                continue;
            }
            if let (Some(rows), Some(read)) = (&want.rows, &sight.menu) {
                for (i, (got, want_row)) in read.text.iter().zip(rows).enumerate() {
                    if got != want_row {
                        failures.push(format!("{} row {i}: {got:?}, want {want_row:?}", want.file));
                    }
                }
                if read.cursor != want.cursor {
                    failures.push(format!(
                        "{}: cursor {:?}, want {:?}",
                        want.file, read.cursor, want.cursor
                    ));
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
        assert_eq!(sight.seen, Seen::Game(GameId::Tetris));
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
