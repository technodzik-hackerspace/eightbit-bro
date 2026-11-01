//! The "300 IN 1 VER2.0" menu (PRD §5.3). Recognized on the half-scale canvas by its header
//! and blue background; read on the full-resolution canvas, where the font is exact: the
//! arrow's row and the 20 numbered games.

use std::ops::Range;

use bro_heart::{MenuState, Title};
use bro_vision::{NesCanvas, Probe, Rect};

use crate::font;

/// "300 IN 1 VER2.0". The scenery below the list scrolls, so it can't be a signature.
const HEADER: Rect = Rect { x1: 92, y1: 17, x2: 216, y2: 29 };
const HEADER_MAX_MISMATCH: f32 = 0.12;
/// The list area: mostly the menu's blue.
const LIST: Rect = Rect { x1: 8, y1: 40, x2: 232, y2: 190 };
const MIN_BLUE: f32 = 0.5;
/// Where the menu's blue is sampled on the reference frame.
const BLUE_SAMPLE: Rect = Rect { x1: 8, y1: 100, x2: 20, y2: 180 };

pub const ROWS: usize = 20;
/// Text cells sit at x = 7 + 8 j, y = 32 + 8 row, shifted per frame by up to a couple of
/// pixels: the menu scrolls by a line between screens.
const CELL_X: i32 = 7;
const CELL_Y: i32 = 32;
const DX: Range<i32> = -2..3;
const DY: Range<i32> = -2..2;
const ARROW_COL: i32 = 2;
const TEXT_COLS: Range<i32> = 3..28;
/// Columns used to find the offset: numbers and the start of names.
const ALIGN_COLS: Range<i32> = 3..13;
/// Red pixels in the arrow cell for the cursor to be on that row (the arrow has ~12).
const MIN_ARROW: usize = 6;

pub struct Menu {
    header: Probe,
    blue: [u8; 3],
}

/// What is written on a menu frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuRead {
    /// Each row as on screen, e.g. `"221.RAID ON BUNGELING BAY"`, `'?'` for unknown cells.
    pub text: Vec<String>,
    /// The arrow's row.
    pub cursor: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuRow {
    pub number: u16,
    pub name: String,
}

impl Menu {
    pub fn new(reference: &NesCanvas) -> Menu {
        let samples: Vec<[u8; 3]> = (BLUE_SAMPLE.y1..BLUE_SAMPLE.y2)
            .flat_map(|y| (BLUE_SAMPLE.x1..BLUE_SAMPLE.x2).map(move |x| (x, y)))
            .map(|(x, y)| reference.rgb_at(x, y))
            .collect();
        let blue = [0, 1, 2].map(|i| {
            (samples.iter().map(|c| c[i] as u32).sum::<u32>() / samples.len() as u32) as u8
        });
        Menu { header: Probe::capture(reference, HEADER), blue }
    }

    pub fn claims(&self, c: &NesCanvas) -> bool {
        self.header.mismatch(c, 2) <= HEADER_MAX_MISMATCH && self.blue_share(c) >= MIN_BLUE
    }

    fn blue_share(&self, c: &NesCanvas) -> f32 {
        let mut blue = 0;
        for y in LIST.y1..LIST.y2 {
            for x in LIST.x1..LIST.x2 {
                let px = c.rgb_at(x, y);
                blue += (0..3).all(|i| px[i].abs_diff(self.blue[i]) <= 48) as u32;
            }
        }
        blue as f32 / LIST.area() as f32
    }

    /// Reads the cursor and the rows from a full-resolution canvas.
    pub fn read(&self, c: &NesCanvas) -> MenuRead {
        let (dx, dy) = align(c);
        let text = (0..ROWS as i32)
            .map(|row| {
                let line: String = TEXT_COLS
                    .map(|j| font::char_for(cell(c, j, row, dx, dy)).unwrap_or('?'))
                    .collect();
                line.trim_end().to_owned()
            })
            .collect();
        let arrows: Vec<usize> = (0..ROWS as i32).map(|row| arrow(c, row, dx, dy)).collect();
        let (row, &most) = arrows.iter().enumerate().max_by_key(|(i, n)| (**n, ROWS - i)).unwrap();
        let cursor = (most >= MIN_ARROW).then_some(row as u8);
        MenuRead { text, cursor }
    }
}

impl MenuRead {
    /// Row `i` as number and name, when fully read.
    pub fn row(&self, i: usize) -> Option<MenuRow> {
        let text = self.text.get(i)?;
        let digits = text.bytes().take_while(u8::is_ascii_digit).count();
        let number: u16 = text.get(..digits)?.parse().ok()?;
        let name = text.get(digits..)?.strip_prefix(['.', ' '])?.trim();
        let ok = (1..=300).contains(&number) && !name.is_empty() && !name.contains('?');
        ok.then(|| MenuRow { number, name: name.to_owned() })
    }

    /// Row `i` belongs to the page its number says (pages are redrawn row by row, so a frame
    /// can show two pages).
    fn in_place(&self, i: usize) -> Option<MenuRow> {
        self.row(i).filter(|r| (r.number as usize - 1) % ROWS == i)
    }

    /// 1-based page: from the cursor's row, else the first row in place.
    pub fn page(&self) -> Option<u8> {
        let row = self.cursor.and_then(|c| self.in_place(c as usize));
        let row = row.or_else(|| (0..ROWS).find_map(|i| self.in_place(i)))?;
        Some(((row.number - 1) / ROWS as u16 + 1) as u8)
    }

    /// The whole page is on screen and read: all 20 rows in place, from one page.
    pub fn complete_page(&self) -> Option<u8> {
        let rows: Vec<MenuRow> = (0..ROWS).map(|i| self.in_place(i)).collect::<Option<_>>()?;
        let page = (rows[0].number - 1) / ROWS as u16;
        rows.iter().all(|r| (r.number - 1) / ROWS as u16 == page).then_some(page as u8 + 1)
    }

    /// The game under the cursor, when its number and name match the catalog.
    pub fn highlighted(&self) -> Option<Title> {
        let row = self.row(self.cursor? as usize)?;
        Title::from_number(row.number).filter(|t| t.name() == row.name)
    }

    /// The menu state, filling in what this frame doesn't show from the previous one.
    pub fn state(&self, previous: Option<&MenuState>) -> MenuState {
        MenuState {
            page: self.page().or(previous.map(|p| p.page)).unwrap_or(1),
            cursor: self.cursor.or(previous.map(|p| p.cursor)).unwrap_or(0),
            highlighted: self.highlighted(),
        }
    }
}

/// The cell offset with the most readable glyphs; ties go to the smallest shift.
fn align(c: &NesCanvas) -> (i32, i32) {
    let mut best: (usize, (i32, i32)) = (0, (0, 0));
    for dy in DY {
        for dx in DX {
            let read = (0..ROWS as i32)
                .flat_map(|row| ALIGN_COLS.map(move |j| (row, j)))
                .map(|(row, j)| cell(c, j, row, dx, dy))
                .filter(|&bits| bits & font::MASK != 0 && font::char_for(bits).is_some())
                .count();
            let closer = dx.abs() + dy.abs() < best.1.0.abs() + best.1.1.abs();
            if read > best.0 || (read == best.0 && closer) {
                best = (read, (dx, dy));
            }
        }
    }
    best.1
}

/// The ink bits of a text cell (row 0 left out, see `font`).
fn cell(c: &NesCanvas, j: i32, row: i32, dx: i32, dy: i32) -> u64 {
    let (x0, y0) = (CELL_X + 8 * j + dx, CELL_Y + 8 * row + dy);
    let mut bits = 0u64;
    for r in 1..8 {
        for col in 0..8 {
            let (x, y) = (x0 + col, y0 + r);
            if (0..256).contains(&x)
                && (0..240).contains(&y)
                && c.luma_at(x as u32, y as u32) > font::INK
            {
                bits |= 1 << (r * 8 + col);
            }
        }
    }
    bits
}

/// Red pixels in the arrow cell of a row.
fn arrow(c: &NesCanvas, row: i32, dx: i32, dy: i32) -> usize {
    let (x0, y0) = (CELL_X + 8 * ARROW_COL + dx, CELL_Y + 8 * row + dy);
    (y0..y0 + 8)
        .flat_map(|y| (x0..x0 + 8).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let [r, g, b] = c.rgb_at(x as u32, y as u32);
            r > 170 && g < 120 && b < 120
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &[&str], cursor: Option<u8>) -> MenuRead {
        let mut text: Vec<String> = text.iter().map(|s| s.to_string()).collect();
        text.resize(ROWS, String::new());
        MenuRead { text, cursor }
    }

    #[test]
    fn rows_parse_with_or_without_the_dot() {
        let r = read(
            &["1.ADVENTURE ISLAND", "2 ADVENTURE ISLAND 2", "299.10-YARD FIGHT", "4?X", "5."],
            None,
        );
        assert_eq!(r.row(0), Some(MenuRow { number: 1, name: "ADVENTURE ISLAND".into() }));
        assert_eq!(r.row(1).unwrap().name, "ADVENTURE ISLAND 2");
        assert_eq!(r.row(2), Some(MenuRow { number: 299, name: "10-YARD FIGHT".into() }));
        assert_eq!(r.row(3), None);
        assert_eq!(r.row(4), None);
    }

    #[test]
    fn page_and_highlight_come_from_the_cursor_row() {
        let rows: Vec<String> = (0..20)
            .map(|i| format!("{}.{}", 261 + i, if i == 6 { "TETRIS II" } else { "X" }))
            .collect();
        let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
        let r = read(&rows, Some(6));
        assert_eq!(r.page(), Some(14));
        assert_eq!(r.complete_page(), Some(14));
        assert_eq!(r.highlighted(), Some(Title::TetrisIi));
        // A wrong name under the cursor is not trusted.
        let r = read(&rows, Some(5));
        assert_eq!(r.highlighted(), None);
    }

    #[test]
    fn mid_redraw_frame_is_not_a_complete_page() {
        let mut rows: Vec<String> = (0..20).map(|i| format!("{}.X", 21 + i)).collect();
        rows[18] = "19.B-WINGS".into();
        rows[19] = "20.BABEL NO TOU".into();
        let r = read(&rows.iter().map(String::as_str).collect::<Vec<_>>(), Some(0));
        assert_eq!(r.page(), Some(2));
        assert_eq!(r.complete_page(), None);
    }

    #[test]
    fn missing_cursor_keeps_the_previous_one() {
        let r = read(&["1.ADVENTURE ISLAND"], None);
        let prev = MenuState { page: 1, cursor: 4, highlighted: None };
        assert_eq!(r.state(Some(&prev)).cursor, 4);
        assert_eq!(r.state(None).cursor, 0);
    }
}
