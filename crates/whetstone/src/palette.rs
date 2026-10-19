//! The palette: the current screen's actions, to run without knowing their
//! keys.

use ratatui::Frame;
use ratatui::crossterm::event::KeyCode;
use ratatui::text::{Line, Span};
use ratatui::widgets::List;

use crate::columns;
use crate::focus::Position;
use crate::keymap::{self, Binding, Command, Keymap, Press};
use crate::overlay::{self, Outcome};
use crate::theme::{ControlState, Theme};
use crate::view::{Route, View};

/// Something the palette can run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Action {
    pub(crate) label: &'static str,
    /// What it does, beside the label.
    pub(crate) does: &'static str,
    pub(crate) command: Command,
}

/// The actions of every screen, after the screen's own.
pub(crate) const EVERYWHERE: &[Action] = &[
    Action {
        label: "Home",
        does: "Go to the home screen",
        command: Command::Go(Route::Home),
    },
    Action {
        label: "Practice",
        does: "Choose areas and a change",
        command: Command::Go(Route::Picker),
    },
    Action {
        label: "Settings",
        does: "See what Practice will use",
        command: Command::Go(Route::Settings),
    },
    Action {
        label: "Help",
        does: "Show the keys for this screen",
        command: Command::KeyHelp,
    },
    Action {
        label: "Quit",
        does: "Leave Whetstone",
        command: Command::Quit,
    },
];

/// The palette over one screen: its actions, one of them selected.
#[derive(Debug, Clone)]
pub(crate) struct Palette {
    /// The screen's name, for the title.
    screen: &'static str,
    actions: Vec<Action>,
    selected: Position,
}

impl Palette {
    pub(crate) const KEYMAP: Keymap = &[
        keymap::MOVE,
        keymap::FOCUS,
        Binding {
            does: "Run the selected action",
            hint: Some("Enter run"),
            ..keymap::ACTIVATE
        },
        Binding {
            keys: &[
                (Press::key(KeyCode::Esc), Command::Back),
                (Press::char('q'), Command::Back),
                (Press::char(':'), Command::Back),
            ],
            does: "Close",
            hint: Some("Esc close"),
        },
    ];

    /// The palette over `screen`, listing its own actions and then the ones
    /// every screen has.
    pub(crate) fn new(screen: &dyn View) -> Self {
        Self {
            screen: screen.name(),
            actions: screen.actions().iter().chain(EVERYWHERE).copied().collect(),
            selected: Position::default(),
        }
    }

    pub(crate) fn apply(&mut self, command: Command) -> Outcome {
        self.selected = self.selected.after(command, self.actions.len());
        match command {
            Command::Activate => {
                return self
                    .actions
                    .get(self.selected.index())
                    .map_or(Outcome::Close, |action| Outcome::Run(action.command));
            }
            Command::Back => return Outcome::Close,
            _ => {}
        }
        Outcome::Stay
    }

    pub(crate) fn render(&self, theme: &Theme, frame: &mut Frame<'_>) {
        // Past the focus marker, and the widest label.
        let does_column = 2 + columns::next_column(self.actions.iter().map(|action| action.label));
        let rows: Vec<Line<'_>> = self
            .actions
            .iter()
            .enumerate()
            .map(|(index, action)| {
                let mut line = theme.control(
                    action.label,
                    ControlState {
                        focused: index == self.selected.index(),
                        ..ControlState::default()
                    },
                );
                columns::pad_to(&mut line, does_column);
                line.push_span(Span::styled(action.does, theme.muted));
                line
            })
            .collect();
        let width = rows.iter().map(Line::width).max().unwrap_or_default();
        let (outer, body) = overlay::boxed(frame.area(), width, rows.len());
        overlay::draw_box(
            theme,
            frame,
            outer,
            &format!("Actions on {}", self.screen),
            Self::KEYMAP,
        );
        frame.render_widget(List::new(rows), body);
    }
}
