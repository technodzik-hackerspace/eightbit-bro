use std::fmt;

use serde::{Deserialize, Serialize};

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
    pub page: u8,
    pub cursor: u8,
    /// Catalog title under the cursor, when the catalog knows it.
    pub highlighted: Option<String>,
}

/// Game-specific subtree. Games without deep recognition have none; Tetris gets its
/// subtree in M3.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GameDetail {
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConsoleState {
    NoSignal,
    Menu(MenuState),
    InGame {
        game: GameId,
        detail: GameDetail,
        /// The title highlighted in the menu when the game was launched (PRD §5.3).
        menu_selection: Option<String>,
    },
}

impl ConsoleState {
    /// The path through the tree, e.g. `InGame/Tetris` or `Menu/p1/c3`. Used in logs and
    /// events, and to decide whether anything changed.
    pub fn path(&self) -> String {
        match self {
            ConsoleState::NoSignal => "NoSignal".into(),
            ConsoleState::Menu(m) => format!("Menu/p{}/c{}", m.page, m.cursor),
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
    Game(GameId),
    /// Not black, and no recognizer claimed it.
    Unrecognized,
}
