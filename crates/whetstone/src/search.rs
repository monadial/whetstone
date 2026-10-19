//! Searching the lines of a pane for what the learner typed.

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation as _;

/// What a search looks for: text that is not empty. A query with no
/// capital letters ignores the case of ASCII letters, as `smartcase` does in
/// vim; one with a capital matches exactly. It matches whole characters as
/// the learner sees them: `e` does not match the start of an `é` written as
/// `e` and a combining accent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Query {
    text: String,
    case: Case,
}

/// Whether a query tells capital letters from small ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Case {
    /// ASCII letters match in either case.
    Ignored,
    /// Letters match only in the case typed.
    Matched,
}

impl Query {
    /// The query for `text`, unless it is empty.
    pub(crate) fn new(text: &str) -> Option<Self> {
        (!text.is_empty()).then(|| Self {
            text: text.to_owned(),
            case: if text.chars().any(char::is_uppercase) {
                Case::Matched
            } else {
                Case::Ignored
            },
        })
    }

    /// What the query looks for, as typed.
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// Where the query is in `line`: the byte range of each occurrence, in
    /// order. Each starts and ends between whole characters (rule 31).
    pub(crate) fn occurrences(&self, line: &str) -> Vec<Range<usize>> {
        let to_range = |(start, found): (usize, &str)| start..start + found.len();
        let found: Vec<Range<usize>> = match self.case {
            // Lowercasing ASCII keeps every byte where it was, so the ranges
            // hold in `line` too.
            Case::Ignored => line
                .to_ascii_lowercase()
                .match_indices(&self.text.to_ascii_lowercase())
                .map(to_range)
                .collect(),
            Case::Matched => line.match_indices(&self.text).map(to_range).collect(),
        };
        if found.is_empty() {
            return found;
        }
        let boundaries: Vec<usize> = line
            .grapheme_indices(true)
            .map(|(at, _)| at)
            .chain([line.len()])
            .collect();
        let between_characters = |at: &usize| boundaries.binary_search(at).is_ok();
        found
            .into_iter()
            .filter(|range| between_characters(&range.start) && between_characters(&range.end))
            .collect()
    }

    /// Whether `text` holds the query.
    pub(crate) fn is_in(&self, text: &str) -> bool {
        !self.occurrences(text).is_empty()
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

/// The first of `matches`, indexes in order, at or after `start`, wrapping
/// around to the first.
pub(crate) fn first_from(matches: &[usize], start: usize) -> Option<usize> {
    matches
        .iter()
        .find(|&&index| index >= start)
        .or(matches.first())
        .copied()
}

/// What a search for `query` found, for the row that says so: which of the
/// `matches` is `current`, if one is, or how many there are, or that there
/// are none in `place`.
pub(crate) fn status(
    query: &Query,
    matches: &[usize],
    current: Option<usize>,
    place: &str,
) -> String {
    let text = query.text();
    let at = current.and_then(|current| matches.iter().position(|&index| index == current));
    match (matches.len(), at) {
        (0, _) => format!("No match for \"{text}\" in {place}"),
        (count, Some(at)) => format!("Match {} of {count} for \"{text}\"", at + 1),
        (1, None) => format!("1 match for \"{text}\""),
        (count, None) => format!("{count} matches for \"{text}\""),
    }
}
