//! The questions screen, read-only for now: one question at a time, with
//! its place among them ("Question 2 of 4"), its area, its stem and options
//! A, B and C, and the next and previous question. The answer key, the
//! rationales, "why it matters", the rubric and the hint stay hidden.
//!
//! When fewer questions passed their checks than were asked for, "*n*
//! passed checks, *N* asked for" heads them, so the learner knows nothing
//! unchecked filled the gap.

use ratatui::Frame;
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, Paragraph};
use whetstone_core::{Area, OptionLetter, Picked, Question, QuestionCount};

use crate::columns;
use crate::focus::Position;
use crate::keymap::{self, Binding, Command, Keymap, Press};
use crate::theme::{ControlState, Theme};

/// The questions screen's own keys.
pub(crate) const KEYMAP: Keymap = &[
    keymap::MOVE,
    keymap::ACTIVATE,
    Binding {
        keys: &[
            (Press::char('h'), Command::Left),
            (Press::key(KeyCode::Left), Command::Left),
            (Press::char('l'), Command::Right),
            (Press::key(KeyCode::Right), Command::Right),
        ],
        does: "Previous or next question",
        hint: Some("h/l question"),
    },
    keymap::BACK,
];

/// The questions screen's controls, in focus order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    Next,
    Previous,
    Back,
}

impl Control {
    const ALL: [Self; 3] = [Self::Next, Self::Previous, Self::Back];
}

/// The questions that passed their checks, picked to show, and which one
/// shows.
#[derive(Debug)]
pub(crate) struct Questions {
    /// Each question, in the order to show them, with its area's name.
    picked: Vec<(Question, String)>,
    /// How many were asked for, when fewer passed.
    shortfall: Option<QuestionCount>,
    /// The place of the question that shows.
    at: usize,
    focus: Position,
}

impl Questions {
    /// The questions `picked`, about the chosen `areas`, showing the first,
    /// or `None` when none passed their checks.
    pub(crate) fn new(picked: Picked, areas: &[Area]) -> Option<Self> {
        let (questions, shortfall) = match picked {
            Picked::All(questions) => (questions, None),
            Picked::Shortfall {
                questions,
                asked_for,
            } => (questions, Some(asked_for)),
            Picked::NonePassed => return None,
        };
        let name = |question: &Question| {
            areas
                .iter()
                .find(|area| area.id() == question.area())
                .map_or_else(
                    || question.area().to_string(),
                    |area| area.name().to_owned(),
                )
        };
        Some(Self {
            picked: questions
                .into_iter()
                .map(|question| {
                    let name = name(&question);
                    (question, name)
                })
                .collect(),
            shortfall,
            at: 0,
            focus: Position::default(),
        })
    }

    fn focused(&self) -> Control {
        Control::ALL[self.focus.index()]
    }

    /// Why the control `control` is disabled, if it is.
    fn disabled(&self, control: Control) -> Option<&'static str> {
        match control {
            Control::Next if self.at + 1 == self.picked.len() => Some("this is the last"),
            Control::Previous if self.at == 0 => Some("this is the first"),
            Control::Next | Control::Previous | Control::Back => None,
        }
    }

    /// Shows the next question, if there is one.
    fn next(&mut self) {
        if self.at + 1 < self.picked.len() {
            self.at += 1;
        }
    }

    /// Shows the previous question, if there is one.
    fn previous(&mut self) {
        self.at = self.at.saturating_sub(1);
    }

    /// Applies `command`, and returns what it asks of the app, if anything.
    pub(crate) fn apply(&mut self, command: Command) -> Option<Command> {
        match command {
            Command::Right => self.next(),
            Command::Left => self.previous(),
            Command::Activate => match self.focused() {
                Control::Next => self.next(),
                Control::Previous => self.previous(),
                Control::Back => return Some(Command::Back),
            },
            command => self.focus = self.focus.after(command, Control::ALL.len()),
        }
        None
    }

    /// Draws the question that shows in `area` with `theme`.
    pub(crate) fn render(&self, theme: &Theme, frame: &mut Frame<'_>, area: Rect) {
        let Some((question, area_name)) = self.picked.get(self.at) else {
            return;
        };
        let width = usize::from(area.width);
        let mut lines: Vec<Line<'_>> = Vec::new();
        if let Some(asked_for) = self.shortfall {
            lines.push(Line::styled(
                format!("{} passed checks, {asked_for} asked for", self.picked.len()),
                theme.muted,
            ));
        }
        lines.push(Line::from(vec![
            Span::styled(
                format!("Question {} of {}", self.at + 1, self.picked.len()),
                theme.title,
            ),
            Span::styled(format!(" · {area_name}"), theme.muted),
        ]));
        lines.push(Line::default());
        lines.extend(
            columns::wrap(question.stem(), width)
                .into_iter()
                .map(Line::raw),
        );
        lines.push(Line::default());
        for letter in OptionLetter::ALL {
            let lead = format!("{letter}  ");
            let indent = " ".repeat(lead.len());
            for (index, text) in
                columns::wrap(question.option(letter), width.saturating_sub(lead.len()))
                    .into_iter()
                    .enumerate()
            {
                let lead = if index == 0 { &lead } else { &indent };
                lines.push(Line::raw(format!("{lead}{text}")));
            }
        }
        // The controls follow the question, and keep their rows on a short
        // screen.
        let rows = u16::try_from(Control::ALL.len()).unwrap_or(u16::MAX);
        let [body, _, controls] = Layout::vertical([
            Constraint::Max(u16::try_from(lines.len()).unwrap_or(u16::MAX)),
            Constraint::Length(1),
            Constraint::Length(rows),
        ])
        .areas(area);
        frame.render_widget(Paragraph::new(lines), body);
        frame.render_widget(
            List::new(Control::ALL.map(|control| {
                theme.control(
                    match control {
                        Control::Next => "Next question",
                        Control::Previous => "Previous question",
                        Control::Back => "Back",
                    },
                    ControlState {
                        focused: self.focused() == control,
                        disabled: self.disabled(control),
                        ..ControlState::default()
                    },
                )
            })),
            controls,
        );
    }
}
