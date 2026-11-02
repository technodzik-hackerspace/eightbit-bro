//! The heart of eightbit-bro: the console state tree (PRD §5), what perception saw in a frame,
//! and the reducer that turns the two into the next state plus events.
//!
//! Everything here is pure: no IO, no clocks. Same observations in, same states and events out
//! (PRD R2).

mod reduce;
mod state;
pub mod tetris;
mod title;

pub use reduce::{Event, EventKind, reduce};
pub use state::{ConsoleState, GameDetail, GameId, GameObs, MenuState, Observation, Seen};
pub use title::{TITLES, Title};
