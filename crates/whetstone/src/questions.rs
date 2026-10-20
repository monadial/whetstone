//! The questions screen: one question at a time, with its place among them
//! ("Question 2 of 4"), its area, its stem and options A, B and C, the
//! reasoning editor, Submit response and Inspect source. The answer key,
//! the rationales, "why it matters", the rubric and the hint stay hidden
//! until the response is submitted.
//!
//! `a`, `b` and `c` choose an option while the options have focus, and
//! Space chooses the focused one. `i`, or Enter on the options or the
//! reasoning, starts editing the reasoning, and Esc stops, keeping the
//! draft. Tab moves to the next control, and from the editor to Submit
//! response, where Enter submits, with no dialog (rules 30 and 58). Submit
//! stays disabled, saying what's missing, until an option is chosen and
//! reasoning is written. Each question's draft stays in memory until it is
//! submitted.
//!
//! The session record starts as the questions show, holding them and the
//! evidence they cite. Submit saves the response to it first. Then the
//! choice outcome shows at once on the question's feedback, and the
//! response is final.
//!
//! When fewer questions passed their checks than were asked for, "*n*
//! passed checks, *N* asked for" heads them, so the learner knows nothing
//! unchecked filled the gap. Below the first question, one line names any
//! chosen area that wasn't asked about, because the relevance gate found it
//! not worth asking about in this change.

use ratatui::Frame;
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, Padding, Paragraph};
use whetstone_core::{Area, Evidence, OptionLetter, Picked, Question, QuestionCount, Response};

use crate::columns;
use crate::feedback::{self, Chosen, Feedback, Place};
use crate::focus::Position;
use crate::keymap::{self, Binding, Command, Input, Keymap, Press};
use crate::palette::Action;
use crate::reasoning_editor::{self, Caret, Editing, ReasoningEditor};
use crate::session_record::{Assessment, NewRecord, SessionId, SessionStore, StoreError};
use crate::theme::{ControlKind, ControlState, Theme};
use crate::view::{Reading, Route};

/// `i` starts writing the reasoning from any control.
const WRITE: Binding = Binding {
    keys: &[(Press::char('i'), Command::Edit)],
    does: "Write your reasoning",
    hint: Some("i write"),
};

/// The keys while one of the options has focus.
const OPTIONS_KEYMAP: Keymap = &[
    keymap::MOVE,
    Binding {
        keys: &[
            (Press::char('a'), Command::Choose(OptionLetter::A)),
            (Press::char('b'), Command::Choose(OptionLetter::B)),
            (Press::char('c'), Command::Choose(OptionLetter::C)),
        ],
        does: "Choose option A, B or C",
        hint: Some("a/b/c choose"),
    },
    Binding {
        keys: &[(Press::char(' '), Command::Activate)],
        does: "Choose the focused option",
        hint: None,
    },
    Binding {
        keys: &[
            (Press::char('i'), Command::Edit),
            (Press::key(KeyCode::Enter), Command::Edit),
        ],
        ..WRITE
    },
    keymap::BACK,
];

/// The keys while another control than the options has focus, and the
/// editor isn't being edited.
const KEYMAP: Keymap = &[keymap::MOVE, keymap::ACTIVATE, WRITE, keymap::BACK];

/// The actions in the palette while a question is answered.
const ACTIONS: &[Action] = &[
    INSPECT_SOURCE,
    Action {
        label: "Back",
        does: "Go back to the areas and changes",
        command: Command::Back,
    },
];

/// The actions in the palette on feedback.
const FEEDBACK_ACTIONS: &[Action] = &[
    INSPECT_SOURCE,
    Action {
        label: "Finish",
        does: "End the Practice, and go home",
        command: Command::Go(Route::Home),
    },
];

/// Inspect source, in the palette.
const INSPECT_SOURCE: Action = Action {
    label: "Inspect source",
    does: "Read the evidence the questions rest on",
    command: Command::Go(Route::Read(Reading::Evidence)),
};

/// The question's controls, in focus order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    /// One of the options.
    Option(OptionLetter),
    /// The reasoning editor.
    Reasoning,
    Submit,
    InspectSource,
}

impl Control {
    const ALL: [Self; 6] = [
        Self::Option(OptionLetter::A),
        Self::Option(OptionLetter::B),
        Self::Option(OptionLetter::C),
        Self::Reasoning,
        Self::Submit,
        Self::InspectSource,
    ];

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

/// Whether the learner is editing the reasoning.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Mode {
    /// Keys move between controls and act on them.
    #[default]
    Moving,
    /// Every key that isn't reserved is text, or edits it (rule 29).
    Editing,
}

/// What a draft lacks to be submitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Missing {
    Choice,
    Reasoning,
    Both,
}

impl Missing {
    /// What Submit says is missing.
    fn reason(self) -> &'static str {
        match self {
            Self::Choice => "choose an option",
            Self::Reasoning => "write your reasoning",
            Self::Both => "choose an option and write your reasoning",
        }
    }
}

/// The learner's draft response to one question, kept in memory until it
/// is submitted.
#[derive(Debug, Default)]
struct Draft {
    choice: Option<OptionLetter>,
    reasoning: ReasoningEditor,
    focus: Position,
    mode: Mode,
    /// Why the last Submit wasn't saved, if it wasn't: the draft stays, and
    /// nothing was submitted.
    unsaved: Option<StoreError>,
}

impl Draft {
    fn focused(&self) -> Control {
        Control::ALL[self.focus.index()]
    }

    /// Whether the learner wrote anything that leaving would lose.
    fn is_started(&self) -> bool {
        self.choice.is_some() || !self.reasoning.text().is_empty()
    }

    /// The response the draft makes, or what it lacks.
    fn response(&self) -> Result<Response, Missing> {
        // Whether reasoning is written is the response's own rule, whatever
        // the choice.
        let response = Response::new(
            self.choice.unwrap_or(OptionLetter::A),
            self.reasoning.text().to_owned(),
        );
        match (self.choice, response) {
            (Some(_), Ok(response)) => Ok(response),
            (Some(_), Err(_)) => Err(Missing::Reasoning),
            (None, Ok(_)) => Err(Missing::Choice),
            (None, Err(_)) => Err(Missing::Both),
        }
    }

    fn choose(&mut self, letter: OptionLetter) {
        self.choice = Some(letter);
        self.focus = Control::Option(letter).position();
    }

    fn edit(&mut self) {
        self.mode = Mode::Editing;
        self.focus = Control::Reasoning.position();
    }
}

/// One question, as the learner answers it.
#[derive(Debug)]
struct Entry {
    question: Question,
    /// Its area's name.
    area_name: String,
    attempt: Attempt,
}

/// Where the learner's response to a question stands.
#[derive(Debug)]
enum Attempt {
    /// Being written.
    Draft(Draft),
    /// Submitted and saved: final, with its feedback.
    Submitted(Feedback),
}

/// The session record of the questions.
#[derive(Debug)]
enum Record {
    /// To start, as soon as the app hands the screen its store.
    ToStart,
    /// Started, with this identity.
    Started(SessionId),
    /// It couldn't start, so no response can be saved.
    NotStarted,
}

/// The questions that passed their checks, picked to show, the learner's
/// response to each, and which one shows.
#[derive(Debug)]
pub(crate) struct Questions {
    /// Each question, in the order to show them.
    picked: Vec<Entry>,
    /// How many were asked for, when fewer passed.
    shortfall: Option<QuestionCount>,
    /// The names of the chosen areas not asked about, in order.
    not_asked_about: Vec<String>,
    /// The place of the question that shows.
    at: usize,
    record: Record,
    /// A response the learner just submitted to the question that shows,
    /// to save before anything else is done with it.
    submitted: Option<Response>,
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
                .map(|question| Entry {
                    area_name: name(&question),
                    question,
                    attempt: Attempt::Draft(Draft::default()),
                })
                .collect(),
            shortfall,
            not_asked_about: Vec::new(),
            at: 0,
            record: Record::ToStart,
            submitted: None,
        })
    }

    /// The same questions, saying below the first that the chosen `areas`
    /// weren't asked about: the relevance gate found them not worth asking
    /// about in this change.
    pub(crate) fn not_asking_about<'a>(
        mut self,
        areas: impl IntoIterator<Item = &'a Area>,
    ) -> Self {
        self.not_asked_about = areas
            .into_iter()
            .map(|area| area.name().to_owned())
            .collect();
        self
    }

    /// The question that shows.
    fn current(&self) -> &Entry {
        &self.picked[self.at]
    }

    /// Where the question that shows is among them.
    fn place(&self) -> Place {
        if self.at + 1 < self.picked.len() {
            Place::BeforeTheLast
        } else {
            Place::Last
        }
    }

    /// The screen's name, as key help and the palette title it.
    pub(crate) fn name(&self) -> &'static str {
        match self.current().attempt {
            Attempt::Draft(_) => "Questions",
            Attempt::Submitted(_) => "Feedback",
        }
    }

    /// The keys, by what has focus.
    pub(crate) fn keymap(&self) -> Keymap {
        match &self.current().attempt {
            Attempt::Draft(draft) => match (draft.mode, draft.focused()) {
                (Mode::Editing, _) => reasoning_editor::KEYMAP,
                (Mode::Moving, Control::Option(_)) => OPTIONS_KEYMAP,
                (Mode::Moving, _) => KEYMAP,
            },
            Attempt::Submitted(_) => feedback::KEYMAP,
        }
    }

    /// The actions in the palette.
    pub(crate) fn actions(&self) -> &'static [Action] {
        match self.current().attempt {
            Attempt::Draft(_) => ACTIONS,
            Attempt::Submitted(_) => FEEDBACK_ACTIONS,
        }
    }

    /// Text while the reasoning is edited, commands otherwise.
    pub(crate) fn input(&self) -> Input {
        match &self.current().attempt {
            Attempt::Draft(Draft {
                mode: Mode::Editing,
                ..
            }) => Input::Text,
            Attempt::Draft(_) | Attempt::Submitted(_) => Input::Commands,
        }
    }

    /// Whether leaving would lose a draft: a choice or reasoning not yet
    /// submitted.
    pub(crate) fn holds_draft(&self) -> bool {
        self.picked.iter().any(|entry| match &entry.attempt {
            Attempt::Draft(draft) => draft.is_started(),
            Attempt::Submitted(_) => false,
        })
    }

    /// Inserts pasted `text` into the reasoning, while it is edited.
    pub(crate) fn paste(&mut self, text: &str) {
        if let Attempt::Draft(draft) = &mut self.picked[self.at].attempt
            && draft.mode == Mode::Editing
        {
            draft.reasoning.paste(text);
        }
    }

    /// Starts the session record in `store`, with the questions and
    /// `evidence`, the evidence exactly as sent, unless it started. Then
    /// saves a response the learner just submitted, before anything else is
    /// done with it, and shows its choice outcome at once.
    pub(crate) fn keep_record(&mut self, store: &mut dyn SessionStore, evidence: &Evidence) {
        if let Record::ToStart = self.record {
            let record = NewRecord {
                questions: self
                    .picked
                    .iter()
                    .map(|entry| entry.question.clone())
                    .collect(),
                evidence: evidence.clone(),
                provenance: Vec::new(),
            };
            self.record = match store.start(record) {
                Ok(session) => {
                    tracing::info!(
                        %session,
                        questions = self.picked.len(),
                        "started the session record"
                    );
                    Record::Started(session)
                }
                Err(error) => {
                    tracing::warn!(%error, "the session record didn't start");
                    Record::NotStarted
                }
            };
        }
        if let Some(response) = self.submitted.take() {
            self.save(store, response);
        }
    }

    /// Saves `response` to the question that shows in `store`, then saves
    /// and shows its choice outcome, from the option ratings. If the store
    /// can't save it, the draft stays, and says why.
    fn save(&mut self, store: &mut dyn SessionStore, response: Response) {
        let Record::Started(session) = self.record else {
            return;
        };
        let place = self.place();
        let entry = &mut self.picked[self.at];
        let Attempt::Draft(draft) = &mut entry.attempt else {
            return;
        };
        let saved = match store.save_response(session, entry.question.id(), response.clone()) {
            Ok(saved) => saved,
            Err(error) => {
                tracing::warn!(%error, "a response wasn't saved");
                draft.unsaved = Some(error);
                return;
            }
        };
        let outcome = entry.question.choice_outcome(response.choice());
        tracing::info!(
            %session,
            attempt = saved.attempt().get(),
            "saved a response, with its choice outcome"
        );
        // The response is saved, so a failure here loses nothing the
        // learner wrote, and the outcome still shows.
        if let Err(error) = store.save_assessment(&saved, Assessment::Choice(outcome)) {
            tracing::warn!(%error, "a choice outcome wasn't saved");
        }
        entry.attempt = Attempt::Submitted(Feedback::new(
            response,
            outcome,
            saved,
            store.durability(),
            place,
        ));
    }

    /// Why Submit is disabled for `draft`, if it is.
    fn submit_disabled(&self, draft: &Draft) -> Option<&'static str> {
        if let Record::NotStarted = self.record {
            return Some("the session record didn't start");
        }
        draft.response().err().map(Missing::reason)
    }

    /// Applies `command` to the screen drawn in `area`, and returns what it
    /// asks of the app, if anything.
    pub(crate) fn apply(&mut self, command: Command, area: Rect) -> Option<Command> {
        let place = self.place();
        let at = self.at;
        let submit_disabled = match &self.picked[at].attempt {
            Attempt::Draft(draft) => self.submit_disabled(draft),
            Attempt::Submitted(_) => None,
        };
        let entry = &mut self.picked[at];
        let draft = match &mut entry.attempt {
            Attempt::Draft(draft) => draft,
            Attempt::Submitted(feedback) => {
                let [_, body] = layout(self.shortfall, area);
                return match feedback.apply(command, body, &entry.question, place)? {
                    Chosen::NextQuestion => {
                        self.at = (at + 1).min(self.picked.len() - 1);
                        None
                    }
                    Chosen::InspectSource => Some(INSPECT_SOURCE.command),
                    Chosen::Finish => Some(Command::Go(Route::Home)),
                };
            }
        };
        if draft.mode == Mode::Editing {
            let [_, body] = layout(self.shortfall, area);
            match draft.reasoning.apply(command, editor_width(body)) {
                Editing::Stay => {}
                Editing::Stop => draft.mode = Mode::Moving,
                Editing::Next => {
                    draft.mode = Mode::Moving;
                    draft.focus = Control::Submit.position();
                }
            }
            return None;
        }
        match command {
            Command::Choose(letter) => draft.choose(letter),
            Command::Edit => draft.edit(),
            Command::Activate => match draft.focused() {
                Control::Option(letter) => draft.choose(letter),
                Control::Reasoning => draft.edit(),
                Control::Submit => {
                    if submit_disabled.is_none()
                        && let Ok(response) = draft.response()
                    {
                        draft.unsaved = None;
                        self.submitted = Some(response);
                    }
                }
                Control::InspectSource => return Some(INSPECT_SOURCE.command),
            },
            command => draft.focus = draft.focus.after(command, Control::ALL.len()),
        }
        None
    }

    /// The lines that head every question: the shortfall, if there is one,
    /// and the question's place and area.
    fn head(&self, theme: &Theme) -> Vec<Line<'static>> {
        let entry = self.current();
        let mut lines = Vec::new();
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
            Span::styled(format!(" · {}", entry.area_name), theme.muted),
        ]));
        lines.push(Line::default());
        lines
    }

    /// Draws the question that shows in `area` with `theme`.
    pub(crate) fn render(&self, theme: &Theme, frame: &mut Frame<'_>, area: Rect) {
        let [head, body] = layout(self.shortfall, area);
        frame.render_widget(Paragraph::new(self.head(theme)), head);
        let entry = self.current();
        match &entry.attempt {
            Attempt::Draft(draft) => self.render_draft(draft, &entry.question, theme, frame, body),
            Attempt::Submitted(feedback) => {
                feedback.render(theme, frame, body, &entry.question, self.place());
            }
        }
    }

    /// Below the first question, the chosen areas not asked about, if any,
    /// wrapped to `width`.
    fn not_asked_about_note(&self, theme: &Theme, width: usize) -> Vec<Line<'static>> {
        if self.at != 0 || self.not_asked_about.is_empty() {
            return Vec::new();
        }
        let mut lines = vec![Line::default()];
        lines.extend(
            columns::wrap(
                &format!(
                    "Not asked about: {} (not worth asking about in this change)",
                    self.not_asked_about.join(", ")
                ),
                width,
            )
            .into_iter()
            .map(|line| Line::styled(line, theme.muted)),
        );
        lines
    }

    /// Draws `question` and the learner's `draft` in `area` with `theme`:
    /// the stem, the options, the reasoning editor and the controls.
    fn render_draft(
        &self,
        draft: &Draft,
        question: &Question,
        theme: &Theme,
        frame: &mut Frame<'_>,
        area: Rect,
    ) {
        let width = usize::from(area.width);
        let mut lines: Vec<Line<'_>> = columns::wrap(question.stem(), width)
            .into_iter()
            .map(Line::raw)
            .collect();
        lines.push(Line::default());
        let focused = (draft.mode == Mode::Moving).then(|| draft.focused());
        for letter in OptionLetter::ALL {
            let lead = format!("{letter}  ");
            let kind = ControlKind::Choice {
                selected: draft.choice == Some(letter),
            };
            // Measured in cells, as the option's lines are.
            let indent = Theme::control_width(&lead, kind);
            for (index, text) in
                columns::wrap(question.option(letter), width.saturating_sub(indent))
                    .into_iter()
                    .enumerate()
            {
                lines.push(if index == 0 {
                    theme.control(
                        format!("{lead}{text}"),
                        ControlState {
                            focused: focused == Some(Control::Option(letter)),
                            kind,
                            ..ControlState::default()
                        },
                    )
                } else {
                    Line::raw(format!("{}{text}", " ".repeat(indent)))
                });
            }
        }
        lines.extend(self.not_asked_about_note(theme, width));
        let mut controls = vec![
            theme.control(
                "Submit response",
                ControlState {
                    focused: focused == Some(Control::Submit),
                    disabled: self.submit_disabled(draft),
                    ..ControlState::default()
                },
            ),
            theme.control(
                "Inspect source",
                ControlState {
                    focused: focused == Some(Control::InspectSource),
                    ..ControlState::default()
                },
            ),
        ];
        if let Some(error) = &draft.unsaved {
            controls.extend(
                columns::wrap(
                    &format!("Your response wasn't saved, so it isn't submitted: {error}."),
                    width,
                )
                .into_iter()
                .map(Line::raw),
            );
        }
        let [text, _, editor, _, buttons] = Layout::vertical([
            Constraint::Max(u16::try_from(lines.len()).unwrap_or(u16::MAX)),
            Constraint::Length(1),
            Constraint::Min(EDITOR_ROWS),
            Constraint::Length(1),
            Constraint::Length(u16::try_from(controls.len()).unwrap_or(u16::MAX)),
        ])
        .areas(area);
        frame.render_widget(Paragraph::new(lines), text);
        let mut title = theme.control(
            "Your reasoning",
            ControlState {
                focused: focused == Some(Control::Reasoning) || draft.mode == Mode::Editing,
                ..ControlState::default()
            },
        );
        title.spans.insert(0, Span::raw(" "));
        title.push_span(" ");
        let block = reasoning_block().title(title);
        let inside = block.inner(editor);
        frame.render_widget(
            block.border_style(theme.border(draft.mode == Mode::Editing)),
            editor,
        );
        if draft.mode == Mode::Moving && draft.reasoning.text().is_empty() {
            frame.render_widget(
                Paragraph::new(Line::styled(
                    "Press i to write your reasoning.",
                    theme.muted,
                )),
                inside,
            );
        } else {
            let caret = match draft.mode {
                Mode::Editing => Caret::Shown,
                Mode::Moving => Caret::Hidden,
            };
            frame.render_widget(draft.reasoning.view(theme, caret), inside);
        }
        frame.render_widget(List::new(controls), buttons);
    }
}

/// The fewest rows the reasoning editor's box takes, its border included.
const EDITOR_ROWS: u16 = 5;

/// The reasoning editor's box, without its title: a border, and a cell of
/// padding on either side of the text.
fn reasoning_block() -> Block<'static> {
    Block::bordered().padding(Padding::horizontal(1))
}

/// How wide the reasoning editor is in the body of the screen, `body`: the
/// width inside its box.
fn editor_width(body: Rect) -> u16 {
    reasoning_block()
        .inner(Rect::new(0, 0, body.width, EDITOR_ROWS))
        .width
}

/// Where the lines heading every question go in `area`, with or without a
/// shortfall, and the body under them.
fn layout(shortfall: Option<QuestionCount>, area: Rect) -> [Rect; 2] {
    // The question's place, a blank row, and the shortfall if there is one.
    let rows = if shortfall.is_some() { 3 } else { 2 };
    Layout::vertical([Constraint::Length(rows), Constraint::Fill(1)]).areas(area)
}
