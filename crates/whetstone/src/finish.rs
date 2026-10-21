//! Asking before Finish ends a Practice while questions are still
//! unattempted, so the learner doesn't end it by accident. It is a box over
//! the questions, which takes the keys until it closes. Focus starts on
//! Keep answering, and `q` and Esc close it, as Keep answering does.

use ratatui::Frame;
use ratatui::crossterm::event::KeyCode;
use ratatui::text::Line;
use ratatui::widgets::List;

use crate::columns;
use crate::focus::Position;
use crate::keymap::{self, Binding, Command, Keymap, Press};
use crate::overlay;
use crate::palette::Action;
use crate::theme::{ControlState, Theme};

/// How many cells wide the box's text is, at most.
const WIDTH: usize = 60;

/// The box's title.
const TITLE: &str = "Finish this Practice?";

/// Its keys.
pub(crate) const KEYMAP: Keymap = &[
    keymap::MOVE,
    Binding {
        does: "Answer",
        hint: Some("Enter answer"),
        ..keymap::ACTIVATE
    },
    Binding {
        keys: &[
            (Press::key(KeyCode::Esc), Command::Cancel),
            (Press::char('q'), Command::Cancel),
        ],
        does: "Keep answering",
        hint: Some("Esc keep answering"),
    },
];

/// The action in the palette while the box is open.
pub(crate) const ACTIONS: &[Action] = &[Action {
    label: "Keep answering",
    does: "Close this, and keep answering the questions",
    command: Command::Cancel,
}];

/// The two answers, in focus order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Answer {
    KeepAnswering,
    Finish,
}

impl Answer {
    const ALL: [Self; 2] = [Self::KeepAnswering, Self::Finish];

    fn label(self) -> &'static str {
        match self {
            Self::KeepAnswering => "Keep answering",
            Self::Finish => "Finish",
        }
    }
}

/// What the learner answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Finishing {
    /// The box stays open.
    Open,
    /// They keep answering, which closes it.
    KeepAnswering,
    /// They finish, which closes it.
    Finish,
}

/// Asking whether to finish.
#[derive(Debug)]
pub(crate) struct FinishCheck {
    /// What is still unattempted, as the box says it.
    says: String,
    focus: Position,
}

impl FinishCheck {
    /// Asks whether to finish while `unattempted` of the `questions` are
    /// still unattempted, saying so, and that a draft is lost, when `drafts`
    /// says one is.
    pub(crate) fn new(unattempted: usize, questions: usize, drafts: Drafts) -> Self {
        let (noun, verb, count) = match (questions, unattempted) {
            (1, _) => ("question", "is", "it counts"),
            (_, 1) => ("questions", "is", "it counts"),
            _ => ("questions", "are", "they count"),
        };
        let mut says = format!(
            "{unattempted} of {questions} {noun} {verb} still unattempted. If you finish now, \
             {count} as skipped."
        );
        if drafts == Drafts::Started {
            says.push_str(" Any draft you started isn't kept.");
        }
        Self {
            says,
            focus: Position::default(),
        }
    }

    /// Applies `command`, and returns what the learner answered.
    pub(crate) fn apply(&mut self, command: Command) -> Finishing {
        match command {
            Command::Cancel => Finishing::KeepAnswering,
            Command::Activate => match Answer::ALL[self.focus.index()] {
                Answer::KeepAnswering => Finishing::KeepAnswering,
                Answer::Finish => Finishing::Finish,
            },
            command => {
                self.focus = self.focus.after(command, Answer::ALL.len());
                Finishing::Open
            }
        }
    }

    /// Draws the box over the screen, in the middle of the frame, with
    /// `theme`.
    pub(crate) fn render(&self, theme: &Theme, frame: &mut Frame<'_>) {
        let mut rows: Vec<Line<'_>> = columns::wrap(&self.says, WIDTH)
            .into_iter()
            .map(Line::raw)
            .collect();
        rows.push(Line::default());
        rows.extend(Answer::ALL.map(|answer| {
            theme.control(
                answer.label(),
                ControlState {
                    focused: Answer::ALL[self.focus.index()] == answer,
                    ..ControlState::default()
                },
            )
        }));
        let width = rows.iter().map(Line::width).max().unwrap_or_default();
        let (outer, body) = overlay::boxed(frame.area(), width, rows.len());
        overlay::draw_box(theme, frame, outer, TITLE, KEYMAP);
        frame.render_widget(List::new(rows), body);
    }
}

/// Whether the learner started a draft response that finishing loses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Drafts {
    /// None was started.
    None,
    /// At least one was.
    Started,
}
