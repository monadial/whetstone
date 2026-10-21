//! The repository picker's commit rows: each commit's choice marker, short
//! ID, author date, author, summary and size, in aligned columns that fit
//! the width. Each commit's size is found in the background, only while its
//! row is on screen, and kept once found, so paging stays instant.

use std::collections::HashMap;
use std::time::SystemTime;

use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation as _;
use whetstone_core::{ChangeSize, CommitId, Comparison};

use crate::author_date::Wording;
use crate::columns::{self, GAP};
use crate::job::{JobId, JobOutput, Jobs, Waiting};
use crate::reasoning_editor::grouped;
use crate::repository::{Availability, Commit, LoadError, RepositoryHandle};
use crate::theme::{ControlKind, ControlState, Theme};
use crate::view::comparison_label;

/// The fewest cells the summary takes before the date shortens or a column
/// goes, as far as the width allows.
const SUMMARY: usize = 30;

/// The most cells an author's name takes.
const AUTHOR: usize = 12;

/// Why a commit whose parent is missing, as at a shallow clone's boundary,
/// cannot be chosen.
const PARENT_MISSING: &str = "parent not in this clone";

/// The fewest cells a size takes, as "1 file  +0 −0" does. The size's
/// column keeps that much room while sizes are still found, so the columns
/// stay where they are once they are.
const SMALLEST_SIZE: usize = 13;

/// What the size's column shows while a size is being found.
const FINDING: &str = "…";

/// What the size's column shows for a commit that can't be read.
const UNREADABLE: &str = "unreadable";

/// The sizes of the commits on screen, found in the background, and kept
/// for each commit once found.
#[derive(Debug)]
pub(crate) struct Sizes {
    /// The repository, for each job to open on its own thread.
    handle: RepositoryHandle,
    /// The size of each commit found so far, or that it can't be read.
    found: HashMap<CommitId, Found>,
    /// The jobs finding sizes, each with the commit it sizes.
    finding: Waiting<CommitId>,
    /// The commits on screen whose sizes are wanted.
    wanted: Vec<CommitId>,
}

/// What finding a commit's size found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Found {
    Size(ChangeSize),
    /// The commit can't be read, so it has no size to show.
    Unreadable,
}

/// What a commit's row shows of its size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Size {
    /// It is still being found.
    Finding,
    /// The files the change touches, and the lines it adds and removes.
    Found(ChangeSize),
    /// The commit can't be read, so it has no size to show.
    Unreadable,
}

impl Sizes {
    /// No sizes yet, of commits in the repository `handle` opens.
    pub(crate) fn new(handle: RepositoryHandle) -> Self {
        Self {
            handle,
            found: HashMap::new(),
            finding: Waiting::new(),
            wanted: Vec::new(),
        }
    }

    /// Wants the sizes of the commits `on_screen` that have one: those
    /// compared with their only parent. A merge or a first commit shows
    /// what it is compared with instead, and a commit whose parent is
    /// missing has nothing to compare with.
    pub(crate) fn want<'c>(&mut self, on_screen: impl IntoIterator<Item = &'c Commit>) {
        self.wanted = on_screen
            .into_iter()
            .filter(|commit| commit.availability() == Availability::Compared(Comparison::Parent))
            .map(Commit::id)
            .collect();
    }

    /// Starts a job with `jobs` for each commit wanted whose size is not
    /// found or being found: off the UI loop (rule 19), one commit each, so
    /// each row's size shows as soon as it is found.
    pub(crate) fn start_jobs(&mut self, jobs: &mut Jobs<JobOutput>) {
        for &commit in &self.wanted {
            if self.found.contains_key(&commit) || self.finding.waits_with(&commit) {
                continue;
            }
            let handle = self.handle.clone();
            let job = jobs.start(move |_| {
                Ok(JobOutput::ChangeSized {
                    commit,
                    size: handle.size(commit),
                })
            });
            self.finding.wait_for(job, commit);
        }
    }

    /// Whether it waits for the job `job`.
    pub(crate) fn waits_for(&self, job: JobId) -> bool {
        self.finding.waits_for(job)
    }

    /// Rule 24: keeps `size`, the result of the job `job`, as the size of
    /// `commit`, only if that job is the one finding it, and the change
    /// list still lists it, as `listed` says. A result for a commit a
    /// filter now hides is ignored, and found again if it shows again.
    pub(crate) fn finish(
        &mut self,
        job: JobId,
        commit: CommitId,
        size: Result<ChangeSize, LoadError>,
        listed: bool,
    ) {
        if self.finding.accept(job, &commit) && listed {
            let found = size.map_or(Found::Unreadable, Found::Size);
            self.found.insert(commit, found);
        }
    }

    /// What the row of `commit` shows of its size.
    pub(crate) fn of(&self, commit: CommitId) -> Size {
        match self.found.get(&commit) {
            None => Size::Finding,
            Some(Found::Size(size)) => Size::Found(*size),
            Some(Found::Unreadable) => Size::Unreadable,
        }
    }
}

/// A commit's row to lay out: the commit, its choice's state, and what it
/// shows of its size.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Row<'a> {
    pub(crate) commit: &'a Commit,
    pub(crate) state: ControlState<'a>,
    pub(crate) size: Size,
}

/// What ends a row, in the size's column.
#[derive(Debug)]
enum End {
    /// The size, as the files the change touches, and the lines it adds
    /// and removes.
    Size { files: String, lines: String },
    /// A size still being found, or one that can't be.
    Pending(&'static str),
    /// What a merge or a first commit is compared with, in place of a size.
    Label(&'static str),
    /// Why the commit can't be chosen, in place of a size.
    Disabled(String),
}

impl End {
    /// What stays at the end of the row once the size's column goes: a
    /// label or why the commit can't be chosen, but not a size.
    fn kept(&self) -> Option<&str> {
        match self {
            Self::Label(label) => Some(label),
            Self::Disabled(reason) => Some(reason),
            Self::Size { .. } | Self::Pending(_) => None,
        }
    }
}

/// A commit's row with its columns' text.
#[derive(Debug)]
struct Cells<'a> {
    state: ControlState<'a>,
    id: String,
    /// The date worded in full, and short.
    dates: [String; 2],
    author: String,
    summary: &'a str,
    end: End,
}

/// Which columns a row shows, and how its date is worded.
#[derive(Debug, Clone, Copy)]
struct Layout {
    date: Wording,
    author: bool,
    size: bool,
}

/// The layouts a width can take, from the widest. The first that leaves
/// the summary [`SUMMARY`] cells or more is used, or the last: the date
/// shortens first, then the size's column goes, then the author's.
const LAYOUTS: [Layout; 4] = [
    Layout {
        date: Wording::Full,
        author: true,
        size: true,
    },
    Layout {
        date: Wording::Short,
        author: true,
        size: true,
    },
    Layout {
        date: Wording::Short,
        author: true,
        size: false,
    },
    Layout {
        date: Wording::Short,
        author: false,
        size: false,
    },
];

/// `rows` as lines `width` cells wide, in aligned columns: the choice
/// marker, short ID, date worded as of `now`, author and summary, and the
/// size at the end. The summary takes the width the others leave, cut to
/// fit. The ID, date, author and size are muted. A merge or a first commit
/// shows what it is compared with in place of a size, and a commit that
/// can't be chosen is disabled, saying why there.
pub(crate) fn lines<'a>(
    theme: &Theme,
    rows: Vec<Row<'a>>,
    now: SystemTime,
    width: u16,
) -> Vec<Line<'a>> {
    let rows: Vec<Cells<'a>> = rows.into_iter().map(|row| cells(row, now)).collect();
    let widest = |width: &dyn Fn(&Cells<'a>) -> usize| rows.iter().map(width).max();
    let id = widest(&|row| cells_in(&row.id)).unwrap_or_default();
    let full_date = widest(&|row| cells_in(&row.dates[0])).unwrap_or_default();
    let short_date = widest(&|row| cells_in(&row.dates[1])).unwrap_or_default();
    let author = widest(&|row| cells_in(&row.author)).unwrap_or_default();
    let files = widest(&|row| match &row.end {
        End::Size { files, .. } => cells_in(files),
        _ => 0,
    })
    .unwrap_or_default();
    let end_width = |end: &End| match end {
        End::Size { lines, .. } => files + GAP + cells_in(lines),
        End::Pending(text) | End::Label(text) => cells_in(text),
        End::Disabled(reason) => cells_in(reason),
    };
    let has_sizes = rows
        .iter()
        .any(|row| matches!(row.end, End::Size { .. } | End::Pending(_)));
    let end = widest(&|row| end_width(&row.end))
        .unwrap_or_default()
        .max(if has_sizes { SMALLEST_SIZE } else { 0 });
    let marker = Theme::control_width("", ControlKind::Choice { selected: false });
    // What a layout leaves the summary.
    let room = |layout: Layout| {
        let date = match layout.date {
            Wording::Full => full_date,
            Wording::Short => short_date,
        };
        usize::from(width)
            .saturating_sub(marker + id + GAP + date + GAP)
            .saturating_sub(if layout.author { author + GAP } else { 0 })
            .saturating_sub(if layout.size { GAP + end } else { 0 })
    };
    let layout = LAYOUTS
        .into_iter()
        .find(|&layout| room(layout) >= SUMMARY)
        .unwrap_or(LAYOUTS[LAYOUTS.len() - 1]);
    let summary_room = room(layout);
    let date_column = id + GAP;
    let author_column = date_column
        + GAP
        + match layout.date {
            Wording::Full => full_date,
            Wording::Short => short_date,
        };
    let summary_column = if layout.author {
        author_column + author + GAP
    } else {
        author_column
    };
    let end_column = summary_column + summary_room + GAP;
    rows.into_iter()
        .map(|row| {
            let mut line = Line::from(Span::styled(row.id, theme.muted));
            columns::pad_to(&mut line, date_column);
            let [full, short] = row.dates;
            line.push_span(Span::styled(
                match layout.date {
                    Wording::Full => full,
                    Wording::Short => short,
                },
                theme.muted,
            ));
            if layout.author {
                columns::pad_to(&mut line, author_column);
                line.push_span(Span::styled(row.author, theme.muted));
            }
            columns::pad_to(&mut line, summary_column);
            let kept = if layout.size { None } else { row.end.kept() };
            let room = kept.map_or(summary_room, |kept| {
                summary_room.saturating_sub(GAP + cells_in(kept))
            });
            line.push_span(Span::raw(columns::truncate(row.summary, room)));
            if let Some(kept) = kept {
                line.push_span(" ".repeat(GAP));
                line.push_span(Span::styled(kept.to_owned(), theme.muted));
            } else if layout.size {
                columns::pad_to(&mut line, end_column);
                line.push_span(Span::styled(
                    match row.end {
                        End::Size {
                            files: size_files,
                            lines,
                        } => {
                            let pad = files.saturating_sub(cells_in(&size_files)) + GAP;
                            format!("{size_files}{}{lines}", " ".repeat(pad))
                        }
                        End::Pending(text) | End::Label(text) => text.to_owned(),
                        End::Disabled(reason) => reason,
                    },
                    theme.muted,
                ));
            }
            theme.control_in_columns(line.spans, row.state)
        })
        .collect()
}

/// The columns' text of `row`, with its date worded as of `now`.
fn cells(row: Row<'_>, now: SystemTime) -> Cells<'_> {
    let Row {
        commit,
        state,
        size,
    } = row;
    let (state, end) = match commit.availability() {
        Availability::ParentMissing => (
            ControlState {
                disabled: Some(PARENT_MISSING),
                ..state
            },
            End::Disabled(format!("({PARENT_MISSING})")),
        ),
        Availability::Compared(comparison) => (
            state,
            match (comparison_label(comparison), size) {
                (Some(label), _) => End::Label(label),
                (None, Size::Finding) => End::Pending(FINDING),
                (None, Size::Unreadable) => End::Pending(UNREADABLE),
                (None, Size::Found(size)) => End::Size {
                    files: match size.files() {
                        1 => "1 file".to_owned(),
                        files => format!("{} files", grouped(files)),
                    },
                    lines: format!("+{} −{}", grouped(size.added()), grouped(size.removed())),
                },
            },
        ),
    };
    let date = commit.date();
    Cells {
        state,
        id: commit.id().short().to_owned(),
        dates: [
            date.worded(now, Wording::Full),
            date.worded(now, Wording::Short),
        ],
        author: author_name(commit.author()),
        summary: commit.summary(),
        end,
    }
}

/// The author's `name` as its column shows it, at most [`AUTHOR`] cells:
/// whole if it fits, or else its first word and its last word's initial,
/// as "Tomas M.", if that fits, or else cut to fit.
fn author_name(name: &str) -> String {
    if cells_in(name) <= AUTHOR {
        return name.to_owned();
    }
    let words: Vec<&str> = name.split_whitespace().collect();
    if let [first, .., last] = words[..]
        && let Some(initial) = last.graphemes(true).next()
    {
        let short = format!("{first} {initial}.");
        if cells_in(&short) <= AUTHOR {
            return short;
        }
    }
    columns::truncate(name, AUTHOR)
}

/// How many cells `text` takes.
fn cells_in(text: &str) -> usize {
    Line::raw(text).width()
}
