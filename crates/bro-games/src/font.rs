//! The menu font: 8×8 tiles, matched on the bright mask of the full-resolution canvas.
//!
//! Learned from menu pages read off the console (`fixtures/frames/pi-2026-11-01`). Bit
//! `row * 8 + col` is set for a bright pixel. Row 0 is always blank in this font, and is left
//! out of matching: Q's tail reaches into the next line's row 0.

/// Luma above this is ink. Text is white (≈ 220) on blue (≈ 90).
pub const INK: u8 = 175;

/// Row 0 left out.
pub const MASK: u64 = !0xff;

pub const GLYPHS: &[(char, u64)] = &[
    (' ', 0x0000000000000000),
    ('&', 0x4c322a0c12120c00),
    ('\'', 0x0000000000040800),
    ('-', 0x0000007c00000000),
    ('.', 0x0800000000000000),
    ('0', 0x3c66666666663c00),
    ('1', 0x18181818181c1800),
    ('2', 0x7e060c3860663c00),
    ('3', 0x3c66603860663c00),
    ('4', 0x30307f33363c3800),
    ('5', 0x3c6660603e067e00),
    ('6', 0x3c66663e06663c00),
    ('7', 0x1818183060667e00),
    ('8', 0x3c66663c66663c00),
    ('9', 0x3c66607c66663c00),
    ('A', 0x6363637f63633e00),
    ('B', 0x3f63633f63633f00),
    ('C', 0x3e63030303633e00),
    ('D', 0x3f63636363633f00),
    ('E', 0x7f03031f03037f00),
    ('F', 0x0303031f03037f00),
    ('G', 0x3e63637303633e00),
    ('H', 0x6363637f63636300),
    ('I', 0x7e18181818187e00),
    ('J', 0x3c66606060606000),
    ('K', 0x63331b0f1b336300),
    ('L', 0x7e06060606060600),
    ('M', 0x6363636b7f776300),
    ('N', 0x6363737b6f676300),
    ('O', 0x3e63636363633e00),
    ('P', 0x0303033f63633f00),
    ('Q', 0x3e737b6363633e00),
    ('R', 0x63331b3f63633f00),
    ('S', 0x3c66603c06663c00),
    ('T', 0x1818181818187e00),
    ('U', 0x3e63636363636300),
    ('V', 0x081c3e6363636300),
    ('W', 0x63777f6b63636300),
    ('X', 0xc3663c183c66c300),
    ('Y', 0x1818183c66666600),
    ('Z', 0x7f060c1830607f00),
    ('_', 0x007e000000000000),
];

/// The character whose glyph is within one pixel of `bits`, if exactly one is that close.
pub fn char_for(bits: u64) -> Option<char> {
    let (mut best, mut second) = ((u32::MAX, ' '), u32::MAX);
    for &(ch, glyph) in GLYPHS {
        let d = ((bits ^ glyph) & MASK).count_ones();
        if d < best.0 {
            second = best.0;
            best = (d, ch);
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
    fn glyphs_are_distinct_and_exact_matches_read() {
        for &(ch, glyph) in GLYPHS {
            assert_eq!(char_for(glyph), Some(ch), "{ch:?}");
            assert_eq!(glyph & !MASK, 0);
        }
    }

    #[test]
    fn one_pixel_off_still_reads_two_does_not() {
        let a = GLYPHS.iter().find(|(c, _)| *c == 'A').unwrap().1;
        let one = a ^ (1 << 20);
        assert_eq!(char_for(one), Some('A'));
        assert_eq!(char_for(a ^ (1 << 20) ^ (1 << 30) ^ (1 << 40)), None);
    }
}
