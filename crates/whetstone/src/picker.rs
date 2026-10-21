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
use crate::connected_code::{Request, ShowcaseTree};
use crate::entry::TextEntry;
use crate::keymap::{self, Binding, Command, Input, Keymap, Press};
use crate::outbound_review::OutboundReview;
use crate::palette::Action;
use crate::practice::Practice;
use crate::reasoning_editor::grouped;
use crate::repository::{Availability, Commit, History, HistoryEnd, HistoryError, Repository};
use crate::search::{self, Query};
use crate::theme::{ColorSupport, ControlKind, ControlState, Theme};
use crate::view::{
    ChangeIndex, ChangeRef, Context, NEEDS_KEY, Project, Route, View, comparison_label,
};
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
    hint: Some("PgDn page"),
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
/// search the commits, with `n` and `N` for the matches.
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
        does: "Search the commits by message or ID",
        ..keymap::SEARCH
    },
    keymap::MATCHES,
    keymap::BACK,
];

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
/// searching the commits.
const REPOSITORY_ACTIONS: &[Action] = &[
    ACTIONS[0],
    Action {
        label: "Search",
        does: "Search the commits by message or ID",
        command: Command::Search,
    },
    Action {
        label: "Next match",
        does: "Go to the next commit the last search matched",
        command: Command::NextMatch,
    },
    Action {
        label: "Previous match",
        does: "Go to the previous commit the last search matched",
        command: Command::PreviousMatch,
    },
];

/// The picker's controls, in focus order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    /// The area at this index.
    Area(usize),
    /// The change at this index.
    Change(usize),
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
    /// The search box, while the learner types a search of the commits.
    search_box: Option<TextEntry>,
    /// What the last search of the commits looked for.
    query: Option<Query>,
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
            query: None,
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
            query: None,
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

    /// How many rows the changes take: one each, and one for why no more
    /// commits could be read, if none or not all could.
    fn change_rows(&self) -> usize {
        match &self.source {
            Source::Showcase { changes, .. } => *changes,
            Source::Repository {
                history: Ok(history),
            } => {
                history.commits().len()
                    + usize::from(matches!(history.end(), HistoryEnd::Failed(_)))
            }
            Source::Repository { history: Err(_) } => 1,
        }
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
                let commits = history.commits();
                let mut items: Vec<Text<'a>> = commits
                    .get(window.start.min(commits.len())..window.end.min(commits.len()))
                    .unwrap_or_default()
                    .iter()
                    .zip(window.start..)
                    .map(|(commit, index)| {
                        Text::from(commit_row(context.theme, commit, state(index), width))
                    })
                    .collect();
                if let HistoryEnd::Failed(error) = history.end()
                    && window.end > commits.len()
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

    /// Searches the commits for `query`, from the focused one, or from the
    /// first if none has focus, and moves focus to the first match. While
    /// none of the commits read so far matches, it reads further.
    fn search(&mut self, query: Query) {
        if let Source::Repository {
            history: Ok(history),
        } = &mut self.source
        {
            while !history
                .commits()
                .iter()
                .any(|commit| is_match(&query, commit))
                && matches!(history.end(), HistoryEnd::More)
            {
                history.read_page();
            }
        }
        let matches = self.matches(&query);
        let start = self.focused_change().unwrap_or_default();
        self.query = Some(query);
        if let Some(index) = search::first_from(&matches, start) {
            self.focus_change(index);
        }
    }

    /// Moves focus to the next or previous match of the last search, as
    /// `command` asks, wrapping around at either end.
    fn go_to_match(&mut self, command: Command) {
        let Some(query) = &self.query else {
            return;
        };
        let matches = self.matches(query);
        let current = self.focused_change();
        let landing = match command {
            Command::NextMatch => {
                search::first_from(&matches, current.map_or(0, |index| index + 1))
            }
            Command::PreviousMatch => search::last_before(&matches, current.unwrap_or_default()),
            _ => None,
        };
        if let Some(index) = landing {
            self.focus_change(index);
        }
    }

    /// The indexes of the commits read so far that `query` matches.
    fn matches(&self, query: &Query) -> Vec<usize> {
        self.commits()
            .iter()
            .enumerate()
            .filter(|(_, commit)| is_match(query, commit))
            .map(|(index, _)| index)
            .collect()
    }

    /// What the last search found, for the row under the picker.
    fn search_status(&self) -> Option<String> {
        let query = self.query.as_ref()?;
        Some(search::status(
            query,
            &self.matches(query),
            self.focused_change(),
            "the commits",
        ))
    }

    /// The index of the focused change, if a change has focus.
    fn focused_change(&self) -> Option<usize> {
        match self.focus {
            Control::Change(index) => Some(index),
            Control::Area(_) | Control::PreviousPage | Control::NextPage | Control::Proceed => None,
        }
    }

    /// Moves focus to the change at `index`.
    fn focus_change(&mut self, index: usize) {
        self.focus = Control::Change(index);
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
    fn top(&self, rows: usize) -> usize {
        let top = self.top.min(self.change_rows().saturating_sub(rows));
        match self.focus {
            Control::Change(index) => top.clamp((index + 1).saturating_sub(rows), index),
            Control::Area(_) | Control::PreviousPage | Control::NextPage | Control::Proceed => top,
        }
    }

    /// Whether the change list, at `top` and `rows` rows, shows its last
    /// row, with no more commits to read after it.
    fn on_last_page(&self, top: usize, rows: usize) -> bool {
        top + rows >= self.change_rows() && !self.has_more()
    }

    /// Where `command` moves focus while the change list shows `rows` rows.
    /// `j` and `k` move through the areas, the changes and the control
    /// after them, stopping at either end, and from the paging controls
    /// to the rows above and below. Tab and Shift+Tab move through the
    /// areas, the change list as one stop, Previous page, Next page and the
    /// control after them, wrapping around. Coming from above, `j` and Tab
    /// land on the first change in view, and Shift+Tab from below on the
    /// last.
    fn moved(&self, command: Command, rows: usize) -> Control {
        let areas = self.areas.len();
        let changes = self.changes();
        let top = self.top(rows);
        let last_area = areas.checked_sub(1).map(Control::Area);
        let first_in_view = (changes > 0).then_some(Control::Change(top));
        let last_in_view = (top + rows)
            .min(changes)
            .checked_sub(1)
            .map(Control::Change);
        let focus = self.focus;
        match command {
            Command::Down => match focus {
                Control::Area(index) if index + 1 < areas => Control::Area(index + 1),
                Control::Area(_) => first_in_view.unwrap_or(Control::Proceed),
                Control::Change(index) if index + 1 < changes => Control::Change(index + 1),
                Control::Change(_)
                | Control::PreviousPage
                | Control::NextPage
                | Control::Proceed => Control::Proceed,
            },
            Command::Up => match focus {
                Control::Area(index) => Control::Area(index.saturating_sub(1)),
                Control::Change(index) => index
                    .checked_sub(1)
                    .map(Control::Change)
                    .or(last_area)
                    .unwrap_or(focus),
                Control::PreviousPage | Control::NextPage => {
                    last_in_view.or(last_area).unwrap_or(focus)
                }
                Control::Proceed => changes
                    .checked_sub(1)
                    .map(Control::Change)
                    .or(last_area)
                    .unwrap_or(focus),
            },
            Command::NextControl => match focus {
                Control::Area(index) if index + 1 < areas => Control::Area(index + 1),
                Control::Area(_) => first_in_view.unwrap_or(Control::PreviousPage),
                Control::Change(_) => Control::PreviousPage,
                Control::PreviousPage => Control::NextPage,
                Control::NextPage => Control::Proceed,
                Control::Proceed => first_control(areas, changes),
            },
            Command::PreviousControl => match focus {
                Control::Area(index) => {
                    index.checked_sub(1).map_or(Control::Proceed, Control::Area)
                }
                Control::Change(_) => last_area.unwrap_or(Control::Proceed),
                Control::PreviousPage => last_in_view.or(last_area).unwrap_or(Control::Proceed),
                Control::NextPage => Control::PreviousPage,
                Control::Proceed => Control::NextPage,
            },
            _ => focus,
        }
    }

    /// Scrolls the change list down `step` changes while it shows `rows`
    /// rows, and the focused change, if one has focus, with it. It reads
    /// further first, until the rows it scrolls to are all read, so a page
    /// never stops short because commits weren't read yet. It stops at the
    /// last whole page and the last change.
    fn scroll_down(&mut self, step: usize, rows: usize) {
        let top = self.top(rows) + step;
        self.read_until(top + rows);
        self.top = top.min(self.change_rows().saturating_sub(rows));
        if let Control::Change(index) = self.focus {
            self.focus = Control::Change((index + step).min(self.changes().saturating_sub(1)));
        }
    }

    /// Scrolls the change list up `step` changes while it shows `rows` rows,
    /// and the focused change, if one has focus, with it, stopping at the
    /// first.
    fn scroll_up(&mut self, step: usize, rows: usize) {
        self.top = self.top(rows).saturating_sub(step);
        if let Control::Change(index) = self.focus {
            self.focus = Control::Change(index.saturating_sub(step));
        }
    }

    /// Moves focus to the oldest change, or for a repository to the oldest
    /// commit read after reading at most [`READ_FOR_OLDEST`] more.
    fn go_to_oldest(&mut self) {
        self.read_until(self.changes() + READ_FOR_OLDEST);
        if let Some(last) = self.changes().checked_sub(1) {
            self.focus = Control::Change(last);
        }
    }

    /// Reads ahead while more commits may follow, until the page after the
    /// one in view is read too, so that Next page knows whether there is
    /// one and the focused commit always has a next to move to.
    fn read_ahead(&mut self, rows: usize) {
        self.read_until(self.top + 2 * rows);
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
    /// `area_rows` rows.
    fn parts(&self, area: Rect, area_rows: usize) -> Parts {
        let status_rows = match self.source {
            Source::Showcase { .. } => 0,
            Source::Repository { .. } => 1,
        };
        layout(area, area_rows, self.change_rows(), status_rows)
    }

    /// How many changes a page holds with the picker drawn in `area`, as
    /// [`View::render`] lays it out.
    fn page(&self, area: Rect) -> usize {
        // Styles never change how many rows the areas take.
        let area_items = self.area_items(&Theme::new(ColorSupport::NoColor), area.width);
        page_of(self.parts(area, height(&area_items)).change_list)
    }

    /// Where the change list is, at `top` and `rows` rows: "Commits 41–60
    /// of 1,000", with a "+" after the total while more commits may follow.
    fn position(&self, top: usize, rows: usize) -> Option<String> {
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
        let more = if self.has_more() { "+" } else { "" };
        Some(format!("{shown} of {}{more}", grouped(changes)))
    }

    /// The row under the change list, `width` cells wide, while the list
    /// shows `rows` rows: Previous page, where the list is, and Next page.
    /// Previous page is disabled on the first page, and Next page on the
    /// last. Where the list is is cut to fit between them.
    fn paging_row(&self, theme: &Theme, rows: usize, width: u16) -> Line<'static> {
        let top = self.top(rows);
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
        // As far from Previous page as Next page's label is from it.
        let before = format!("{}{UNAVAILABLE_INDENT}", " ".repeat(columns::GAP));
        let room = usize::from(width)
            .saturating_sub(row.width() + before.len() + columns::GAP + next.width());
        let position = self.position(top, rows).unwrap_or_default();
        row.push_span(before);
        row.push_span(Span::styled(
            columns::truncate(&position, room),
            theme.muted,
        ));
        row.push_span(" ".repeat(columns::GAP));
        row.extend(next.spans);
        row
    }

    /// Applies `command` while the change list shows `rows` rows, and
    /// returns what it asks of the app, if anything.
    fn run(&mut self, command: Command, rows: usize) -> Option<Command> {
        if self.search_box.is_some() {
            if let Some(query) = search::edit(&mut self.search_box, command) {
                self.search(query);
            }
            return None;
        }
        match command {
            Command::Search => {
                self.search_box = Some(TextEntry::default());
                return None;
            }
            Command::NextMatch | Command::PreviousMatch => {
                self.go_to_match(command);
                return None;
            }
            Command::PageDown => self.scroll_down(rows, rows),
            Command::PageUp => self.scroll_up(rows, rows),
            Command::HalfPageDown => self.scroll_down((rows / 2).max(1), rows),
            Command::HalfPageUp => self.scroll_up((rows / 2).max(1), rows),
            Command::Top if self.changes() > 0 => self.focus = Control::Change(0),
            Command::Bottom => self.go_to_oldest(),
            _ => self.focus = self.moved(command, rows),
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
            Control::Proceed => self.next_screen(),
        }
    }
}

/// A commit's row `width` cells wide: a choice in `state`, showing its
/// short ID and summary, then what it is compared with if that is not
/// simply its parent. A commit that cannot be opened is disabled, saying
/// why. The summary is cut to fit.
fn commit_row<'a>(theme: &Theme, commit: &Commit, state: ControlState<'a>, width: u16) -> Line<'a> {
    let (state, label) = match commit.availability() {
        Availability::Compared(comparison) => (state, comparison_label(comparison)),
        Availability::ParentMissing => (
            ControlState {
                disabled: Some(PARENT_MISSING),
                ..state
            },
            None,
        ),
    };
    let label_width = label.map_or(0, |label| columns::GAP + Line::raw(label).width());
    // Past the focus and choice markers, and why it is disabled.
    let room = usize::from(width)
        .saturating_sub(Theme::control_width("", state.kind))
        .saturating_sub(
            state
                .disabled
                .map_or(0, |reason| Line::raw(format!(" ({reason})")).width()),
        )
        .saturating_sub(label_width);
    let text = format!("{}  {}", commit.id().short(), commit.summary());
    let mut line = theme.control(columns::truncate(&text, room), state);
    if let Some(label) = label {
        line.push_span(" ".repeat(columns::GAP));
        line.push_span(Span::styled(label, theme.muted));
    }
    line
}

/// Whether `query` matches `commit`: its whole message, or the start of its
/// ID.
fn is_match(query: &Query, commit: &Commit) -> bool {
    query.is_in(commit.message())
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

/// Why a commit whose parent is missing, as at a shallow clone's boundary,
/// cannot be chosen.
const PARENT_MISSING: &str = "parent not in this clone";

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
fn page_of(change_list: Rect) -> usize {
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
            (_, Some(_)) => keymap::SEARCH_BOX,
            (Source::Showcase { .. }, None) => KEYMAP,
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
        }
    }

    fn actions(&self) -> &'static [Action] {
        match self.source {
            Source::Showcase { .. } => ACTIONS,
            Source::Repository { .. } => REPOSITORY_ACTIONS,
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

    fn apply(&mut self, command: Command, area: Rect) -> Option<Command> {
        let rows = self.page(area);
        let next = self.run(command, rows);
        // The change list stays scrolled to show where focus went, and the
        // page after it is read.
        self.top = self.top(rows);
        self.read_ahead(rows);
        next
    }

    fn render(&self, context: &Context<'_>, frame: &mut Frame<'_>, area: Rect) {
        let theme = context.theme;
        let area_items = self.area_items(theme, area.width);
        let parts = self.parts(area, height(&area_items));
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
            Control::Change(_) | Control::PreviousPage | Control::NextPage | Control::Proceed => {
                None
            }
        };
        render_list(frame, parts.area_list, area_items, shown_area);
        frame.render_widget(
            Paragraph::new(Line::styled(CHANGE_HEADING, theme.muted)),
            parts.change_heading,
        );
        // The change list stays where it was scrolled to, as far as the
        // focused change allows.
        let rows = page_of(parts.change_list);
        let top = self.top(rows);
        render_list(
            frame,
            parts.change_list,
            self.change_items(context, area.width, top..top + rows),
            None,
        );
        frame.render_widget(
            self.paging_row(theme, rows, parts.paging.width),
            parts.paging,
        );
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
        } else if let Some(status) = self.search_status() {
            frame.render_widget(Paragraph::new(status), parts.status);
        }
    }
}
