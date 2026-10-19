//! The area and change picker, where Practice starts: the learner chooses
//! the areas questions focus on and one change, from the showcase project's
//! curated history or from the commits of their own repository.

use std::ops::Range;

use ratatui::Frame;
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{List, ListState, Paragraph};
use whetstone_core::{Area, InvalidAreaFile, ShowcaseProject};

use crate::columns;
use crate::entry::TextEntry;
use crate::focus::Position;
use crate::keymap::{self, Binding, Command, Input, Keymap, Press};
use crate::palette::Action;
use crate::repository::{Availability, Commit, History, HistoryEnd, HistoryError, Repository};
use crate::search::{self, Query};
use crate::theme::{ControlKind, ControlState, Theme};
use crate::view::{ChangeIndex, ChangeRef, Context, Project, Route, View, comparison_label};

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

/// The showcase picker's own keys. Space makes a choice, and Enter also
/// opens the chosen change.
const KEYMAP: Keymap = &[keymap::MOVE, SPACE, OPEN, keymap::BACK];

/// The repository picker's own keys: the showcase picker's, and `/` to
/// search the commits, with `n` and `N` for the matches.
const REPOSITORY_KEYMAP: Keymap = &[
    keymap::MOVE,
    SPACE,
    OPEN,
    Binding {
        does: "Search the commits by message or ID",
        ..keymap::SEARCH
    },
    keymap::MATCHES,
    keymap::BACK,
];

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
    /// What comes after choosing: "Generate questions" on the showcase
    /// project, once Jev arrives, and "Review what will be sent" on the
    /// learner's repository.
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
    /// The showcase project, with this many curated changes.
    Showcase { changes: usize },
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
    /// Whether each area can be chosen and is, in the order offered.
    areas: Vec<AreaChoice>,
    /// The index of the chosen change, once the learner chooses one.
    chosen: Option<usize>,
    focus: Position,
    /// The search box, while the learner types a search of the commits.
    search_box: Option<TextEntry>,
    /// What the last search of the commits looked for.
    query: Option<Query>,
}

impl Picker {
    /// The picker over `showcase`, as it first opens: every area selected.
    pub(crate) fn new(showcase: &ShowcaseProject) -> Self {
        Self {
            source: Source::Showcase {
                changes: showcase.changes().len(),
            },
            areas: vec![AreaChoice::Valid { selected: true }; showcase.areas().len()],
            chosen: None,
            focus: Position::default(),
            search_box: None,
            query: None,
        }
    }

    /// The picker over the learner's `repository`, as it first opens: every
    /// valid area selected, and its first commits read.
    pub(crate) fn for_repository(repository: &Repository) -> Self {
        Self {
            areas: repository
                .areas()
                .iter()
                .map(|area| match area {
                    Ok(_) => AreaChoice::Valid { selected: true },
                    Err(_) => AreaChoice::Invalid,
                })
                .collect(),
            source: Source::Repository {
                history: repository.history().map(Box::new),
            },
            chosen: None,
            focus: Position::default(),
            search_box: None,
            query: None,
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
            Source::Showcase { changes } => changes,
            Source::Repository { .. } => self.commits().len(),
        }
    }

    /// How many controls the picker has.
    fn controls(&self) -> usize {
        self.areas.len() + self.changes() + 1
    }

    /// The control at `index` in focus order.
    fn control(&self, index: usize) -> Control {
        if index < self.areas.len() {
            Control::Area(index)
        } else if index < self.areas.len() + self.changes() {
            Control::Change(index - self.areas.len())
        } else {
            Control::Proceed
        }
    }

    fn focused(&self) -> Control {
        self.control(self.focus.index())
    }

    /// Whether any area is selected: Practice needs one or more.
    fn has_area(&self) -> bool {
        self.areas.contains(&AreaChoice::Valid { selected: true })
    }

    /// The areas the picker offers, each valid or an invalid area file.
    fn offered<'a>(&self, context: &Context<'a>) -> Vec<Result<&'a Area, &'a InvalidAreaFile>> {
        match self.source {
            Source::Showcase { .. } => context.showcase.areas().iter().map(Ok).collect(),
            Source::Repository { .. } => context
                .repository
                .map(|repository| repository.areas().iter().map(Result::as_ref).collect())
                .unwrap_or_default(),
        }
    }

    /// The areas as list items `width` cells wide. Each is a choice showing
    /// its name, with its one-line description in a column after the
    /// widest name, cut to fit. An invalid area file shows its file name,
    /// disabled, and the reason it is invalid, wrapped in that column so it
    /// can be read whole.
    fn area_items<'a>(&self, context: &Context<'a>, width: u16) -> Vec<Text<'a>> {
        let theme = context.theme;
        let offered = self.offered(context);
        let rows: Vec<Line<'a>> = offered
            .iter()
            .zip(&self.areas)
            .enumerate()
            .map(|(index, (area, &choice))| {
                let focused = self.focused() == Control::Area(index);
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
            (rows.iter().map(Line::width).max().unwrap_or_default() + COLUMN_GAP).min(width / 2);
        rows.into_iter()
            .zip(offered)
            .map(|(mut line, area)| {
                let start = column.max(line.width() + COLUMN_GAP);
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
            Source::Showcase { changes } => *changes,
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
            focused: self.focused() == Control::Change(index),
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
        match self.focused() {
            Control::Change(index) => Some(index),
            Control::Area(_) | Control::Proceed => None,
        }
    }

    /// Moves focus to the change at `index`.
    fn focus_change(&mut self, index: usize) {
        self.focus = Position::at(self.areas.len() + index);
        self.read_ahead();
    }

    /// Reads more commits if the focused one is the last read, so the next
    /// is there to move to.
    fn read_ahead(&mut self) {
        let Control::Change(index) = self.focused() else {
            return;
        };
        if let Source::Repository {
            history: Ok(history),
        } = &mut self.source
            && index + 1 == history.commits().len()
        {
            history.read_page();
        }
    }

    /// The control after the lists, and why it is disabled, if it is.
    fn proceed(&self) -> (&'static str, Option<&'static str>) {
        match self.source {
            Source::Showcase { .. } => ("Generate questions", Some(GENERATE_QUESTIONS_UNAVAILABLE)),
            Source::Repository { .. } => (
                "Review what will be sent",
                match (self.chosen.is_some(), self.has_area()) {
                    (true, true) => None,
                    (true, false) => Some("choose an area"),
                    (false, true) => Some("choose a change"),
                    (false, false) => Some("choose a change and an area"),
                },
            ),
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
    let label_width = label.map_or(0, |label| COLUMN_GAP + Line::raw(label).width());
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
        line.push_span(" ".repeat(COLUMN_GAP));
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
        .map(|(line, areas)| line.width() + COLUMN_GAP + areas_width(areas))
        .max()
        .unwrap_or_default()
        .min(usize::from(width));
    rows.into_iter()
        .map(|(mut line, areas)| {
            let column = end
                .saturating_sub(areas_width(&areas))
                .max(line.width() + COLUMN_GAP);
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

/// Why a commit whose parent is missing, as at a shallow clone's boundary,
/// cannot be chosen.
const PARENT_MISSING: &str = "parent not in this clone";

/// Why "Generate questions" cannot be used yet.
const GENERATE_QUESTIONS_UNAVAILABLE: &str = "arrives with Jev";

/// The cells between an area's name and its description, and between a
/// change's summary and its areas, at least.
const COLUMN_GAP: usize = 2;

/// Where the picker's parts go, top to bottom.
#[derive(Debug)]
struct Parts {
    title: Rect,
    area_heading: Rect,
    area_list: Rect,
    change_heading: Rect,
    change_list: Rect,
    proceed: Rect,
    /// The row at the bottom, for the search box and what a search found.
    status: Rect,
}

/// Where the picker's parts go in `area`, for lists of `area_rows` and
/// `change_rows` rows, and `status_rows` at the bottom. The title,
/// headings, lists and the control after them get their rows first, and
/// the gaps between them share what is left, so a short terminal keeps
/// every control. When even the lists do not fit, they
/// scroll, and the change list always keeps at least one row.
fn layout(area: Rect, area_rows: usize, change_rows: usize, status_rows: u16) -> Parts {
    const TITLE: u16 = 1;
    const HEADING: u16 = 1;
    const PROCEED: u16 = 1;
    let [area, status] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(status_rows)]).areas(area);
    let rows = |count: usize| u16::try_from(count).unwrap_or(u16::MAX);
    let mut left = area.height.saturating_sub(TITLE + 2 * HEADING + PROCEED);
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
        proceed,
        status,
    }
}

/// How many rows `items` take.
fn height(items: &[Text<'_>]) -> usize {
    items.iter().map(Text::height).sum()
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

    fn actions(&self) -> &'static [Action] {
        match self.source {
            Source::Showcase { .. } => ACTIONS,
            Source::Repository { .. } => REPOSITORY_ACTIONS,
        }
    }

    fn apply(&mut self, command: Command, _area: Rect) -> Option<Command> {
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
            _ => {}
        }
        self.focus = self.focus.after(command, self.controls());
        self.read_ahead();
        if !matches!(command, Command::Activate | Command::Open) {
            return None;
        }
        match self.focused() {
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
                self.chosen = Some(index);
                (command == Command::Open).then_some(Command::Go(Route::Viewer(change)))
            }
            // "Generate questions" is disabled until Jev arrives, as it
            // says, and the outbound review is not built yet.
            Control::Proceed => None,
        }
    }

    fn render(&self, context: &Context<'_>, frame: &mut Frame<'_>, area: Rect) {
        let theme = context.theme;
        let area_items = self.area_items(context, area.width);
        let status_rows = match self.source {
            Source::Showcase { .. } => 0,
            Source::Repository { .. } => 1,
        };
        let parts = layout(area, height(&area_items), self.change_rows(), status_rows);
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
        } else {
            format!("{AREA_HEADING}: choose at least one")
        };
        frame.render_widget(
            Paragraph::new(Line::styled(area_heading, theme.muted)),
            parts.area_heading,
        );
        let shown_area = match self.focused() {
            Control::Area(index) => Some(index),
            Control::Change(_) | Control::Proceed => None,
        };
        render_list(frame, parts.area_list, area_items, shown_area);
        frame.render_widget(
            Paragraph::new(Line::styled(CHANGE_HEADING, theme.muted)),
            parts.change_heading,
        );
        // The focused change, or else the chosen one, stays in view.
        let shown_change = match self.focused() {
            Control::Change(index) => Some(index),
            Control::Area(_) | Control::Proceed => self.chosen,
        };
        // Scrolled as a list scrolls to show an item: to where it is the
        // last row in view.
        let rows = usize::from(parts.change_list.height);
        let offset = shown_change.map_or(0, |shown| (shown + 1).saturating_sub(rows));
        render_list(
            frame,
            parts.change_list,
            self.change_items(context, area.width, offset..offset + rows),
            shown_change.and_then(|shown| shown.checked_sub(offset)),
        );
        let (label, disabled) = self.proceed();
        frame.render_widget(
            theme.control(
                label,
                ControlState {
                    focused: self.focused() == Control::Proceed,
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
