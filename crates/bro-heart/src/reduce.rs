use serde::{Deserialize, Serialize};

use crate::state::{ConsoleState, GameDetail, GameId, Observation, Seen};

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
}

/// Next state and the events of the transition. Start from `ConsoleState::NoSignal`, which
/// behaves like "no previous state" in every rule.
pub fn reduce(prev: &ConsoleState, obs: &Observation) -> (ConsoleState, Vec<Event>) {
    let next = match (&obs.seen, prev) {
        (Seen::SignalLost, _) => ConsoleState::NoSignal,
        // R5: positive recognition always wins.
        (Seen::Menu(m), _) => ConsoleState::Menu(m.clone()),
        (Seen::Game(game), _) => enter_game(prev, *game),
        // R4: black frames keep the game (level transitions) and the menu.
        (Seen::Black, _) => prev.clone(),
        // R4: the game is kept; the frame itself goes to the review queue (outside `reduce`).
        (Seen::Unrecognized, ConsoleState::InGame { .. }) => prev.clone(),
        (Seen::Unrecognized, _) => enter_game(prev, GameId::Other),
    };

    let (from, to) = (prev.path(), next.path());
    let events = match from == to {
        true => Vec::new(),
        false => vec![Event { frame_no: obs.frame_no, kind: EventKind::Changed { from, to } }],
    };
    (next, events)
}

fn enter_game(prev: &ConsoleState, game: GameId) -> ConsoleState {
    let menu_selection = match prev {
        ConsoleState::InGame { game: g, .. } if *g == game => return prev.clone(),
        ConsoleState::InGame { menu_selection, .. } => menu_selection.clone(),
        ConsoleState::Menu(m) => m.highlighted.clone(),
        ConsoleState::NoSignal => None,
    };
    ConsoleState::InGame { game, detail: GameDetail::None, menu_selection }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::MenuState;

    fn menu(cursor: u8, title: &str) -> ConsoleState {
        ConsoleState::Menu(MenuState { page: 1, cursor, highlighted: Some(title.into()) })
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
        let prev = menu(3, "Contra");
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
        let (next, events) = step(&menu(3, "Contra"), Seen::Unrecognized);
        assert_eq!(
            next,
            ConsoleState::InGame {
                game: GameId::Other,
                detail: GameDetail::None,
                menu_selection: Some("Contra".into()),
            }
        );
        assert_eq!(
            events,
            vec![EventKind::Changed { from: "Menu/p1/c3".into(), to: "InGame/Other".into() }]
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
        let (other, _) = step(&menu(5, "Tetris"), Seen::Unrecognized);
        let (next, _) = step(&other, Seen::Game(GameId::Tetris));
        assert_eq!(
            next,
            ConsoleState::InGame {
                game: GameId::Tetris,
                detail: GameDetail::None,
                menu_selection: Some("Tetris".into()),
            }
        );
    }

    #[test]
    fn same_game_again_is_no_event() {
        let prev = in_game(GameId::Jewelry);
        assert_eq!(step(&prev, Seen::Game(GameId::Jewelry)), (prev, vec![]));
    }

    #[test]
    fn signal_lost_is_no_signal_from_anywhere() {
        let (next, events) = step(&in_game(GameId::Tetris), Seen::SignalLost);
        assert_eq!(next, ConsoleState::NoSignal);
        assert_eq!(events.len(), 1);
    }
}
