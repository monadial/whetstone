//! Asking before a draft would be lost: Back from a screen that holds one,
//! or going home or quitting while one is open. Drafts stay only in memory,
//! so the learner keeps answering, or leaves and loses the draft. Ctrl+C is
//! reserved, and quits without asking (rule 28).

use ratatui::Frame;
use ratatui::crossterm::event::KeyCode;
use ratatui::text::Line;
use ratatui::widgets::List;

use crate::columns;
use crate::focus::Position;
use crate::keymap::{self, Binding, Command, Keymap, Press};
use crate::overlay::{self, Outcome};
use crate::theme::{ControlState, Theme};
use crate::view::Route;

/// What the question says, wrapped to this many cells.
const WIDTH: usize = 52;

/// Asking whether to leave, losing a draft, with what leaving would run.
#[derive(Debug, Clone)]
pub(crate) struct Leave {
    /// What runs if the learner leaves: Back, going home or quitting.
    then: Command,
    selected: Position,
}

/// The two answers, in focus order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Answer {
    Stay,
    Leave,
}

impl Answer {
    const ALL: [Self; 2] = [Self::Stay, Self::Leave];
}

impl Leave {
    pub(crate) const KEYMAP: Keymap = &[
        keymap::MOVE,
        keymap::FOCUS,
        Binding {
            does: "Answer",
            hint: Some("Enter answer"),
            ..keymap::ACTIVATE
        },
        Binding {
            keys: &[
                (Press::key(KeyCode::Esc), Command::Back),
                (Press::char('q'), Command::Back),
            ],
            does: "Keep the draft, and stay",
            hint: Some("Esc stay"),
        },
    ];

    /// Asks whether to run `then`, which would lose a draft. Focus starts
    /// on staying.
    pub(crate) fn new(then: Command) -> Self {
        Self {
            then,
            selected: Position::default(),
        }
    }

    pub(crate) fn apply(&mut self, command: Command) -> Outcome {
        self.selected = self.selected.after(command, Answer::ALL.len());
        match command {
            Command::Activate => match Answer::ALL[self.selected.index()] {
                Answer::Stay => Outcome::Close,
                Answer::Leave => Outcome::Leave(self.then),
            },
            Command::Back => Outcome::Close,
            _ => Outcome::Stay,
        }
    }

    /// What leaving does, as its answer says it.
    fn leaving(&self) -> &'static str {
        match self.then {
            Command::Quit => "Quit, and lose the draft",
            Command::Go(Route::Home) => "Go home, and lose the draft",
            _ => "Go back, and lose the draft",
        }
    }

    pub(crate) fn render(&self, theme: &Theme, frame: &mut Frame<'_>) {
        let mut rows: Vec<Line<'_>> = columns::wrap(
            "Your draft response hasn't been submitted, and drafts are kept only while \
             Practice is open.",
            WIDTH,
        )
        .into_iter()
        .map(Line::raw)
        .collect();
        rows.push(Line::default());
        rows.extend(Answer::ALL.map(|answer| {
            theme.control(
                match answer {
                    Answer::Stay => "Keep answering",
                    Answer::Leave => self.leaving(),
                },
                ControlState {
                    focused: Answer::ALL[self.selected.index()] == answer,
                    ..ControlState::default()
                },
            )
        }));
        let width = rows.iter().map(Line::width).max().unwrap_or_default();
        let (outer, body) = overlay::boxed(frame.area(), width, rows.len());
        overlay::draw_box(theme, frame, outer, "Lose your draft?", Self::KEYMAP);
        frame.render_widget(List::new(rows), body);
    }
}
