//! Tetris (PRD §6.1): what one frame shows, and the state across frames.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TetrisScreen {
    /// "1993 TETRIS GAME / PUSH START BUTTON". The demo starts from here.
    Title,
    /// "GAME SELECT": the mode list.
    ModeSelect,
    /// "LEVEL SELECT": the start level, per player.
    LevelSelect,
    /// "HANDICAP SELECT": 0, 3, 6, 9 or 12 rows of garbage, per player.
    HandicapSelect,
    /// "MUSIC SELECT": the last screen before the game.
    MusicSelect,
    /// A game (or the demo) is on: wells and NEXT.
    Playing,
    /// A Tetris screen we don't know yet: the frame goes to the review queue.
    Unknown,
}

/// The game select list, top to bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TetrisMode {
    OnePlayer,
    TwoPlayer,
    Cooperative,
    VersusComputer,
    WithComputer,
}

impl TetrisMode {
    pub const ALL: [TetrisMode; 5] = [
        TetrisMode::OnePlayer,
        TetrisMode::TwoPlayer,
        TetrisMode::Cooperative,
        TetrisMode::VersusComputer,
        TetrisMode::WithComputer,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TetrisMode::OnePlayer => "1P",
            TetrisMode::TwoPlayer => "2P",
            TetrisMode::Cooperative => "coop",
            TetrisMode::VersusComputer => "vsCPU",
            TetrisMode::WithComputer => "withCPU",
        }
    }
}

/// The music select list, top to bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TetrisMusic {
    Silence,
    Loginska,
    Bradinsky,
    Karinka,
    Troika,
}

impl TetrisMusic {
    pub const ALL: [TetrisMusic; 5] = [
        TetrisMusic::Silence,
        TetrisMusic::Loginska,
        TetrisMusic::Bradinsky,
        TetrisMusic::Karinka,
        TetrisMusic::Troika,
    ];
}

/// Handicap values on the handicap select screen, top to bottom.
pub const HANDICAPS: [u8; 5] = [0, 3, 6, 9, 12];

/// The playfield layout: one well with STATS, two wells, or one shared wide well
/// (cooperative).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Layout {
    Single,
    Double,
    Coop,
}

/// A player's numbers on the HUD; `None` where not shown or not read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerStats {
    pub score: Option<u32>,
    pub lines: Option<u32>,
    pub level: Option<u32>,
}

impl PlayerStats {
    /// This frame's values, the previous ones where this frame has none.
    fn or(self, previous: PlayerStats) -> PlayerStats {
        PlayerStats {
            score: self.score.or(previous.score),
            lines: self.lines.or(previous.lines),
            level: self.level.or(previous.level),
        }
    }
}

/// Why a game ended (PRD T8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EndReason {
    /// The well topped out: the player's in 1P and coop, both players' in 2P.
    GameOver,
    /// Back to the splash or the select screens without a game over.
    NewGame,
    /// The console left Tetris: reset to the menu, another game, or no signal.
    Left,
}

/// One finished game: the event the recorder, Telegram and high scores build on (PRD E3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameResult {
    pub reason: EndReason,
    pub session: Session,
    pub layout: Option<Layout>,
    pub mode: Option<TetrisMode>,
    pub start_level: [Option<u8>; 2],
    pub handicap: [Option<u8>; 2],
    /// Final numbers, frozen when each player topped out (PRD T7).
    pub players: [PlayerStats; 2],
    pub topped_out: [bool; 2],
}

/// One Tetris frame, without history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TetrisObs {
    pub screen: TetrisScreen,
    /// The arrow on the game select screen.
    pub mode_cursor: Option<TetrisMode>,
    /// The arrows on the level select screen: player 1 (left of the digits), player 2 (right).
    pub level_cursor: [Option<u8>; 2],
    /// The arrows on the handicap select screen, as handicap values.
    pub handicap_cursor: [Option<u8>; 2],
    /// The arrow on the music select screen.
    pub music_cursor: Option<TetrisMusic>,
    pub layout: Option<Layout>,
    pub paused: bool,
    /// BONUS shown in the left / right well.
    pub bonus: [bool; 2],
    /// GAME OVER shown in the left / right well.
    pub game_over: [bool; 2],
    /// The HUD numbers on a playing screen: player 1 (left of the labels), player 2 (right).
    pub players: [PlayerStats; 2],
}

impl TetrisObs {
    pub fn screen(screen: TetrisScreen) -> TetrisObs {
        TetrisObs {
            screen,
            mode_cursor: None,
            level_cursor: [None; 2],
            handicap_cursor: [None; 2],
            music_cursor: None,
            layout: None,
            paused: false,
            bonus: [false; 2],
            game_over: [false; 2],
            players: [PlayerStats::default(); 2],
        }
    }
}

/// How a game session began (PRD T3, T4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Session {
    /// Started through game select / level select: a human game.
    Clean,
    /// Joined mid-game (cold start): treated as human.
    Dirty,
    /// Playing began without the select screens: the console's demo. Never recorded.
    Demo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TetrisState {
    /// The last known screen (unknown screens don't change it).
    pub screen: Option<TetrisScreen>,
    /// Highlighted on game select, kept for the game.
    pub mode: Option<TetrisMode>,
    /// Highlighted on level select (per player), kept for the game.
    pub start_level: [Option<u8>; 2],
    /// Highlighted on handicap select (per player), kept for the game.
    pub handicap: [Option<u8>; 2],
    pub music: Option<TetrisMusic>,
    pub layout: Option<Layout>,
    pub session: Option<Session>,
    pub paused: bool,
    pub bonus: [bool; 2],
    /// Sticky for the session (PRD T7).
    pub topped_out: [bool; 2],
    /// The last numbers read, frozen for a player once they top out.
    pub players: [PlayerStats; 2],
    /// The game's result has been reported.
    pub finished: bool,
}

impl TetrisState {
    fn new() -> TetrisState {
        TetrisState {
            screen: None,
            mode: None,
            start_level: [None; 2],
            handicap: [None; 2],
            music: None,
            layout: None,
            session: None,
            paused: false,
            bonus: [false; 2],
            topped_out: [false; 2],
            players: [PlayerStats::default(); 2],
            finished: false,
        }
    }

    /// The result, if a game is on and hasn't been reported yet.
    pub fn result(&self, reason: EndReason) -> Option<GameResult> {
        let playing = !self.finished && self.screen == Some(TetrisScreen::Playing);
        let session = self.session.filter(|_| playing)?;
        Some(GameResult {
            reason,
            session,
            layout: self.layout,
            mode: self.mode,
            start_level: self.start_level,
            handicap: self.handicap,
            players: self.players,
            topped_out: self.topped_out,
        })
    }

    /// The well(s) topped out: the end of the game (PRD T8).
    fn over(&self) -> bool {
        match self.layout {
            Some(Layout::Double) => self.topped_out[0] && self.topped_out[1],
            _ => self.topped_out[0],
        }
    }

    /// e.g. `ModeSelect/1P`, `LevelSelect/L3-5`, `Playing/2P/clean/L3-5/paused/P2 over`.
    pub fn path(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        match self.screen {
            None | Some(TetrisScreen::Unknown) => return String::new(),
            Some(TetrisScreen::Title) => parts.push("Title".into()),
            Some(TetrisScreen::ModeSelect) => {
                parts.push("ModeSelect".into());
                parts.extend(self.mode.map(|m| m.label().to_owned()));
            }
            Some(TetrisScreen::LevelSelect) => {
                parts.push("LevelSelect".into());
                parts.extend(per_player("L", self.start_level));
            }
            Some(TetrisScreen::HandicapSelect) => {
                parts.push("HandicapSelect".into());
                parts.extend(per_player("H", self.handicap));
            }
            Some(TetrisScreen::MusicSelect) => {
                parts.push("MusicSelect".into());
                parts.extend(self.music.map(|m| format!("{m:?}")));
            }
            Some(TetrisScreen::Playing) => {
                parts.push("Playing".into());
                parts.extend(self.layout.map(|l| match l {
                    Layout::Single => "1P".to_owned(),
                    Layout::Double => "2P".to_owned(),
                    Layout::Coop => "coop".to_owned(),
                }));
                parts.extend(self.session.map(|s| format!("{s:?}").to_lowercase()));
                parts.extend(per_player("L", self.start_level));
                if self.paused {
                    parts.push("paused".into());
                }
                for (i, side) in ["P1", "P2"].iter().enumerate() {
                    if self.bonus[i] {
                        parts.push(format!("{side} bonus"));
                    }
                    if self.topped_out[i] {
                        parts.push(format!("{side} over"));
                    }
                }
            }
        }
        parts.join("/")
    }
}

/// `L3` for one player, `L3-5` for two; nothing when unknown.
fn per_player(prefix: &str, values: [Option<u8>; 2]) -> Option<String> {
    match values {
        [Some(a), Some(b)] => Some(format!("{prefix}{a}-{b}")),
        [Some(a), None] => Some(format!("{prefix}{a}")),
        _ => None,
    }
}

/// The next Tetris state, and the result of a game that just ended. `prev` is `None` when the
/// console just entered Tetris; `cold` when it came from no signal (joined mid-game).
pub fn reduce(
    prev: Option<&TetrisState>,
    obs: &TetrisObs,
    cold: bool,
) -> (TetrisState, Option<GameResult>) {
    let mut s = prev.cloned().unwrap_or_else(TetrisState::new);
    // Leaving a game for the splash or the select screens ends it.
    let mut result = match obs.screen {
        TetrisScreen::Playing | TetrisScreen::Unknown => None,
        _ => s.result(EndReason::NewGame),
    };
    match obs.screen {
        TetrisScreen::Unknown => {}
        TetrisScreen::Title => {
            // Back at the splash: whatever game there was is over.
            s = TetrisState { screen: Some(TetrisScreen::Title), ..TetrisState::new() };
        }
        TetrisScreen::ModeSelect => {
            // A new game is being set up.
            s = TetrisState { mode: obs.mode_cursor.or(s.mode), ..TetrisState::new() };
            s.screen = Some(TetrisScreen::ModeSelect);
        }
        TetrisScreen::LevelSelect => {
            s.start_level = [0, 1].map(|i| obs.level_cursor[i].or(s.start_level[i]));
            s.session = None;
            s.topped_out = [false; 2];
            s.screen = Some(TetrisScreen::LevelSelect);
        }
        TetrisScreen::HandicapSelect => {
            s.handicap = [0, 1].map(|i| obs.handicap_cursor[i].or(s.handicap[i]));
            s.session = None;
            s.screen = Some(TetrisScreen::HandicapSelect);
        }
        TetrisScreen::MusicSelect => {
            s.music = obs.music_cursor.or(s.music);
            s.session = None;
            s.screen = Some(TetrisScreen::MusicSelect);
        }
        TetrisScreen::Playing => {
            let starting = match s.screen {
                Some(
                    TetrisScreen::ModeSelect
                    | TetrisScreen::LevelSelect
                    | TetrisScreen::HandicapSelect
                    | TetrisScreen::MusicSelect,
                ) => Some(Session::Clean),
                _ if s.session.is_some() => None,
                _ if prev.is_none() && cold => Some(Session::Dirty),
                // Title (or nothing) → playing: the demo (PRD T3).
                _ => Some(Session::Demo),
            };
            if let Some(session) = starting {
                s.session = Some(session);
                s.topped_out = [false; 2];
                s.players = [PlayerStats::default(); 2];
                s.finished = false;
            }
            s.screen = Some(TetrisScreen::Playing);
            s.layout = obs.layout.or(s.layout);
            s.paused = obs.paused;
            s.bonus = obs.bonus;
            // Numbers from this frame, then frozen once the player has topped out (T7).
            for i in 0..2 {
                if !s.topped_out[i] {
                    s.players[i] = obs.players[i].or(s.players[i]);
                }
            }
            s.topped_out =
                [s.topped_out[0] || obs.game_over[0], s.topped_out[1] || obs.game_over[1]];
            if s.over() {
                result = s.result(EndReason::GameOver);
                s.finished = true;
            }
        }
    }
    (s, result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playing() -> TetrisObs {
        TetrisObs { layout: Some(Layout::Double), ..TetrisObs::screen(TetrisScreen::Playing) }
    }

    fn run(start: Option<TetrisState>, cold: bool, frames: &[TetrisObs]) -> TetrisState {
        let mut state = start;
        for (i, obs) in frames.iter().enumerate() {
            state = Some(reduce(state.as_ref(), obs, cold && i == 0).0);
        }
        state.unwrap()
    }

    #[test]
    fn select_screens_then_playing_is_a_clean_game() {
        let s = run(
            None,
            false,
            &[
                TetrisObs {
                    mode_cursor: Some(TetrisMode::OnePlayer),
                    ..TetrisObs::screen(TetrisScreen::ModeSelect)
                },
                TetrisObs {
                    mode_cursor: Some(TetrisMode::TwoPlayer),
                    ..TetrisObs::screen(TetrisScreen::ModeSelect)
                },
                TetrisObs {
                    level_cursor: [Some(3), Some(5)],
                    ..TetrisObs::screen(TetrisScreen::LevelSelect)
                },
                TetrisObs {
                    handicap_cursor: [Some(0), Some(6)],
                    ..TetrisObs::screen(TetrisScreen::HandicapSelect)
                },
                TetrisObs {
                    music_cursor: Some(TetrisMusic::Troika),
                    ..TetrisObs::screen(TetrisScreen::MusicSelect)
                },
                playing(),
            ],
        );
        assert_eq!(s.mode, Some(TetrisMode::TwoPlayer));
        assert_eq!(s.start_level, [Some(3), Some(5)]);
        assert_eq!(s.handicap, [Some(0), Some(6)]);
        assert_eq!(s.music, Some(TetrisMusic::Troika));
        assert_eq!(s.session, Some(Session::Clean));
        assert_eq!(s.path(), "Playing/2P/clean/L3-5");
    }

    #[test]
    fn title_then_playing_is_the_demo_even_after_a_game() {
        let s = run(None, true, &[playing(), TetrisObs::screen(TetrisScreen::Title), playing()]);
        assert_eq!(s.session, Some(Session::Demo));
        assert_eq!(s.path(), "Playing/2P/demo");
    }

    #[test]
    fn playing_without_select_screens_is_the_demo() {
        let s = run(None, false, &[TetrisObs::screen(TetrisScreen::Unknown), playing()]);
        assert_eq!(s.session, Some(Session::Demo));
    }

    #[test]
    fn cold_start_mid_game_is_dirty() {
        assert_eq!(run(None, true, &[playing()]).session, Some(Session::Dirty));
    }

    #[test]
    fn game_over_is_sticky_and_pause_is_not() {
        let over_left = TetrisObs { game_over: [true, false], paused: true, ..playing() };
        let s = run(None, true, &[playing(), over_left, playing()]);
        assert_eq!(s.topped_out, [true, false]);
        assert!(!s.paused);
        assert_eq!(s.path(), "Playing/2P/dirty/P1 over");
    }

    #[test]
    fn unknown_screen_keeps_the_state() {
        let s = run(None, false, &[playing()]);
        assert_eq!(reduce(Some(&s), &TetrisObs::screen(TetrisScreen::Unknown), false), (s, None));
    }

    #[test]
    fn new_game_resets_the_session() {
        let s = run(
            None,
            true,
            &[
                TetrisObs { game_over: [true, true], ..playing() },
                TetrisObs::screen(TetrisScreen::ModeSelect),
                TetrisObs::screen(TetrisScreen::LevelSelect),
                playing(),
            ],
        );
        assert_eq!(s.session, Some(Session::Clean));
        assert_eq!(s.topped_out, [false, false]);
    }

    fn scores(p1: u32, p2: u32) -> [PlayerStats; 2] {
        [p1, p2].map(|score| PlayerStats {
            score: Some(score),
            lines: Some(score / 100),
            level: Some(3),
        })
    }

    #[test]
    fn two_player_game_ends_when_both_top_out_with_frozen_scores() {
        let mut state = None;
        let mut results = Vec::new();
        let frames = [
            TetrisObs::screen(TetrisScreen::MusicSelect),
            TetrisObs { players: scores(100, 200), ..playing() },
            // P1 tops out at 500; the number stays even if the HUD showed more.
            TetrisObs { players: scores(500, 900), game_over: [true, false], ..playing() },
            TetrisObs { players: scores(700, 1500), game_over: [true, false], ..playing() },
            TetrisObs { players: scores(700, 2000), game_over: [true, true], ..playing() },
            TetrisObs { players: scores(700, 2000), game_over: [true, true], ..playing() },
        ];
        for obs in &frames {
            let (s, result) = reduce(state.as_ref(), obs, false);
            results.extend(result);
            state = Some(s);
        }
        assert_eq!(results.len(), 1, "reported once");
        let r = &results[0];
        assert_eq!((r.reason, r.session), (EndReason::GameOver, Session::Clean));
        assert_eq!(r.players[0].score, Some(500));
        assert_eq!(r.players[1].score, Some(2000));
        assert_eq!(r.topped_out, [true, true]);
    }

    #[test]
    fn one_player_game_ends_at_its_top_out() {
        let single = |players, game_over| TetrisObs {
            layout: Some(Layout::Single),
            players,
            game_over,
            ..playing()
        };
        let (s, _) = reduce(None, &single(scores(9588, 0), [false; 2]), true);
        let (_, result) = reduce(Some(&s), &single(scores(10253, 0), [true, false]), false);
        let r = result.unwrap();
        assert_eq!((r.reason, r.session), (EndReason::GameOver, Session::Dirty));
        assert_eq!(r.players[0].score, Some(10253));
    }

    #[test]
    fn going_back_to_select_screens_ends_an_unfinished_game() {
        let (s, _) = reduce(None, &TetrisObs { players: scores(42, 7), ..playing() }, true);
        let (_, result) = reduce(Some(&s), &TetrisObs::screen(TetrisScreen::ModeSelect), false);
        assert_eq!(result.unwrap().reason, EndReason::NewGame);
    }
}
