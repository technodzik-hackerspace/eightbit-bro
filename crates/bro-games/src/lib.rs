//! Perception: what is on one frame, without knowing earlier frames (tech design §3.4).
//! Adding a game is a recognizer module registered in `Perception::standard` (PRD §6.3).

use bro_heart::{GameId, Seen};
use bro_vision::NesCanvas;

pub trait GameRecognizer: Send + Sync {
    fn id(&self) -> GameId;
    /// Patterns that are always visible in this game. Must be exclusive: on every fixture at
    /// most one recognizer claims the frame.
    fn claims(&self, c: &NesCanvas) -> bool;
}

pub struct Perception {
    games: Vec<Box<dyn GameRecognizer>>,
}

impl Perception {
    pub fn new(games: Vec<Box<dyn GameRecognizer>>) -> Perception {
        Perception { games }
    }

    /// All recognizers. The menu and the five games come with their fixtures (M1, M2).
    pub fn standard() -> Perception {
        Perception::new(Vec::new())
    }

    /// `hint` (usually the current game) is tried first. It changes speed, never the result.
    pub fn perceive(&self, c: &NesCanvas, hint: Option<GameId>) -> Seen {
        if c.is_black() {
            return Seen::Black;
        }
        let hinted = self.games.iter().filter(|g| Some(g.id()) == hint);
        let others = self.games.iter().filter(|g| Some(g.id()) != hint);
        match hinted.chain(others).find(|g| g.claims(c)) {
            Some(g) => Seen::Game(g.id()),
            None => Seen::Unrecognized,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        NesCanvas::sample(
            &img,
            &bro_vision::Geometry {
                picture: NesCanvas::full(),
            },
            None,
        )
    }

    #[test]
    fn hint_never_changes_the_result() {
        let p = Perception::new(vec![
            Box::new(Fake(GameId::Tetris, 10)),
            Box::new(Fake(GameId::Jewelry, 20)),
        ]);
        for red in [10, 20, 30] {
            let plain = p.perceive(&canvas(red), None);
            for hint in [GameId::Tetris, GameId::Jewelry, GameId::Other] {
                assert_eq!(p.perceive(&canvas(red), Some(hint)), plain);
            }
        }
        assert_eq!(p.perceive(&canvas(20), None), Seen::Game(GameId::Jewelry));
        assert_eq!(p.perceive(&canvas(30), None), Seen::Unrecognized);
    }

    #[test]
    fn black_comes_first() {
        let p = Perception::new(vec![Box::new(Fake(GameId::Tetris, 0))]);
        let img = image::RgbImage::new(256, 240);
        let black = NesCanvas::sample(
            &img,
            &bro_vision::Geometry {
                picture: NesCanvas::full(),
            },
            None,
        );
        assert_eq!(p.perceive(&black, None), Seen::Black);
    }
}
