//! Text laid out in columns, measured in terminal cells rather than bytes.

use ratatui::text::Line;

/// The width in cells of the widest of `texts`, plus a two-cell gap: where
/// the next column starts.
pub(crate) fn next_column<'a>(texts: impl IntoIterator<Item = &'a str>) -> usize {
    texts
        .into_iter()
        .map(|text| Line::raw(text).width())
        .max()
        .unwrap_or_default()
        + 2
}

/// Pads `line` with spaces until it is `width` cells wide, so whatever is
/// pushed next starts in that column.
pub(crate) fn pad_to(line: &mut Line<'_>, width: usize) {
    let gap = width.saturating_sub(line.width());
    if gap > 0 {
        line.push_span(" ".repeat(gap));
    }
}
