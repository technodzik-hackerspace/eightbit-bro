use std::fmt;

use serde::{Deserialize, Serialize};

use crate::Title;
use crate::tetris::{TetrisObs, TetrisState};

/// Games with a recognizer. Everything else the console runs is `Other`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GameId {
    Tetris,
    SuperMario,
    BattleCity,
    DrMario,
    Jewelry,
    Other,
}

impl fmt::Display for GameId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

/// The "300 IN 1" menu: 20 games per page and a cursor (PRD §5.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuState {
    /// 1-based: page 1 is games 1–20.
    pub page: u8,
    /// Row of the arrow, 0..20.
    pub cursor: u8,
    /// The game under the cursor, when its number and name were read and match the catalog.
    pub highlighted: Option<Title>,
}

/// Game-specific subtree. Games without deep recognition have none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GameDetail {
    None,
    Tetris(TetrisState),
}

/// What a game recognizer saw in one frame, beyond "this is the game".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GameObs {
    None,
    Tetris(TetrisObs),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConsoleState {
    NoSignal,
    Menu(MenuState),
    InGame {
        game: GameId,
        detail: GameDetail,
        /// The title highlighted in the menu when the game was launched (PRD §5.3).
        menu_selection: Option<Title>,
    },
}

impl ConsoleState {
    /// The path through the tree, e.g. `InGame/Tetris` or `Menu/p14/c6/TETRIS II`. Used in
    /// logs and events, and to decide whether anything changed.
    pub fn path(&self) -> String {
        match self {
            ConsoleState::NoSignal => "NoSignal".into(),
            ConsoleState::Menu(m) => match m.highlighted {
                Some(title) => format!("Menu/p{}/c{}/{}", m.page, m.cursor, title.name()),
                None => format!("Menu/p{}/c{}", m.page, m.cursor),
            },
            ConsoleState::InGame { game, detail: GameDetail::Tetris(t), .. }
                if !t.path().is_empty() =>
            {
                format!("InGame/{game}/{}", t.path())
            }
            ConsoleState::InGame { game, .. } => format!("InGame/{game}"),
        }
    }
}

/// What perception saw in one frame, with no knowledge of earlier frames (PRD R3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observation {
    pub frame_no: u64,
    pub seen: Seen,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Seen {
    /// The source lost the device or got no frames for a while.
    SignalLost,
    /// Almost every pixel is black.
    Black,
    Menu(MenuState),
    /// A recognizer claimed the frame.
    Game(GameId, GameObs),
    /// Not black, and no recognizer claimed it.
    Unrecognized,
}

impl Seen {
    /// Frames for the review queue: nobody claimed them, or a game claimed them but doesn't
    /// know the screen.
    pub fn needs_review(&self) -> bool {
        matches!(
            self,
            Seen::Unrecognized
                | Seen::Game(
                    _,
                    GameObs::Tetris(TetrisObs { screen: crate::tetris::TetrisScreen::Unknown, .. })
                )
        )
    }
}
