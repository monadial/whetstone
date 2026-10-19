//! Searching the lines of a pane for what the learner typed.

use std::ops::Range;

/// What a search looks for: text that is not empty. A query with no
/// capital letters ignores the case of ASCII letters, as `smartcase` does in
/// vim; one with a capital matches exactly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Query {
    text: String,
    ignore_case: bool,
}

impl Query {
    /// The query for `text`, unless it is empty.
    pub(crate) fn new(text: &str) -> Option<Self> {
        (!text.is_empty()).then(|| Self {
            text: text.to_owned(),
            ignore_case: !text.chars().any(char::is_uppercase),
        })
    }

    /// What the query looks for, as typed.
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// Where the query is in `line`: the byte range of each occurrence, in
    /// order.
    pub(crate) fn occurrences(&self, line: &str) -> Vec<Range<usize>> {
        let to_range = |(start, found): (usize, &str)| start..start + found.len();
        if self.ignore_case {
            // Lowercasing ASCII keeps every byte where it was, so the ranges
            // hold in `line` too.
            line.to_ascii_lowercase()
                .match_indices(&self.text.to_ascii_lowercase())
                .map(to_range)
                .collect()
        } else {
            line.match_indices(&self.text).map(to_range).collect()
        }
    }

    /// The indexes of the `lines` that hold the query.
    pub(crate) fn matching_lines<'a>(&self, lines: impl Iterator<Item = &'a str>) -> Vec<usize> {
        lines
            .enumerate()
            .filter(|(_, line)| !self.occurrences(line).is_empty())
            .map(|(index, _)| index)
            .collect()
    }
}
