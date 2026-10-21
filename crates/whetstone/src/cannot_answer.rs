//! A question the learner said can't be answered from this context: the
//! evidence isn't enough to answer it. It needs no response, so it has no
//! outcome, and it counts as neither incorrect nor skipped: it isn't
//! unattempted, so it doesn't come round again as a skipped question does.
//! When Next question comes round to it, it says so, and offers Next
//! question, Inspect source, Report and Finish.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::{List, Paragraph};
use whetstone_core::Question;

use crate::columns;
use crate::feedback::{Place, Step};
use crate::focus::Position;
use crate::keymap::{self, Command, Keymap};
use crate::theme::{ControlState, Theme};

/// Its keys.
pub(crate) const KEYMAP: Keymap = &[keymap::MOVE, keymap::ACTIVATE, keymap::BACK];

/// Its controls, in focus order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    NextQuestion,
    InspectSource,
    Report,
    Finish,
}

impl Control {
    const ALL: [Self; 4] = [
        Self::NextQuestion,
        Self::InspectSource,
        Self::Report,
        Self::Finish,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::NextQuestion => "Next question",
            Self::InspectSource => "Inspect source",
            Self::Report => "Report",
            Self::Finish => "Finish",
        }
    }

    /// Why it is disabled at `place`, if it is.
    fn disabled(self, place: Place) -> Option<&'static str> {
        match (self, place) {
            (Self::NextQuestion, Place::Last) => Some("this is the last"),
            _ => None,
        }
    }

    /// Its place in [`Control::ALL`].
    fn position(self) -> Position {
        Position::at(
            Self::ALL
                .iter()
                .position(|&control| control == self)
                .unwrap_or_default(),
        )
    }
}

/// A question the learner can't answer from this context, as it shows.
#[derive(Debug)]
pub(crate) struct CannotAnswer {
    focus: Position,
}

impl CannotAnswer {
    /// The question at `place`, which the learner can't answer. Focus
    /// starts on Next question, or on Finish when there is none.
    pub(crate) fn new(place: Place) -> Self {
        Self {
            focus: match place {
                Place::BeforeTheLast => Control::NextQuestion,
                Place::Last => Control::Finish,
            }
            .position(),
        }
    }

    /// Applies `command` at `place`, and returns what the learner does
    /// next, if they chose.
    pub(crate) fn apply(&mut self, command: Command, place: Place) -> Option<Step> {
        if command != Command::Activate {
            self.focus = self.focus.after(command, Control::ALL.len());
            return None;
        }
        let control = Control::ALL[self.focus.index()];
        if control.disabled(place).is_some() {
            return None;
        }
        Some(match control {
            Control::NextQuestion => Step::NextQuestion,
            Control::InspectSource => Step::InspectSource,
            Control::Report => Step::Report,
            Control::Finish => Step::Finish,
        })
    }

    /// Draws `question`, at `place`, in `area` with `theme`.
    pub(crate) fn render(
        &self,
        theme: &Theme,
        frame: &mut Frame<'_>,
        area: Rect,
        question: &Question,
        place: Place,
    ) {
        let width = usize::from(area.width);
        let wrapped = |text: &str| columns::wrap(text, width).into_iter().map(Line::raw);
        let mut lines: Vec<Line<'_>> = wrapped(question.stem()).collect();
        lines.push(Line::default());
        lines.push(Line::styled("Cannot answer from this context", theme.title));
        lines.extend(wrapped(
            "You said the evidence isn't enough to answer this question. It needs no \
             response, and counts as neither incorrect nor skipped.",
        ));
        let rows = u16::try_from(Control::ALL.len()).unwrap_or(u16::MAX);
        let [text, _, buttons] = Layout::vertical([
            Constraint::Max(u16::try_from(lines.len()).unwrap_or(u16::MAX)),
            Constraint::Length(1),
            Constraint::Length(rows),
        ])
        .areas(area);
        frame.render_widget(Paragraph::new(lines), text);
        let focused = Control::ALL[self.focus.index()];
        frame.render_widget(
            List::new(Control::ALL.map(|control| {
                theme.control(
                    control.label(),
                    ControlState {
                        focused: control == focused,
                        disabled: control.disabled(place),
                        ..ControlState::default()
                    },
                )
            })),
            buttons,
        );
    }
}
