//! The flat-stone mark drawn in terminal cells, for the home screen.
//!
//! [`PROFILE`] samples the stone in `assets/brand/whetstone-logo.svg` at the
//! centre of each square of a 32 by 8 grid laid over it. A terminal cell is
//! about twice as tall as it is wide, so each cell shows two samples, one
//! above the other, with half blocks: the brand manual's 32x4 profile.

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span, Text};

use crate::theme::Stone;

/// The stone's bands: `e` the working edge, `b` the body, `d` the base, and
/// `.` outside the stone.
const PROFILE: [&str; 8] = [
    "eeeeeeeeeeeeeeeeeeeeeeeee.......",
    "eeeeeeeeeeeeeeeeeeeeeeeeee......",
    "bbbbbbbbbbbbbbbbbbbbbbbbeee.....",
    "bbbbbbbbbbbbbbbbbbbbbbbbbeee....",
    "bbbbbbbbbbbbbbbbbbbbbbbbbbeee...",
    "bbbbbbbbbbbbbbbbbbbbbbbbbbbeee..",
    "ddddddddddddddddddddddddddddeee.",
    "dddddddddddddddddddddddddddddeee",
];

/// The mark's width in cells.
pub(crate) const WIDTH: u16 = 32;
/// The mark's height in cells.
pub(crate) const HEIGHT: u16 = 4;

/// The mark with its bands in `stone`'s colours.
pub(crate) fn text(stone: Stone) -> Text<'static> {
    PROFILE
        .as_chunks::<2>()
        .0
        .iter()
        .map(|[top, bottom]| {
            top.bytes()
                .zip(bottom.bytes())
                .map(|(top, bottom)| cell(band(stone, top), band(stone, bottom)))
                .collect::<Line<'_>>()
        })
        .collect()
}

fn band(stone: Stone, sample: u8) -> Option<Color> {
    match sample {
        b'e' => Some(stone.edge),
        b'b' => Some(stone.body),
        b'd' => Some(stone.base),
        _ => None,
    }
}

/// One cell showing `top` above `bottom`. `None` leaves the canvas showing.
fn cell(top: Option<Color>, bottom: Option<Color>) -> Span<'static> {
    match (top, bottom) {
        (None, None) => Span::raw(" "),
        (Some(top), None) => Span::styled("▀", Style::new().fg(top)),
        (None, Some(bottom)) => Span::styled("▄", Style::new().fg(bottom)),
        (Some(top), Some(bottom)) if top == bottom => Span::styled("█", Style::new().fg(top)),
        // As a background, `Reset` is the terminal's background rather than
        // its text colour, so a `Reset` band always takes the glyph.
        (Some(top), Some(Color::Reset)) => Span::styled("▄", Style::new().fg(Color::Reset).bg(top)),
        (Some(top), Some(bottom)) => Span::styled("▀", Style::new().fg(top).bg(bottom)),
    }
}
