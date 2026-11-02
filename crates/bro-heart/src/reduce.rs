use serde::{Deserialize, Serialize};

use crate::state::{ConsoleState, GameDetail, GameId, GameObs, Observation, Seen};
use crate::tetris::{self, EndReason, GameResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub frame_no: u64,
    pub kind: EventKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventKind {
    /// The state path changed (PRD E1).
    Changed { from: String, to: String },
    /// A frame nobody claimed was saved to the review queue as a new item (PRD R4, L1).
    /// Emitted by the review queue, once per deduplicated item, not by `reduce`.
    UnknownScreen { item: i64, state: String },
    /// A game ended, with its result (PRD E2, E3, T8).
    GameFinished { game: GameId, result: GameResult },
}

/// Next state and the events of the transition. Start from `ConsoleState::NoSignal`, which
/// behaves like "no previous state" in every rule.
pub fn reduce(prev: &ConsoleState, obs: &Observation) -> (ConsoleState, Vec<Event>) {
    let mut result = None;
    let next = match (&obs.seen, prev) {
        (Seen::SignalLost, _) => ConsoleState::NoSignal,
        // R5: positive recognition always wins.
        (Seen::Menu(m), _) => ConsoleState::Menu(m.clone()),
        (Seen::Game(game, obs), _) => {
            let (next, finished) = enter_game(prev, *game, obs);
            result = finished;
            next
        }
        // R4: black frames keep the game (level transitions) and the menu.
        (Seen::Black, _) => prev.clone(),
        // R4: the game is kept; the frame itself goes to the review queue (outside `reduce`).
        (Seen::Unrecognized, ConsoleState::InGame { .. }) => prev.clone(),
        (Seen::Unrecognized, _) => enter_game(prev, GameId::Other, &GameObs::None).0,
    };
    // Leaving Tetris mid-game (reset to the menu, another game, no signal) ends the game.
    if let ConsoleState::InGame { game: GameId::Tetris, detail: GameDetail::Tetris(t), .. } = prev {
        let still_tetris = matches!(next, ConsoleState::InGame { game: GameId::Tetris, .. });
        if !still_tetris {
            result = t.result(EndReason::Left);
        }
    }

    let mut events = Vec::new();
    let (from, to) = (prev.path(), next.path());
    if from != to {
        events.push(Event { frame_no: obs.frame_no, kind: EventKind::Changed { from, to } });
    }
    if let Some(result) = result {
        let kind = EventKind::GameFinished { game: GameId::Tetris, result };
        events.push(Event { frame_no: obs.frame_no, kind });
    }
    (next, events)
}

fn enter_game(
    prev: &ConsoleState,
    game: GameId,
    obs: &GameObs,
) -> (ConsoleState, Option<GameResult>) {
    let (prev_detail, menu_selection) = match prev {
        ConsoleState::InGame { game: g, detail, menu_selection } if *g == game => {
            (Some(detail), *menu_selection)
        }
        ConsoleState::InGame { menu_selection, .. } => (None, *menu_selection),
        ConsoleState::Menu(m) => (None, m.highlighted),
        ConsoleState::NoSignal => (None, None),
    };
    let cold = *prev == ConsoleState::NoSignal;
    let mut result = None;
    let detail = match obs {
        GameObs::None => prev_detail.cloned().unwrap_or(GameDetail::None),
        GameObs::Tetris(o) => {
            let prev = match prev_detail {
                Some(GameDetail::Tetris(t)) => Some(t),
                _ => None,
            };
            let (state, finished) = tetris::reduce(prev, o, cold);
            result = finished;
            GameDetail::Tetris(state)
        }
        // An unknown screen keeps the last known one (R4).
        GameObs::Screen(o) if o.screen.is_none() => {
            prev_detail.cloned().unwrap_or(GameDetail::None)
        }
        GameObs::Screen(o) => GameDetail::Screen(o.clone()),
    };
    (ConsoleState::InGame { game, detail, menu_selection }, result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::MenuState;
    use crate::title::Title;

    fn menu(cursor: u8, title: Title) -> ConsoleState {
        ConsoleState::Menu(MenuState { page: 1, cursor, highlighted: Some(title) })
    }

    fn in_game(game: GameId) -> ConsoleState {
        ConsoleState::InGame { game, detail: GameDetail::None, menu_selection: None }
    }

    fn step(prev: &ConsoleState, seen: Seen) -> (ConsoleState, Vec<EventKind>) {
        let (next, events) = reduce(prev, &Observation { frame_no: 7, seen });
        (next, events.into_iter().map(|e| e.kind).collect())
    }

    // PRD §5.2 R4, one test per row of the inheritance table.

    #[test]
    fn r4_black_in_game_keeps_game() {
        let prev = in_game(GameId::Tetris);
        assert_eq!(step(&prev, Seen::Black), (prev, vec![]));
    }

    #[test]
    fn r4_black_without_state_is_no_signal() {
        assert_eq!(step(&ConsoleState::NoSignal, Seen::Black), (ConsoleState::NoSignal, vec![]));
    }

    #[test]
    fn r4_black_in_menu_keeps_menu() {
        let prev = menu(3, Title::Argus);
        assert_eq!(step(&prev, Seen::Black), (prev, vec![]));
    }

    #[test]
    fn r4_unrecognized_in_game_keeps_game() {
        for game in [GameId::Tetris, GameId::Other] {
            let prev = in_game(game);
            assert_eq!(step(&prev, Seen::Unrecognized), (prev, vec![]));
        }
    }

    #[test]
    fn r4_unrecognized_without_state_is_other() {
        let (next, _) = step(&ConsoleState::NoSignal, Seen::Unrecognized);
        assert_eq!(next, in_game(GameId::Other));
    }

    #[test]
    fn r4_unrecognized_from_menu_is_other_with_menu_selection() {
        let (next, events) = step(&menu(3, Title::Argus), Seen::Unrecognized);
        assert_eq!(
            next,
            ConsoleState::InGame {
                game: GameId::Other,
                detail: GameDetail::None,
                menu_selection: Some(Title::Argus),
            }
        );
        assert_eq!(
            events,
            vec![EventKind::Changed { from: "Menu/p1/c3/ARGUS".into(), to: "InGame/Other".into() }]
        );
    }

    #[test]
    fn r5_menu_during_game_means_reset() {
        let (next, _) = step(
            &in_game(GameId::Tetris),
            Seen::Menu(MenuState { page: 1, cursor: 0, highlighted: None }),
        );
        assert_eq!(next.path(), "Menu/p1/c0");
    }

    #[test]
    fn r5_recognized_game_replaces_other_and_keeps_menu_selection() {
        let (other, _) = step(&menu(5, Title::TetrisIi), Seen::Unrecognized);
        let (next, _) = step(&other, Seen::Game(GameId::Tetris, GameObs::None));
        assert_eq!(
            next,
            ConsoleState::InGame {
                game: GameId::Tetris,
                detail: GameDetail::None,
                menu_selection: Some(Title::TetrisIi),
            }
        );
    }

    #[test]
    fn same_game_again_is_no_event() {
        let prev = in_game(GameId::Jewelry);
        assert_eq!(step(&prev, Seen::Game(GameId::Jewelry, GameObs::None)), (prev, vec![]));
    }

    #[test]
    fn signal_lost_is_no_signal_from_anywhere() {
        let (next, events) = step(&in_game(GameId::Tetris), Seen::SignalLost);
        assert_eq!(next, ConsoleState::NoSignal);
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn tetris_detail_follows_the_screens() {
        use crate::tetris::{TetrisMode, TetrisObs, TetrisScreen};
        let mode = TetrisObs {
            mode_cursor: Some(TetrisMode::OnePlayer),
            ..TetrisObs::screen(TetrisScreen::ModeSelect)
        };
        let (s, events) =
            step(&menu(6, Title::TetrisIi), Seen::Game(GameId::Tetris, GameObs::Tetris(mode)));
        assert_eq!(s.path(), "InGame/Tetris/ModeSelect/1P");
        assert_eq!(
            events,
            vec![EventKind::Changed {
                from: "Menu/p1/c6/TETRIS II".into(),
                to: "InGame/Tetris/ModeSelect/1P".into()
            }]
        );
        let (s, _) = step(&s, Seen::Unrecognized);
        assert_eq!(s.path(), "InGame/Tetris/ModeSelect/1P", "unknown frames keep the state");
    }

    #[test]
    fn screen_table_games_keep_the_last_known_screen() {
        let screen = |s: Option<&str>, flags: &[&str]| {
            Seen::Game(
                GameId::DrMario,
                GameObs::Screen(crate::ScreenObs {
                    screen: s.map(str::to_owned),
                    flags: flags.iter().map(|f| f.to_string()).collect(),
                }),
            )
        };
        let (s, _) = step(&ConsoleState::NoSignal, screen(Some("Playing 2P"), &["P1 over"]));
        assert_eq!(s.path(), "InGame/DrMario/Playing 2P/P1 over");
        let unknown = screen(None, &[]);
        assert!(unknown.needs_review());
        assert_eq!(step(&s, unknown).0.path(), "InGame/DrMario/Playing 2P/P1 over");
    }

    #[test]
    fn reset_to_the_menu_mid_game_reports_the_game() {
        use crate::tetris::{Layout, PlayerStats, TetrisObs, TetrisScreen};
        let playing = TetrisObs {
            layout: Some(Layout::Single),
            players: [
                PlayerStats { score: Some(4910), lines: Some(9), level: Some(9) },
                PlayerStats::default(),
            ],
            ..TetrisObs::screen(TetrisScreen::Playing)
        };
        let (s, events) =
            step(&ConsoleState::NoSignal, Seen::Game(GameId::Tetris, GameObs::Tetris(playing)));
        assert_eq!(events.len(), 1, "no result yet");
        let (_, events) =
            step(&s, Seen::Menu(MenuState { page: 14, cursor: 6, highlighted: None }));
        let Some(EventKind::GameFinished { result, .. }) = events.last() else {
            panic!("{events:?}")
        };
        assert_eq!(result.reason, EndReason::Left);
        assert_eq!(result.players[0].score, Some(4910));
    }
}
