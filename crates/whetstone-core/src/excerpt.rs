//! What the evidence sends of a citation: the cited lines and a few lines
//! around them, numbered as in their file, as the questions screen shows
//! them beside a question. In a touched file, the lines the change added
//! and removed are marked, as its diff marks them. In connected code, a
//! related test or related configuration, the excerpt keeps why the
//! connection is there.
//!
//! An excerpt comes only from what the evidence sends, so it shows nothing
//! the learner didn't approve, and nothing is read from the repository.

use std::fmt;

use crate::connection::Relation;
use crate::evidence::{Evidence, EvidenceConnection, EvidenceFile, SentFile, Side};
use crate::question::Citation;

/// The cited lines of one citation and the lines around them, as the
/// evidence sends them.
#[derive(Clone, PartialEq, Eq)]
pub struct Excerpt {
    source: ExcerptSource,
    lines: Vec<ExcerptLine>,
}

/// Where an excerpt's lines come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExcerptSource {
    /// A touched file's full content: after the change, or before it if the
    /// change deletes the file.
    TouchedFile,
    /// Connections of the file: connected code, a related test or related
    /// configuration, there for these relations, in order, each once.
    Connection {
        /// Why the connections are there.
        relations: Vec<Relation>,
    },
}

/// One line of an excerpt.
#[derive(Clone, PartialEq, Eq)]
pub struct ExcerptLine {
    number: Option<usize>,
    mark: DiffMark,
    text: String,
}

/// What the change did to a line of a touched file, as its diff marks it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffMark {
    /// The change left it as it was, or the line isn't in a touched file.
    Unchanged,
    /// The change added it, or modified it: `+` in the diff.
    Added,
    /// The change removed it: `-` in the diff.
    Removed,
}

impl Citation {
    /// What `evidence` sends of the cited lines, with up to `around` lines
    /// on either side as far as it sends them: from the touched file's full
    /// content if it sends the file, with the lines its diff adds and
    /// removes marked, or else from the file's connections. `None` if it
    /// doesn't send every cited line, which plain checks never let through.
    ///
    /// ```
    /// use whetstone_core::{
    ///     Change, Citation, ConnectedCodeLimit, Connections, DiffMark, Evidence,
    ///     ExcludePatterns, ReviewChoices, TouchedFile, WrittenCitation,
    /// };
    ///
    /// let change = Change::new(
    ///     "Retry failed payments".to_owned(),
    ///     Vec::new(),
    ///     "diff --git a/src/pay.rs b/src/pay.rs\n@@ -1 +1,2 @@\n pay\n+retry\n".to_owned(),
    ///     vec![TouchedFile::modified(
    ///         "src/pay.rs".to_owned(),
    ///         "pay\n".to_owned(),
    ///         "pay\nretry\n".to_owned(),
    ///     )],
    /// );
    /// let evidence = Evidence::build(
    ///     &change,
    ///     &ExcludePatterns::default(),
    ///     &[],
    ///     &Connections::NoneFound,
    ///     ConnectedCodeLimit::effective(None, None),
    ///     &ReviewChoices::default(),
    /// );
    /// let cited = WrittenCitation {
    ///     path: "src/pay.rs".to_owned(),
    ///     first_line: 2,
    ///     last_line: 2,
    /// };
    /// let citation = Citation::in_evidence(cited, &evidence).expect("line 2 is sent");
    ///
    /// let excerpt = citation.excerpt(&evidence, 3).expect("it is sent");
    /// let lines: Vec<_> = excerpt
    ///     .lines()
    ///     .iter()
    ///     .map(|line| (line.number(), line.mark(), line.text()))
    ///     .collect();
    /// assert_eq!(
    ///     lines,
    ///     [
    ///         (Some(1), DiffMark::Unchanged, "pay"),
    ///         (Some(2), DiffMark::Added, "retry"),
    ///     ]
    /// );
    /// ```
    #[must_use]
    pub fn excerpt(&self, evidence: &Evidence, around: usize) -> Option<Excerpt> {
        evidence
            .files()
            .iter()
            .filter(|file| file.path() == self.path())
            .find_map(EvidenceFile::sent)
            .and_then(|sent| self.in_touched_file(sent, around))
            .or_else(|| self.in_connections(evidence, around))
    }

    /// The excerpt from a touched file that sends `sent`, if its full
    /// content holds every cited line.
    fn in_touched_file(&self, sent: SentFile<'_>, around: usize) -> Option<Excerpt> {
        let content: Vec<&str> = sent.content.lines().collect();
        let (first, last) = (self.lines().first(), self.lines().last());
        if last > content.len() {
            return None;
        }
        let shown = first.saturating_sub(around).max(1)..=(last + around).min(content.len());
        let marked = |number: usize, mark| ExcerptLine {
            number: Some(number),
            mark,
            text: content[number - 1].to_owned(),
        };
        let lines = match sent.side {
            // The change deletes the file: every line is one it removes.
            Side::Before => shown
                .map(|number| marked(number, DiffMark::Removed))
                .collect(),
            Side::After => {
                let diff = Diff::read(sent.diff);
                let removed_after = |after: usize| {
                    diff.removed
                        .iter()
                        .filter(move |(at, _)| *at == after)
                        .map(|(_, text)| ExcerptLine {
                            number: None,
                            mark: DiffMark::Removed,
                            text: (*text).to_owned(),
                        })
                };
                // Lines removed before the first line shown come before it,
                // as a modified line's old text comes before its new one.
                // Those removed after the last line shown belong to the line
                // after it, unless the file ends there.
                let end = *shown.end();
                let mut lines: Vec<ExcerptLine> = removed_after(shown.start() - 1).collect();
                for number in shown {
                    let mark = if diff.added.contains(&number) {
                        DiffMark::Added
                    } else {
                        DiffMark::Unchanged
                    };
                    lines.push(marked(number, mark));
                    if number < end || number == content.len() {
                        lines.extend(removed_after(number));
                    }
                }
                lines
            }
        };
        Some(Excerpt {
            source: ExcerptSource::TouchedFile,
            lines,
        })
    }

    /// The excerpt from the connections of the cited file that `evidence`
    /// sends, if together they hold every cited line.
    fn in_connections(&self, evidence: &Evidence, around: usize) -> Option<Excerpt> {
        let connections: Vec<&EvidenceConnection> = evidence
            .connections()
            .iter()
            .filter(|connection| connection.id().path() == self.path())
            .collect();
        // The text of line `number`, from the first connection that sends
        // it, and that connection.
        let line = |number: usize| {
            connections.iter().find_map(|connection| {
                let sent = connection.sent()?;
                let text = sent
                    .text
                    .lines()
                    .nth(number.checked_sub(sent.first_line)?)?;
                Some((text, *connection))
            })
        };
        let (first, last) = (self.lines().first(), self.lines().last());
        let mut relations: Vec<Relation> = Vec::new();
        for number in first..=last {
            let (_, connection) = line(number)?;
            for relation in connection.relations() {
                if !relations.contains(relation) {
                    relations.push(relation.clone());
                }
            }
        }
        let above = (1..=around)
            .take_while(|&step| first > step && line(first - step).is_some())
            .count();
        let below = (1..=around)
            .take_while(|&step| line(last + step).is_some())
            .count();
        let lines = (first - above..=last + below)
            .filter_map(|number| {
                line(number).map(|(text, _)| ExcerptLine {
                    number: Some(number),
                    mark: DiffMark::Unchanged,
                    text: text.to_owned(),
                })
            })
            .collect();
        Some(Excerpt {
            source: ExcerptSource::Connection { relations },
            lines,
        })
    }
}

impl Excerpt {
    /// Where its lines come from.
    #[must_use]
    pub fn source(&self) -> &ExcerptSource {
        &self.source
    }

    /// Its lines, in the order the file has them, each line the change
    /// removed before the line that took its place.
    #[must_use]
    pub fn lines(&self) -> &[ExcerptLine] {
        &self.lines
    }
}

impl ExcerptLine {
    /// Its number in its file, or none for a line the change removed,
    /// which the file no longer has.
    #[must_use]
    pub fn number(&self) -> Option<usize> {
        self.number
    }

    /// What the change did to it.
    #[must_use]
    pub fn mark(&self) -> DiffMark {
        self.mark
    }

    /// Its text, without its line ending.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }
}

/// Shows its source and each line's number and mark, never their text.
impl fmt::Debug for Excerpt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Excerpt")
            .field("source", &self.source)
            .field("lines", &self.lines)
            .finish()
    }
}

/// Shows its number and mark, and its text's length, never the text.
impl fmt::Debug for ExcerptLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExcerptLine")
            .field("number", &self.number)
            .field("mark", &self.mark)
            .field("text_bytes", &self.text.len())
            .finish()
    }
}

/// What a touched file's unified diff adds and removes, numbered on the
/// side after the change.
#[derive(Debug, Default)]
struct Diff<'a> {
    /// The line numbers after the change of each line it adds or modifies.
    added: Vec<usize>,
    /// Each line it removes, after the line it follows after the change: 0
    /// at the very top of the file.
    removed: Vec<(usize, &'a str)>,
}

impl<'a> Diff<'a> {
    /// Reads `diff`, the diff of one file: its hunks, after its header.
    fn read(diff: &'a str) -> Self {
        let mut read = Self::default();
        // The next line's number after the change, once inside a hunk.
        let mut next: Option<usize> = None;
        for line in diff.lines() {
            if let Some(header) = line.strip_prefix("@@ ") {
                next = new_start(header);
                continue;
            }
            let Some(number) = next.as_mut() else {
                continue;
            };
            match line.as_bytes().first() {
                Some(b'+') => {
                    read.added.push(*number);
                    *number += 1;
                }
                Some(b'-') => read
                    .removed
                    .push((*number - 1, line.strip_prefix('-').unwrap_or_default())),
                // `\ No newline at end of file` belongs to the line before it.
                Some(b'\\') => {}
                // Context, which a tool that trims trailing spaces may leave
                // empty.
                _ => *number += 1,
            }
        }
        read
    }
}

/// The first line after the change of the hunk whose header, after `@@ `,
/// is `header`: `-3,4 +5,6 @@` starts at line 5. A hunk that adds to an
/// empty file starts at 0, so its first line is 1.
fn new_start(header: &str) -> Option<usize> {
    let new = header.split_whitespace().nth(1)?.strip_prefix('+')?;
    let start: usize = new.split(',').next()?.parse().ok()?;
    Some(start.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::change::{Change, TouchedFile};
    use crate::connected_code_limit::ConnectedCodeLimit;
    use crate::connection::{Connection, ConnectionGroup, Connections, Extent, LineRange, Tier};
    use crate::evidence::ReviewChoices;
    use crate::exclusion::ExcludePatterns;
    use crate::question::WrittenCitation;

    /// `src/pay.rs` before the change: a line for each of `a` to `j`.
    const BEFORE: &str = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\n";

    /// After it: `e` becomes `E`, `X` is added after `g`, and `i` is
    /// removed.
    const AFTER: &str = "a\nb\nc\nd\nE\nf\ng\nX\nh\nj\n";

    /// Its diff.
    const DIFF: &str = "diff --git a/src/pay.rs b/src/pay.rs\n\
                        --- a/src/pay.rs\n\
                        +++ b/src/pay.rs\n\
                        @@ -2,9 +2,9 @@\n b\n c\n d\n-e\n+E\n f\n g\n+X\n h\n-i\n j\n";

    /// Evidence of the change to `src/pay.rs`, which also deletes
    /// `src/old.rs`, with lines 10–12 and 13–14 of `src/client.rs`, which
    /// call `charge` and `refund`, and the whole of `tests/pay.rs`, its
    /// test.
    fn evidence() -> Evidence {
        let numbered = |count: usize| {
            (1..=count).fold(String::new(), |text, number| {
                text + &format!("client {number}\n")
            })
        };
        let change = Change::new(
            "Retry failed payments".to_owned(),
            Vec::new(),
            format!(
                "{DIFF}diff --git a/src/old.rs b/src/old.rs\n@@ -1,2 +0,0 @@\n-old 1\n-old 2\n"
            ),
            vec![
                TouchedFile::modified("src/pay.rs".to_owned(), BEFORE.to_owned(), AFTER.to_owned()),
                TouchedFile::deleted("src/old.rs".to_owned(), "old 1\nold 2\n".to_owned()),
            ],
        );
        let calls = |callable: &str| Relation::Calls {
            callable: callable.to_owned(),
            touched_file: "src/pay.rs".to_owned(),
        };
        let client = |first, last, relation| {
            Connection::new(
                "src/client.rs".to_owned(),
                Extent::Lines(LineRange::new(first, last).expect("a range")),
                ConnectionGroup::ConnectedCode,
                Tier::High,
                vec![relation],
                numbered(20).into(),
            )
        };
        Evidence::build(
            &change,
            &ExcludePatterns::default(),
            &[],
            &Connections::Found(vec![
                client(10, 12, calls("charge")),
                client(13, 14, calls("refund")),
                Connection::new(
                    "tests/pay.rs".to_owned(),
                    Extent::WholeFile,
                    ConnectionGroup::RelatedTest,
                    Tier::High,
                    vec![Relation::TestFor {
                        touched_file: "src/pay.rs".to_owned(),
                    }],
                    "test 1\ntest 2\n".to_owned().into(),
                ),
            ]),
            ConnectedCodeLimit::effective(None, None),
            &ReviewChoices::default(),
        )
    }

    /// The excerpt of lines `first` to `last` of `path`, with 3 lines
    /// around them.
    fn excerpt(path: &str, first: usize, last: usize) -> Option<Excerpt> {
        let evidence = evidence();
        let written = WrittenCitation {
            path: path.to_owned(),
            first_line: first,
            last_line: last,
        };
        Citation::in_evidence(written, &evidence)?.excerpt(&evidence, 3)
    }

    /// Each of `excerpt`'s lines: its number, mark and text.
    fn lines(excerpt: &Excerpt) -> Vec<(Option<usize>, DiffMark, &str)> {
        excerpt
            .lines()
            .iter()
            .map(|line| (line.number(), line.mark(), line.text()))
            .collect()
    }

    use DiffMark::{Added, Removed, Unchanged};

    #[test]
    fn a_cited_line_of_a_touched_file_shows_with_three_lines_around_it_and_the_diffs_marks() {
        let excerpt = excerpt("src/pay.rs", 5, 5).expect("line 5 is sent");

        assert_eq!(excerpt.source(), &ExcerptSource::TouchedFile);
        assert_eq!(
            lines(&excerpt),
            [
                (Some(2), Unchanged, "b"),
                (Some(3), Unchanged, "c"),
                (Some(4), Unchanged, "d"),
                (None, Removed, "e"),
                (Some(5), Added, "E"),
                (Some(6), Unchanged, "f"),
                (Some(7), Unchanged, "g"),
                (Some(8), Added, "X"),
            ],
            "a removed line comes before the line that took its place, unnumbered"
        );
    }

    #[test]
    fn a_removed_line_between_two_shown_lines_sits_between_them() {
        let excerpt = excerpt("src/pay.rs", 9, 10).expect("lines 9–10 are sent");

        assert_eq!(
            lines(&excerpt),
            [
                (Some(6), Unchanged, "f"),
                (Some(7), Unchanged, "g"),
                (Some(8), Added, "X"),
                (Some(9), Unchanged, "h"),
                (None, Removed, "i"),
                (Some(10), Unchanged, "j"),
            ],
            "the file ends at line 10"
        );
    }

    #[test]
    fn the_lines_around_stop_at_the_top_of_the_file() {
        let excerpt = excerpt("src/pay.rs", 1, 1).expect("line 1 is sent");

        assert_eq!(
            lines(&excerpt),
            [
                (Some(1), Unchanged, "a"),
                (Some(2), Unchanged, "b"),
                (Some(3), Unchanged, "c"),
                (Some(4), Unchanged, "d"),
            ]
        );
    }

    #[test]
    fn every_line_of_a_deleted_file_is_removed_and_keeps_its_number() {
        let excerpt = excerpt("src/old.rs", 2, 2).expect("the deleted file is sent");

        assert_eq!(
            lines(&excerpt),
            [(Some(1), Removed, "old 1"), (Some(2), Removed, "old 2")]
        );
    }

    #[test]
    fn a_citation_into_connected_code_keeps_its_relations_and_only_the_lines_sent() {
        let excerpt = excerpt("src/client.rs", 12, 13).expect("lines 12–13 are sent");

        let calls = |callable: &str| Relation::Calls {
            callable: callable.to_owned(),
            touched_file: "src/pay.rs".to_owned(),
        };
        assert_eq!(
            excerpt.source(),
            &ExcerptSource::Connection {
                relations: vec![calls("charge"), calls("refund")]
            },
            "both connections the citation spans"
        );
        assert_eq!(
            lines(&excerpt),
            [
                (Some(10), Unchanged, "client 10"),
                (Some(11), Unchanged, "client 11"),
                (Some(12), Unchanged, "client 12"),
                (Some(13), Unchanged, "client 13"),
                (Some(14), Unchanged, "client 14"),
            ],
            "lines 9 and 15 aren't sent, so they don't show"
        );
    }

    #[test]
    fn a_citation_into_a_related_test_keeps_its_relation() {
        let excerpt = excerpt("tests/pay.rs", 1, 1).expect("the test is sent");

        assert_eq!(
            excerpt.source(),
            &ExcerptSource::Connection {
                relations: vec![Relation::TestFor {
                    touched_file: "src/pay.rs".to_owned()
                }]
            }
        );
        assert_eq!(
            lines(&excerpt),
            [
                (Some(1), Unchanged, "test 1"),
                (Some(2), Unchanged, "test 2")
            ]
        );
    }

    #[test]
    fn no_excerpt_holds_lines_the_evidence_does_not_send() {
        let evidence = evidence();
        let citation = Citation::in_evidence(
            WrittenCitation {
                path: "src/client.rs".to_owned(),
                first_line: 12,
                last_line: 13,
            },
            &evidence,
        )
        .expect("lines 12–13 are sent");
        let other = Evidence::build(
            &Change::new(String::new(), Vec::new(), String::new(), Vec::new()),
            &ExcludePatterns::default(),
            &[],
            &Connections::NoneFound,
            ConnectedCodeLimit::effective(None, None),
            &ReviewChoices::default(),
        );

        assert_eq!(citation.excerpt(&other, 3), None);
    }
}
