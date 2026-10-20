//! A one-line text entry, such as the search box.

use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation as _;

use crate::keymap::Command;
use crate::paste;
use crate::theme::Theme;

/// The text typed so far. The cursor is always at its end.
#[derive(Debug, Clone, Default)]
pub(crate) struct TextEntry {
    text: String,
}

impl TextEntry {
    /// Applies `command`: a typed character goes at the end, and erasing
    /// takes off the last character as the learner sees it, a whole
    /// grapheme (rule 31).
    pub(crate) fn apply(&mut self, command: Command) {
        match command {
            Command::Type(c) => self.text.push(c),
            Command::Erase => {
                if let Some((start, _)) = self.text.grapheme_indices(true).next_back() {
                    self.text.truncate(start);
                }
            }
            _ => {}
        }
    }

    /// Inserts pasted `text` at the end, as one insertion, on one line:
    /// each line break and tab is a space, and control characters are
    /// dropped (rule 57).
    pub(crate) fn paste(&mut self, text: &str) {
        self.text.push_str(&paste::on_one_line(text));
    }

    /// The text typed so far.
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// The entry as one line at most `width` cells wide: `prompt`, the
    /// text, then the cursor. Text too wide for the line loses its start, a
    /// whole grapheme at a time, so the end, where the typing is, stays in
    /// view.
    pub(crate) fn line<'a>(&'a self, theme: &Theme, prompt: &'a str, width: usize) -> Line<'a> {
        let cursor = Span::styled(" ", theme.cursor);
        let room = width.saturating_sub(Span::raw(prompt).width() + cursor.width());
        let mut start = self.text.len();
        let mut used = 0;
        for (at, grapheme) in self.text.grapheme_indices(true).rev() {
            used += Span::raw(grapheme).width();
            if used > room {
                break;
            }
            start = at;
        }
        Line::from(vec![
            Span::raw(prompt),
            Span::raw(self.text.get(start..).unwrap_or_default()),
            cursor,
        ])
    }
}
