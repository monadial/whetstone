//! Helpers for the colour tests: the colours a frame uses, and the brand
//! tokens they must come from.

use std::collections::HashSet;

use ratatui::buffer::Buffer;
use ratatui::style::Color;

/// Every colour a frame uses, as foreground or background.
pub(crate) fn colors_used(frame: &Buffer) -> HashSet<Color> {
    frame
        .content()
        .iter()
        .flat_map(|cell| [cell.fg, cell.bg])
        .collect()
}

/// The brand token file, the source of truth for the palette.
const BRAND_TOKENS: &str = include_str!("../../../../assets/brand/brand-tokens.json");

/// The brand token text from the key `start` up to the key `end`.
fn tokens_between(start: &str, end: &str) -> &'static str {
    let (_, after) = BRAND_TOKENS
        .split_once(start)
        .expect("the brand tokens have the start key");
    let (section, _) = after
        .split_once(end)
        .expect("the brand tokens have the end key after it");
    section
}

/// The 24-bit colours of the brand tokens' dark theme.
pub(crate) fn dark_token_colors() -> HashSet<Color> {
    tokens_between("\"dark\":", "\"ansi256Dark\"")
        .split('"')
        .filter_map(|text| text.strip_prefix('#'))
        .map(|hex| {
            let [_, red, green, blue] = u32::from_str_radix(hex, 16)
                .expect("a hex colour")
                .to_be_bytes();
            Color::Rgb(red, green, blue)
        })
        .collect()
}

/// The indexed colours of the brand tokens' 256-colour dark theme.
pub(crate) fn ansi256_token_colors() -> HashSet<Color> {
    tokens_between("\"ansi256Dark\":", "\"ansi16\"")
        .split(|c: char| !c.is_ascii_digit())
        .filter_map(|number| number.parse().ok())
        .map(Color::Indexed)
        .collect()
}
