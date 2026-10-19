//! The area and change picker, where Practice on the showcase project
//! starts: the learner chooses the areas questions focus on and one change
//! from the project's curated history.

use ratatui::Frame;
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListState, Paragraph};
use whetstone_core::{Area, Change, ShowcaseProject};

use crate::columns;
use crate::focus::Position;
use crate::keymap::{self, Binding, Command, Keymap, Press};
use crate::palette::Action;
use crate::theme::{ControlKind, ControlState};
use crate::view::{ChangeIndex, Context, Route, View};

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
    /// The showcase project's area at this index.
    Area(usize),
    /// The curated change at this index.
    Change(usize),
    /// Generates questions about the chosen change, once Jev arrives.
    GenerateQuestions,
}

/// The picker's state: which areas are selected, which change is chosen,
/// and which control has focus.
#[derive(Debug, Clone)]
pub(crate) struct Picker {
    /// Whether each of the showcase project's areas is selected, in the
    /// project's order.
    areas: Vec<bool>,
    /// How many changes the showcase project has.
    changes: usize,
    /// The index of the chosen change, once the learner chooses one.
    chosen: Option<usize>,
    focus: Position,
}

impl Picker {
    /// The picker over `showcase`, as it first opens: every area selected.
    pub(crate) fn new(showcase: &ShowcaseProject) -> Self {
        Self {
            areas: vec![true; showcase.areas().len()],
            changes: showcase.changes().len(),
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
            Control::GenerateQuestions
        }
    }

    fn focused(&self) -> Control {
        self.control(self.focus.index())
    }

    /// The areas, each a choice.
    fn area_rows<'a>(&self, context: &Context<'a>) -> Vec<Line<'a>> {
        context
            .showcase
            .areas()
            .iter()
            .zip(&self.areas)
            .enumerate()
            .map(|(index, (area, &selected))| {
                context.theme.control(
                    area.name(),
                    ControlState {
                        focused: self.focused() == Control::Area(index),
                        kind: ControlKind::Choice { selected },
                        disabled: None,
                    },
                )
            })
            .collect()
    }

    /// The changes, each a choice showing its summary, with the areas it
    /// touches right-aligned in a column. The column ends where the widest
    /// row does, as far as `width` allows.
    fn change_rows<'a>(&self, context: &Context<'a>, width: u16) -> Vec<Line<'a>> {
        let rows: Vec<(Line<'a>, String)> = context
            .showcase
            .changes()
            .iter()
            .enumerate()
            .map(|(index, change)| {
                let line = context.theme.control(
                    change.summary(),
                    ControlState {
                        focused: self.focused() == Control::Change(index),
                        kind: ControlKind::Choice {
                            selected: self.chosen == Some(index),
                        },
                        disabled: None,
                    },
                );
                (line, areas_of(change))
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
}

/// The heading above the areas.
const AREA_HEADING: &str = "Areas the questions focus on";
/// The heading above the changes.
const CHANGE_HEADING: &str = "Change to practice on";

/// Why "Generate questions" cannot be used yet.
const GENERATE_QUESTIONS_UNAVAILABLE: &str = "arrives with Jev";

/// The cells between a change's summary and its areas, at least.
const COLUMN_GAP: usize = 2;

/// The areas `change` touches, as the change list shows them.
fn areas_of(change: &Change) -> String {
    change
        .areas()
        .iter()
        .map(Area::name)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Where the picker's parts go, top to bottom.
#[derive(Debug)]
struct Parts {
    title: Rect,
    area_heading: Rect,
    area_list: Rect,
    change_heading: Rect,
    change_list: Rect,
    generate_questions: Rect,
}

/// Where the picker's parts go in `area`, for lists of `area_count` areas
/// and `change_count` changes. The title, headings, lists and "Generate
/// questions" get their rows first, and the gaps between them share what is
/// left, so a short terminal keeps every control. When even the lists do not
/// fit, they scroll, and the change list always keeps at least one row.
fn layout(area: Rect, area_count: usize, change_count: usize) -> Parts {
    const TITLE: u16 = 1;
    const HEADING: u16 = 1;
    const GENERATE_QUESTIONS: u16 = 1;
    let rows = |count: usize| u16::try_from(count).unwrap_or(u16::MAX);
    let mut left = area
        .height
        .saturating_sub(TITLE + 2 * HEADING + GENERATE_QUESTIONS);
    let area_rows = rows(area_count).min(left.saturating_sub(rows(change_count.min(1))));
    left -= area_rows;
    let change_rows = rows(change_count).min(left);
    left -= change_rows;
    let mut take = |wanted: u16| {
        let taken = wanted.min(left);
        left -= taken;
        taken
    };
    let before_changes = take(1);
    let before_generate_questions = take(1);
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
        generate_questions,
    ] = Layout::vertical([
        Constraint::Length(TITLE),
        Constraint::Length(after_title),
        Constraint::Length(HEADING),
        Constraint::Length(area_rows),
        Constraint::Length(before_changes),
        Constraint::Length(HEADING),
        Constraint::Length(change_rows),
        Constraint::Length(before_generate_questions),
        Constraint::Length(GENERATE_QUESTIONS),
    ])
    .areas(area);
    Parts {
        title,
        area_heading,
        area_list,
        change_heading,
        change_list,
        generate_questions,
    }
}

/// `rows` as a list in `area`, scrolled so that the row at `shown`, if any,
/// is in view.
fn render_list(frame: &mut Frame<'_>, area: Rect, rows: Vec<Line<'_>>, shown: Option<usize>) {
    frame.render_stateful_widget(
        List::new(rows),
        area,
        &mut ListState::default().with_selected(shown),
    );
}

impl View for Picker {
    fn route(&self) -> Route {
        Route::Picker
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
                if let Some(selected) = self.areas.get_mut(index) {
                    *selected = !*selected;
                }
                None
            }
            Control::Change(index) => {
                self.chosen = Some(index);
                (command == Command::Open)
                    .then_some(Command::Go(Route::Viewer(ChangeIndex::new(index))))
            }
            // Disabled until Jev arrives, as it says.
            Control::GenerateQuestions => None,
        }
    }

    fn render(&self, context: &Context<'_>, frame: &mut Frame<'_>, area: Rect) {
        let theme = context.theme;
        let parts = layout(area, self.areas.len(), self.changes);
        frame.render_widget(
            Paragraph::new(Span::styled(
                "Practice on the showcase project",
                theme.title,
            )),
            parts.title,
        );
        // Practice needs one area or more.
        let area_heading = if self.areas.contains(&true) {
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
            Control::Change(_) | Control::GenerateQuestions => None,
        };
        render_list(frame, parts.area_list, self.area_rows(context), shown_area);
        frame.render_widget(
            Paragraph::new(Line::styled(CHANGE_HEADING, theme.muted)),
            parts.change_heading,
        );
        // The focused change, or else the chosen one, stays in view.
        let shown_change = match self.focused() {
            Control::Change(index) => Some(index),
            Control::Area(_) | Control::GenerateQuestions => self.chosen,
        };
        render_list(
            frame,
            parts.change_list,
            self.change_rows(context, parts.change_list.width),
            shown_change,
        );
        frame.render_widget(
            theme.control(
                "Generate questions",
                ControlState {
                    focused: self.focused() == Control::GenerateQuestions,
                    disabled: Some(GENERATE_QUESTIONS_UNAVAILABLE),
                    ..ControlState::default()
                },
            ),
            parts.generate_questions,
        );
    }
}
