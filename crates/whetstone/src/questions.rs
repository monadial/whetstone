//! The questions screen: one question at a time, with its place among them
//! ("Question 2 of 4"), its area, its stem and options A, B and C, the
//! reasoning editor, Submit response and Inspect source, then the help
//! controls and signals: Hint, Show answer, Skip, Cannot answer from this
//! context and Report. The answer key, the rationales, "why it matters",
//! the rubric and the hint stay hidden until the response is submitted, or
//! the learner asks for them.
//!
//! `a`, `b` and `c` choose an option while the options have focus, and
//! Space chooses the focused one. `i`, or Enter on the options or the
//! reasoning, starts editing the reasoning, and Esc stops, keeping the
//! draft. Tab moves to the next control, and from the editor to Submit
//! response, where Enter submits, with no dialog (rules 30 and 58). Submit
//! stays disabled, saying what's missing, until an option is chosen.
//! Writing reasoning is optional, and the empty editor says so: only
//! written reasoning is assessed. Each question's draft stays in memory
//! until it is submitted.
//!
//! The help controls and signals are reached by focus, like the rest, and
//! Hint also by `h`, but never while the reasoning is edited, where every
//! key stays text (rule 29). None is held against the learner:
//! - **Hint** shows the question's checked hint below the options, or "No
//!   hint for this question" when its hint failed its checks. The attempt
//!   stays the learner's own.
//! - **Show answer** opens the answer key, the rationales, "why it
//!   matters" and the rubric in a reader, and makes the attempt an
//!   assisted attempt. The learner can still submit, and its outcomes are
//!   marked Assisted.
//! - **Skip** leaves the question unattempted, keeping its draft, and moves
//!   to the next question still unattempted. Next question goes to the
//!   next question, and after the last comes round to the first while one
//!   is still unattempted, so skipped questions come round again, and so
//!   does a submitted question's feedback, with any Retry it offers.
//! - **Cannot answer from this context** needs no response, and counts as
//!   neither incorrect nor skipped: the question is no longer unattempted.
//!   It moves on as Skip does.
//! - **Report** opens a box over the screen for a kind and an optional
//!   note (see [`crate::report`]). It is offered on feedback too, and
//!   never changes an outcome.
//!
//! **Finish**, on feedback, on a question that can't be answered and in the
//! palette, ends the Practice, for its summary. While a question is still
//! unattempted, it asks first (see [`crate::finish`]).
//!
//! Each signal is saved to the session record, in order, as soon as it is
//! given: the hint used, Show answer, each skip, cannot answer and each
//! report.
//!
//! Beside the question, and beside its feedback, the code it cites shows,
//! as the evidence sent it (see [`crate::question_context`]): on the left
//! at 120 columns or wider, and above the question otherwise, in at most
//! two fifths of the screen's height. Tab from the last control gives it
//! focus, and Shift+Tab from the first, and it then scrolls on its own. `z`
//! shows it on the whole screen, and `z`, `q` or Esc there shows the
//! question again. Inspect source opens the whole evidence at the first
//! citation. The reasoning editor starts at eight rows, and grows while it
//! has focus.
//!
//! The session record starts as the questions show, holding them, the
//! evidence they cite and every request sent so far, each with what
//! produced its result. Each request that ends later is saved to it as it
//! ends, before the outcomes it gives. Submit saves the response to it
//! first. Then the
//! choice outcome shows at once on the question's feedback, the response is
//! final, and, if its reasoning is written, Jev is asked about it with the
//! store's acknowledgement (rule 39). Its result applies to the response it
//! names, even once another question shows, and its outcomes are saved to the
//! record. So are the generator's explanation of what the reasoning missed
//! and Jev's check of it, asked with the same acknowledgement.
//!
//! When fewer questions passed their checks than were asked for, "*n*
//! passed checks, *N* asked for" heads them, so the learner knows nothing
//! unchecked filled the gap, and offers See why fewer passed in the
//! palette, which reads the reasons No questions passed checks gives
//! (see [`crate::withheld`]), and, while any candidate was withheld, Review
//! withheld questions (see [`crate::review_withheld`]). Below the first
//! question, one line names any
//! chosen area that wasn't asked about, because the relevance gate found it
//! not worth asking about in this change.

use std::mem;

use ratatui::Frame;
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, Padding, Paragraph, Wrap};
use whetstone_ai::{ParagraphCheck, RequestFailure, WrittenParagraph};
use whetstone_core::{
    Area, CandidateId, CriterionOutcome, Evidence, OptionLetter, Picked, Probability, Question,
    QuestionCount, Response,
};

use crate::cannot_answer::{self, CannotAnswer};
use crate::columns;
use crate::failures::FailedRequests;
use crate::feedback::{self, AttemptKind, Feedback, Place, Step};
use crate::finish::{self, ConfirmFinish, Drafts, Finishing};
use crate::focus::Position;
use crate::jev_answers;
use crate::keymap::{self, Binding, Command, Input, Keymap, Press};
use crate::palette::Action;
use crate::question_context::{self, QuestionContext};
use crate::reader::{Reader, Section};
use crate::reasoning_editor::{self, Caret, Editing, ReasoningEditor};
use crate::report::{self, ReportForm, Reporting};
use crate::sent_requests::{SentRequest, SentRequests};
use crate::session_record::{
    Assessment, NewRecord, SavedReasoning, SavedResponse, SessionId, SessionStore, Signal,
    StoreError,
};
use crate::summary::{Ended, Listed};
use crate::theme::{ColorSupport, ControlKind, ControlState, Theme};
use crate::view::{Reading, Route};

/// `i` starts writing the reasoning from any control.
const WRITE: Binding = Binding {
    keys: &[(Press::char('i'), Command::Edit)],
    does: "Write your reasoning",
    hint: Some("i write"),
};

/// `h` shows the question's hint from any control.
const HINT: Binding = Binding {
    keys: &[(Press::char('h'), Command::ShowHint)],
    does: "Show the question's hint",
    hint: Some("h hint"),
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
    HINT,
    question_context::ZOOM,
    keymap::BACK,
];

/// The keys while another control than the options has focus, and the
/// editor isn't being edited.
const KEYMAP: Keymap = &[
    keymap::MOVE,
    keymap::ACTIVATE,
    WRITE,
    HINT,
    question_context::ZOOM,
    keymap::BACK,
];

/// The keys while the code beside a question has focus.
const CONTEXT_KEYMAP: Keymap = &[
    question_context::SCROLL,
    question_context::PAGE,
    question_context::ENDS,
    question_context::HALF_PAGE,
    WRITE,
    HINT,
    question_context::ZOOM,
    keymap::BACK,
];

/// The keys while the code beside a question's feedback has focus.
const FEEDBACK_CONTEXT_KEYMAP: Keymap = &[
    question_context::SCROLL,
    question_context::PAGE,
    question_context::ENDS,
    question_context::HALF_PAGE,
    question_context::ZOOM,
    keymap::BACK,
];

/// The actions in the palette while a question is answered.
const ACTIONS: &[Action] = &[INSPECT_SOURCE, FINISH, BACK];

/// The actions in the palette while a question is answered, when fewer
/// passed their checks than were asked for.
const SHORTFALL_ACTIONS: &[Action] = &[INSPECT_SOURCE, SEE_WHY, FINISH, BACK];

/// The same, when the question check withheld any of the candidates.
const SHORTFALL_WITHHELD_ACTIONS: &[Action] =
    &[INSPECT_SOURCE, SEE_WHY, REVIEW_WITHHELD, FINISH, BACK];

/// The actions in the palette on feedback.
const FEEDBACK_ACTIONS: &[Action] = &[INSPECT_SOURCE, FINISH];

/// The actions in the palette on feedback, when fewer passed their checks
/// than were asked for.
const SHORTFALL_FEEDBACK_ACTIONS: &[Action] = &[INSPECT_SOURCE, SEE_WHY, FINISH];

/// The same, when the question check withheld any of the candidates.
const SHORTFALL_WITHHELD_FEEDBACK_ACTIONS: &[Action] =
    &[INSPECT_SOURCE, SEE_WHY, REVIEW_WITHHELD, FINISH];

/// See why fewer passed, in the palette when fewer passed their checks
/// than were asked for.
const SEE_WHY: Action = Action {
    label: "See why fewer passed",
    does: "Read why the other questions written didn't pass their checks",
    command: Command::Go(Route::Read(Reading::WhyWithheld)),
};

/// Review withheld questions, in the palette beside See why fewer passed,
/// when the question check withheld any of the candidates.
const REVIEW_WITHHELD: Action = Action {
    label: "Review withheld questions",
    does: "Vote on each check that withheld a question, saved as a label on this machine",
    command: Command::Go(Route::Read(Reading::Withheld)),
};

/// Back, in the palette.
const BACK: Action = Action {
    label: "Back",
    does: "Go back to the areas and changes",
    command: Command::Back,
};

/// Finish, in the palette.
const FINISH: Action = Action {
    label: "Finish",
    does: "End the Practice, and see its summary",
    command: Command::Finish,
};

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
    /// Shows the question's checked hint, and leaves the attempt the
    /// learner's own.
    Hint,
    /// Reveals the answer key, the rationales and the rubric, and makes the
    /// attempt an assisted attempt.
    ShowAnswer,
    /// Leaves the question unattempted, keeping its draft, and moves to the
    /// next unattempted question.
    Skip,
    /// Records that the evidence isn't enough to answer the question, which
    /// needs no response, and moves on.
    CannotAnswer,
    /// Reports the question as a hallucination, a mistake or ambiguous.
    Report,
}

impl Control {
    const ALL: [Self; 11] = [
        Self::Option(OptionLetter::A),
        Self::Option(OptionLetter::B),
        Self::Option(OptionLetter::C),
        Self::Reasoning,
        Self::Submit,
        Self::InspectSource,
        Self::Hint,
        Self::ShowAnswer,
        Self::Skip,
        Self::CannotAnswer,
        Self::Report,
    ];

    /// The help controls and signals, in focus order: they share the rows
    /// below Submit response and Inspect source.
    const HELP: [Self; 5] = [
        Self::Hint,
        Self::ShowAnswer,
        Self::Skip,
        Self::CannotAnswer,
        Self::Report,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Option(_) | Self::Reasoning => "",
            Self::Submit => "Submit response",
            Self::InspectSource => "Inspect source",
            Self::Hint => "Hint",
            Self::ShowAnswer => "Show answer",
            Self::Skip => "Skip",
            Self::CannotAnswer => "Cannot answer from this context",
            Self::Report => "Report",
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

/// What has the keys on the questions screen.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Pane {
    /// The question, or its feedback.
    #[default]
    Question,
    /// The code the question cites, beside it.
    Context,
    /// The code the question cites, on the whole screen.
    Zoomed,
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

/// What Submit says a draft lacks, until an option is chosen: reasoning
/// is optional.
const NO_CHOICE: &str = "choose an option";

/// What the empty reasoning editor says first: writing reasoning is
/// optional.
const OPTIONAL_NOTE: &str = "Optional: explain why, to have your reasoning assessed.";

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

    /// The response the draft makes, once an option is chosen. Whether its
    /// reasoning is written is the response's own rule.
    fn response(&self) -> Option<Response> {
        Some(Response::new(
            self.choice?,
            self.reasoning.text().to_owned(),
        ))
    }

    fn choose(&mut self, letter: OptionLetter) {
        self.choice = Some(letter);
        self.focus = Control::Option(letter).position();
    }

    fn edit(&mut self) {
        self.mode = Mode::Editing;
        self.focus = Control::Reasoning.position();
    }

    /// Whether `command` moves focus out of the controls, round past the
    /// last or the first.
    fn wraps(&self, command: Command) -> bool {
        match command {
            Command::NextControl => self.focus.index() + 1 == Control::ALL.len(),
            Command::PreviousControl => self.focus.index() == 0,
            _ => false,
        }
    }

    /// Whether the reasoning editor has focus, so that it grows.
    fn editor_grows(&self) -> bool {
        self.mode == Mode::Editing || self.focused() == Control::Reasoning
    }
}

/// One question, as the learner answers it.
#[derive(Debug)]
struct Entry {
    question: Question,
    /// Its area's name.
    area_name: String,
    /// The code it cites, beside it.
    context: QuestionContext,
    attempt: Attempt,
    /// Whether the learner asked for the question's hint.
    hint: HintState,
    /// Whether the attempt is the learner's own, or assisted, once they
    /// chose Show answer.
    kind: AttemptKind,
    /// How many times the learner reported it.
    reports: usize,
}

impl Entry {
    /// Whether the learner has yet to submit a response or say they cannot
    /// answer it: a skipped question still is.
    fn is_unattempted(&self) -> bool {
        match self.attempt {
            Attempt::Draft(_) => true,
            Attempt::Submitted(_) | Attempt::CannotAnswer(_) => false,
        }
    }
}

/// Whether the learner asked for a question's hint.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum HintState {
    #[default]
    Hidden,
    /// It shows below the options, or "No hint for this question" does.
    Shown,
}

/// Where the learner's response to a question stands.
#[derive(Debug)]
enum Attempt {
    /// Being written.
    Draft(Draft),
    /// Submitted and saved: final, with its feedback.
    Submitted(Feedback),
    /// The learner said the evidence isn't enough to answer it: final, with
    /// no response.
    CannotAnswer(CannotAnswer),
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
    /// Each question, in the order to show them: never none.
    picked: Vec<Entry>,
    /// How many were asked for, when fewer passed.
    shortfall: Option<QuestionCount>,
    /// The names of the chosen areas not asked about, in order.
    not_asked_about: Vec<String>,
    /// How many candidates the question check withheld.
    withheld: usize,
    /// The place of the question that shows.
    at: usize,
    record: Record,
    /// A response the learner just submitted to the question that shows,
    /// to save before anything else is done with it.
    submitted: Option<Response>,
    /// The signals the learner just gave, each about a question, to save
    /// in order.
    signals: Vec<(CandidateId, Signal)>,
    /// The report the learner is writing about the question that shows, if
    /// they are.
    reporting: Option<Box<ReportForm>>,
    /// Asking whether to finish while questions are still unattempted, if
    /// the learner chose Finish then.
    finishing: Option<Box<ConfirmFinish>>,
    /// The saved responses whose evaluation the learner cancelled, for the
    /// screen to stop.
    cancelled: Vec<SavedResponse>,
    /// What the learner can do about a request about a response that failed.
    failed_requests: FailedRequests,
    /// What has the keys: the question, or the code it cites.
    pane: Pane,
}

impl Questions {
    /// The questions `picked`, about the chosen `areas`, showing the first,
    /// or `None` when there are none to show, each beside the code it
    /// cites as `evidence` sends it, under `change`, which names the
    /// change. A request about a response that fails says what to do as
    /// `failed_requests` words it.
    pub(crate) fn new(
        picked: Picked,
        areas: &[Area],
        failed_requests: FailedRequests,
        evidence: &Evidence,
        change: &str,
    ) -> Option<Self> {
        let (questions, shortfall) = match picked {
            Picked::All(questions) => (questions, None),
            Picked::Shortfall {
                questions,
                asked_for,
            } => (questions, Some(asked_for)),
            Picked::NonePassed => return None,
        };
        // The screen always shows one of them.
        if questions.is_empty() {
            return None;
        }
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
                    context: QuestionContext::new(change.to_owned(), &question, evidence),
                    question,
                    attempt: Attempt::Draft(Draft::default()),
                    hint: HintState::Hidden,
                    kind: AttemptKind::Own,
                    reports: 0,
                })
                .collect(),
            shortfall,
            not_asked_about: Vec::new(),
            withheld: 0,
            at: 0,
            record: Record::ToStart,
            submitted: None,
            signals: Vec::new(),
            reporting: None,
            finishing: None,
            cancelled: Vec::new(),
            failed_requests,
            pane: Pane::Question,
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

    /// How many questions there are.
    pub(crate) fn len(&self) -> usize {
        self.picked.len()
    }

    /// The same questions, offering Review withheld questions in the
    /// palette beside See why fewer passed when the question check withheld
    /// any of the `withheld` candidates.
    pub(crate) fn withholding(self, withheld: usize) -> Self {
        Self { withheld, ..self }
    }

    /// The question that shows.
    fn current(&self) -> &Entry {
        &self.picked[self.at]
    }

    /// The question that shows, whose first citation Inspect source opens
    /// the evidence at.
    pub(crate) fn current_question(&self) -> &Question {
        &self.current().question
    }

    /// What has the keys: the question, on a page that has no code beside
    /// it, as Cannot answer from this context has none.
    fn pane(&self) -> Pane {
        match self.current().attempt {
            Attempt::Draft(_) | Attempt::Submitted(_) => self.pane,
            Attempt::CannotAnswer(_) => Pane::Question,
        }
    }

    /// Where the question that shows is among them.
    fn place(&self) -> Place {
        match self.next_question(self.at) {
            Some(_) => Place::BeforeTheLast,
            None => Place::Last,
        }
    }

    /// The place of the question Next question goes to from the one at
    /// `at`: the next, or after the last, the first, while another question
    /// is still unattempted, so skipped questions come round again.
    fn next_question(&self, at: usize) -> Option<usize> {
        if at + 1 < self.picked.len() {
            return Some(at + 1);
        }
        self.picked
            .iter()
            .enumerate()
            .any(|(place, entry)| place != at && entry.is_unattempted())
            .then_some(0)
    }

    /// The place of the next question after the one at `at` still
    /// unattempted, coming round after the last, if another is.
    fn next_unattempted(&self, at: usize) -> Option<usize> {
        let count = self.picked.len();
        (1..count)
            .map(|step| (at + step) % count)
            .find(|&place| self.picked[place].is_unattempted())
    }

    /// The place of the question Skip goes to from the one at `at`: the next
    /// still unattempted, or with none, the next, coming round after the
    /// last, so the skipped one comes round again. With only one question,
    /// there is none.
    fn skip_to(&self, at: usize) -> Option<usize> {
        let count = self.picked.len();
        self.next_unattempted(at)
            .or_else(|| (count > 1).then(|| (at + 1) % count))
    }

    /// The screen's name, as key help and the palette title it.
    pub(crate) fn name(&self) -> &'static str {
        if self.finishing.is_some() {
            return "Finish";
        }
        if self.reporting.is_some() {
            return "Report";
        }
        if self.pane() == Pane::Zoomed {
            return "Cited code";
        }
        match self.current().attempt {
            Attempt::Draft(_) => "Questions",
            Attempt::Submitted(_) => "Feedback",
            Attempt::CannotAnswer(_) => "Cannot answer from this context",
        }
    }

    /// The keys, by what has focus.
    pub(crate) fn keymap(&self) -> Keymap {
        if self.finishing.is_some() {
            return finish::KEYMAP;
        }
        if let Some(form) = &self.reporting {
            return form.keymap();
        }
        match (self.pane(), &self.current().attempt) {
            (Pane::Zoomed, _) => return question_context::ZOOMED_KEYMAP,
            (Pane::Context, Attempt::Draft(_)) => return CONTEXT_KEYMAP,
            (Pane::Context, _) => return FEEDBACK_CONTEXT_KEYMAP,
            (Pane::Question, _) => {}
        }
        match &self.current().attempt {
            Attempt::Draft(draft) => match (draft.mode, draft.focused()) {
                (Mode::Editing, _) => reasoning_editor::KEYMAP,
                (Mode::Moving, Control::Option(_)) => OPTIONS_KEYMAP,
                (Mode::Moving, _) => KEYMAP,
            },
            Attempt::Submitted(_) => feedback::KEYMAP,
            Attempt::CannotAnswer(_) => cannot_answer::KEYMAP,
        }
    }

    /// The actions in the palette.
    pub(crate) fn actions(&self) -> &'static [Action] {
        if self.finishing.is_some() {
            return finish::ACTIONS;
        }
        if self.reporting.is_some() {
            return report::ACTIONS;
        }
        match (&self.current().attempt, self.shortfall, self.withheld) {
            (Attempt::Draft(_), None, _) => ACTIONS,
            (Attempt::Draft(_), Some(_), 0) => SHORTFALL_ACTIONS,
            (Attempt::Draft(_), Some(_), _) => SHORTFALL_WITHHELD_ACTIONS,
            (Attempt::Submitted(_) | Attempt::CannotAnswer(_), None, _) => FEEDBACK_ACTIONS,
            (Attempt::Submitted(_) | Attempt::CannotAnswer(_), Some(_), 0) => {
                SHORTFALL_FEEDBACK_ACTIONS
            }
            (Attempt::Submitted(_) | Attempt::CannotAnswer(_), Some(_), _) => {
                SHORTFALL_WITHHELD_FEEDBACK_ACTIONS
            }
        }
    }

    /// Text while the reasoning is edited, commands otherwise.
    pub(crate) fn input(&self) -> Input {
        if self.finishing.is_some() || self.pane() != Pane::Question {
            return Input::Commands;
        }
        if let Some(form) = &self.reporting {
            return form.input();
        }
        match &self.current().attempt {
            Attempt::Draft(Draft {
                mode: Mode::Editing,
                ..
            }) => Input::Text,
            Attempt::Draft(_) | Attempt::Submitted(_) | Attempt::CannotAnswer(_) => Input::Commands,
        }
    }

    /// Whether leaving would lose a draft: a choice or reasoning not yet
    /// submitted.
    pub(crate) fn holds_draft(&self) -> bool {
        self.picked.iter().any(|entry| match &entry.attempt {
            Attempt::Draft(draft) => draft.is_started(),
            Attempt::Submitted(_) | Attempt::CannotAnswer(_) => false,
        })
    }

    /// Inserts pasted `text` into the reasoning, while it is edited, or into
    /// a report's note, while it has focus.
    pub(crate) fn paste(&mut self, text: &str) {
        if let Some(form) = &mut self.reporting {
            form.paste(text);
        } else if let Attempt::Draft(draft) = &mut self.picked[self.at].attempt
            && draft.mode == Mode::Editing
        {
            draft.reasoning.paste(text);
        }
    }

    /// Starts the session record in `store`, with the questions,
    /// `evidence`, the evidence exactly as sent, and every request in
    /// `sent` so far, unless it started. Then saves each request that ended
    /// since, then a response the learner just submitted, before anything
    /// else is done with it, showing its choice outcome at once, then the
    /// outcomes known since.
    pub(crate) fn keep_record(
        &mut self,
        store: &mut dyn SessionStore,
        evidence: &Evidence,
        sent: &mut SentRequests,
    ) {
        if let Record::ToStart = self.record {
            let record = NewRecord {
                questions: self
                    .picked
                    .iter()
                    .map(|entry| entry.question.clone())
                    .collect(),
                evidence: evidence.clone(),
                requests: sent.take_unrecorded(),
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
        if let Record::Started(session) = self.record {
            for request in sent.take_unrecorded() {
                let id = request.provenance.request();
                match store.save_request(session, request) {
                    Ok(()) => tracing::info!(%session, request = %id, "saved a request"),
                    Err(error) => tracing::warn!(%error, request = %id, "a request wasn't saved"),
                }
            }
        }
        for (question, signal) in mem::take(&mut self.signals) {
            let Record::Started(session) = self.record else {
                tracing::warn!("a signal wasn't saved: the session record didn't start");
                continue;
            };
            match store.save_signal(session, &question, signal.clone()) {
                // A report's note never shows in its `Debug` output.
                Ok(()) => tracing::info!(%session, ?signal, "saved a signal"),
                Err(error) => tracing::warn!(%error, "a signal wasn't saved"),
            }
        }
        if let Some(response) = self.submitted.take() {
            self.save(store, response);
        }
        for entry in &mut self.picked {
            if let Attempt::Submitted(feedback) = &mut entry.attempt {
                feedback.keep_record(store);
            }
        }
    }

    /// Hands `evaluate` each evaluation of the reasoning that waits to be
    /// asked for: the store's acknowledgement of the response, with the
    /// reasoning as saved, and its question. Each then awaits its result. A
    /// response with no reasoning written has none.
    pub(crate) fn evaluate(&mut self, mut evaluate: impl FnMut(SavedReasoning<'_>, &Question)) {
        for entry in &mut self.picked {
            if let Attempt::Submitted(feedback) = &mut entry.attempt
                && let Some(reasoning) = feedback.take_evaluation()
            {
                evaluate(reasoning, &entry.question);
            }
        }
    }

    /// The saved responses whose evaluation the learner cancelled since the
    /// last take, for the screen to stop.
    pub(crate) fn take_cancelled(&mut self) -> Vec<SavedResponse> {
        mem::take(&mut self.cancelled)
    }

    /// Applies Jev's `reply` about the reasoning of the response `saved`
    /// acknowledges: a probability for each criterion, or why the request
    /// failed.
    pub(crate) fn evaluated(
        &mut self,
        saved: &SavedResponse,
        reply: Result<Vec<Probability>, RequestFailure>,
    ) {
        if let Some((question, feedback)) = self.feedback_on(saved) {
            feedback.evaluated(question, reply);
        }
    }

    /// Hands `explain` each explanation of the reasoning that waits to be
    /// asked for: the store's acknowledgement of the response, with its
    /// reasoning, its question, and the criteria to explain, each with its
    /// outcome. They are then being written.
    pub(crate) fn explain(
        &mut self,
        mut explain: impl FnMut(SavedReasoning<'_>, &Question, &[(usize, CriterionOutcome)]),
    ) {
        for entry in &mut self.picked {
            if let Attempt::Submitted(feedback) = &mut entry.attempt
                && let Some((reasoning, criteria)) = feedback.take_explanation()
            {
                explain(reasoning, &entry.question, &criteria);
            }
        }
    }

    /// Hands `check` each explanation that waits for Jev's check: the
    /// store's acknowledgement of the response, with its reasoning, its
    /// question, and the paragraphs to check. They are then being checked.
    pub(crate) fn check_explanations(
        &mut self,
        mut check: impl FnMut(SavedReasoning<'_>, &Question, &[WrittenParagraph]),
    ) {
        for entry in &mut self.picked {
            if let Attempt::Submitted(feedback) = &mut entry.attempt
                && let Some((reasoning, paragraphs)) = feedback.take_explanation_check()
            {
                check(reasoning, &entry.question, &paragraphs);
            }
        }
    }

    /// Applies the generator's `reply` explaining the reasoning of the
    /// response `saved` acknowledges, whose citations are checked against
    /// `evidence`: a paragraph for each criterion asked about, or why the
    /// request failed.
    pub(crate) fn explained(
        &mut self,
        saved: &SavedResponse,
        evidence: &Evidence,
        reply: Result<Vec<WrittenParagraph>, RequestFailure>,
    ) {
        if let Some((_, feedback)) = self.feedback_on(saved) {
            feedback.explained(evidence, reply);
        }
    }

    /// Applies Jev's `reply` checking the explanation of the response
    /// `saved` acknowledges: its answers about each paragraph, or why the
    /// request failed.
    pub(crate) fn explanation_checked(
        &mut self,
        saved: &SavedResponse,
        reply: Result<Vec<ParagraphCheck>, RequestFailure>,
    ) {
        if let Some((question, feedback)) = self.feedback_on(saved) {
            feedback.explanation_checked(question, reply);
        }
    }

    /// The question the response `saved` acknowledges responds to, and the
    /// feedback on it, if it was submitted.
    fn feedback_on(&mut self, saved: &SavedResponse) -> Option<(&Question, &mut Feedback)> {
        self.picked
            .iter_mut()
            .find_map(|entry| match &mut entry.attempt {
                Attempt::Submitted(feedback) if feedback.saved() == saved => {
                    Some((&entry.question, feedback))
                }
                Attempt::Draft(_) | Attempt::Submitted(_) | Attempt::CannotAnswer(_) => None,
            })
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
        let saved = match store.save_response(session, entry.question.id(), response) {
            Ok(saved) => saved,
            Err(error) => {
                tracing::warn!(%error, "a response wasn't saved");
                draft.unsaved = Some(error);
                return;
            }
        };
        let outcome = entry.question.choice_outcome(saved.response().choice());
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
            outcome,
            saved,
            entry.kind,
            store.durability(),
            place,
        ));
    }

    /// Why Submit is disabled for `draft`, if it is.
    fn submit_disabled(&self, draft: &Draft) -> Option<&'static str> {
        if let Record::NotStarted = self.record {
            return Some("the session record didn't start");
        }
        draft.response().is_none().then_some(NO_CHOICE)
    }

    /// Applies `command` to the screen drawn in `area`, and returns what it
    /// asks of the app, if anything.
    pub(crate) fn apply(&mut self, command: Command, area: Rect) -> Option<Command> {
        if let Some(check) = &mut self.finishing {
            return match check.apply(command) {
                Finishing::Open => None,
                Finishing::KeepAnswering => {
                    self.finishing = None;
                    None
                }
                Finishing::Finish => {
                    self.finishing = None;
                    Some(Command::Finish)
                }
            };
        }
        if let Some(form) = &mut self.reporting {
            let reporting = form.apply(command);
            self.reported(reporting);
            return None;
        }
        if command == Command::Finish {
            return self.finish();
        }
        if self.pane() != Pane::Question {
            self.apply_context(command, area);
            return None;
        }
        let beside = self.parts(area).question.unwrap_or(area);
        let place = self.place();
        let Entry {
            question, attempt, ..
        } = &mut self.picked[self.at];
        let step = match attempt {
            Attempt::Draft(_) => return self.apply_draft(command, area),
            Attempt::Submitted(feedback) => {
                if command == Command::Zoom {
                    self.pane = Pane::Zoomed;
                    return None;
                }
                if feedback.wraps(command) {
                    self.pane = Pane::Context;
                    return None;
                }
                let [_, body] = layout(self.shortfall, beside);
                let step = feedback.apply(command, body, question, place, &self.failed_requests)?;
                if step == Step::CancelEvaluation {
                    self.cancelled.push(feedback.saved().clone());
                }
                step
            }
            Attempt::CannotAnswer(page) => page.apply(command, place)?,
        };
        match step {
            Step::NextQuestion => {
                if let Some(next) = self.next_question(self.at) {
                    self.at = next;
                }
                None
            }
            Step::InspectSource => Some(INSPECT_SOURCE.command),
            Step::InspectJevAnswers => Some(Command::Go(Route::Read(Reading::JevAnswers))),
            Step::Finish => self.finish(),
            Step::Report => {
                self.reporting = Some(Box::default());
                None
            }
            Step::CancelEvaluation => None,
        }
    }

    /// Applies `command` to the code the question that shows cites, beside
    /// it or on the whole screen, drawn in `area`: it scrolls the code, or
    /// moves focus back to the question.
    fn apply_context(&mut self, command: Command, area: Rect) {
        let parts = self.parts(area);
        let pane = self.pane;
        let entry = &mut self.picked[self.at];
        match (pane, command) {
            (Pane::Zoomed, Command::Zoom) => self.pane = Pane::Question,
            (_, Command::Zoom) => self.pane = Pane::Zoomed,
            (Pane::Context, Command::NextControl | Command::PreviousControl) => {
                self.pane = Pane::Question;
                // Focus comes back to the first control after the code, and
                // to the last before it.
                match &mut entry.attempt {
                    Attempt::Draft(draft) => {
                        draft.focus = match command {
                            Command::NextControl => Position::default(),
                            _ => Position::at(Control::ALL.len() - 1),
                        };
                    }
                    Attempt::Submitted(feedback) => feedback.enter(command),
                    Attempt::CannotAnswer(_) => {}
                }
            }
            (Pane::Context, Command::Edit) => {
                if let Attempt::Draft(draft) = &mut entry.attempt {
                    self.pane = Pane::Question;
                    draft.edit();
                }
            }
            (Pane::Context, Command::ShowHint) => show_hint(entry, &mut self.signals),
            (_, command) => {
                if let Some(context) = parts.context {
                    entry.context.scroll(command, context);
                }
            }
        }
    }

    /// Where the code the question that shows cites and the question go in
    /// `area`. They are side by side at 120 columns or wider, the code on
    /// the left. Narrower, the code is above, in what the question leaves,
    /// at least a few rows, but no more than two fifths of the height, or
    /// than it takes to show the code whole. The code
    /// alone shows while it is on the whole screen, and the question alone
    /// on a page that has no code beside it.
    fn parts(&self, area: Rect) -> Parts {
        match (self.pane(), &self.current().attempt) {
            (_, Attempt::CannotAnswer(_)) => {
                return Parts {
                    context: None,
                    question: Some(area),
                };
            }
            (Pane::Zoomed, _) => {
                return Parts {
                    context: Some(area),
                    question: None,
                };
            }
            (Pane::Question | Pane::Context, _) => {}
        }
        let [context, _, question] = if area.width >= WIDE {
            Layout::horizontal([
                Constraint::Percentage(CONTEXT_PERCENT),
                Constraint::Length(GAP),
                Constraint::Fill(1),
            ])
            .areas(area)
        } else {
            let most = (area.height * 2 / 5).min(self.current().context.rows(area.width));
            let rows = area
                .height
                .saturating_sub(self.rows_needed(area.width) + 1)
                .min(most)
                .max(CONTEXT_ROWS.min(most));
            Layout::vertical([
                Constraint::Length(rows),
                Constraint::Length(1),
                Constraint::Fill(1),
            ])
            .areas(area)
        };
        Parts {
            context: Some(context),
            question: Some(question),
        }
    }

    /// The control of `draft` that shows focus: none while the learner
    /// edits the reasoning, or while the code beside it has focus.
    fn draft_focus(&self, draft: &Draft) -> Option<Control> {
        (self.pane() == Pane::Question && draft.mode == Mode::Moving).then(|| draft.focused())
    }

    /// The fewest rows the question that shows needs, `width` cells wide,
    /// with the lines heading it: its text and controls, with the editor at
    /// its fewest rows, or as many as it starts with while it grows, or
    /// feedback's controls and a few rows of its text.
    fn rows_needed(&self, width: u16) -> u16 {
        let entry = self.current();
        let rows = match &entry.attempt {
            Attempt::Draft(draft) => {
                // Styles do not change how the text wraps.
                let theme = Theme::new(ColorSupport::NoColor);
                let width = usize::from(width);
                let editor = if draft.editor_grows() {
                    EDITOR_START
                } else {
                    EDITOR_ROWS
                };
                self.draft_text(draft, entry, &theme, width).len()
                    + self.draft_controls(draft, &theme, width).len()
                    + usize::from(editor)
                    + 2
            }
            Attempt::Submitted(feedback) => FEEDBACK_ROWS + 1 + feedback.control_rows(),
            Attempt::CannotAnswer(_) => 0,
        };
        head_rows(self.shortfall).saturating_add(u16::try_from(rows).unwrap_or(u16::MAX))
    }

    /// Finish, as the learner chose it: [`Command::Finish`], for the screen to
    /// end the Practice, or first, while a question is still unattempted,
    /// the box asking whether to finish.
    fn finish(&mut self) -> Option<Command> {
        let unattempted = self
            .picked
            .iter()
            .filter(|entry| entry.is_unattempted())
            .count();
        if unattempted == 0 {
            return Some(Command::Finish);
        }
        let drafts = if self.holds_draft() {
            Drafts::Started
        } else {
            Drafts::NotStarted
        };
        self.finishing = Some(Box::new(ConfirmFinish::new(
            unattempted,
            self.picked.len(),
            drafts,
        )));
        None
    }

    /// Applies what the learner did in the report about the question that
    /// shows: a report they made is saved, and changes nothing else.
    fn reported(&mut self, reporting: Reporting) {
        match reporting {
            Reporting::Open => {}
            Reporting::Cancelled => self.reporting = None,
            Reporting::Reported(report) => {
                self.reporting = None;
                let entry = &mut self.picked[self.at];
                entry.reports += 1;
                tracing::info!(
                    question = self.at + 1,
                    kind = ?report.kind(),
                    noted = report.note().is_some(),
                    "reported a question"
                );
                self.signals
                    .push((entry.question.id().clone(), Signal::Report(report)));
            }
        }
    }

    /// Applies `command` to the draft response to the question that shows,
    /// drawn in `area`, and returns what it asks of the app, if anything.
    fn apply_draft(&mut self, command: Command, area: Rect) -> Option<Command> {
        let beside = self.parts(area).question.unwrap_or(area);
        let at = self.at;
        let skip_to = self.skip_to(at);
        let submit_disabled = match &self.picked[at].attempt {
            Attempt::Draft(draft) => self.submit_disabled(draft),
            Attempt::Submitted(_) | Attempt::CannotAnswer(_) => None,
        };
        if command == Command::ShowHint && self.input() == Input::Commands {
            show_hint(&mut self.picked[at], &mut self.signals);
            return None;
        }
        let Entry {
            question,
            attempt: Attempt::Draft(draft),
            kind,
            ..
        } = &mut self.picked[at]
        else {
            return None;
        };
        if draft.mode == Mode::Editing {
            let [_, body] = layout(self.shortfall, beside);
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
            Command::Zoom => self.pane = Pane::Zoomed,
            Command::NextControl | Command::PreviousControl if draft.wraps(command) => {
                self.pane = Pane::Context;
            }
            Command::Activate => match draft.focused() {
                Control::Option(letter) => draft.choose(letter),
                Control::Reasoning => draft.edit(),
                Control::Submit => {
                    if submit_disabled.is_none()
                        && let Some(response) = draft.response()
                    {
                        draft.unsaved = None;
                        self.submitted = Some(response);
                    }
                }
                Control::InspectSource => return Some(INSPECT_SOURCE.command),
                Control::Hint => show_hint(&mut self.picked[at], &mut self.signals),
                Control::ShowAnswer => {
                    if *kind == AttemptKind::Own {
                        *kind = AttemptKind::Assisted;
                        self.signals
                            .push((question.id().clone(), Signal::ShowAnswer));
                    }
                    return Some(Command::Go(Route::Read(Reading::Answer)));
                }
                Control::Skip => {
                    if let Some(next) = skip_to {
                        tracing::info!(question = at + 1, "skipped a question");
                        self.signals.push((question.id().clone(), Signal::Skip));
                        self.at = next;
                    }
                }
                Control::CannotAnswer => self.cannot_answer(),
                Control::Report => self.reporting = Some(Box::default()),
            },
            command => draft.focus = draft.focus.after(command, Control::ALL.len()),
        }
        None
    }

    /// Records that the question that shows can't be answered from this
    /// context, and moves on: to the next question still unattempted, or
    /// with none, to the next question, if there is one.
    fn cannot_answer(&mut self) {
        let at = self.at;
        tracing::info!(
            question = at + 1,
            "the learner can't answer a question from this context"
        );
        let id = self.picked[at].question.id().clone();
        self.signals.push((id, Signal::CannotAnswer));
        // Whether a question follows it doesn't hang on its own attempt.
        let place = self.place();
        self.picked[at].attempt = Attempt::CannotAnswer(CannotAnswer::new(place));
        if let Some(next) = self.next_unattempted(at).or(self.next_question(at)) {
            self.at = next;
        }
    }

    /// Each question, in order, as the summary lists it.
    pub(crate) fn listed(&self) -> Vec<Listed<'_>> {
        self.picked
            .iter()
            .map(|entry| Listed {
                question: &entry.question,
                area_name: &entry.area_name,
                kind: entry.kind,
                reports: entry.reports,
                attempt: match &entry.attempt {
                    Attempt::Draft(_) => Ended::Skipped,
                    Attempt::Submitted(feedback) => Ended::Answered(feedback),
                    Attempt::CannotAnswer(_) => Ended::CannotAnswer,
                },
            })
            .collect()
    }

    /// The reader Show answer opens on the question that shows: its answer
    /// key, each option's rationale, "why it matters", and each criterion
    /// of its rubric, with what meeting it and not meeting it look like.
    pub(crate) fn answer_reader(&self) -> Reader {
        let question = &self.current().question;
        let key = question.answer_key();
        let mut sections = vec![
            Section::plain(
                "Assisted attempt",
                "You chose Show answer, so this is an assisted attempt. You can still \
                 submit your response, and its outcomes are marked Assisted.",
            ),
            Section::plain("Answer key", &format!("{key}: {}", question.option(key))),
        ];
        sections.extend(OptionLetter::ALL.map(|letter| {
            Section::plain(
                &format!("Rationale for {letter}"),
                question.rationale(letter),
            )
        }));
        sections.push(Section::plain("Why it matters", question.why_it_matters()));
        let count = question.rubric().len();
        sections.extend(
            question
                .rubric()
                .iter()
                .enumerate()
                .map(|(index, criterion)| {
                    Section::plain(
                        &format!("Rubric, criterion {} of {count}", index + 1),
                        &format!(
                            "{}\nMet: {}\nNot met: {}",
                            criterion.statement(),
                            criterion.met(),
                            criterion.not_met()
                        ),
                    )
                }),
        );
        Reader::new(
            Route::Read(Reading::Answer),
            format!(
                "Answer to question {} of {}",
                self.at + 1,
                self.picked.len()
            ),
            sections,
        )
    }

    /// The reader Inspect Jev's answers opens on the question that shows, if
    /// its feedback does: each of Jev's answers about it, with the models of
    /// the `requests` that asked them.
    pub(crate) fn jev_answers(&self, requests: &[SentRequest]) -> Option<Reader> {
        let entry = self.current();
        let Attempt::Submitted(feedback) = &entry.attempt else {
            return None;
        };
        let place = format!("question {} of {}", self.at + 1, self.picked.len());
        Some(jev_answers::reader(
            &place,
            &entry.question,
            feedback,
            requests,
        ))
    }

    /// The lines that head every question: the shortfall, if there is one,
    /// offering why, and the question's place and area.
    fn head(&self, theme: &Theme) -> Vec<Line<'static>> {
        let entry = self.current();
        let mut lines = Vec::new();
        if let Some(asked_for) = self.shortfall {
            lines.push(Line::styled(
                format!(
                    "{} passed checks, {asked_for} asked for · see why in actions",
                    self.picked.len()
                ),
                theme.muted,
            ));
        }
        let mut place = vec![
            Span::styled(
                format!("Question {} of {}", self.at + 1, self.picked.len()),
                theme.title,
            ),
            Span::styled(format!(" · {}", entry.area_name), theme.muted),
        ];
        if entry.kind == AttemptKind::Assisted {
            place.push(Span::styled(feedback::ASSISTED, theme.muted));
        }
        if entry.reports > 0 {
            place.push(Span::styled(" · Reported", theme.muted));
        }
        lines.push(Line::from(place));
        lines.push(Line::default());
        lines
    }

    /// Draws the question that shows in `area` with `theme`.
    pub(crate) fn render(&self, theme: &Theme, frame: &mut Frame<'_>, area: Rect) {
        let parts = self.parts(area);
        let entry = self.current();
        if let Some(context) = parts.context {
            let focused = self.pane() != Pane::Question;
            entry.context.render(theme, frame, context, focused);
        }
        if let Some(question) = parts.question {
            let [head, body] = layout(self.shortfall, question);
            frame.render_widget(Paragraph::new(self.head(theme)), head);
            match &entry.attempt {
                Attempt::Draft(draft) => self.render_draft(draft, entry, theme, frame, body),
                Attempt::Submitted(feedback) => {
                    feedback.render(
                        theme,
                        frame,
                        body,
                        &entry.question,
                        self.place(),
                        &self.failed_requests,
                        self.pane() == Pane::Question,
                    );
                }
                Attempt::CannotAnswer(page) => {
                    page.render(theme, frame, body, &entry.question, self.place());
                }
            }
        }
        if let Some(form) = &self.reporting {
            form.render(theme, frame);
        }
        if let Some(check) = &self.finishing {
            check.render(theme, frame);
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

    /// The controls below the reasoning editor, with `draft`'s focus, drawn
    /// with `theme` in rows `width` cells wide: Submit response and Inspect
    /// source, each on its own row, then the help controls and signals,
    /// packed, then why Submit didn't save the response, if it didn't.
    fn draft_controls(&self, draft: &Draft, theme: &Theme, width: usize) -> Vec<Line<'static>> {
        let focused = self.draft_focus(draft);
        let control = |control: Control, disabled| {
            theme.control(
                control.label(),
                ControlState {
                    focused: focused == Some(control),
                    disabled,
                    ..ControlState::default()
                },
            )
        };
        let mut controls = fitted(control(Control::Submit, self.submit_disabled(draft)), width);
        controls.push(control(Control::InspectSource, None));
        let skip = self
            .skip_to(self.at)
            .is_none()
            .then_some("it's the only question");
        controls.extend(packed(
            Control::HELP
                .map(|help| match help {
                    Control::Skip => control(help, skip),
                    help => control(help, None),
                })
                .to_vec(),
            width,
        ));
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
        controls
    }

    /// The text above the reasoning editor of `entry`'s question, with the
    /// learner's `draft`, drawn with `theme` in rows `width` cells wide: the
    /// stem, the options, the hint once it is asked for, and below the
    /// first question, the chosen areas not asked about.
    fn draft_text<'a>(
        &self,
        draft: &Draft,
        entry: &'a Entry,
        theme: &Theme,
        width: usize,
    ) -> Vec<Line<'a>> {
        let question = &entry.question;
        let mut lines: Vec<Line<'_>> = columns::wrap(question.stem(), width)
            .into_iter()
            .map(Line::raw)
            .collect();
        lines.push(Line::default());
        let focused = self.draft_focus(draft);
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
        lines.extend(hint_lines(entry, theme, width));
        lines.extend(self.not_asked_about_note(theme, width));
        lines
    }

    /// Draws `question` and the learner's `draft` in `area` with `theme`:
    /// the stem, the options, the reasoning editor and the controls. The
    /// editor starts at [`EDITOR_START`] rows, and while it has focus, it
    /// takes every row left.
    fn render_draft(
        &self,
        draft: &Draft,
        entry: &Entry,
        theme: &Theme,
        frame: &mut Frame<'_>,
        area: Rect,
    ) {
        let width = usize::from(area.width);
        let lines = self.draft_text(draft, entry, theme, width);
        let controls = self.draft_controls(draft, theme, width);
        let rows = |count: usize| u16::try_from(count).unwrap_or(u16::MAX);
        let free = area
            .height
            .saturating_sub(rows(lines.len() + controls.len() + 2));
        let editor_rows = if draft.editor_grows() {
            free
        } else {
            free.min(EDITOR_START)
        };
        let [text, _, editor, _, buttons] = Layout::vertical([
            Constraint::Max(rows(lines.len())),
            Constraint::Length(1),
            Constraint::Length(editor_rows.max(EDITOR_SQUEEZED)),
            Constraint::Length(1),
            Constraint::Length(rows(controls.len())),
        ])
        .areas(area);
        let focused = self.draft_focus(draft);
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
                Paragraph::new(vec![
                    Line::styled(OPTIONAL_NOTE, theme.muted),
                    Line::styled("Press i to write your reasoning.", theme.muted),
                ])
                .wrap(Wrap { trim: true }),
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

/// Shows `entry`'s hint below its options, once: a hint that passed its
/// checks is then used, which `signals` records.
fn show_hint(entry: &mut Entry, signals: &mut Vec<(CandidateId, Signal)>) {
    if entry.hint == HintState::Hidden {
        entry.hint = HintState::Shown;
        // Only a hint that passed its checks is used.
        if entry.question.hint().is_some() {
            signals.push((entry.question.id().clone(), Signal::HintUsed));
        }
    }
}

/// Below the options, the question's hint once the learner asked for it,
/// wrapped to `width`, or "No hint for this question" when its hint failed
/// its checks.
fn hint_lines(entry: &Entry, theme: &Theme, width: usize) -> Vec<Line<'static>> {
    if entry.hint == HintState::Hidden {
        return Vec::new();
    }
    let mut lines = vec![Line::default()];
    match entry.question.hint() {
        Some(hint) => lines.extend(
            columns::wrap(&format!("Hint: {}", hint.text()), width)
                .into_iter()
                .map(Line::raw),
        ),
        None => lines.push(Line::styled("No hint for this question", theme.muted)),
    }
    lines
}

/// `control`, one line, in rows `width` cells wide: whole if it fits, or
/// else its label, with why it is disabled wrapped below, under the label.
fn fitted(control: Line<'static>, width: usize) -> Vec<Line<'static>> {
    // A disabled action is its focus marker, its label and why.
    if control.width() <= width || control.spans.len() < 3 {
        return vec![control];
    }
    let style = control.style;
    let mut spans = control.spans;
    let reason = spans.split_off(2);
    let indent = spans.first().map(Span::width).unwrap_or_default();
    let why: String = reason.iter().map(|span| span.content.as_ref()).collect();
    let why_style = reason.first().map(|span| span.style).unwrap_or_default();
    let mut rows = vec![Line::from(spans).style(style)];
    rows.extend(
        columns::wrap(why.trim_start(), width.saturating_sub(indent))
            .into_iter()
            .map(|row| {
                Line::from(vec![
                    Span::raw(" ".repeat(indent)),
                    Span::styled(row, why_style),
                ])
            }),
    );
    rows
}

/// `controls`, each one line, packed in order into rows `width` cells wide,
/// two cells apart, so a row holds as many as fit.
fn packed(controls: Vec<Line<'static>>, width: usize) -> Vec<Line<'static>> {
    let mut rows: Vec<Line<'static>> = Vec::new();
    for control in controls {
        match rows.last_mut() {
            Some(row) if row.width() + 2 + control.width() <= width => {
                row.push_span(Span::raw("  "));
                row.spans.extend(control.spans);
            }
            _ => rows.push(control),
        }
    }
    rows
}

/// The fewest rows the reasoning editor's box takes, its border included:
/// room for the empty editor's two lines, that reasoning is optional and
/// how to write it.
const EDITOR_ROWS: u16 = 4;

/// The rows the reasoning editor's box keeps, its border included, when the
/// text above it leaves it no more: one row inside.
const EDITOR_SQUEEZED: u16 = 3;

/// The rows the reasoning editor's box starts at, its border included,
/// while it hasn't focus and the screen has room.
const EDITOR_START: u16 = 8;

/// The fewest rows feedback's text takes, below the code it cites, when
/// the screen is narrower than [`WIDE`].
const FEEDBACK_ROWS: usize = 4;

/// The width of the screen's area at 120 columns, from which the code a
/// question cites shows beside it: the app keeps two cells either side.
const WIDE: u16 = 116;

/// How much of the width the code beside a question takes, in percent.
const CONTEXT_PERCENT: u16 = 55;

/// The cells between the code and the question beside it.
const GAP: u16 = 2;

/// The fewest rows the code above a question takes, its header included.
const CONTEXT_ROWS: u16 = 3;

/// Where the code a question cites and the question go on the screen.
#[derive(Debug, Clone, Copy)]
struct Parts {
    /// The code, unless the page has none beside it.
    context: Option<Rect>,
    /// The question, or its feedback, with the lines heading it, unless the
    /// code shows on the whole screen.
    question: Option<Rect>,
}

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
    Layout::vertical([
        Constraint::Length(head_rows(shortfall)),
        Constraint::Fill(1),
    ])
    .areas(area)
}

/// The rows the lines heading every question take, with or without a
/// shortfall: the question's place, a blank row, and the shortfall if there
/// is one.
fn head_rows(shortfall: Option<QuestionCount>) -> u16 {
    if shortfall.is_some() { 3 } else { 2 }
}
