//! Feedback on a submitted response: the choice outcome at once, from the
//! option ratings, with the chosen option's rationale, the answer key's
//! rationale, "why it matters" and the citations, then the reasoning, which
//! this version doesn't assess yet. The outcome is Supported or Not
//! supported, never Needs review, and in words only: Jev's probabilities
//! stay out of it.
//!
//! A submitted response is final, so feedback offers no way back to it. Its
//! controls are Next question, Inspect source and Finish.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, Paragraph};
use whetstone_core::{Assessed, ChoiceOutcome, Extent, OptionLetter, Question, Response};

use crate::columns;
use crate::focus::Position;
use crate::keymap::{self, Binding, Command, Keymap};
use crate::session_record::{Durability, SavedResponse};
use crate::theme::{ColorSupport, ControlState, Theme};

/// Feedback's own keys.
pub(crate) const KEYMAP: Keymap = &[
    keymap::MOVE,
    keymap::ACTIVATE,
    Binding {
        does: "Scroll the feedback down or up half a page",
        hint: Some("Ctrl+d/u scroll"),
        ..keymap::HALF_PAGE
    },
    Binding {
        does: "Scroll to the top or bottom of the feedback",
        ..keymap::ENDS
    },
    keymap::BACK,
];

/// Feedback's controls, in focus order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    NextQuestion,
    InspectSource,
    Finish,
}

impl Control {
    const ALL: [Self; 3] = [Self::NextQuestion, Self::InspectSource, Self::Finish];

    fn label(self) -> &'static str {
        match self {
            Self::NextQuestion => "Next question",
            Self::InspectSource => "Inspect source",
            Self::Finish => "Finish",
        }
    }
}

/// What the learner does next from feedback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// Answer the next question.
    NextQuestion,
    /// Read the evidence the question rests on.
    InspectSource,
    /// End the Practice.
    Finish,
}

/// Where the question is among the questions shown, which decides whether
/// there is a next one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Place {
    /// Another question follows it.
    BeforeTheLast,
    /// It is the last.
    Last,
}

/// Feedback on one submitted response.
#[derive(Debug)]
pub(crate) struct Feedback {
    response: Response,
    outcome: Assessed<ChoiceOutcome>,
    /// The store's acknowledgement of the response, which names its
    /// session, question and attempt.
    #[expect(
        dead_code,
        reason = "#91 asks Jev about the reasoning with it, and waits for the result by it"
    )]
    saved: SavedResponse,
    /// Whether the store that saved the response keeps it after Whetstone
    /// closes: only then does "Saved" show.
    durability: Durability,
    focus: Position,
    /// The row at the top of the feedback.
    top: Position,
}

impl Feedback {
    /// Feedback on `response`, saved as `saved` by a store of `durability`,
    /// whose choice's outcome is `outcome`, for a question at `place`. Focus
    /// starts on Next question, or on Finish after the last.
    pub(crate) fn new(
        response: Response,
        outcome: Assessed<ChoiceOutcome>,
        saved: SavedResponse,
        durability: Durability,
        place: Place,
    ) -> Self {
        let first = match place {
            Place::BeforeTheLast => Control::NextQuestion,
            Place::Last => Control::Finish,
        };
        Self {
            response,
            outcome,
            saved,
            durability,
            focus: Position::at(
                Control::ALL
                    .iter()
                    .position(|&control| control == first)
                    .unwrap_or_default(),
            ),
            top: Position::default(),
        }
    }

    fn focused(&self) -> Control {
        Control::ALL[self.focus.index()]
    }

    /// Why the control `control` is disabled at `place`, if it is.
    fn disabled(control: Control, place: Place) -> Option<&'static str> {
        match (control, place) {
            (Control::NextQuestion, Place::Last) => Some("this is the last"),
            _ => None,
        }
    }

    /// Applies `command` to the feedback on `question`, at `place`, drawn
    /// in `area`, and returns what the learner does next, if they chose.
    pub(crate) fn apply(
        &mut self,
        command: Command,
        area: Rect,
        question: &Question,
        place: Place,
    ) -> Option<Step> {
        match command {
            Command::Activate => {
                let control = self.focused();
                if Self::disabled(control, place).is_some() {
                    return None;
                }
                return Some(match control {
                    Control::NextQuestion => Step::NextQuestion,
                    Control::InspectSource => Step::InspectSource,
                    Control::Finish => Step::Finish,
                });
            }
            Command::HalfPageDown | Command::HalfPageUp | Command::Top | Command::Bottom => {
                // Styles do not change how the text wraps.
                let lines = self.lines(question, &Theme::new(ColorSupport::NoColor), area.width);
                let [body, ..] = layout(lines.len(), area);
                let last = lines.len().saturating_sub(usize::from(body.height));
                self.top = self.top.scrolled(command, last, usize::from(body.height));
            }
            command => self.focus = self.focus.after(command, Control::ALL.len()),
        }
        None
    }

    /// The feedback's text about `question`, drawn with `theme`, `width`
    /// cells wide.
    fn lines(&self, question: &Question, theme: &Theme, width: u16) -> Vec<Line<'static>> {
        let width = usize::from(width);
        let mut lines = Vec::new();
        let wrapped = |lines: &mut Vec<Line<'static>>, text: &str| {
            lines.extend(columns::wrap(text, width).into_iter().map(Line::raw));
        };
        wrapped(&mut lines, question.stem());
        lines.push(Line::default());
        let chosen = self.response.choice();
        let answer_key = question.answer_key();
        lines.push(Line::from(vec![
            Span::styled("Choice · ", theme.title),
            Span::styled(
                match self.outcome.outcome() {
                    ChoiceOutcome::Supported => "Supported",
                    ChoiceOutcome::NotSupported => "Not supported",
                },
                theme.title,
            ),
        ]));
        wrapped(
            &mut lines,
            &match self.outcome.outcome() {
                ChoiceOutcome::Supported => {
                    format!("Jev supports {chosen}, the option you chose.")
                }
                ChoiceOutcome::NotSupported => format!(
                    "Jev doesn't support {chosen}, the option you chose. It supports \
                     {answer_key}, the answer key."
                ),
            },
        );
        lines.push(Line::default());
        let option = |lines: &mut Vec<Line<'static>>, lead: &str, letter: OptionLetter| {
            lines.push(Line::styled(
                format!("{lead} {letter}: {}", question.option(letter)),
                theme.title,
            ));
            wrapped(lines, question.rationale(letter));
        };
        option(&mut lines, "You chose", chosen);
        if chosen != answer_key {
            lines.push(Line::default());
            option(&mut lines, "The answer key is", answer_key);
        }
        lines.push(Line::default());
        lines.push(Line::styled("Why it matters", theme.title));
        wrapped(&mut lines, question.why_it_matters());
        lines.push(Line::default());
        lines.push(Line::styled("Cited", theme.title));
        for citation in question.citations() {
            wrapped(
                &mut lines,
                &format!("{}, {}", citation.path(), Extent::Lines(citation.lines())),
            );
        }
        lines.push(Line::default());
        lines.push(Line::styled("Your reasoning", theme.title));
        for line in self.response.reasoning().lines() {
            if line.trim().is_empty() {
                lines.push(Line::default());
            } else {
                wrapped(&mut lines, line);
            }
        }
        lines.push(Line::styled(
            "Your reasoning isn't assessed yet in this version.",
            theme.muted,
        ));
        if self.durability == Durability::Durable {
            lines.push(Line::default());
            lines.push(Line::styled("Saved", theme.muted));
        }
        lines
    }

    /// Draws the feedback on `question`, at `place`, in `area` with `theme`.
    pub(crate) fn render(
        &self,
        theme: &Theme,
        frame: &mut Frame<'_>,
        area: Rect,
        question: &Question,
        place: Place,
    ) {
        let lines = self.lines(question, theme, area.width);
        let [body, _, controls] = layout(lines.len(), area);
        let last = lines.len().saturating_sub(usize::from(body.height));
        let top = self.top.index().min(last);
        frame.render_widget(
            Paragraph::new(lines).scroll((u16::try_from(top).unwrap_or(u16::MAX), 0)),
            body,
        );
        frame.render_widget(
            List::new(Control::ALL.map(|control| {
                theme.control(
                    control.label(),
                    ControlState {
                        focused: self.focused() == control,
                        disabled: Self::disabled(control, place),
                        ..ControlState::default()
                    },
                )
            })),
            controls,
        );
    }
}

/// Where the feedback's text of `lines` rows, a blank row and its controls
/// go in `area`. The controls keep their rows on a short screen, and the
/// text scrolls above them.
fn layout(lines: usize, area: Rect) -> [Rect; 3] {
    let rows = u16::try_from(Control::ALL.len()).unwrap_or(u16::MAX);
    Layout::vertical([
        Constraint::Max(u16::try_from(lines).unwrap_or(u16::MAX)),
        Constraint::Length(1),
        Constraint::Length(rows),
    ])
    .areas(area)
}
