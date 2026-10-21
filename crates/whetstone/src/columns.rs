//! Text laid out in columns, measured in terminal cells rather than bytes.

use ratatui::text::Line;
use unicode_segmentation::UnicodeSegmentation as _;

/// The cells between two columns, at least.
pub(crate) const GAP: usize = 2;

/// The width in cells of the widest of `texts`, plus the gap: where the
/// next column starts.
pub(crate) fn next_column<'a>(texts: impl IntoIterator<Item = &'a str>) -> usize {
    texts
        .into_iter()
        .map(|text| Line::raw(text).width())
        .max()
        .unwrap_or_default()
        + GAP
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

/// `text` cut into pieces at most `width` cells wide, as code is wrapped:
/// after the last space that fits, past the line's indent, or else between
/// graphemes. Every character stays, spaces included, so joining the
/// pieces gives `text` back. Text that fits is one piece, and so is empty
/// text.
pub(crate) fn cut(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut pieces = vec![String::new()];
    for grapheme in text.graphemes(true) {
        let piece = pieces
            .last_mut()
            .expect("there is always a piece to add to");
        if piece.is_empty()
            || Line::raw(piece.as_str()).width() + Line::raw(grapheme).width() <= width
        {
            piece.push_str(grapheme);
            continue;
        }
        // The word being written moves down whole, unless it is all the
        // piece has after its indent.
        let indent = piece.len() - piece.trim_start().len();
        let mut next = match piece.rfind(' ') {
            Some(space) if space > indent => piece.split_off(space + 1),
            _ => String::new(),
        };
        next.push_str(grapheme);
        pieces.push(next);
    }
    pieces
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_is_cut_after_the_last_space_that_fits_and_keeps_every_character() {
        let code = "    let key = IdempotencyKey::parse(raw)?;";

        let pieces = cut(code, 20);

        assert_eq!(
            pieces,
            ["    let key = ", "IdempotencyKey::pars", "e(raw)?;"]
        );
        assert_eq!(pieces.concat(), code);
    }

    #[test]
    fn an_indent_is_never_a_place_to_cut() {
        assert_eq!(
            cut("        operation_key,", 12),
            ["        oper", "ation_key,"]
        );
    }

    #[test]
    fn text_that_fits_is_one_piece() {
        assert_eq!(cut("Self {", 6), ["Self {"]);
        assert_eq!(cut("", 6), [""]);
    }
}
