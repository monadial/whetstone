//! Reporting a question: the learner's claim that it may be a
//! hallucination, mistaken or ambiguous, with an optional note. It opens
//! from the question, from its feedback, and from a question the learner
//! can't answer, as a box over the screen, which takes the keys until it
//! closes. A report is kept in the session record. It is not a confirmed
//! defect, and it never changes an outcome.
//!
//! Report stays disabled, saying what's missing, until a kind is chosen.
//! While the note has focus, every key but the reserved ones is text (rule
//! 29), and a paste goes in on one line (rule 57). Tab, Shift+Tab and Enter
//! leave the note, and Esc closes the box, keeping nothing, as `q` does
//! anywhere else in it.

use ratatui::Frame;
use ratatui::crossterm::event::KeyCode;
use ratatui::text::Line;
use ratatui::widgets::List;

use crate::columns;
use crate::entry::TextEntry;
use crate::focus::Position;
use crate::keymap::{self, Binding, Command, Input, Keymap, Press};
use crate::overlay;
use crate::palette::Action;
use crate::session_record::{Report, ReportKind};
use crate::theme::{ControlKind, ControlState, Theme};

/// How many cells wide the box's text is, at most.
const WIDTH: usize = 70;

/// `q` and Esc close the box, keeping nothing.
const CANCEL: Binding = Binding {
    keys: &[
        (Press::key(KeyCode::Esc), Command::Cancel),
        (Press::char('q'), Command::Cancel),
    ],
    does: "Close the report, keeping nothing",
    hint: Some("Esc cancel"),
};

/// The keys while anything but the note has focus.
const KEYMAP: Keymap = &[
    keymap::MOVE,
    Binding {
        does: "Choose the focused kind, or activate the focused control",
        hint: Some("Enter choose"),
        ..keymap::ACTIVATE
    },
    CANCEL,
];

/// The keys while the note has focus: every other key types.
const NOTE_KEYMAP: Keymap = &[
    Binding {
        keys: &[
            (Press::key(KeyCode::Tab), Command::NextControl),
            (Press::key(KeyCode::Enter), Command::NextControl),
            (Press::key(KeyCode::BackTab), Command::PreviousControl),
        ],
        does: "Leave the note, for the next or previous control",
        hint: Some("Tab next"),
    },
    Binding {
        keys: &[(Press::key(KeyCode::Backspace), Command::Erase)],
        does: "Erase the last character",
        hint: None,
    },
    Binding {
        keys: &[(Press::key(KeyCode::Esc), Command::Cancel)],
        ..CANCEL
    },
];

/// The action in the palette while the box is open.
pub(crate) const ACTIONS: &[Action] = &[Action {
    label: "Cancel report",
    does: "Close the report, keeping nothing",
    command: Command::Cancel,
}];

/// What the box holds, in focus order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    /// A kind of report to choose.
    Kind(ReportKind),
    Note,
    /// Keeps the report.
    Report,
    Cancel,
}

impl Field {
    const ALL: [Self; 6] = [
        Self::Kind(ReportKind::Hallucination),
        Self::Kind(ReportKind::Mistake),
        Self::Kind(ReportKind::Ambiguity),
        Self::Note,
        Self::Report,
        Self::Cancel,
    ];
}

/// What the learner did in the box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Reporting {
    /// It stays open.
    Open,
    /// They closed it, keeping nothing.
    Cancelled,
    /// They reported the question, which closes it.
    Reported(Report),
}

/// A report being written.
#[derive(Debug, Default)]
pub(crate) struct ReportForm {
    kind: Option<ReportKind>,
    note: TextEntry,
    focus: Position,
}

impl ReportForm {
    fn focused(&self) -> Field {
        Field::ALL[self.focus.index()]
    }

    /// The keys, by what has focus.
    pub(crate) fn keymap(&self) -> Keymap {
        match self.focused() {
            Field::Note => NOTE_KEYMAP,
            Field::Kind(_) | Field::Report | Field::Cancel => KEYMAP,
        }
    }

    /// Text while the note has focus, commands otherwise.
    pub(crate) fn input(&self) -> Input {
        match self.focused() {
            Field::Note => Input::Text,
            Field::Kind(_) | Field::Report | Field::Cancel => Input::Commands,
        }
    }

    /// Inserts pasted `text` into the note, on one line, while it has focus.
    pub(crate) fn paste(&mut self, text: &str) {
        if self.focused() == Field::Note {
            self.note.paste(text);
        }
    }

    /// Why Report is disabled, if it is.
    fn report_disabled(&self) -> Option<&'static str> {
        self.kind.is_none().then_some("choose what may be wrong")
    }

    /// Applies `command`, and returns what the learner did.
    pub(crate) fn apply(&mut self, command: Command) -> Reporting {
        match (self.focused(), command) {
            (_, Command::Cancel) | (Field::Cancel, Command::Activate) => Reporting::Cancelled,
            (Field::Note, Command::Type(_) | Command::Erase) => {
                self.note.apply(command);
                Reporting::Open
            }
            (Field::Kind(kind), Command::Activate) => {
                self.kind = Some(kind);
                Reporting::Open
            }
            (Field::Report, Command::Activate) => match self.kind {
                Some(kind) => {
                    let note = self.note.text().trim();
                    Reporting::Reported(Report::new(
                        kind,
                        (!note.is_empty()).then(|| note.to_owned()),
                    ))
                }
                None => Reporting::Open,
            },
            (_, command) => {
                self.focus = self.focus.after(command, Field::ALL.len());
                Reporting::Open
            }
        }
    }

    /// Draws the box over the screen, in the middle of the frame, with
    /// `theme`.
    pub(crate) fn render(&self, theme: &Theme, frame: &mut Frame<'_>) {
        let focused = self.focused();
        let state = |field: Field| ControlState {
            focused: focused == field,
            ..ControlState::default()
        };
        let mut rows: Vec<Line<'_>> = vec![
            Line::raw("What may be wrong with this question?"),
            Line::default(),
        ];
        rows.extend(Field::ALL.into_iter().filter_map(|field| match field {
            Field::Kind(kind) => Some(theme.control(
                kind_label(kind),
                ControlState {
                    kind: ControlKind::Choice {
                        selected: self.kind == Some(kind),
                    },
                    ..state(field)
                },
            )),
            Field::Note | Field::Report | Field::Cancel => None,
        }));
        rows.push(Line::default());
        let mut note = theme.control("Note (optional): ", state(Field::Note));
        let room = WIDTH.saturating_sub(note.width());
        let mut typed = self.note.line(theme, "", room).spans;
        if focused != Field::Note {
            // The cursor shows only while the note has focus.
            typed.pop();
        }
        note.spans.extend(typed);
        rows.push(note);
        rows.push(Line::default());
        rows.push(theme.control(
            "Report",
            ControlState {
                disabled: self.report_disabled(),
                ..state(Field::Report)
            },
        ));
        rows.push(theme.control("Cancel", state(Field::Cancel)));
        rows.push(Line::default());
        rows.extend(
            columns::wrap(
                "A report is kept in your session record. It never changes an outcome.",
                WIDTH,
            )
            .into_iter()
            .map(|line| Line::styled(line, theme.muted)),
        );
        let width = rows.iter().map(Line::width).max().unwrap_or_default();
        let (outer, body) = overlay::boxed(frame.area(), width.min(WIDTH), rows.len());
        overlay::draw_box(theme, frame, outer, "Report this question", self.keymap());
        frame.render_widget(List::new(rows), body);
    }
}

/// What a report of `kind` claims, as the box offers it.
fn kind_label(kind: ReportKind) -> &'static str {
    match kind {
        ReportKind::Hallucination => "Hallucination: it states something the evidence doesn't hold",
        ReportKind::Mistake => "Mistake: it is wrong",
        ReportKind::Ambiguity => "Ambiguity: it can be read more than one way",
    }
}
