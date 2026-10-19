//! Text laid out in columns, measured in terminal cells rather than bytes.

use ratatui::text::Line;
use unicode_segmentation::UnicodeSegmentation as _;

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

/// `text` broken into lines at most `width` cells wide, between words where
/// it can and between graphemes inside a word too wide for a line. Runs of
/// whitespace become one space, and a line never starts or ends with one.
pub(crate) fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let cells = |text: &str| Line::raw(text).width();
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && cells(&line) + 1 + cells(word) <= width {
            line.push(' ');
            line.push_str(word);
            continue;
        }
        if !line.is_empty() {
            lines.push(std::mem::take(&mut line));
        }
        for grapheme in word.graphemes(true) {
            if !line.is_empty() && cells(&line) + cells(grapheme) > width {
                lines.push(std::mem::take(&mut line));
            }
            line.push_str(grapheme);
        }
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

/// `text` cut to at most `width` cells, ending in `…` when it is cut, a
/// whole grapheme at a time.
pub(crate) fn truncate(text: &str, width: usize) -> String {
    if Line::raw(text).width() <= width {
        return text.to_owned();
    }
    let room = width.saturating_sub(1);
    let mut cut = String::new();
    for grapheme in text.graphemes(true) {
        if Line::raw(cut.as_str()).width() + Line::raw(grapheme).width() > room {
            break;
        }
        cut.push_str(grapheme);
    }
    if width > 0 {
        cut.push('…');
    }
    cut
}
