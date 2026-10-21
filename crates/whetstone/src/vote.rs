//! Voting on one check that withheld a candidate: Rightly withheld or
//! Should have passed, with an optional note, or Skip. It opens from the
//! review of withheld questions as a box over the screen, which takes the
//! keys until it closes. A vote is saved to the learner's labels file (see
//! [`crate::labels_file`]). It never changes an outcome, and never brings a
//! question into Practice.
//!
//! Save vote stays disabled, saying what's missing, until a verdict is
//! chosen. While the note has focus, every key but the reserved ones is
//! text (rule 29), and a paste goes in on one line (rule 57). Tab,
//! Shift+Tab and Enter leave the note, and Esc closes the box, voting
//! nothing, as `q` does anywhere else in it.

use ratatui::Frame;
use ratatui::crossterm::event::KeyCode;
use ratatui::text::Line;
use ratatui::widgets::List;

use crate::columns;
use crate::entry::TextEntry;
use crate::focus::Position;
use crate::keymap::{self, Binding, Command, Input, Keymap, Press};
use crate::labels_file::{Note, Verdict};
use crate::overlay;
use crate::palette::Action;
use crate::theme::{ControlKind, ControlState, Theme};

/// How many cells wide the box's text is, at most.
const WIDTH: usize = 70;

/// `q` and Esc close the box, voting nothing.
const CANCEL: Binding = Binding {
    keys: &[
        (Press::key(KeyCode::Esc), Command::Cancel),
        (Press::char('q'), Command::Cancel),
    ],
    does: "Close the vote, voting nothing",
    hint: Some("Esc cancel"),
};

/// The keys while anything but the note has focus.
const KEYMAP: Keymap = &[
    keymap::MOVE,
    Binding {
        does: "Choose the focused verdict, or activate the focused control",
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
    label: "Cancel vote",
    does: "Close the vote, voting nothing",
    command: Command::Cancel,
}];

/// What the box holds, in focus order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    /// A verdict to choose.
    Verdict(Verdict),
    Note,
    /// Saves the vote.
    Save,
    /// Skips the check, voting nothing.
    Skip,
}

impl Field {
    const ALL: [Self; 5] = [
        Self::Verdict(Verdict::RightlyWithheld),
        Self::Verdict(Verdict::ShouldHavePassed),
        Self::Note,
        Self::Save,
        Self::Skip,
    ];
}

/// What the learner did in the box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Voting {
    /// It stays open.
    Open,
    /// They closed it, voting nothing.
    Cancelled,
    /// They skipped the check, voting nothing, which closes it.
    Skipped,
    /// They voted, with the note they wrote, if any, which closes it.
    Voted(Verdict, Option<Note>),
}

/// A vote being cast on one check: the check in plain words, and what
/// Jev's band was against what the show rule needed.
#[derive(Debug)]
pub(crate) struct VoteForm {
    /// The check's number among those that withheld the candidate, from 1.
    number: usize,
    /// What the check asks, in plain words.
    statement: String,
    /// Jev's band, against what the show rule needed, in plain words.
    banded: String,
    /// Where the vote is saved, as the review shows it.
    file: String,
    verdict: Option<Verdict>,
    note: TextEntry,
    focus: Position,
}

impl VoteForm {
    /// A vote on check `number`, which asks `statement`, banded as `banded`
    /// says, to save in `file`.
    pub(crate) fn new(number: usize, statement: String, banded: String, file: String) -> Self {
        Self {
            number,
            statement,
            banded,
            file,
            verdict: None,
            note: TextEntry::default(),
            focus: Position::default(),
        }
    }

    fn focused(&self) -> Field {
        Field::ALL[self.focus.index()]
    }

    /// The keys, by what has focus.
    pub(crate) fn keymap(&self) -> Keymap {
        match self.focused() {
            Field::Note => NOTE_KEYMAP,
            Field::Verdict(_) | Field::Save | Field::Skip => KEYMAP,
        }
    }

    /// Text while the note has focus, commands otherwise.
    pub(crate) fn input(&self) -> Input {
        match self.focused() {
            Field::Note => Input::Text,
            Field::Verdict(_) | Field::Save | Field::Skip => Input::Commands,
        }
    }

    /// Inserts pasted `text` into the note, on one line, while it has focus.
    pub(crate) fn paste(&mut self, text: &str) {
        if self.focused() == Field::Note {
            self.note.paste(text);
        }
    }

    /// Why Save vote is disabled, if it is.
    fn save_disabled(&self) -> Option<&'static str> {
        self.verdict
            .is_none()
            .then_some("choose Rightly withheld or Should have passed")
    }

    /// Applies `command`, and returns what the learner did.
    pub(crate) fn apply(&mut self, command: Command) -> Voting {
        match (self.focused(), command) {
            (_, Command::Cancel) => Voting::Cancelled,
            (Field::Skip, Command::Activate) => Voting::Skipped,
            (Field::Note, Command::Type(_) | Command::Erase) => {
                self.note.apply(command);
                Voting::Open
            }
            (Field::Verdict(verdict), Command::Activate) => {
                self.verdict = Some(verdict);
                Voting::Open
            }
            (Field::Save, Command::Activate) => match self.verdict {
                Some(verdict) => Voting::Voted(verdict, Note::new(self.note.text())),
                None => Voting::Open,
            },
            (_, command) => {
                self.focus = self.focus.after(command, Field::ALL.len());
                Voting::Open
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
        let mut rows: Vec<Line<'_>> = columns::wrap(&self.statement, WIDTH)
            .into_iter()
            .map(|line| Line::styled(line, theme.title))
            .collect();
        rows.extend(
            columns::wrap(&self.banded, WIDTH)
                .into_iter()
                .map(|line| Line::styled(line, theme.muted)),
        );
        rows.push(Line::default());
        rows.extend(Field::ALL.into_iter().filter_map(|field| match field {
            Field::Verdict(verdict) => Some(theme.control(
                verdict_label(verdict),
                ControlState {
                    kind: ControlKind::Choice {
                        selected: self.verdict == Some(verdict),
                    },
                    ..state(field)
                },
            )),
            Field::Note | Field::Save | Field::Skip => None,
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
            "Save vote",
            ControlState {
                disabled: self.save_disabled(),
                ..state(Field::Save)
            },
        ));
        rows.push(theme.control("Skip", state(Field::Skip)));
        rows.push(Line::default());
        rows.extend(
            columns::wrap(
                &format!(
                    "A vote is saved as a labelled example in {}, on this machine only, and is \
                     never sent anywhere. It never changes an outcome.",
                    self.file
                ),
                WIDTH,
            )
            .into_iter()
            .map(|line| Line::styled(line, theme.muted)),
        );
        let width = rows.iter().map(Line::width).max().unwrap_or_default();
        let (outer, body) = overlay::boxed(frame.area(), width.min(WIDTH), rows.len());
        overlay::draw_box(
            theme,
            frame,
            outer,
            &format!("Vote on check {}", self.number),
            self.keymap(),
        );
        frame.render_widget(List::new(rows), body);
    }
}

/// What a vote of `verdict` says, as the box offers it.
fn verdict_label(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::RightlyWithheld => "Rightly withheld: the check was right to hold it back",
        Verdict::ShouldHavePassed => "Should have passed: the question is fine on this check",
    }
}
