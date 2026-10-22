//! The session record: a Practice's questions, the evidence they cite, the
//! learner's responses and every outcome, with every request sent to Jev
//! and the generator and what produced its result, kept through one port,
//! [`SessionStore`], which the app holds.
//!
//! A response is saved before anything else is done with it, and the store
//! acknowledges it with a [`SavedResponse`], which only a store can make. A
//! paid evaluation of the reasoning takes that acknowledgement with the
//! reasoning written ([`SavedReasoning`]), so the compiler keeps an
//! evaluation from being requested for a response that could still be lost,
//! or that has no reasoning to assess (rule 39).
//!
//! [`InMemoryStore`] keeps records for as long as Whetstone runs: for tests,
//! and for the app until the durable store arrives (#6). It is not durable,
//! so "Saved" never shows with it (rule 40).

use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use whetstone_ai::{ParagraphCheck, WrittenParagraph};
use whetstone_core::{
    AppliedCutOffs, Assessed, CandidateId, ChoiceOutcome, CriterionOutcome, Evidence, Question,
    Response, WrittenReasoning,
};

use crate::sent_requests::SentRequest;

/// Where the app keeps session records. Each save either returns once the
/// store holds what it saved, or fails and holds nothing new.
pub trait SessionStore: fmt::Debug {
    /// Whether what the store saves outlives Whetstone, which decides
    /// whether the UI may show "Saved".
    fn durability(&self) -> Durability;

    /// Starts the record of a new Practice, once its questions pass their
    /// checks, and returns its identity.
    fn start(&mut self, record: NewRecord) -> Result<SessionId, StoreError>;

    /// Saves `response`, the learner's response to `question` in the record
    /// of `session`, and acknowledges it. Each response saved to a question
    /// is a new attempt at it, the first numbered 1.
    fn save_response(
        &mut self,
        session: SessionId,
        question: &CandidateId,
        response: Response,
    ) -> Result<SavedResponse, StoreError>;

    /// Saves `assessment`, an outcome of the response the store
    /// acknowledged with `saved`.
    fn save_assessment(
        &mut self,
        saved: &SavedResponse,
        assessment: Assessment,
    ) -> Result<(), StoreError>;

    /// Saves `signal`, which the learner gave about `question` in the record
    /// of `session`.
    fn save_signal(
        &mut self,
        session: SessionId,
        question: &CandidateId,
        signal: Signal,
    ) -> Result<(), StoreError>;

    /// Saves `request`, a request to Jev or the generator that ended after
    /// the record of `session` started, with what produced its result.
    fn save_request(&mut self, session: SessionId, request: SentRequest) -> Result<(), StoreError>;
}

/// Whether what a store saves outlives Whetstone, so the UI can tell
/// whether to show "Saved".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Durability {
    /// Kept only in memory, and lost when Whetstone closes: "Saved" never
    /// shows.
    InMemory,
    /// Written durably before each save returns: "Saved" may show once it
    /// has (rule 40).
    Durable,
}

/// What a record starts with.
#[derive(Debug, Clone, PartialEq)]
pub struct NewRecord {
    /// The questions that passed their checks, in the order they are asked.
    pub questions: Vec<Question>,
    /// The evidence the questions cite, exactly as it was sent.
    pub evidence: Evidence,
    /// Every request that ended so far, in the order they ended: the
    /// relevance gate, writing and each call of each question check, with
    /// what produced each result.
    pub requests: Vec<SentRequest>,
}

/// A session record's identity, which the store gives it when it starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(u64);

/// Which attempt at a question a response is: the first is 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AttemptId(u32);

/// A store's acknowledgement that it saved a response: which session record
/// holds it, which question it responds to, which attempt at that question
/// it is, and the response, as saved, its reasoning written or not.
///
/// Only a store makes one, in [`SessionStore::save_response`], so a
/// reasoning evaluation, which takes one with its reasoning written
/// ([`SavedReasoning`]) and sends that reasoning, can be requested only for
/// a saved response (rule 39). It also names that
/// evaluation's result, so a result for another session, question or
/// attempt is never applied (rule 24): it can be the key a screen waits for
/// the result with.
///
/// Its `Debug` output shows its identities, never the response.
///
/// Nothing outside a store can make one:
///
/// ```compile_fail,E0451
/// use whetstone::{AttemptId, SavedResponse, SessionId};
/// use whetstone_core::{CandidateId, Response};
///
/// fn forge(
///     session: SessionId,
///     question: CandidateId,
///     attempt: AttemptId,
///     response: Response,
/// ) -> SavedResponse {
///     SavedResponse { session, question, attempt, response }
/// }
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct SavedResponse {
    session: SessionId,
    question: CandidateId,
    attempt: AttemptId,
    response: Response,
}

/// A store's acknowledgement of a saved response whose reasoning is
/// written, and that reasoning: what a reasoning evaluation, the
/// explanation of what the reasoning missed and its check are asked with
/// (rule 39). A response with no reasoning written has none, so nothing is
/// ever asked about its reasoning.
///
/// Only [`SavedResponse::written_reasoning`] makes one.
///
/// Its `Debug` output is the acknowledgement's: its identities, never the
/// response.
///
/// Nothing else can make one:
///
/// ```compile_fail,E0451
/// use whetstone::{SavedReasoning, SavedResponse};
/// use whetstone_core::WrittenReasoning;
///
/// fn forge<'a>(saved: &'a SavedResponse, reasoning: &'a WrittenReasoning) -> SavedReasoning<'a> {
///     SavedReasoning { saved, reasoning }
/// }
/// ```
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SavedReasoning<'a> {
    saved: &'a SavedResponse,
    reasoning: &'a WrittenReasoning,
}

/// An outcome of a saved response. Each kind of outcome a later slice
/// saves is a variant of its own.
#[derive(Debug, Clone, PartialEq)]
pub enum Assessment {
    /// The choice's outcome, at once at Submit, from the option ratings.
    Choice(Assessed<ChoiceOutcome>),
    /// One criterion's outcome, from Jev's assessment of the reasoning.
    Criterion {
        /// The criterion's place in the question's rubric, counting from 0.
        criterion: usize,
        /// Met, not met or needs review, with Jev's answer.
        outcome: Assessed<CriterionOutcome>,
    },
    /// A paragraph of the explanation of what the reasoning missed, as the
    /// generator wrote it, and what its checks made of it. Every paragraph
    /// checked is saved, shown or withheld, including each Retry's.
    Explanation {
        /// The paragraph: the criterion it explains, that criterion's
        /// outcome, its text and its citations.
        paragraph: WrittenParagraph,
        /// Whether it passed its checks, and so showed.
        check: ExplanationCheck,
    },
}

/// What the checks made of a paragraph of an explanation: plain code
/// checks its citations, then Jev checks it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ExplanationCheck {
    /// Jev's answers were clear yes on both, under `cut_offs`: it showed.
    Passed {
        /// Jev's answers, as returned.
        answers: ParagraphCheck,
        /// The cut-offs that banded them, with their policy's version and
        /// whether they are its defaults or the learner's own.
        cut_offs: AppliedCutOffs,
    },
    /// Jev's answers weren't clear yes on both, under `cut_offs`: it was
    /// withheld.
    Failed {
        /// Jev's answers, as returned.
        answers: ParagraphCheck,
        /// The cut-offs that banded them, with their policy's version and
        /// whether they are its defaults or the learner's own.
        cut_offs: AppliedCutOffs,
    },
    /// Plain code withheld it before Jev was asked: it has no text or no
    /// citation, or a citation names lines the evidence doesn't send.
    FailedPlainChecks,
}

/// What the learner signalled about a question. None is held against them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signal {
    /// They opened the question's hint. The attempt stays their own.
    HintUsed,
    /// They chose Show answer, which makes the attempt an assisted attempt.
    ShowAnswer,
    /// They skipped the question, leaving it unattempted for now.
    Skip,
    /// They chose Cannot answer from this context: the evidence isn't
    /// enough to answer it.
    CannotAnswer,
    /// They reported it.
    Report(Report),
}

/// A learner's claim that a question may be a hallucination, mistaken or
/// ambiguous, with an optional note. It is not a confirmed defect, and it
/// never changes an outcome.
///
/// Its `Debug` output shows whether there is a note, never the note.
#[derive(Clone, PartialEq, Eq)]
pub struct Report {
    kind: ReportKind,
    note: Option<String>,
}

/// What a report claims.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportKind {
    /// It states something the evidence doesn't hold.
    Hallucination,
    /// It is wrong.
    Mistake,
    /// It can be read more than one way.
    Ambiguity,
}

/// Why a store could not save.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    /// The store started no record with this identity.
    #[error("no session record {0} was started")]
    UnknownSession(SessionId),
    /// The record holds no such question.
    #[error("session record {0} has no such question")]
    UnknownQuestion(SessionId),
    /// The record holds no such response.
    #[error("session record {0} has no such response")]
    UnknownResponse(SessionId),
}

/// One operation in an [`InMemoryStore`]'s log: what it saved.
#[derive(Debug, Clone, PartialEq)]
pub enum StoreEntry {
    /// A record started.
    Started {
        session: SessionId,
        record: NewRecord,
    },
    /// A response was saved.
    Response {
        saved: SavedResponse,
        response: Response,
    },
    /// An outcome of a saved response was saved.
    Assessment {
        saved: SavedResponse,
        assessment: Assessment,
    },
    /// A signal about a question was saved.
    Signal {
        session: SessionId,
        question: CandidateId,
        signal: Signal,
    },
    /// A request that ended after the record started was saved.
    Request {
        session: SessionId,
        request: SentRequest,
    },
}

/// A store that keeps records in memory, for as long as Whetstone runs. It
/// is not durable, so "Saved" never shows with it.
///
/// It keeps a log of every operation that saved something, in order. Its
/// clones share one log, so a test can keep one and give the app another,
/// then read what the app saved and when: a response saved before its
/// evaluation was requested is in the log by the time the request is made.
#[derive(Debug, Clone, Default)]
pub struct InMemoryStore {
    log: Arc<Mutex<Vec<StoreEntry>>>,
}

impl InMemoryStore {
    /// A store with no records.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Every operation that saved something so far, in order.
    #[must_use]
    pub fn log(&self) -> Vec<StoreEntry> {
        self.entries().clone()
    }

    fn entries(&self) -> MutexGuard<'_, Vec<StoreEntry>> {
        // The log is only ever pushed to, so it is whole even if a thread
        // panicked while holding it.
        self.log.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The record of `session` in `log`, if it started.
fn started(log: &[StoreEntry], session: SessionId) -> Result<&NewRecord, StoreError> {
    log.iter()
        .find_map(|entry| match entry {
            StoreEntry::Started {
                session: started,
                record,
            } if *started == session => Some(record),
            _ => None,
        })
        .ok_or(StoreError::UnknownSession(session))
}

/// Whether the record of `session` in `log` asks `question`.
fn check_question(
    log: &[StoreEntry],
    session: SessionId,
    question: &CandidateId,
) -> Result<(), StoreError> {
    let questions = &started(log, session)?.questions;
    if questions.iter().any(|asked| asked.id() == question) {
        Ok(())
    } else {
        Err(StoreError::UnknownQuestion(session))
    }
}

impl SessionStore for InMemoryStore {
    fn durability(&self) -> Durability {
        Durability::InMemory
    }

    fn start(&mut self, record: NewRecord) -> Result<SessionId, StoreError> {
        let mut log = self.entries();
        let started = log
            .iter()
            .filter(|entry| matches!(entry, StoreEntry::Started { .. }))
            .count();
        let session = SessionId(u64::try_from(started + 1).expect("records are counted in u64"));
        log.push(StoreEntry::Started { session, record });
        Ok(session)
    }

    fn save_response(
        &mut self,
        session: SessionId,
        question: &CandidateId,
        response: Response,
    ) -> Result<SavedResponse, StoreError> {
        let mut log = self.entries();
        check_question(&log, session, question)?;
        let earlier = log
            .iter()
            .filter(|entry| {
                matches!(entry, StoreEntry::Response { saved, .. }
                    if saved.session == session && saved.question == *question)
            })
            .count();
        let saved = SavedResponse {
            session,
            question: question.clone(),
            attempt: AttemptId(u32::try_from(earlier + 1).expect("attempts are counted in u32")),
            response: response.clone(),
        };
        log.push(StoreEntry::Response {
            saved: saved.clone(),
            response,
        });
        Ok(saved)
    }

    fn save_assessment(
        &mut self,
        saved: &SavedResponse,
        assessment: Assessment,
    ) -> Result<(), StoreError> {
        let mut log = self.entries();
        if !log.iter().any(|entry| {
            matches!(entry, StoreEntry::Response { saved: response, .. } if response == saved)
        }) {
            return Err(StoreError::UnknownResponse(saved.session));
        }
        log.push(StoreEntry::Assessment {
            saved: saved.clone(),
            assessment,
        });
        Ok(())
    }

    fn save_signal(
        &mut self,
        session: SessionId,
        question: &CandidateId,
        signal: Signal,
    ) -> Result<(), StoreError> {
        let mut log = self.entries();
        check_question(&log, session, question)?;
        log.push(StoreEntry::Signal {
            session,
            question: question.clone(),
            signal,
        });
        Ok(())
    }

    fn save_request(&mut self, session: SessionId, request: SentRequest) -> Result<(), StoreError> {
        let mut log = self.entries();
        started(&log, session)?;
        log.push(StoreEntry::Request { session, request });
        Ok(())
    }
}

/// The acknowledgement a reasoning evaluation is asked with, and the
/// reasoning it sends (rule 39).
impl whetstone_ai::Saved for SavedReasoning<'_> {
    fn reasoning(&self) -> &str {
        self.reasoning.as_str()
    }
}

/// Shows the acknowledgement's identities, never the response.
impl fmt::Debug for SavedReasoning<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.saved.fmt(f)
    }
}

/// Shows the identities, never the response, whose choice a reasoning
/// evaluation must never carry.
impl fmt::Debug for SavedResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SavedResponse")
            .field("session", &self.session)
            .field("question", &self.question)
            .field("attempt", &self.attempt)
            .finish_non_exhaustive()
    }
}

impl SavedResponse {
    /// The session record the response is saved in.
    #[must_use]
    pub fn session(&self) -> SessionId {
        self.session
    }

    /// The question it responds to.
    #[must_use]
    pub fn question(&self) -> &CandidateId {
        &self.question
    }

    /// Which attempt at the question it is.
    #[must_use]
    pub fn attempt(&self) -> AttemptId {
        self.attempt
    }

    /// The response, as saved.
    #[must_use]
    pub fn response(&self) -> &Response {
        &self.response
    }

    /// The acknowledgement with the response's reasoning, if it is written:
    /// what Jev is asked about the reasoning with. A response with no
    /// reasoning written has none.
    #[must_use]
    pub fn written_reasoning(&self) -> Option<SavedReasoning<'_>> {
        Some(SavedReasoning {
            saved: self,
            reasoning: self.response.reasoning().written()?,
        })
    }
}

impl<'a> SavedReasoning<'a> {
    /// The acknowledgement of the response the reasoning is in.
    #[must_use]
    pub fn saved(&self) -> &'a SavedResponse {
        self.saved
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl AttemptId {
    /// The attempt's number: 1 for the first.
    #[must_use]
    pub fn get(self) -> u32 {
        self.0
    }
}

impl Report {
    /// A report claiming `kind`, with the learner's `note`, if they wrote
    /// one.
    #[must_use]
    pub fn new(kind: ReportKind, note: Option<String>) -> Self {
        Self { kind, note }
    }

    /// What it claims.
    #[must_use]
    pub fn kind(&self) -> ReportKind {
        self.kind
    }

    /// The learner's note, as they wrote it.
    #[must_use]
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }
}

/// Shows the kind and whether there is a note, never the note.
impl fmt::Debug for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Report")
            .field("kind", &self.kind)
            .field("noted", &self.note.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use whetstone_core::{
        AreaId, AssessmentPolicy, CandidateCheck, Change, ConnectedCodeLimit, Connections, Depth,
        ExcludePatterns, OptionLetter, Probability, QuestionCheckAnswers, QuestionType,
        ReviewChoices, TouchedFile, WrittenCandidate, WrittenCitation, WrittenCriterion,
        WrittenOption, plain_checks, show_rule,
    };

    use super::*;

    /// Evidence that sends `src/pay.rs`, three lines long.
    fn evidence() -> Evidence {
        let change = Change::new(
            "Retry failed payments".to_owned(),
            Vec::new(),
            "diff --git a/src/pay.rs b/src/pay.rs\n+retry\n".to_owned(),
            vec![TouchedFile::modified(
                "src/pay.rs".to_owned(),
                "line\n".repeat(2),
                "line\n".repeat(3),
            )],
        );
        Evidence::build(
            &change,
            &ExcludePatterns::default(),
            &[],
            &Connections::NoneFound,
            ConnectedCodeLimit::effective(None, None),
            &ReviewChoices::default(),
        )
    }

    fn probability(value: f64) -> Probability {
        Probability::new(value).expect("from 0 to 1")
    }

    /// The questions that pass their checks in `area`: one per stem, citing
    /// `src/pay.rs`, with B as the answer key.
    fn questions(area: &str, stems: &[&str]) -> Vec<Question> {
        let area: AreaId = area.parse().expect("a valid area ID");
        let written = stems.iter().map(|stem| WrittenCandidate {
            question_type: QuestionType::SingleChoice,
            stem: (*stem).to_owned(),
            options: OptionLetter::ALL
                .into_iter()
                .map(|letter| WrittenOption {
                    letter: letter.to_string(),
                    text: format!("Option {letter}"),
                    rationale: format!("Rationale {letter}"),
                })
                .collect(),
            answer_key: "B".to_owned(),
            why_it_matters: "A retry must not charge twice.".to_owned(),
            rubric: vec![WrittenCriterion {
                statement: "Links the key to deduplication".to_owned(),
                met: "Says the provider deduplicates by key".to_owned(),
                not_met: "Doesn't mention deduplication".to_owned(),
            }],
            hint: "Look at how the key is made.".to_owned(),
            citations: vec![WrittenCitation {
                path: "src/pay.rs".to_owned(),
                first_line: 1,
                last_line: 3,
            }],
        });
        plain_checks(&area, written.collect(), &evidence())
            .into_passed()
            .into_iter()
            .map(|candidate| {
                let answers: Vec<_> = candidate
                    .checks()
                    .into_iter()
                    .map(|check| {
                        let answer = match check {
                            CandidateCheck::Option(letter) if letter != OptionLetter::B => 0.1,
                            _ => 0.9,
                        };
                        (check, probability(answer))
                    })
                    .collect();
                let depth = Depth::new([0.2, 0.3, 0.5].map(probability), 1.3).expect("0 to 2");
                show_rule(
                    candidate,
                    QuestionCheckAnswers::new(answers, depth),
                    AssessmentPolicy::current(),
                )
                .expect("every check passes")
            })
            .collect()
    }

    fn record(questions: Vec<Question>) -> NewRecord {
        NewRecord {
            questions,
            evidence: evidence(),
            requests: Vec::new(),
        }
    }

    fn response(reasoning: &str) -> Response {
        Response::new(OptionLetter::A, reasoning.to_owned())
    }

    #[test]
    fn the_in_memory_store_is_not_durable() {
        assert_eq!(InMemoryStore::new().durability(), Durability::InMemory);
    }

    #[test]
    fn a_saved_response_names_its_session_question_and_first_attempt() {
        let questions = questions(
            "domain",
            &["Why reuse the key?", "What does the retry cap?"],
        );
        let second = questions[1].id().clone();
        let mut store = InMemoryStore::new();
        let session = store.start(record(questions.clone())).expect("started");

        let saved = store
            .save_response(session, &second, response("The key dedupes."))
            .expect("saved");

        assert_eq!(saved.session(), session);
        assert_eq!(saved.question(), &second);
        assert_eq!(saved.attempt().get(), 1);
        assert_eq!(
            store.log(),
            [
                StoreEntry::Started {
                    session,
                    record: record(questions),
                },
                StoreEntry::Response {
                    saved,
                    response: response("The key dedupes."),
                },
            ]
        );
    }

    #[test]
    fn only_a_saved_response_with_reasoning_written_can_have_its_reasoning_assessed() {
        use whetstone_ai::Saved;

        let asked = questions("domain", &["Why reuse the key?"]);
        let mut store = InMemoryStore::new();
        let session = store.start(record(asked.clone())).expect("started");
        let mut save = |reasoning| {
            store
                .save_response(session, asked[0].id(), response(reasoning))
                .expect("saved")
        };
        let written = save("The key dedupes.");
        let not_written = save(" \n\t");

        let reasoning = written
            .written_reasoning()
            .expect("its reasoning is written");
        assert_eq!(reasoning.reasoning(), "The key dedupes.");
        assert_eq!(reasoning.saved(), &written);
        assert_eq!(format!("{reasoning:?}"), format!("{written:?}"));
        assert!(not_written.written_reasoning().is_none());
    }

    #[test]
    fn each_record_is_its_own_session_and_each_response_to_a_question_its_next_attempt() {
        let asked = questions("domain", &["Why reuse the key?"]);
        let question = asked[0].id();
        let mut store = InMemoryStore::new();
        let first = store.start(record(asked.clone())).expect("started");
        let second = store.start(record(asked.clone())).expect("started");

        let attempts = [first, first, second].map(|session| {
            store
                .save_response(session, question, response("Dedupes."))
                .expect("saved")
        });

        assert_ne!(first, second);
        assert_eq!(
            attempts
                .each_ref()
                .map(|saved| (saved.session(), saved.attempt().get())),
            [(first, 1), (first, 2), (second, 1)]
        );
    }

    #[test]
    fn a_response_is_saved_only_to_a_question_in_a_started_record() {
        let asked = questions("domain", &["Why reuse the key?"]);
        let not_asked = questions("observability", &["What is logged?"]);
        let mut elsewhere = InMemoryStore::new();
        let session = elsewhere.start(record(asked.clone())).expect("started");
        let mut store = InMemoryStore::new();

        assert_eq!(
            store.save_response(session, asked[0].id(), response("Dedupes.")),
            Err(StoreError::UnknownSession(session))
        );
        assert_eq!(store.log(), []);

        assert_eq!(
            store.save_signal(session, asked[0].id(), Signal::Skip),
            Err(StoreError::UnknownSession(session))
        );
        assert_eq!(store.log(), []);

        let session = store.start(record(asked)).expect("started");
        assert_eq!(
            store.save_response(session, not_asked[0].id(), response("Dedupes.")),
            Err(StoreError::UnknownQuestion(session))
        );
        assert_eq!(
            store.save_signal(session, not_asked[0].id(), Signal::Skip),
            Err(StoreError::UnknownQuestion(session))
        );
        assert_eq!(store.log().len(), 1, "only the start");
    }

    #[test]
    fn the_log_keeps_every_operation_in_order() {
        let asked = questions(
            "domain",
            &["Why reuse the key?", "What does the retry cap?"],
        );
        let [first, second] = [asked[0].clone(), asked[1].clone()];
        let mut store = InMemoryStore::new();
        let session = store.start(record(asked.clone())).expect("started");
        let report = Report::new(ReportKind::Ambiguity, Some("B and C agree.".to_owned()));

        for signal in [Signal::HintUsed, Signal::ShowAnswer] {
            store
                .save_signal(session, first.id(), signal)
                .expect("saved");
        }
        let saved = store
            .save_response(session, first.id(), response("Dedupes."))
            .expect("saved");
        let choice = first.choice_outcome(OptionLetter::A);
        store
            .save_assessment(&saved, Assessment::Choice(choice))
            .expect("saved");
        for signal in [
            Signal::Report(report.clone()),
            Signal::Skip,
            Signal::CannotAnswer,
        ] {
            store
                .save_signal(session, second.id(), signal)
                .expect("saved");
        }

        let signal = |question: &Question, signal| StoreEntry::Signal {
            session,
            question: question.id().clone(),
            signal,
        };
        assert_eq!(
            store.log(),
            [
                StoreEntry::Started {
                    session,
                    record: record(asked),
                },
                signal(&first, Signal::HintUsed),
                signal(&first, Signal::ShowAnswer),
                StoreEntry::Response {
                    saved: saved.clone(),
                    response: response("Dedupes."),
                },
                StoreEntry::Assessment {
                    saved,
                    assessment: Assessment::Choice(choice),
                },
                signal(&second, Signal::Report(report)),
                signal(&second, Signal::Skip),
                signal(&second, Signal::CannotAnswer),
            ]
        );
    }

    #[test]
    fn an_assessment_is_saved_only_to_a_response_the_store_saved() {
        let asked = questions("domain", &["Why reuse the key?"]);
        let mut elsewhere = InMemoryStore::new();
        let session = elsewhere.start(record(asked.clone())).expect("started");
        let saved = elsewhere
            .save_response(session, asked[0].id(), response("Dedupes."))
            .expect("saved");
        let choice = Assessment::Choice(asked[0].choice_outcome(OptionLetter::B));
        let mut store = InMemoryStore::new();
        store.start(record(asked)).expect("started");

        assert_eq!(
            store.save_assessment(&saved, choice),
            Err(StoreError::UnknownResponse(session))
        );
        assert_eq!(store.log().len(), 1, "only the start");
    }

    #[test]
    fn a_reports_note_never_shows_in_debug_output() {
        let report = Report::new(
            ReportKind::Hallucination,
            Some("No retry cap exists.".to_owned()),
        );

        let debug = format!("{:?}", Signal::Report(report.clone()));

        assert_eq!(report.kind(), ReportKind::Hallucination);
        assert_eq!(report.note(), Some("No retry cap exists."));
        assert!(!debug.contains("retry cap"), "{debug}");
        assert!(debug.contains("Hallucination"), "{debug}");
    }

    #[test]
    fn clones_share_one_store_so_a_test_reads_what_the_app_saved() {
        let store = InMemoryStore::new();
        let mut held: Box<dyn SessionStore> = Box::new(store.clone());

        let session = held
            .start(record(questions("domain", &["Why reuse the key?"])))
            .expect("started");

        assert!(matches!(
            store.log().as_slice(),
            [StoreEntry::Started { session: started, .. }] if *started == session
        ));
    }
}
