//! Tetris HUD numbers (PRD T9): score, lines and level per player, read on the
//! full-resolution canvas with the HUD's own digit font (learned from the Pi's 720p capture,
//! 2026-11-01/02; at half scale some digits blur together).
//!
//! Digit cells are 8 px wide at x ≡ 7 (mod 8): player 1's numbers end at x = 95 (read right to
//! left), player 2's start at x = 151 (left to right). Rows: score y = 16, lines 32, level 48.

use bro_heart::tetris::{Layout, PlayerStats};
use bro_vision::NesCanvas;

/// Bit `row * 8 + col` for the 7 glyph rows below the cell top.
const DIGITS: &[(char, u64)] = &[
    ('0', 0x001c26636363321c),
    ('1', 0x003f0c0c0c0c0e0c),
    ('2', 0x007f071e3c70633e),
    ('3', 0x003e63603c18307e),
    ('4', 0x0030307f33363c38),
    ('5', 0x003e6360603f033f),
    ('6', 0x003e63633f03063c),
    ('7', 0x000c0c0c1830637f),
    ('8', 0x003e61791e27231e),
    ('9', 0x001e30607e63633e),
];

/// Luma above this is ink (white digits on black).
const INK: u8 = 140;
const ROWS: [u32; 3] = [16, 32, 48];
const P1_LAST: u32 = 95;
const P2_FIRST: u32 = 151;
const MAX_DIGITS: u32 = 7;

/// The players' numbers for a playing screen. Player 2 only in two-player layouts: in 1P the
/// same places hold HIGH SCORE and STATS.
pub fn read(c: &NesCanvas, layout: Option<Layout>) -> [PlayerStats; 2] {
    let p2 = matches!(layout, Some(Layout::Double | Layout::Coop));
    [read_player(c, 0), if p2 { read_player(c, 1) } else { PlayerStats::default() }]
}

fn read_player(c: &NesCanvas, player: usize) -> PlayerStats {
    let [score, lines, level] = ROWS.map(|y| number(c, y, player));
    PlayerStats { score, lines, level }
}

/// The number in a row: player 1's right-aligned, player 2's left-aligned. `None` when
/// empty, or when any cell isn't a digit (never guessed).
fn number(c: &NesCanvas, y: u32, player: usize) -> Option<u32> {
    let mut digits = Vec::new();
    for k in 0..MAX_DIGITS {
        let x = if player == 0 { P1_LAST.checked_sub(8 * k)? } else { P2_FIRST + 8 * k };
        let bits = cell(c, x, y);
        if bits == 0 {
            break;
        }
        digits.push(digit(bits)?);
    }
    if player == 0 {
        digits.reverse();
    }
    digits
        .iter()
        .try_fold(0u32, |n, &d| n.checked_mul(10)?.checked_add(d))
        .filter(|_| !digits.is_empty())
}

fn cell(c: &NesCanvas, x0: u32, y0: u32) -> u64 {
    let mut bits = 0;
    for r in 0..7 {
        for col in 0..8 {
            let (x, y) = (x0 + col, y0 + 1 + r);
            if x < 256 && y < 240 && c.luma_at(x, y) > INK {
                bits |= 1 << (r * 8 + col);
            }
        }
    }
    bits
}

/// The digit within one pixel of `bits`, if exactly one is that close.
fn digit(bits: u64) -> Option<u32> {
    let (mut best, mut second) = ((u32::MAX, 0), u32::MAX);
    for &(ch, glyph) in DIGITS {
        let d = (bits ^ glyph).count_ones();
        if d < best.0 {
            second = best.0;
            best = (d, ch.to_digit(10).unwrap());
        } else if d < second {
            second = d;
        }
    }
    (best.0 <= 1 && second > best.0).then_some(best.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_are_distinct() {
        for &(ch, glyph) in DIGITS {
            assert_eq!(digit(glyph), ch.to_digit(10));
            assert_eq!(digit(glyph ^ 1 << 9), ch.to_digit(10), "one pixel off");
        }
    }
}
