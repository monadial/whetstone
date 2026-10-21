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
//! stays disabled, saying what's missing, until an option is chosen and
//! reasoning is written. Each question's draft stays in memory until it is
//! submitted.
//!
//! The help controls and signals are reached by focus, like the rest, never
//! by a letter, so every key in the editor stays text (rule 29). None is
//! held against the learner:
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
//! The session record starts as the questions show, holding them and the
//! evidence they cite. Submit saves the response to it first. Then the
//! choice outcome shows at once on the question's feedback, the response is
//! final, and Jev is asked about its reasoning with the store's
//! acknowledgement (rule 39). Its result applies to the response it names,
//! even once another question shows, and its outcomes are saved to the
//! record. So are the generator's explanation of what the reasoning missed
//! and Jev's check of it, asked with the same acknowledgement.
//!
//! When fewer questions passed their checks than were asked for, "*n*
//! passed checks, *N* asked for" heads them, so the learner knows nothing
//! unchecked filled the gap. Below the first question, one line names any
//! chosen area that wasn't asked about, because the relevance gate found it
//! not worth asking about in this change.

use std::mem;

use ratatui::Frame;
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, Padding, Paragraph};
use whetstone_ai::{ParagraphCheck, RequestFailure, WrittenParagraph};
use whetstone_core::{
    Area, CandidateId, CriterionOutcome, Evidence, OptionLetter, Picked, Probability, Provenance,
    Question, QuestionCount, Response,
};

use crate::cannot_answer::{self, CannotAnswer};
use crate::columns;
use crate::failures::FailedRequests;
use crate::feedback::{self, AttemptKind, Feedback, Place, Step};
use crate::finish::{self, Drafts, FinishCheck, Finishing};
use crate::focus::Position;
use crate::keymap::{self, Binding, Command, Input, Keymap, Press};
use crate::palette::Action;
use crate::reader::{Reader, Section};
use crate::reasoning_editor::{self, Caret, Editing, ReasoningEditor};
use crate::report::{self, ReportForm, Reporting};
use crate::session_record::{
    Assessment, NewRecord, SavedResponse, SessionId, SessionStore, Signal, StoreError,
};
use crate::summary::{Ended, Listed};
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
    FINISH,
    Action {
        label: "Back",
        does: "Go back to the areas and changes",
        command: Command::Back,
    },
];

/// The actions in the palette on feedback.
const FEEDBACK_ACTIONS: &[Action] = &[INSPECT_SOURCE, FINISH];

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
    finishing: Option<Box<FinishCheck>>,
    /// The saved responses whose evaluation the learner cancelled, for the
    /// screen to stop.
    cancelled: Vec<SavedResponse>,
    /// What the learner can do about a request about a response that failed.
    failed_requests: FailedRequests,
}

impl Questions {
    /// The questions `picked`, about the chosen `areas`, showing the first,
    /// or `None` when there are none to show. A request about a response
    /// that fails says what to do as `failed_requests` words it.
    pub(crate) fn new(
        picked: Picked,
        areas: &[Area],
        failed_requests: FailedRequests,
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
                    question,
                    attempt: Attempt::Draft(Draft::default()),
                    hint: HintState::Hidden,
                    kind: AttemptKind::Own,
                    reports: 0,
                })
                .collect(),
            shortfall,
            not_asked_about: Vec::new(),
            at: 0,
            record: Record::ToStart,
            submitted: None,
            signals: Vec::new(),
            reporting: None,
            finishing: None,
            cancelled: Vec::new(),
            failed_requests,
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
        match self.current().attempt {
            Attempt::Draft(_) => ACTIONS,
            Attempt::Submitted(_) | Attempt::CannotAnswer(_) => FEEDBACK_ACTIONS,
        }
    }

    /// Text while the reasoning is edited, commands otherwise.
    pub(crate) fn input(&self) -> Input {
        if self.finishing.is_some() {
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
    /// `evidence`, the evidence exactly as sent, and the `provenance` of
    /// every request so far, unless it started. Then saves a response the
    /// learner just submitted, before anything else is done with it, and
    /// shows its choice outcome at once.
    pub(crate) fn keep_record(
        &mut self,
        store: &mut dyn SessionStore,
        evidence: &Evidence,
        provenance: &[Provenance],
    ) {
        if let Record::ToStart = self.record {
            let record = NewRecord {
                questions: self
                    .picked
                    .iter()
                    .map(|entry| entry.question.clone())
                    .collect(),
                evidence: evidence.clone(),
                provenance: provenance.to_vec(),
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
    /// asked for: the store's acknowledgement of the response, which holds
    /// the reasoning as saved, and its question. Each then awaits its
    /// result.
    pub(crate) fn evaluate(&mut self, mut evaluate: impl FnMut(&SavedResponse, &Question)) {
        for entry in &mut self.picked {
            if let Attempt::Submitted(feedback) = &mut entry.attempt
                && let Some(saved) = feedback.take_evaluation()
            {
                evaluate(saved, &entry.question);
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
    /// asked for: the store's acknowledgement of the response, its
    /// question, and the criteria to explain, each with its outcome. They
    /// are then being written.
    pub(crate) fn explain(
        &mut self,
        mut explain: impl FnMut(&SavedResponse, &Question, &[(usize, CriterionOutcome)]),
    ) {
        for entry in &mut self.picked {
            if let Attempt::Submitted(feedback) = &mut entry.attempt
                && let Some((saved, criteria)) = feedback.take_explanation()
            {
                explain(saved, &entry.question, &criteria);
            }
        }
    }

    /// Hands `check` each explanation that waits for Jev's check: the
    /// store's acknowledgement of the response, its question, and the
    /// paragraphs to check. They are then being checked.
    pub(crate) fn check_explanations(
        &mut self,
        mut check: impl FnMut(&SavedResponse, &Question, &[WrittenParagraph]),
    ) {
        for entry in &mut self.picked {
            if let Attempt::Submitted(feedback) = &mut entry.attempt
                && let Some((saved, paragraphs)) = feedback.take_explanation_check()
            {
                check(saved, &entry.question, &paragraphs);
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
        if let Some((_, feedback)) = self.feedback_on(saved) {
            feedback.explanation_checked(reply);
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
        draft.response().err().map(Missing::reason)
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
        let place = self.place();
        let Entry {
            question, attempt, ..
        } = &mut self.picked[self.at];
        let step = match attempt {
            Attempt::Draft(_) => return self.apply_draft(command, area),
            Attempt::Submitted(feedback) => {
                let [_, body] = layout(self.shortfall, area);
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
            Step::Finish => self.finish(),
            Step::Report => {
                self.reporting = Some(Box::default());
                None
            }
            Step::CancelEvaluation => None,
        }
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
            Drafts::None
        };
        self.finishing = Some(Box::new(FinishCheck::new(
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
        let at = self.at;
        let skip_to = self.skip_to(at);
        let submit_disabled = match &self.picked[at].attempt {
            Attempt::Draft(draft) => self.submit_disabled(draft),
            Attempt::Submitted(_) | Attempt::CannotAnswer(_) => None,
        };
        let Entry {
            question,
            attempt: Attempt::Draft(draft),
            hint,
            kind,
            ..
        } = &mut self.picked[at]
        else {
            return None;
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
                Control::Hint => {
                    if *hint == HintState::Hidden {
                        *hint = HintState::Shown;
                        // Only a hint that passed its checks is used.
                        if question.hint().is_some() {
                            self.signals.push((question.id().clone(), Signal::HintUsed));
                        }
                    }
                }
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
        let [head, body] = layout(self.shortfall, area);
        frame.render_widget(Paragraph::new(self.head(theme)), head);
        let entry = self.current();
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
                );
            }
            Attempt::CannotAnswer(page) => {
                page.render(theme, frame, body, &entry.question, self.place());
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
        let focused = (draft.mode == Mode::Moving).then(|| draft.focused());
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
        let mut controls = vec![
            control(Control::Submit, self.submit_disabled(draft)),
            control(Control::InspectSource, None),
        ];
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

    /// Draws `question` and the learner's `draft` in `area` with `theme`:
    /// the stem, the options, the reasoning editor and the controls.
    fn render_draft(
        &self,
        draft: &Draft,
        entry: &Entry,
        theme: &Theme,
        frame: &mut Frame<'_>,
        area: Rect,
    ) {
        let question = &entry.question;
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
        lines.extend(hint_lines(entry, theme, width));
        lines.extend(self.not_asked_about_note(theme, width));
        let controls = self.draft_controls(draft, theme, width);
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
