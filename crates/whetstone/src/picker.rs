//! The area and change picker, where Practice starts: the learner chooses
//! the areas questions focus on and one change, from the showcase project's
//! curated history or from the commits of their own repository.

use ratatui::Frame;
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{List, ListState, Paragraph};
use whetstone_core::{Area, InvalidAreaFile, ShowcaseProject};

use crate::columns;
use crate::focus::Position;
use crate::keymap::{self, Binding, Command, Keymap, Press};
use crate::palette::Action;
use crate::repository::{Commit, HistoryError, Repository};
use crate::theme::{ControlKind, ControlState, Theme};
use crate::view::{ChangeIndex, Context, Project, Route, View};

/// The picker's own keys. Space makes a choice, and Enter also opens the
/// chosen change.
const KEYMAP: Keymap = &[
    keymap::MOVE,
    Binding {
        keys: &[(Press::char(' '), Command::Activate)],
        does: "Toggle an area, or choose a change",
        hint: Some("Space toggle"),
    },
    Binding {
        keys: &[(Press::key(KeyCode::Enter), Command::Open)],
        does: "Toggle an area, or choose and open a change",
        hint: Some("Enter open"),
    },
    keymap::BACK,
];

/// The picker's own action in the palette.
const ACTIONS: &[Action] = &[Action {
    label: "Back",
    does: "Return to the screen before",
    command: Command::Back,
}];

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
    Next,
}

/// Whether an area can be chosen, and whether it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AreaChoice {
    /// A valid area, selected or not.
    Valid { selected: bool },
    /// An invalid area file, which cannot be selected.
    Invalid,
}

/// What the picker practices on, and what it read of it on opening.
#[derive(Debug)]
enum Source {
    /// The showcase project's curated changes.
    Showcase,
    /// The learner's repository, with the first commits reachable from
    /// HEAD, newest first, or why they could not be read.
    Repository {
        commits: Result<Vec<Commit>, HistoryError>,
    },
}

/// The picker's state: which areas are selected, which change is chosen,
/// and which control has focus.
#[derive(Debug)]
pub(crate) struct Picker {
    source: Source,
    /// Whether each area can be chosen and is, in the order offered.
    areas: Vec<AreaChoice>,
    /// How many changes the picker lists.
    changes: usize,
    /// The index of the chosen change, once the learner chooses one.
    chosen: Option<usize>,
    focus: Position,
}

impl Picker {
    /// The picker over `showcase`, as it first opens: every area selected.
    pub(crate) fn new(showcase: &ShowcaseProject) -> Self {
        Self {
            source: Source::Showcase,
            areas: vec![AreaChoice::Valid { selected: true }; showcase.areas().len()],
            changes: showcase.changes().len(),
            chosen: None,
            focus: Position::default(),
        }
    }

    /// The picker over the learner's `repository`, as it first opens: every
    /// valid area selected, and its first commits read.
    pub(crate) fn for_repository(repository: &Repository) -> Self {
        let commits = repository.first_commits();
        Self {
            areas: repository
                .areas()
                .iter()
                .map(|area| match area {
                    Ok(_) => AreaChoice::Valid { selected: true },
                    Err(_) => AreaChoice::Invalid,
                })
                .collect(),
            changes: commits.as_ref().map_or(0, Vec::len),
            source: Source::Repository { commits },
            chosen: None,
            focus: Position::default(),
        }
    }

    /// How many controls the picker has.
    fn controls(&self) -> usize {
        self.areas.len() + self.changes + 1
    }

    /// The control at `index` in focus order.
    fn control(&self, index: usize) -> Control {
        if index < self.areas.len() {
            Control::Area(index)
        } else if index < self.areas.len() + self.changes {
            Control::Change(index - self.areas.len())
        } else {
            Control::Next
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
            Source::Showcase => context.showcase.areas().iter().map(Ok).collect(),
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
                    Err(invalid_file) => invalid(theme, invalid_file.file_name(), focused),
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

    /// The changes as list items `width` cells wide, each a choice.
    fn change_items<'a>(&'a self, context: &Context<'a>, width: u16) -> Vec<Text<'a>> {
        let state = |index| ControlState {
            focused: self.focused() == Control::Change(index),
            kind: ControlKind::Choice {
                selected: self.chosen == Some(index),
            },
            disabled: None,
        };
        match &self.source {
            Source::Showcase => showcase_rows(context, width, state)
                .into_iter()
                .map(Text::from)
                .collect(),
            Source::Repository {
                commits: Ok(commits),
            } => {
                // Past the focus and choice markers.
                let room = usize::from(width).saturating_sub(Theme::control_width(
                    "",
                    ControlKind::Choice { selected: false },
                ));
                commits
                    .iter()
                    .enumerate()
                    .map(|(index, commit)| {
                        let label = format!("{}  {}", commit.short_id(), commit.summary());
                        Text::from(
                            context
                                .theme
                                .control(columns::truncate(&label, room), state(index)),
                        )
                    })
                    .collect()
            }
            Source::Repository {
                commits: Err(error),
            } => {
                vec![Text::styled(
                    format!("{UNAVAILABLE_INDENT}{error}"),
                    context.theme.muted,
                )]
            }
        }
    }

    /// The control after the lists, and why it is disabled, if it is.
    fn next(&self) -> (&'static str, Option<&'static str>) {
        match self.source {
            Source::Showcase => ("Generate questions", Some(GENERATE_QUESTIONS_UNAVAILABLE)),
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

/// An area that cannot be chosen, labelled `label`.
fn invalid<'a>(theme: &Theme, label: &'a str, focused: bool) -> Line<'a> {
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
    next: Rect,
}

/// Where the picker's parts go in `area`, for lists of `area_rows` and
/// `change_rows` rows. The title, headings, lists and the control after them
/// get their rows first, and the gaps between them share what is left, so a
/// short terminal keeps every control. When even the lists do not fit, they
/// scroll, and the change list always keeps at least one row.
fn layout(area: Rect, area_rows: usize, change_rows: usize) -> Parts {
    const TITLE: u16 = 1;
    const HEADING: u16 = 1;
    const NEXT: u16 = 1;
    let rows = |count: usize| u16::try_from(count).unwrap_or(u16::MAX);
    let mut left = area.height.saturating_sub(TITLE + 2 * HEADING + NEXT);
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
    let before_next = take(1);
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
        next,
    ] = Layout::vertical([
        Constraint::Length(TITLE),
        Constraint::Length(after_title),
        Constraint::Length(HEADING),
        Constraint::Length(area_rows),
        Constraint::Length(before_changes),
        Constraint::Length(HEADING),
        Constraint::Length(change_rows),
        Constraint::Length(before_next),
        Constraint::Length(NEXT),
    ])
    .areas(area);
    Parts {
        title,
        area_heading,
        area_list,
        change_heading,
        change_list,
        next,
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
            Source::Showcase => Project::Showcase,
            Source::Repository { .. } => Project::Repository,
        })
    }

    fn name(&self) -> &'static str {
        "Practice"
    }

    fn keymap(&self) -> Keymap {
        KEYMAP
    }

    fn actions(&self) -> &'static [Action] {
        ACTIONS
    }

    fn apply(&mut self, command: Command, _area: Rect) -> Option<Command> {
        self.focus = self.focus.after(command, self.controls());
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
                self.chosen = Some(index);
                match self.source {
                    Source::Showcase => (command == Command::Open)
                        .then_some(Command::Go(Route::Viewer(ChangeIndex::new(index)))),
                    // Opening a commit in the viewer is not built yet, so
                    // Enter only chooses it.
                    Source::Repository { .. } => None,
                }
            }
            // "Generate questions" is disabled until Jev arrives, as it
            // says, and the outbound review is not built yet.
            Control::Next => None,
        }
    }

    fn render(&self, context: &Context<'_>, frame: &mut Frame<'_>, area: Rect) {
        let theme = context.theme;
        let area_items = self.area_items(context, area.width);
        let change_items = self.change_items(context, area.width);
        let parts = layout(area, height(&area_items), height(&change_items));
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
            Control::Change(_) | Control::Next => None,
        };
        render_list(frame, parts.area_list, area_items, shown_area);
        frame.render_widget(
            Paragraph::new(Line::styled(CHANGE_HEADING, theme.muted)),
            parts.change_heading,
        );
        // The focused change, or else the chosen one, stays in view.
        let shown_change = match self.focused() {
            Control::Change(index) => Some(index),
            Control::Area(_) | Control::Next => self.chosen,
        };
        render_list(frame, parts.change_list, change_items, shown_change);
        let (label, disabled) = self.next();
        frame.render_widget(
            theme.control(
                label,
                ControlState {
                    focused: self.focused() == Control::Next,
                    disabled,
                    ..ControlState::default()
                },
            ),
            parts.next,
        );
    }
}
