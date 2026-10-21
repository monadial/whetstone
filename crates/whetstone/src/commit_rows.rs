//! The repository picker's commit rows: each commit's choice marker, short
//! ID, author date, author, summary and size, in aligned columns that fit
//! the width. Sizes are found in the background, a few at a time, for the
//! commits on screen, and kept for each commit once found, so paging stays
//! instant.

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

/// How many sizes are found at once, at most: enough to fill a page
/// quickly, and few enough that paging fast never piles up work. The rest
/// wait their turn while they stay on screen.
const AT_ONCE: usize = 4;

/// Why a commit whose parent is missing, as at a shallow clone's boundary,
/// cannot be chosen.
const PARENT_MISSING: &str = "parent not in this clone";

/// The fewest cells a size takes, as "1 file  +0 −0" does. The size's
/// column keeps that much room while sizes are still found, so sizes as
/// small as that move no column when they come.
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
    /// What finding each commit's size found so far.
    found: HashMap<CommitId, Sizing>,
    /// The jobs finding sizes, each with the commit it sizes.
    finding: Waiting<CommitId>,
    /// The commits on screen whose sizes are wanted, in the order shown.
    wanted: Vec<CommitId>,
}

/// Where finding a commit's size is, as its row shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sizing {
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
    /// found or being found, while fewer than [`AT_ONCE`] are: off the UI
    /// loop (rule 19), one commit each, so each row's size shows as soon as
    /// it is found.
    pub(crate) fn start_jobs(&mut self, jobs: &mut Jobs<JobOutput>) {
        for &commit in &self.wanted {
            if self.finding.len() >= AT_ONCE {
                break;
            }
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
            let found = size.map_or(Sizing::Unreadable, Sizing::Found);
            self.found.insert(commit, found);
        }
    }

    /// Where finding the size of `commit` is.
    pub(crate) fn of(&self, commit: CommitId) -> Sizing {
        self.found.get(&commit).copied().unwrap_or(Sizing::Finding)
    }
}

/// A commit's row to lay out.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Row<'a> {
    /// The commit it shows.
    pub(crate) commit: &'a Commit,
    /// Its choice's state: whether it has focus, and is chosen.
    pub(crate) state: ControlState<'a>,
    /// Where finding its size is.
    pub(crate) size: Sizing,
}

/// What ends a row, in the size's column.
#[derive(Debug)]
enum End {
    /// The size: the `files` the change touches, and the `lines` it adds
    /// and removes.
    Size { files: String, lines: String },
    /// A size still being found.
    Finding,
    /// No size, as the commit can't be read.
    Unreadable,
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
            Self::Size { .. } | Self::Finding | Self::Unreadable => None,
        }
    }

    /// Whether it is a size, or stands in for one.
    fn is_size(&self) -> bool {
        matches!(self, Self::Size { .. } | Self::Finding | Self::Unreadable)
    }

    /// How many cells it takes, with the files of a size `files` cells
    /// wide.
    fn width(&self, files: usize) -> usize {
        match self {
            Self::Size { lines, .. } => files + GAP + width_of(lines),
            Self::Finding => width_of(FINDING),
            Self::Unreadable => width_of(UNREADABLE),
            Self::Label(label) => width_of(label),
            Self::Disabled(reason) => width_of(reason),
        }
    }

    /// Its text, with the files of a size padded to `files` cells, so the
    /// lines added and removed line up.
    fn text(self, files: usize) -> String {
        match self {
            Self::Size {
                files: touched,
                lines,
            } => {
                let pad = files.saturating_sub(width_of(&touched)) + GAP;
                format!("{touched}{}{lines}", " ".repeat(pad))
            }
            Self::Finding => FINDING.to_owned(),
            Self::Unreadable => UNREADABLE.to_owned(),
            Self::Label(label) => label.to_owned(),
            Self::Disabled(reason) => reason,
        }
    }
}

/// A commit's row with its columns' text.
#[derive(Debug)]
struct RowText<'a> {
    state: ControlState<'a>,
    id: String,
    date: Dates,
    author: String,
    summary: &'a str,
    end: End,
}

/// A date worded both ways.
#[derive(Debug)]
struct Dates {
    full: String,
    short: String,
}

impl Dates {
    /// The date as `wording` words it.
    fn worded(&self, wording: Wording) -> &str {
        match wording {
            Wording::Full => &self.full,
            Wording::Short => &self.short,
        }
    }
}

/// Which columns a row shows, and how its date is worded.
#[derive(Debug, Clone, Copy)]
struct Layout {
    date: Wording,
    author: bool,
    size: bool,
}

/// The layouts a width can take, from the widest. The first that leaves
/// every summary [`SUMMARY`] cells or more is used, or else the last: the
/// date shortens first, then the size's column goes, then the author's.
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

/// How wide each column of some rows is: as wide as its widest text.
#[derive(Debug)]
struct Widths {
    id: usize,
    full_date: usize,
    short_date: usize,
    author: usize,
    /// The files of a size.
    files: usize,
    /// The size's column.
    end: usize,
    /// What stays at the end of a row once the size's column goes.
    kept: usize,
}

impl Widths {
    /// The widths of the columns of `rows`.
    fn of(rows: &[RowText<'_>]) -> Self {
        let widest = |width: &dyn Fn(&RowText<'_>) -> usize| {
            rows.iter().map(width).max().unwrap_or_default()
        };
        let files = widest(&|row| match &row.end {
            End::Size { files, .. } => width_of(files),
            _ => 0,
        });
        let has_sizes = rows.iter().any(|row| row.end.is_size());
        Self {
            id: widest(&|row| width_of(&row.id)),
            full_date: widest(&|row| width_of(&row.date.full)),
            short_date: widest(&|row| width_of(&row.date.short)),
            author: widest(&|row| width_of(&row.author)),
            files,
            end: widest(&|row| row.end.width(files)).max(if has_sizes { SMALLEST_SIZE } else { 0 }),
            kept: widest(&|row| row.end.kept().map_or(0, width_of)),
        }
    }

    /// The date's width, worded as `wording` says.
    fn date(&self, wording: Wording) -> usize {
        match wording {
            Wording::Full => self.full_date,
            Wording::Short => self.short_date,
        }
    }

    /// Where the summary starts, as `layout` lays a row out.
    fn summary_column(&self, layout: Layout) -> usize {
        let author_column = self.id + GAP + self.date(layout.date) + GAP;
        if layout.author {
            author_column + self.author + GAP
        } else {
            author_column
        }
    }

    /// The summary's column, as `layout` lays a row `width` cells wide out.
    fn summary(&self, layout: Layout, width: usize) -> usize {
        let marker = Theme::control_width("", ControlKind::Choice { selected: false });
        width
            .saturating_sub(marker + self.summary_column(layout))
            .saturating_sub(if layout.size { GAP + self.end } else { 0 })
    }

    /// The fewest cells `layout` leaves any summary in a row `width` cells
    /// wide: the summary's column, less what a row keeps after its summary
    /// once the size's column goes.
    fn least_summary(&self, layout: Layout, width: usize) -> usize {
        let summary = self.summary(layout, width);
        if layout.size || self.kept == 0 {
            summary
        } else {
            summary.saturating_sub(GAP + self.kept)
        }
    }
}

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
    let rows: Vec<RowText<'a>> = rows.into_iter().map(|row| row_text(row, now)).collect();
    let widths = Widths::of(&rows);
    let width = usize::from(width);
    let layout = LAYOUTS
        .into_iter()
        .find(|&layout| widths.least_summary(layout, width) >= SUMMARY)
        .unwrap_or(LAYOUTS[LAYOUTS.len() - 1]);
    rows.into_iter()
        .map(|row| line(theme, row, &widths, layout, width))
        .collect()
}

/// `row` as a line `width` cells wide, with columns `widths` wide, laid out
/// as `layout` says. What it keeps after its summary once the size's column
/// goes takes room from its summary, and never goes past its end.
fn line<'a>(
    theme: &Theme,
    row: RowText<'a>,
    widths: &Widths,
    layout: Layout,
    width: usize,
) -> Line<'a> {
    let room = widths.summary(layout, width);
    let summary_column = widths.summary_column(layout);
    let mut line = Line::from(Span::styled(row.id, theme.muted));
    columns::pad_to(&mut line, widths.id + GAP);
    line.push_span(Span::styled(
        row.date.worded(layout.date).to_owned(),
        theme.muted,
    ));
    if layout.author {
        columns::pad_to(&mut line, summary_column - widths.author - GAP);
        line.push_span(Span::styled(row.author, theme.muted));
    }
    columns::pad_to(&mut line, summary_column);
    let kept = if layout.size {
        None
    } else {
        row.end
            .kept()
            .map(|kept| columns::truncate(kept, room.saturating_sub(GAP)))
    };
    if let Some(kept) = kept {
        let room = room.saturating_sub(GAP + width_of(&kept));
        line.push_span(Span::raw(columns::truncate(row.summary, room)));
        line.push_span(" ".repeat(GAP));
        line.push_span(Span::styled(kept, theme.muted));
    } else {
        line.push_span(Span::raw(columns::truncate(row.summary, room)));
        if layout.size {
            columns::pad_to(&mut line, summary_column + room + GAP);
            line.push_span(Span::styled(row.end.text(widths.files), theme.muted));
        }
    }
    theme.control_in_columns(line.spans, row.state)
}

/// The columns' text of `row`, with its date worded as of `now`.
fn row_text(row: Row<'_>, now: SystemTime) -> RowText<'_> {
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
                (None, Sizing::Finding) => End::Finding,
                (None, Sizing::Unreadable) => End::Unreadable,
                (None, Sizing::Found(size)) => End::Size {
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
    RowText {
        state,
        id: commit.id().short().to_owned(),
        date: Dates {
            full: date.worded(now, Wording::Full),
            short: date.worded(now, Wording::Short),
        },
        author: author_name(commit.author()),
        summary: commit.summary(),
        end,
    }
}

/// The author's `name` as its column shows it, at most [`AUTHOR`] cells:
/// whole if it fits, or else its first word and its last word's initial,
/// as "Tomas M.", if that fits, or else cut to fit.
fn author_name(name: &str) -> String {
    if width_of(name) <= AUTHOR {
        return name.to_owned();
    }
    let words: Vec<&str> = name.split_whitespace().collect();
    if let [first, .., last] = words[..]
        && let Some(initial) = last.graphemes(true).next()
    {
        let short = format!("{first} {initial}.");
        if width_of(&short) <= AUTHOR {
            return short;
        }
    }
    columns::truncate(name, AUTHOR)
}

/// How many cells `text` takes.
fn width_of(text: &str) -> usize {
    Line::raw(text).width()
}
