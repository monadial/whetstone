//! The area and change picker, where Practice starts: the learner chooses
//! the areas questions focus on and one change, from the showcase project's
//! curated history or from the commits of their own repository.

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use ratatui::Frame;
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{List, ListState, Paragraph};
use whetstone_core::{Area, CommitId, InvalidAreaFile, ReviewChoices, ShowcaseProject};

use crate::columns;
use crate::commit_rows::{self, Row, Sizes};
use crate::connected_code::{Request, ShowcaseTree};
use crate::entry::TextEntry;
use crate::job::{JobId, JobOutput, Jobs};
use crate::keymap::{self, Binding, Command, Input, Keymap, Press};
use crate::outbound_review::OutboundReview;
use crate::palette::Action;
use crate::practice::Practice;
use crate::reasoning_editor::grouped;
use crate::repository::{Availability, Commit, History, HistoryEnd, HistoryError, Repository};
use crate::search::Query;
use crate::theme::{ColorSupport, ControlKind, ControlState, Theme};
use crate::view::{ChangeIndex, ChangeRef, Context, NEEDS_KEY, Project, Route, View};
use crate::viewer::Unreadable;

/// Space on the picker: it makes a choice.
const SPACE: Binding = Binding {
    keys: &[(Press::char(' '), Command::Activate)],
    does: "Toggle an area, or choose a change",
    hint: Some("Space toggle"),
};

/// Enter on the picker: it makes a choice, and opens a chosen change.
const OPEN: Binding = Binding {
    keys: &[(Press::key(KeyCode::Enter), Command::Open)],
    does: "Toggle an area, or choose and open a change",
    hint: Some("Enter open"),
};

/// Page Down and Page Up on the picker: they page through the changes.
const PAGE: Binding = Binding {
    keys: &[
        (Press::key(KeyCode::PageDown), Command::PageDown),
        (Press::key(KeyCode::PageUp), Command::PageUp),
    ],
    does: "Move a page down or up the changes",
    hint: Some("PgDn/Up page"),
};

/// Ctrl+D and Ctrl+U on the picker: they move half a page.
const HALF_PAGE: Binding = Binding {
    does: "Move half a page down or up the changes",
    ..keymap::HALF_PAGE
};

/// The showcase picker's own keys. Space makes a choice, and Enter also
/// opens the chosen change. The paging keys move through the changes.
const KEYMAP: Keymap = &[
    keymap::MOVE,
    SPACE,
    OPEN,
    PAGE,
    HALF_PAGE,
    Binding {
        does: "Go to the first or last change",
        ..keymap::ENDS
    },
    keymap::BACK,
];

/// The repository picker's own keys: the showcase picker's, and `/` to
/// filter the commits.
const REPOSITORY_KEYMAP: Keymap = &[
    keymap::MOVE,
    SPACE,
    OPEN,
    PAGE,
    HALF_PAGE,
    Binding {
        does: "Go to the newest commit, or the oldest read so far",
        ..keymap::ENDS
    },
    Binding {
        does: "Filter the commits by message, author or ID",
        ..keymap::SEARCH
    },
    keymap::BACK,
];

/// The repository picker's own keys while a filter is on: as without one,
/// but Esc clears the filter, and `q` alone goes back.
const FILTERED_KEYMAP: Keymap = &[
    keymap::MOVE,
    SPACE,
    OPEN,
    PAGE,
    HALF_PAGE,
    Binding {
        does: "Go to the newest commit, or the oldest read so far",
        ..keymap::ENDS
    },
    Binding {
        does: "Change the filter",
        ..keymap::SEARCH
    },
    Binding {
        keys: &[(Press::key(KeyCode::Esc), Command::Cancel)],
        does: "Clear the filter",
        hint: None,
    },
    Binding {
        keys: &[(Press::char('q'), Command::Back)],
        ..keymap::BACK
    },
];

/// The keys of the repository picker's search box, while the learner types
/// a filter. Every other key but the reserved ones types, and the list
/// narrows as they do.
const FILTER_BOX: Keymap = &[
    Binding {
        keys: &[(Press::key(KeyCode::Enter), Command::Activate)],
        does: "Keep the filter, and go to the commits",
        hint: Some("Enter keep"),
    },
    Binding {
        keys: &[(Press::key(KeyCode::Backspace), Command::Erase)],
        does: "Erase the last character",
        hint: None,
    },
    Binding {
        keys: &[(Press::key(KeyCode::Esc), Command::Cancel)],
        does: "Clear the filter",
        hint: Some("Esc clear"),
    },
];

/// How many commits one background read for the filter reads at most, so
/// the filtered list grows as matches are found on a long history.
const BACKGROUND_BATCH: usize = 1_000;

/// How many more commits `G` reads, at most, before it goes to the oldest
/// commit read: enough to reach the end of most histories at once, and few
/// enough to read in a moment, so the picker stays responsive on a history
/// of any length. Each `G` goes this much further.
const READ_FOR_OLDEST: usize = 1_000;

/// The picker's own action in the palette.
const ACTIONS: &[Action] = &[Action {
    label: "Back",
    does: "Return to the screen before",
    command: Command::Back,
}];

/// The repository picker's own actions in the palette: the picker's, and
/// filtering the commits.
const REPOSITORY_ACTIONS: &[Action] = &[
    ACTIONS[0],
    Action {
        label: "Search",
        does: "Filter the commits by message, author or ID",
        command: Command::Search,
    },
];

/// The repository picker's own actions while a filter is on: as without
/// one, and clearing it.
const FILTERED_ACTIONS: &[Action] = &[
    ACTIONS[0],
    REPOSITORY_ACTIONS[1],
    Action {
        label: "Clear the filter",
        does: "Show every commit again",
        command: Command::Cancel,
    },
];

/// The picker's controls, in focus order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    /// The area at this index.
    Area(usize),
    /// The change at this index.
    Change(usize),
    /// Clear the filter, in the change list while the filter matches
    /// nothing.
    ClearFilter,
    /// Previous page, under the changes: the page before shows.
    PreviousPage,
    /// Next page, under the changes: the page after shows.
    NextPage,
    /// What comes after choosing: "Generate questions" on the showcase
    /// project, which starts Practice once Practice can ask Jev and the
    /// generator, and "Review what will be sent" on the learner's
    /// repository.
    Proceed,
}

/// Whether an area can be chosen, and whether it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AreaChoice {
    /// A valid area, selected or not.
    Valid { selected: bool },
    /// An invalid area file, which cannot be selected.
    Invalid,
}

/// What the picker practices on, and what it read of it.
#[derive(Debug)]
enum Source {
    /// The showcase project, with this many curated changes, and whether
    /// Practice can ask Jev and the generator.
    Showcase { changes: usize, ai_available: bool },
    /// The learner's repository, with the commits reachable from HEAD read
    /// so far, newest first, or why they could not be read. The history
    /// holds the repository it reads, so it is boxed.
    Repository {
        history: Result<Box<History>, HistoryError>,
    },
}

/// A filter of the commits: what it looks for, and which of the commits
/// read so far match.
#[derive(Debug)]
struct Filter {
    query: Query,
    /// The indexes of the matching commits, in order.
    matches: Vec<usize>,
    /// How many of the commits read so far it has looked at.
    looked_at: usize,
    /// How many matches the change list wants: enough for the page in view
    /// and the one after. Older commits are read in the background until it
    /// has them or the history ends.
    wanted: usize,
}

impl Filter {
    fn new(query: Query) -> Self {
        Self {
            query,
            matches: Vec::new(),
            looked_at: 0,
            wanted: 0,
        }
    }

    /// The index of the match at `position` among the matches.
    fn at(&self, position: usize) -> Option<usize> {
        self.matches.get(position).copied()
    }

    /// Where the commit at `index` is among the matches, if it matches.
    fn position_of(&self, index: usize) -> Option<usize> {
        self.matches.binary_search(&index).ok()
    }

    /// Looks at the `commits` read since it last looked, for matches.
    fn look_at(&mut self, commits: &[Commit]) {
        let new = commits.iter().enumerate().skip(self.looked_at);
        self.matches.extend(
            new.filter(|(_, commit)| is_match(&self.query, commit))
                .map(|(index, _)| index),
        );
        self.looked_at = commits.len();
    }
}

/// The picker's state: which areas are selected, which change is chosen,
/// and which control has focus.
#[derive(Debug)]
pub(crate) struct Picker {
    source: Source,
    /// The areas offered, each valid or an invalid area file, kept so that
    /// a key press knows how many rows they take, and so how many changes a
    /// page holds, as the frame draws them.
    offered: Vec<Result<Area, InvalidAreaFile>>,
    /// Whether each area can be chosen and is, in the order offered.
    areas: Vec<AreaChoice>,
    /// The index of the chosen change, once the learner chooses one.
    chosen: Option<usize>,
    focus: Control,
    /// The index of the change the change list was last scrolled to show at
    /// its top. A frame moves it only as far as it must to show the focused
    /// change, so a resize changes nothing here.
    top: usize,
    /// The search box, while the learner types a filter of the commits.
    search_box: Option<TextEntry>,
    /// The filter of the commits, while one is on.
    filter: Option<Filter>,
    /// The background read of older commits for the filter, while one
    /// runs.
    reading: Option<JobId>,
    /// The sizes of the commits on screen, found in the background, on the
    /// learner's repository.
    sizes: Option<Sizes>,
    /// The learner's choices on the outbound review of the chosen commit,
    /// kept while it stays chosen, and shared with the review while it is
    /// open.
    review_choices: Option<(CommitId, Rc<RefCell<ReviewChoices>>)>,
}

impl Picker {
    /// The picker over `showcase`, as it first opens: every area selected.
    /// Generate questions starts Practice only if Practice can ask Jev and
    /// the generator, as `ai_available` says.
    pub(crate) fn new(showcase: &ShowcaseProject, ai_available: bool) -> Self {
        Self {
            source: Source::Showcase {
                changes: showcase.changes().len(),
                ai_available,
            },
            offered: showcase.areas().iter().cloned().map(Ok).collect(),
            areas: vec![AreaChoice::Valid { selected: true }; showcase.areas().len()],
            chosen: None,
            focus: first_control(showcase.areas().len(), showcase.changes().len()),
            top: 0,
            search_box: None,
            filter: None,
            reading: None,
            sizes: None,
            review_choices: None,
        }
    }

    /// The picker over the learner's `repository`, as it first opens: every
    /// valid area selected, and its first commits read.
    pub(crate) fn for_repository(repository: &Repository) -> Self {
        let history = repository.history().map(Box::new);
        let commits = history
            .as_ref()
            .map_or(0, |history| history.commits().len());
        Self {
            offered: repository.areas().to_vec(),
            areas: repository
                .areas()
                .iter()
                .map(|area| match area {
                    Ok(_) => AreaChoice::Valid { selected: true },
                    Err(_) => AreaChoice::Invalid,
                })
                .collect(),
            focus: first_control(repository.areas().len(), commits),
            source: Source::Repository { history },
            chosen: None,
            top: 0,
            search_box: None,
            filter: None,
            reading: None,
            sizes: Some(Sizes::new(repository.handle())),
            review_choices: None,
        }
    }

    /// The commits read so far, newest first, if the picker is over the
    /// learner's repository and they could be read.
    fn commits(&self) -> &[Commit] {
        match &self.source {
            Source::Repository {
                history: Ok(history),
            } => history.commits(),
            Source::Showcase { .. } | Source::Repository { history: Err(_) } => &[],
        }
    }

    /// How many changes the picker lists.
    fn changes(&self) -> usize {
        match self.source {
            Source::Showcase { changes, .. } => changes,
            Source::Repository { .. } => self.commits().len(),
        }
    }

    /// Whether any area is selected: Practice needs one or more.
    fn has_area(&self) -> bool {
        self.areas.contains(&AreaChoice::Valid { selected: true })
    }

    /// Whether any area can be selected: a repository whose area files are
    /// all invalid offers none.
    fn offers_an_area(&self) -> bool {
        self.areas
            .iter()
            .any(|area| matches!(area, AreaChoice::Valid { .. }))
    }

    /// The areas as list items `width` cells wide. Each is a choice showing
    /// its name, with its one-line description in a column after the
    /// widest name, cut to fit. An invalid area file shows its file name,
    /// disabled, and the reason it is invalid, wrapped in that column so it
    /// can be read whole.
    fn area_items(&self, theme: &Theme, width: u16) -> Vec<Text<'_>> {
        let offered = &self.offered;
        let rows: Vec<Line<'_>> = offered
            .iter()
            .zip(&self.areas)
            .enumerate()
            .map(|(index, (area, &choice))| {
                let focused = self.focus == Control::Area(index);
                match area {
                    Ok(area) => theme.control(
                        area.name(),
                        ControlState {
                            focused,
                            kind: ControlKind::Choice {
                                selected: choice == AreaChoice::Valid { selected: true },
                            },
                            disabled: None,
                        },
                    ),
                    Err(invalid_file) => invalid_area(theme, invalid_file.file_name(), focused),
                }
            })
            .collect();
        let width = usize::from(width);
        // Past the widest name, but leaving descriptions half of the row.
        let column =
            (rows.iter().map(Line::width).max().unwrap_or_default() + columns::GAP).min(width / 2);
        rows.into_iter()
            .zip(offered)
            .map(|(mut line, area)| {
                let start = column.max(line.width() + columns::GAP);
                match area {
                    Ok(area) => {
                        columns::pad_to(&mut line, start);
                        line.push_span(Span::styled(
                            columns::truncate(area.description(), width.saturating_sub(start)),
                            theme.muted,
                        ));
                        Text::from(line)
                    }
                    Err(invalid_file) => {
                        let reason = invalid_file.reason().to_string();
                        let mut wrapped =
                            columns::wrap(&reason, width.saturating_sub(column)).into_iter();
                        let mut lines = Vec::new();
                        if start == column {
                            columns::pad_to(&mut line, column);
                            line.extend(
                                wrapped.next().map(|first| Span::styled(first, theme.muted)),
                            );
                        }
                        lines.push(line);
                        lines.extend(wrapped.map(|rest| {
                            Line::from(vec![
                                Span::raw(" ".repeat(column)),
                                Span::styled(rest, theme.muted),
                            ])
                        }));
                        Text::from(lines)
                    }
                }
            })
            .collect()
    }

    /// How many rows the changes take: one for each listed, one saying no
    /// commit matches the filter when none does, and Clear the filter after
    /// it, and one for why no more commits could be read, if none or not
    /// all could.
    fn change_rows(&self) -> usize {
        match &self.source {
            Source::Showcase { changes, .. } => *changes,
            Source::Repository {
                history: Ok(history),
            } => {
                let failed = usize::from(matches!(history.end(), HistoryEnd::Failed(_)));
                let no_match =
                    usize::from(self.matches_nothing()) + usize::from(self.offers_clear());
                self.listed() + no_match + failed
            }
            Source::Repository { history: Err(_) } => 1,
        }
    }

    /// How many changes the change list lists: every change, or while a
    /// filter is on, those it matches.
    fn listed(&self) -> usize {
        match &self.filter {
            Some(filter) => filter.matches.len(),
            None => self.changes(),
        }
    }

    /// The change at `position` in the change list, if it lists one there.
    fn listed_at(&self, position: usize) -> Option<Control> {
        let index = match &self.filter {
            Some(filter) => filter.at(position)?,
            None => position,
        };
        (index < self.changes()).then_some(Control::Change(index))
    }

    /// The commits the change list lists at the positions in `window`, by
    /// index.
    fn in_window(&self, window: Range<usize>) -> impl Iterator<Item = (usize, &Commit)> {
        let listed = self.listed();
        (window.start.min(listed)..window.end.min(listed)).filter_map(|position| {
            match self.listed_at(position)? {
                Control::Change(index) => Some((index, self.commits().get(index)?)),
                _ => None,
            }
        })
    }

    /// Wants the sizes of the commits on screen while the change list shows
    /// `rows` rows, for [`View::start_jobs`] to find.
    fn want_sizes(&mut self, rows: usize) {
        let top = self.top_in_view(rows);
        let Some(mut sizes) = self.sizes.take() else {
            return;
        };
        sizes.want(self.in_window(top..top + rows).map(|(_, commit)| commit));
        self.sizes = Some(sizes);
    }

    /// Whether the change list lists the commit `id`: every commit read, or
    /// while a filter is on, those it matches.
    fn lists(&self, id: CommitId) -> bool {
        self.commits()
            .iter()
            .position(|commit| commit.id() == id)
            .is_some_and(|index| {
                self.filter
                    .as_ref()
                    .is_none_or(|filter| filter.position_of(index).is_some())
            })
    }

    /// Where the focused change is in the change list, if a change has focus
    /// and the list lists it.
    fn focused_position(&self) -> Option<usize> {
        let index = self.focused_change()?;
        match &self.filter {
            Some(filter) => filter.position_of(index),
            None => Some(index),
        }
    }

    /// Whether a filter is on that matches none of the commits, with no more
    /// to read.
    fn matches_nothing(&self) -> bool {
        self.filter
            .as_ref()
            .is_some_and(|filter| filter.matches.is_empty())
            && !self.has_more()
    }

    /// Whether Clear the filter shows: while the filter matches nothing
    /// so far, once the learner has stopped typing it, so focus always has
    /// somewhere in the list to go.
    fn offers_clear(&self) -> bool {
        self.search_box.is_none()
            && self
                .filter
                .as_ref()
                .is_some_and(|filter| filter.matches.is_empty())
    }

    /// The changes in `window`, by index, as list items `width` cells wide,
    /// each a choice. Only these are built, so a long history read whole
    /// draws as fast as its first page.
    fn change_items<'a>(
        &'a self,
        context: &Context<'a>,
        width: u16,
        window: Range<usize>,
    ) -> Vec<Text<'a>> {
        let state = |index| ControlState {
            focused: self.focus == Control::Change(index),
            kind: ControlKind::Choice {
                selected: self.chosen == Some(index),
            },
            disabled: None,
        };
        let items: Vec<Text<'a>> = match &self.source {
            // The column of areas depends on every row, so all are built.
            Source::Showcase { .. } => showcase_rows(context, width, state)
                .into_iter()
                .map(Text::from)
                .collect(),
            Source::Repository {
                history: Ok(history),
            } => {
                let listed = self.listed();
                let rows = self
                    .in_window(window.clone())
                    .map(|(index, commit)| Row {
                        commit,
                        state: state(index),
                        size: self
                            .sizes
                            .as_ref()
                            .map_or(commit_rows::Sizing::Finding, |sizes| sizes.of(commit.id())),
                    })
                    .collect();
                let mut items: Vec<Text<'a>> =
                    commit_rows::lines(context.theme, rows, context.now, width)
                        .into_iter()
                        .map(Text::from)
                        .collect();
                if let Some(filter) = &self.filter
                    && self.matches_nothing()
                {
                    let said = format!("No commits match “{}”", filter.query.text());
                    let room = usize::from(width).saturating_sub(UNAVAILABLE_INDENT.len());
                    items.push(Text::styled(
                        format!("{UNAVAILABLE_INDENT}{}", columns::truncate(&said, room)),
                        context.theme.muted,
                    ));
                }
                if self.offers_clear() {
                    items.push(Text::from(context.theme.control(
                        "Clear the filter",
                        ControlState {
                            focused: self.focus == Control::ClearFilter,
                            ..ControlState::default()
                        },
                    )));
                }
                if let HistoryEnd::Failed(error) = history.end()
                    && window.end > listed
                {
                    items.push(unavailable_row(context.theme, error));
                }
                return items;
            }
            Source::Repository {
                history: Err(error),
            } => {
                vec![unavailable_row(context.theme, error)]
            }
        };
        items
            .into_iter()
            .skip(window.start)
            .take(window.len())
            .collect()
    }

    /// Filters the change list by `query`, or shows every change again
    /// without one. A new filter shows its matches from the first, and
    /// clearing it scrolls back to the focused change, or else the chosen
    /// one.
    fn set_filter(&mut self, query: Option<Query>) {
        if self.filter.as_ref().map(|filter| &filter.query) == query.as_ref() {
            return;
        }
        if let Some(query) = query {
            let mut filter = Filter::new(query);
            filter.look_at(self.commits());
            self.filter = Some(filter);
            self.top = 0;
        } else {
            self.filter = None;
            if self.focus == Control::ClearFilter {
                self.focus = self.chosen.map_or(
                    first_control(self.areas.len(), self.changes()),
                    Control::Change,
                );
            }
            self.top = self.focused_change().or(self.chosen).unwrap_or_default();
        }
    }

    /// Applies `command` to the search box while the learner types a filter
    /// and the change list shows `rows` rows: Enter keeps the filter, Esc
    /// clears it, and anything else edits it, which filters the list anew.
    fn edit_filter(&mut self, command: Command, rows: usize) {
        let Some(search_box) = &mut self.search_box else {
            return;
        };
        match command {
            Command::Activate => {
                self.search_box = None;
                self.focus_filtered(rows);
            }
            Command::Cancel => {
                self.search_box = None;
                self.set_filter(None);
            }
            command => {
                search_box.apply(command);
                self.refilter();
            }
        }
    }

    /// Filters the change list by what the search box holds, as the learner
    /// types or pastes it.
    fn refilter(&mut self) {
        let query = self
            .search_box
            .as_ref()
            .and_then(|search_box| Query::new(search_box.text()));
        self.set_filter(query);
    }

    /// Moves focus to the filtered list as the learner stops typing the
    /// filter: to the first match in view, unless the focused change
    /// matches, or to Clear the filter when nothing does.
    fn focus_filtered(&mut self, rows: usize) {
        if self.filter.is_none() || self.focused_position().is_some() {
            return;
        }
        let first = self.listed_at(self.top_in_view(rows));
        if let Some(control) = first.or(self.offers_clear().then_some(Control::ClearFilter)) {
            self.focus = control;
        }
    }

    /// The index of the focused change, if a change has focus.
    fn focused_change(&self) -> Option<usize> {
        match self.focus {
            Control::Change(index) => Some(index),
            Control::Area(_)
            | Control::ClearFilter
            | Control::PreviousPage
            | Control::NextPage
            | Control::Proceed => None,
        }
    }

    /// Whether more commits may follow those read so far.
    fn has_more(&self) -> bool {
        matches!(
            &self.source,
            Source::Repository { history: Ok(history) } if matches!(history.end(), HistoryEnd::More)
        )
    }

    /// Reads further, if more commits may follow, until `count` are read.
    fn read_until(&mut self, count: usize) {
        if let Source::Repository {
            history: Ok(history),
        } = &mut self.source
        {
            history.read_until(count);
        }
    }

    /// The index of the change at the top of the change list when it shows
    /// `rows` rows: where it was scrolled to, moved just far enough to show
    /// the focused change, and never past the last whole page.
    fn top_in_view(&self, rows: usize) -> usize {
        let top = self.top.min(self.change_rows().saturating_sub(rows));
        match self.focused_position() {
            Some(position) => top.max((position + 1).saturating_sub(rows)).min(position),
            None => top,
        }
    }

    /// Whether the change list, at `top` and `rows` rows, shows its last
    /// row, with no more commits to read after it.
    fn on_last_page(&self, top: usize, rows: usize) -> bool {
        top + rows >= self.change_rows() && !self.has_more()
    }

    /// Where `command` moves focus while the change list shows `rows` rows.
    /// `j` and `k` move through the areas, the listed changes, the paging
    /// row and the control after them, stopping at either end. Tab and
    /// Shift+Tab move through the areas, the change list as one stop,
    /// Previous page, Next page and the control after them, wrapping around.
    /// Into the change list, they land on the page in view: on its first
    /// change from above, and on its last from below. Clear the filter
    /// stands in for the changes while it shows.
    fn moved(&self, command: Command, rows: usize) -> Control {
        let areas = self.areas.len();
        let top = self.top_in_view(rows);
        let first_area = (areas > 0).then_some(Control::Area(0));
        let last_area = areas.checked_sub(1).map(Control::Area);
        let clear = self.offers_clear().then_some(Control::ClearFilter);
        let first_in_view = self.listed_at(top).or(clear);
        let last_in_view = (top + rows)
            .min(self.listed())
            .checked_sub(1)
            .and_then(|position| self.listed_at(position))
            .or(clear);
        let position = self.focused_position();
        let focus = self.focus;
        match command {
            Command::Down => match focus {
                Control::Area(index) if index + 1 < areas => Control::Area(index + 1),
                Control::Area(_) => first_in_view.unwrap_or(Control::Proceed),
                // Past the last listed, more may follow, so focus waits.
                Control::Change(_) => position
                    .and_then(|position| self.listed_at(position + 1))
                    .unwrap_or(if self.has_more() {
                        focus
                    } else {
                        Control::Proceed
                    }),
                Control::ClearFilter
                | Control::PreviousPage
                | Control::NextPage
                | Control::Proceed => Control::Proceed,
            },
            Command::Up => match focus {
                Control::Area(index) => Control::Area(index.saturating_sub(1)),
                Control::Change(_) => position
                    .and_then(|position| position.checked_sub(1))
                    .and_then(|position| self.listed_at(position))
                    .or(last_area)
                    .unwrap_or(focus),
                Control::ClearFilter => last_area.unwrap_or(focus),
                Control::PreviousPage | Control::NextPage | Control::Proceed => {
                    last_in_view.or(last_area).unwrap_or(focus)
                }
            },
            Command::NextControl => match focus {
                Control::Area(index) if index + 1 < areas => Control::Area(index + 1),
                Control::Area(_) => first_in_view.unwrap_or(Control::PreviousPage),
                Control::Change(_) | Control::ClearFilter => Control::PreviousPage,
                Control::PreviousPage => Control::NextPage,
                Control::NextPage => Control::Proceed,
                Control::Proceed => first_area
                    .or(first_in_view)
                    .unwrap_or(Control::PreviousPage),
            },
            Command::PreviousControl => match focus {
                Control::Area(index) => {
                    index.checked_sub(1).map_or(Control::Proceed, Control::Area)
                }
                Control::Change(_) | Control::ClearFilter => last_area.unwrap_or(Control::Proceed),
                Control::PreviousPage => last_in_view.or(last_area).unwrap_or(Control::Proceed),
                Control::NextPage => Control::PreviousPage,
                Control::Proceed => Control::NextPage,
            },
            _ => focus,
        }
    }

    /// Moves focus to the first change in view, while the change list shows
    /// `rows` rows, unless a change has focus already: the paging keys move
    /// focus through the changes from anywhere on the picker.
    fn focus_page_in_view(&mut self, rows: usize) {
        if self.focused_change().is_none()
            && let Some(first) = self.listed_at(self.top_in_view(rows))
        {
            self.focus = first;
        }
    }

    /// Scrolls the change list down `step` changes while it shows `rows`
    /// rows, and the focused change, if one has focus, with it. Unfiltered,
    /// it reads further first, until the rows it scrolls to are all read, so
    /// a page never stops short because commits weren't read yet, unless a
    /// background read has the reader for a moment; a filter reads in the
    /// background instead. It stops at the last whole page and the last
    /// change.
    fn scroll_down(&mut self, step: usize, rows: usize) {
        let top = self.top_in_view(rows) + step;
        if self.filter.is_none() {
            self.read_until(top + rows);
        }
        self.top = top.min(self.change_rows().saturating_sub(rows));
        if let Some(position) = self.focused_position()
            && let Some(moved) =
                self.listed_at((position + step).min(self.listed().saturating_sub(1)))
        {
            self.focus = moved;
        }
    }

    /// Scrolls the change list up `step` changes while it shows `rows` rows,
    /// and the focused change, if one has focus, with it, stopping at the
    /// first.
    fn scroll_up(&mut self, step: usize, rows: usize) {
        self.top = self.top_in_view(rows).saturating_sub(step);
        if let Some(position) = self.focused_position()
            && let Some(moved) = self.listed_at(position.saturating_sub(step))
        {
            self.focus = moved;
        }
    }

    /// Moves focus to the first change listed.
    fn go_to_first(&mut self) {
        if let Some(first) = self.listed_at(0) {
            self.focus = first;
        }
    }

    /// Moves focus to the last change listed: the oldest change, or for a
    /// repository the oldest commit read, after reading at most
    /// [`READ_FOR_OLDEST`] more unless a filter is on.
    fn go_to_oldest(&mut self) {
        if self.filter.is_none() {
            self.read_until(self.changes() + READ_FOR_OLDEST);
        }
        if let Some(last) = self
            .listed()
            .checked_sub(1)
            .and_then(|position| self.listed_at(position))
        {
            self.focus = last;
        }
    }

    /// Reads ahead while more commits may follow, until the page after the
    /// one in view, `rows` rows long, is read too: the page in view is
    /// whole, and Next page knows whether there is one. A filter wants as
    /// many matches, which a background read finds.
    fn read_ahead(&mut self, rows: usize) {
        let wanted = self.top + 2 * rows;
        match &mut self.filter {
            Some(filter) => filter.wanted = wanted,
            None => self.read_until(wanted),
        }
    }

    /// What comes after choosing, once a change and an area are chosen:
    /// Practice on the chosen showcase change, or the outbound review of the
    /// chosen commit, with the choices kept for it. While it is disabled, as
    /// it says, nothing happens.
    fn next_screen(&mut self) -> Option<Command> {
        if self.proceed().1.is_some() {
            return None;
        }
        if let Source::Showcase { .. } = self.source {
            return Some(Command::Go(Route::Practice(ChangeRef::Showcase(
                ChangeIndex::new(self.chosen?),
            ))));
        }
        let id = self.commits().get(self.chosen?)?.id();
        if self.kept_choices(id).is_none() {
            self.review_choices = Some((id, Rc::default()));
        }
        Some(Command::Go(Route::OutboundReview(id)))
    }

    /// The review choices kept for the commit `id`, if any are.
    fn kept_choices(&self, id: CommitId) -> Option<&Rc<RefCell<ReviewChoices>>> {
        self.review_choices
            .as_ref()
            .filter(|(kept, _)| *kept == id)
            .map(|(_, choices)| choices)
    }

    /// The control after the lists, and why it is disabled, if it is.
    fn proceed(&self) -> (&'static str, Option<&'static str>) {
        match self.source {
            Source::Showcase {
                ai_available: false,
                ..
            } => ("Generate questions", Some(NEEDS_KEY)),
            Source::Showcase {
                ai_available: true, ..
            } => ("Generate questions", self.choices_missing()),
            Source::Repository { .. } => ("Review what will be sent", self.choices_missing()),
        }
    }

    /// What is still to choose before Practice can go on, if anything.
    fn choices_missing(&self) -> Option<&'static str> {
        match (self.chosen.is_some(), self.has_area()) {
            _ if !self.offers_an_area() => Some(NO_VALID_AREA),
            (true, true) => None,
            (true, false) => Some("choose an area"),
            (false, true) => Some("choose a change"),
            (false, false) => Some("choose a change and an area"),
        }
    }

    /// The areas chosen among `offered`, in the order offered: those valid
    /// and selected.
    fn chosen<T>(&self, offered: impl IntoIterator<Item = T>) -> impl Iterator<Item = T> {
        offered
            .into_iter()
            .zip(&self.areas)
            .filter(|(_, choice)| **choice == AreaChoice::Valid { selected: true })
            .map(|(area, _)| area)
    }

    /// Where the picker's parts go in `area`, with the areas taking
    /// `area_rows` rows and the changes `change_rows`.
    fn parts(&self, area: Rect, area_rows: usize, change_rows: usize) -> Parts {
        let status_rows = match self.source {
            Source::Showcase { .. } => 0,
            Source::Repository { .. } => 1,
        };
        layout(area, area_rows, change_rows, status_rows)
    }

    /// How many changes a page holds with the picker drawn in `area`, as
    /// [`View::render`] lays it out, with `change_rows` rows of changes to
    /// show.
    fn page_size(&self, area: Rect, change_rows: usize) -> usize {
        // Styles never change how many rows the areas take.
        let area_items = self.area_items(&Theme::new(ColorSupport::NoColor), area.width);
        page_size_of(
            self.parts(area, height(&area_items), change_rows)
                .change_list,
        )
    }

    /// How many changes the change list has room for in `area`: a page, once
    /// enough commits are read to fill it.
    fn room(&self, area: Rect) -> usize {
        self.page_size(area, usize::MAX)
    }

    /// Where the change list is, at `top` and `rows` rows: "Commits 41–60
    /// of 1,000", with a "+" after the total while more commits may follow.
    /// Filtered, it says so: "12 matching “retry” · 1–12", with a "+" while
    /// more may match, and while older commits are read for more matches,
    /// that they are, with the query cut to fit `room` cells.
    fn position(&self, top: usize, rows: usize, room: usize) -> Option<String> {
        let more = if self.has_more() { "+" } else { "" };
        if let Some(filter) = &self.filter {
            let found = filter.matches.len();
            let said = |query: &str| {
                let mut said = match found.checked_sub(1) {
                    None => String::new(),
                    Some(last_found) => {
                        let first = top.min(last_found) + 1;
                        let last = (top + rows).min(found);
                        let shown = if first == last {
                            grouped(first)
                        } else {
                            format!("{}–{}", grouped(first), grouped(last))
                        };
                        format!("{}{more} matching “{query}” · {shown}", grouped(found))
                    }
                };
                if self.reading.is_some() {
                    said.push_str(if said.is_empty() {
                        "Searching older commits…"
                    } else {
                        " · searching older commits…"
                    });
                }
                said
            };
            // The query is cut to fit, so the rest always shows.
            let around = Line::raw(said("")).width();
            let query = columns::truncate(filter.query.text(), room.saturating_sub(around));
            return Some(said(&query));
        }
        let changes = self.changes();
        let first = top.min(changes.checked_sub(1)?) + 1;
        let last = (top + rows).min(changes);
        let (one, many) = match self.source {
            Source::Showcase { .. } => ("Change", "Changes"),
            Source::Repository { .. } => ("Commit", "Commits"),
        };
        let shown = if first == last {
            format!("{one} {}", grouped(first))
        } else {
            format!("{many} {}–{}", grouped(first), grouped(last))
        };
        Some(format!("{shown} of {}{more}", grouped(changes)))
    }

    /// The heading above the change list, `width` cells wide, while the
    /// list shows `rows` rows: "Change to practice on", and after it where
    /// the list is, cut to fit.
    fn change_heading(&self, rows: usize, width: u16) -> String {
        let before = format!("{CHANGE_HEADING} · ");
        let room = usize::from(width).saturating_sub(Line::raw(&before).width());
        let heading = match self.position(self.top_in_view(rows), rows, room) {
            Some(position) if !position.is_empty() => format!("{before}{position}"),
            _ => CHANGE_HEADING.to_owned(),
        };
        columns::truncate(&heading, usize::from(width))
    }

    /// The row under the change list while it shows `rows` rows: Previous
    /// page, disabled on the first page, and Next page, disabled on the
    /// last.
    fn paging_row(&self, theme: &Theme, rows: usize) -> Line<'static> {
        let top = self.top_in_view(rows);
        let control = |label, control, disabled| {
            theme.control(
                label,
                ControlState {
                    focused: self.focus == control,
                    disabled,
                    ..ControlState::default()
                },
            )
        };
        let mut row = control(
            "Previous page",
            Control::PreviousPage,
            (top == 0).then_some(FIRST_PAGE),
        );
        let next = control(
            "Next page",
            Control::NextPage,
            self.on_last_page(top, rows).then_some(LAST_PAGE),
        );
        row.push_span(" ".repeat(columns::GAP));
        row.extend(next.spans);
        row
    }

    /// Applies `command` itself while the change list shows `rows` rows, and
    /// returns what it asks of the app, if anything.
    fn apply_command(&mut self, command: Command, rows: usize) -> Option<Command> {
        if self.search_box.is_some() {
            self.edit_filter(command, rows);
            return None;
        }
        match command {
            Command::Search => {
                // It opens on the filter in place, to change or clear it.
                let mut search_box = TextEntry::default();
                if let Some(filter) = &self.filter {
                    search_box.paste(filter.query.text());
                }
                self.search_box = Some(search_box);
                return None;
            }
            Command::Cancel => {
                self.set_filter(None);
                return None;
            }
            Command::PageDown => {
                self.focus_page_in_view(rows);
                self.scroll_down(rows, rows);
            }
            Command::PageUp => {
                self.focus_page_in_view(rows);
                self.scroll_up(rows, rows);
            }
            Command::HalfPageDown => {
                self.focus_page_in_view(rows);
                self.scroll_down((rows / 2).max(1), rows);
            }
            Command::HalfPageUp => {
                self.focus_page_in_view(rows);
                self.scroll_up((rows / 2).max(1), rows);
            }
            Command::Top => self.go_to_first(),
            Command::Bottom => self.go_to_oldest(),
            _ => {
                // Moving past the last commit read reads further first. A
                // filter reads in the background instead.
                if command == Command::Down
                    && self.filter.is_none()
                    && self
                        .focused_change()
                        .is_some_and(|index| index + 1 == self.changes())
                {
                    self.read_until(self.changes() + 1);
                }
                self.focus = self.moved(command, rows);
            }
        }
        if !matches!(command, Command::Activate | Command::Open) {
            return None;
        }
        match self.focus {
            Control::Area(index) => {
                if let Some(AreaChoice::Valid { selected }) = self.areas.get_mut(index) {
                    *selected = !*selected;
                }
                None
            }
            // A change the filter hides can't be chosen or opened.
            Control::Change(_) if self.focused_position().is_none() => None,
            Control::Change(index) => {
                let change = match self.source {
                    Source::Showcase { .. } => ChangeRef::Showcase(ChangeIndex::new(index)),
                    Source::Repository { .. } => match self.commits().get(index) {
                        Some(commit) if commit.availability() != Availability::ParentMissing => {
                            ChangeRef::Commit(commit.id())
                        }
                        _ => return None,
                    },
                };
                // Review choices are for one change, and never carry to
                // another.
                if let ChangeRef::Commit(id) = change
                    && self.kept_choices(id).is_none()
                {
                    self.review_choices = None;
                }
                self.chosen = Some(index);
                (command == Command::Open).then_some(Command::Go(Route::Viewer(change)))
            }
            Control::PreviousPage => {
                self.scroll_up(rows, rows);
                None
            }
            Control::NextPage => {
                self.scroll_down(rows, rows);
                None
            }
            Control::ClearFilter => {
                self.set_filter(None);
                None
            }
            Control::Proceed => self.next_screen(),
        }
    }
}

/// Whether `query` matches `commit`: its whole message, its author's name,
/// or the start of its ID.
fn is_match(query: &Query, commit: &Commit) -> bool {
    query.is_in(commit.message())
        || query.is_in(commit.author())
        || commit
            .id()
            .full()
            .starts_with(&query.text().to_ascii_lowercase())
}

/// A muted row in a list saying why it cannot list more.
fn unavailable_row<'a>(theme: &Theme, error: &HistoryError) -> Text<'a> {
    Text::styled(format!("{UNAVAILABLE_INDENT}{error}"), theme.muted)
}

/// The control of an invalid area file, labelled `label`: a choice that is
/// disabled.
fn invalid_area<'a>(theme: &Theme, label: &'a str, focused: bool) -> Line<'a> {
    theme.control(
        label,
        ControlState {
            focused,
            kind: ControlKind::Choice { selected: false },
            disabled: Some("invalid"),
        },
    )
}

/// The showcase project's changes as rows `width` cells wide, each a choice
/// in the state `state` gives for its index, showing its summary, with the
/// areas it touches right-aligned in a column. The column ends where the
/// widest row does, as far as `width` allows.
fn showcase_rows<'a>(
    context: &Context<'a>,
    width: u16,
    state: impl Fn(usize) -> ControlState<'a>,
) -> Vec<Line<'a>> {
    let rows: Vec<(Line<'a>, String)> = context
        .showcase
        .changes()
        .iter()
        .enumerate()
        .map(|(index, change)| {
            let line = context.theme.control(change.summary(), state(index));
            (line, context.showcase.area_names(change).join(", "))
        })
        .collect();
    let areas_width = |areas: &str| Line::raw(areas).width();
    let end = rows
        .iter()
        .map(|(line, areas)| line.width() + columns::GAP + areas_width(areas))
        .max()
        .unwrap_or_default()
        .min(usize::from(width));
    rows.into_iter()
        .map(|(mut line, areas)| {
            let column = end
                .saturating_sub(areas_width(&areas))
                .max(line.width() + columns::GAP);
            columns::pad_to(&mut line, column);
            line.push_span(Span::styled(areas, context.theme.muted));
            line
        })
        .collect()
}

/// In front of a line in a list that is not a control, as wide as the focus
/// marker.
const UNAVAILABLE_INDENT: &str = "  ";

/// The heading above the areas.
const AREA_HEADING: &str = "Areas the questions focus on";
/// The heading above the changes.
const CHANGE_HEADING: &str = "Change to practice on";

/// Why Practice cannot start where every area file is invalid.
const NO_VALID_AREA: &str = "no area file is valid";

/// Why Previous page is disabled on the first page.
const FIRST_PAGE: &str = "first page";
/// Why Next page is disabled on the last page.
const LAST_PAGE: &str = "last page";

/// Where focus starts on a picker with `areas` areas and `changes` changes:
/// the first area, or what comes after if there is none.
fn first_control(areas: usize, changes: usize) -> Control {
    if areas > 0 {
        Control::Area(0)
    } else if changes > 0 {
        Control::Change(0)
    } else {
        Control::Proceed
    }
}

/// Where the picker's parts go, top to bottom.
#[derive(Debug)]
struct Parts {
    title: Rect,
    area_heading: Rect,
    area_list: Rect,
    change_heading: Rect,
    change_list: Rect,
    /// The row under the change list, with Previous page and Next page.
    paging: Rect,
    proceed: Rect,
    /// The row at the bottom, for the search box and what a search found.
    status: Rect,
}

/// Where the picker's parts go in `area`, for lists of `area_rows` and
/// `change_rows` rows, and `status_rows` at the bottom. The title,
/// headings, lists, the paging row under the changes and the control after
/// them get their rows first, and the gaps between them share what is
/// left, so a short terminal keeps every control. When even the lists do
/// not fit, they scroll, and the change list always keeps at least one row.
fn layout(area: Rect, area_rows: usize, change_rows: usize, status_rows: u16) -> Parts {
    const TITLE: u16 = 1;
    const HEADING: u16 = 1;
    const PAGING: u16 = 1;
    const PROCEED: u16 = 1;
    let [area, status] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(status_rows)]).areas(area);
    let rows = |count: usize| u16::try_from(count).unwrap_or(u16::MAX);
    let mut left = area
        .height
        .saturating_sub(TITLE + 2 * HEADING + PAGING + PROCEED);
    let area_rows = rows(area_rows).min(left.saturating_sub(rows(change_rows.min(1))));
    left -= area_rows;
    let change_rows = rows(change_rows).min(left);
    left -= change_rows;
    let mut take = |wanted: u16| {
        let taken = wanted.min(left);
        left -= taken;
        taken
    };
    let before_changes = take(1);
    let before_proceed = take(1);
    let after_title = take(1);
    let [
        title,
        _,
        area_heading,
        area_list,
        _,
        change_heading,
        change_list,
        paging,
        _,
        proceed,
    ] = Layout::vertical([
        Constraint::Length(TITLE),
        Constraint::Length(after_title),
        Constraint::Length(HEADING),
        Constraint::Length(area_rows),
        Constraint::Length(before_changes),
        Constraint::Length(HEADING),
        Constraint::Length(change_rows),
        Constraint::Length(PAGING),
        Constraint::Length(before_proceed),
        Constraint::Length(PROCEED),
    ])
    .areas(area);
    Parts {
        title,
        area_heading,
        area_list,
        change_heading,
        change_list,
        paging,
        proceed,
        status,
    }
}

/// How many rows `items` take.
fn height(items: &[Text<'_>]) -> usize {
    items.iter().map(Text::height).sum()
}

/// How many changes a page holds: as many as `change_list` shows, and at
/// least one.
fn page_size_of(change_list: Rect) -> usize {
    usize::from(change_list.height).max(1)
}

/// `items` as a list in `area`, scrolled so that the item at `shown`, if
/// any, is in view.
fn render_list(frame: &mut Frame<'_>, area: Rect, items: Vec<Text<'_>>, shown: Option<usize>) {
    frame.render_stateful_widget(
        List::new(items),
        area,
        &mut ListState::default().with_selected(shown),
    );
}

impl View for Picker {
    fn route(&self) -> Route {
        Route::Picker(match self.source {
            Source::Showcase { .. } => Project::Showcase,
            Source::Repository { .. } => Project::Repository,
        })
    }

    fn name(&self) -> &'static str {
        "Practice"
    }

    fn keymap(&self) -> Keymap {
        match (&self.source, &self.search_box) {
            (_, Some(_)) => FILTER_BOX,
            (Source::Showcase { .. }, None) => KEYMAP,
            (Source::Repository { .. }, None) if self.filter.is_some() => FILTERED_KEYMAP,
            (Source::Repository { .. }, None) => REPOSITORY_KEYMAP,
        }
    }

    fn input(&self) -> Input {
        match self.search_box {
            Some(_) => Input::Text,
            None => Input::Commands,
        }
    }

    fn paste(&mut self, text: &str) {
        if let Some(search_box) = &mut self.search_box {
            search_box.paste(text);
            self.refilter();
        }
    }

    fn actions(&self) -> &'static [Action] {
        match self.source {
            Source::Showcase { .. } => ACTIONS,
            Source::Repository { .. } if self.filter.is_some() => FILTERED_ACTIONS,
            Source::Repository { .. } => REPOSITORY_ACTIONS,
        }
    }

    fn start_jobs(&mut self, jobs: &mut Jobs<JobOutput>) {
        if let Some(sizes) = &mut self.sizes {
            sizes.start_jobs(jobs);
        }
        let (Some(filter), None) = (&self.filter, self.reading) else {
            return;
        };
        let mut wanted = filter.wanted.saturating_sub(filter.matches.len());
        let query = filter.query.clone();
        let Source::Repository {
            history: Ok(history),
        } = &mut self.source
        else {
            return;
        };
        if wanted == 0 {
            return;
        }
        let Some(reader) = history.take_reader() else {
            return;
        };
        // Off the UI loop (rule 19), until the matches wanted are found, a
        // batch is read, or the history ends.
        self.reading = Some(jobs.start(move |token| {
            let batch = reader.read(token, BACKGROUND_BATCH, |commit| {
                if is_match(&query, commit) {
                    wanted -= 1;
                }
                wanted == 0
            })?;
            Ok(JobOutput::CommitsRead(Box::new(batch)))
        }));
    }

    fn waits_for(&self, job: JobId) -> bool {
        self.reading == Some(job)
            || self
                .sizes
                .as_ref()
                .is_some_and(|sizes| sizes.waits_for(job))
    }

    fn finish_job(&mut self, job: JobId, output: JobOutput) {
        let output = match output {
            JobOutput::ChangeSized { commit, size } => {
                let listed = self.lists(commit);
                if let Some(sizes) = &mut self.sizes {
                    sizes.finish(job, commit, size, listed);
                }
                return;
            }
            output => output,
        };
        if self.reading != Some(job) {
            return;
        }
        self.reading = None;
        if let (
            JobOutput::CommitsRead(batch),
            Source::Repository {
                history: Ok(history),
            },
        ) = (output, &mut self.source)
        {
            history.give_back(*batch);
            if let Some(filter) = &mut self.filter {
                filter.look_at(history.commits());
            }
        }
        // Focus waited on Clear the filter for the first matches, which
        // have come.
        if self.focus == Control::ClearFilter && !self.offers_clear() {
            self.focus = self.listed_at(self.top).unwrap_or(Control::ClearFilter);
        }
    }

    fn open(&self, route: Route, context: &Context<'_>) -> Option<Box<dyn View>> {
        if let Route::Practice(ChangeRef::Showcase(index)) = route {
            let change = index.of(context.showcase)?.clone();
            let tree = ShowcaseTree::at(context.showcase, index.get())?;
            return Some(Box::new(Practice::on_showcase(
                index,
                change,
                tree,
                self.chosen(context.showcase.areas()).cloned().collect(),
                context,
            )?));
        }
        let Route::OutboundReview(id) = route else {
            return None;
        };
        let repository = context.repository.ok()?;
        let choices = self.kept_choices(id)?;
        let change = match repository.load(id) {
            Ok(change) => change,
            Err(error) => return Some(Box::new(Unreadable::new(route, id, &error))),
        };
        let areas = self
            .chosen(repository.areas())
            .filter_map(|area| area.as_ref().ok().cloned())
            .collect();
        let connected_code = Request::new(
            repository.handle(),
            id,
            change.clone(),
            repository.config().exclude().clone(),
            context.analysis.clone(),
            Arc::clone(context.cache),
        );
        Some(Box::new(OutboundReview::new(
            id,
            context,
            change,
            areas,
            repository.config(),
            Rc::clone(choices),
            connected_code,
        )))
    }

    fn fit(&mut self, area: Rect) {
        self.read_ahead(self.room(area));
        self.want_sizes(self.page_size(area, self.change_rows()));
    }

    fn apply(&mut self, command: Command, area: Rect) -> Option<Command> {
        let rows = self.page_size(area, self.change_rows());
        let next = self.apply_command(command, rows);
        // The change list stays scrolled to show where focus went, and the
        // page after it is read, but for after `G`, which goes to the
        // oldest commit read.
        self.top = self.top_in_view(rows);
        if command != Command::Bottom {
            self.read_ahead(self.room(area));
        }
        self.want_sizes(self.page_size(area, self.change_rows()));
        next
    }

    fn render(&self, context: &Context<'_>, frame: &mut Frame<'_>, area: Rect) {
        let theme = context.theme;
        let area_items = self.area_items(theme, area.width);
        let parts = self.parts(area, height(&area_items), self.change_rows());
        let title = match (&self.source, context.repository) {
            (Source::Repository { .. }, Ok(repository)) => {
                format!("Practice on {}", repository.name())
            }
            _ => "Practice on the showcase project".to_owned(),
        };
        frame.render_widget(
            Paragraph::new(Span::styled(title, theme.title)),
            parts.title,
        );
        // Practice needs one area or more.
        let area_heading = if self.has_area() {
            AREA_HEADING.to_owned()
        } else if self.offers_an_area() {
            format!("{AREA_HEADING}: choose at least one")
        } else {
            format!("{AREA_HEADING}: {NO_VALID_AREA}")
        };
        frame.render_widget(
            Paragraph::new(Line::styled(area_heading, theme.muted)),
            parts.area_heading,
        );
        let shown_area = match self.focus {
            Control::Area(index) => Some(index),
            Control::Change(_)
            | Control::ClearFilter
            | Control::PreviousPage
            | Control::NextPage
            | Control::Proceed => None,
        };
        render_list(frame, parts.area_list, area_items, shown_area);
        // The change list stays where it was scrolled to, as far as the
        // focused change allows.
        let rows = page_size_of(parts.change_list);
        frame.render_widget(
            Paragraph::new(Line::styled(
                self.change_heading(rows, parts.change_heading.width),
                theme.muted,
            )),
            parts.change_heading,
        );
        let top = self.top_in_view(rows);
        render_list(
            frame,
            parts.change_list,
            self.change_items(context, area.width, top..top + rows),
            None,
        );
        frame.render_widget(self.paging_row(theme, rows), parts.paging);
        let (label, disabled) = self.proceed();
        frame.render_widget(
            theme.control(
                label,
                ControlState {
                    focused: self.focus == Control::Proceed,
                    disabled,
                    ..ControlState::default()
                },
            ),
            parts.proceed,
        );
        if let Some(search_box) = &self.search_box {
            frame.render_widget(
                Paragraph::new(search_box.line(theme, "/", usize::from(parts.status.width))),
                parts.status,
            );
        }
    }
}
