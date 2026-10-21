//! Practice on a change: Preparing questions, then the questions, then,
//! once the learner finishes, the summary.
//!
//! On the showcase project, Generate questions opens it. Preparing
//! questions finds the change's connected code the way the outbound review
//! finds a commit's, and prepares the evidence from it. On the learner's
//! repository, Send on the outbound review opens it in the review's place
//! with the evidence the learner approved there, and every request carries
//! exactly that evidence: a Retry or an Ask anyway needs no new review.
//!
//! Then Jev's relevance gate decides, in one call, which chosen areas the
//! change is worth asking about: an unclear verdict lets an area through, and only a
//! clear no drops it. The generator writes candidates, one request per area
//! that passed, all at once; plain checks drop the malformed ones; Jev
//! checks each candidate left, all at once; the show rule keeps the
//! clear-cut ones; and picking chooses up to `questions` of them for the
//! questions screen. The stage row shows where it is. On the questions
//! screen, and on feedback, Inspect source reads the evidence the questions
//! rest on, exactly as it was sent. Once a response is saved, Jev assesses
//! its reasoning, one request per response, and Cancel evaluation stops that
//! request alone. When a criterion isn't met or needs review, the generator
//! explains it, one request per response, and Jev checks that explanation,
//! one more.
//!
//! When the gate drops every chosen area, Nothing to practice in this change
//! gives each its verdict and offers Ask anyway, which writes about every
//! chosen area without the gate; their questions are still checked. Retry on
//! No questions passed checks keeps the gate's decision.
//!
//! No questions passed checks says why in words, and so does the
//! shortfall's See why fewer passed: Practice keeps, for the candidates
//! written last, the plain checks' drops and each candidate the question
//! check withheld, with Jev's answers (see [`crate::withheld`]).
//!
//! A failed request stops its stage, which names itself and offers Retry,
//! which asks again only what failed, and Back. Nothing is retried
//! automatically. Cancel, Back or going home cancels every request still
//! running, keeping each as cancelled, and a result that comes after changes
//! nothing (rule 24). Finding
//! connected code runs on instead, as it does for the outbound review, so
//! its graph is cached.
//!
//! Every request is stamped as it is sent with its own identity and the
//! versions behind it, and kept as it ends, with its reply's provenance,
//! or, when it failed or the learner cancelled it, with provenance that
//! reports no cost, and how it ended. The session record starts with them,
//! each request that ends later is saved to it as it ends, and the summary
//! adds up the session's cost from them (see [`crate::summary`]). Once the
//! learner finishes, Practice asks for nothing more: the requests still
//! running run on, and their results still apply.

use std::convert::Infallible;
use std::mem;
use std::sync::Arc;
use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::{List, Paragraph};
use whetstone_ai::{Ai, CandidateCount, Pending, QuestionWithoutKey, Reply, RequestFailure, Stamp};
use whetstone_core::{
    Area, AreaId, AssessmentPolicy, Band, Candidate, CandidateCheck, CandidateId, Change, CommitId,
    ConnectedCodeLimit, Connections, ConnectionsState, Evidence, ExcludePatterns, Job, Probability,
    QuestionCheckAnswers, QuestionCount, RepositoryConfig, ReviewChoices, Role, Side, Withheld,
    WrittenCandidate, pick, plain_checks, show_rule,
};

use crate::area_files;
use crate::columns;
use crate::connected_code::{AnalysisDirs, GraphCache, ShowcaseTree};
use crate::failures::FailedRequests;
use crate::focus::Position;
use crate::job::{JobId, JobOutput, Jobs, Waiting};
use crate::keymap::{self, Binding, Command, Input, Keymap};
use crate::palette::Action;
use crate::questions::Questions;
use crate::reader::{Reader, Section};
use crate::sent_requests::{Asking, SentRequests, Spending};
use crate::session_record::SessionStore;
use crate::stage_row::{PracticeStage, Stage as RowStage, StageRow, StageState};
use crate::summary::{self, Summary};
use crate::theme::{ControlState, Theme};
use crate::view::{ChangeIndex, ChangeRef, Context, Reading, Route, View};
use crate::withheld::WhyWithheld;

/// Preparing questions' own keys: `q` and Esc cancel.
const KEYMAP: Keymap = &[
    keymap::MOVE,
    keymap::ACTIVATE,
    Binding {
        does: "Cancel, and go back to the areas and changes",
        hint: Some("q cancel"),
        ..keymap::BACK
    },
];

/// The keys once a request failed, no area was worth asking about, or no
/// question passed: `q` and Esc go back, stopping any request still running.
const BACK_KEYMAP: Keymap = &[
    keymap::MOVE,
    keymap::ACTIVATE,
    Binding {
        does: "Go back to the areas and changes",
        ..keymap::BACK
    },
];

/// Preparing questions' own action in the palette.
const ACTIONS: &[Action] = &[Action {
    label: "Cancel",
    does: "Stop preparing questions, and go back to the areas and changes",
    command: Command::Back,
}];

/// The action in the palette once a request failed, and on Nothing to
/// practice in this change and No questions passed checks.
const BACK_ACTIONS: &[Action] = &[Action {
    label: "Back",
    does: "Go back to the areas and changes",
    command: Command::Back,
}];

/// Practice on one change, from Preparing questions to the questions.
#[derive(Debug)]
pub(crate) struct Practice {
    /// The change, as its route names it.
    change_ref: ChangeRef,
    change: Change,
    /// The chosen areas, in the order offered.
    areas: Vec<Area>,
    /// How many questions the learner asked for.
    count: QuestionCount,
    ai: Arc<dyn Ai>,
    /// What the learner can do about a failed request, and the models
    /// `OpenRouter` refused.
    failed_requests: FailedRequests,
    connected_code_limit: ConnectedCodeLimit,
    /// How long it waits for connected code.
    time_limit: Duration,
    /// The evidence, prepared once connected code is found or given up on:
    /// none only while it is found.
    evidence: Option<Evidence>,
    /// What the relevance gate decided about the chosen areas.
    gate: Gate,
    /// Why the candidates written last weren't shown: the plain checks'
    /// drops, and each candidate the question check withheld, with Jev's
    /// answers.
    why_withheld: WhyWithheld,
    stage: Stage,
    /// The requests to Jev and the generator it waits for, each with what
    /// it asked (rule 24).
    waiting: Waiting<Asking>,
    /// Every request sent so far, with its provenance, for the session
    /// record and the summary's cost.
    sent: SentRequests,
    focus: Position,
}

/// Finding the change's connected code.
#[derive(Debug)]
enum Lookup {
    /// Not started yet: the job starts as the screen opens.
    Requested(Arc<ShowcaseLookup>),
    /// The job finding it, which the screen waits for.
    Running(JobId),
}

/// What the job that finds a showcase change's connected code needs.
#[derive(Debug)]
struct ShowcaseLookup {
    tree: ShowcaseTree,
    position: usize,
    dirs: AnalysisDirs,
    cache: Arc<GraphCache>,
}

/// What the relevance gate decided about the chosen areas. Retry on No
/// questions passed checks keeps it, so the gate isn't asked again; only
/// the learner's Ask anyway changes it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Gate {
    /// Jev hasn't decided yet.
    Undecided,
    /// Jev decided. `dropped` are the chosen areas it clearly said aren't
    /// worth asking about, in order; every other chosen area passed.
    Decided { dropped: Vec<AreaId> },
    /// The learner chose Ask anyway: every chosen area, without the gate.
    AskedAnyway,
}

impl Gate {
    /// What Jev's `bands`, its answer for each area banded, decide about the
    /// chosen `areas`. Only a clear no drops an area: an unclear verdict lets
    /// it through, for the question check to filter.
    fn decided(areas: &[Area], bands: &[(AreaId, Band)]) -> Self {
        let dropped = areas
            .iter()
            .map(Area::id)
            .filter(|chosen| {
                bands
                    .iter()
                    .any(|(area, band)| area == *chosen && *band == Band::ClearNo)
            })
            .cloned()
            .collect();
        Self::Decided { dropped }
    }

    /// Whether the gate clearly said `area` isn't worth asking about.
    fn drops(&self, area: &Area) -> bool {
        match self {
            Self::Undecided | Self::AskedAnyway => false,
            Self::Decided { dropped } => dropped.contains(area.id()),
        }
    }

    /// Of the chosen `areas`, those the gate clearly said aren't worth
    /// asking about, in order.
    fn dropped<'a>(&self, areas: &'a [Area]) -> Vec<&'a Area> {
        areas.iter().filter(|area| self.drops(area)).collect()
    }

    /// Of the chosen `areas`, those Practice asks about, in order: those the
    /// gate let through.
    fn asked_about<'a>(&self, areas: &'a [Area]) -> Vec<&'a Area> {
        areas.iter().filter(|area| !self.drops(area)).collect()
    }
}

/// Where Practice stands.
#[derive(Debug)]
enum Stage {
    /// Finding the change's connected code, before the evidence is
    /// prepared.
    FindingConnectedCode(Lookup),
    /// Jev's relevance gate decides which chosen areas are worth asking
    /// about. Its reply moves Practice on at once, so none is ever held.
    CheckingAreas(Request<Infallible>),
    /// Jev clearly said no chosen area is worth asking about in this change.
    NothingToPractice,
    /// The generator writes candidates about each area asked about, in area
    /// order: what passed the plain checks.
    Writing(Vec<Request<Vec<Candidate>>>),
    /// Jev checks each candidate that passed the plain checks, in the order
    /// written, area by area.
    Checking(Vec<(Candidate, Request<QuestionCheckAnswers>)>),
    /// The questions picked.
    Shown(Box<Questions>),
    /// No question passed its checks.
    NonePassed,
    /// The learner finished: the summary of the questions shows.
    Finished(Box<Summary>),
}

impl Stage {
    /// The questions, while they show or once their summary does.
    fn questions_mut(&mut self) -> Option<&mut Questions> {
        match self {
            Self::Shown(questions) => Some(questions.as_mut()),
            Self::Finished(summary) => Some(summary.questions_mut()),
            Self::FindingConnectedCode(_)
            | Self::CheckingAreas(_)
            | Self::NothingToPractice
            | Self::Writing(_)
            | Self::Checking(_)
            | Self::NonePassed => None,
        }
    }
}

/// One request to Jev or the generator, from sending it to its reply.
#[derive(Debug)]
enum Request<T> {
    /// To send, as soon as the screen starts its jobs.
    Unsent,
    /// Sent, and waited for.
    Sent,
    /// It failed, and waits for the learner's Retry.
    Failed(RequestFailure),
    /// Its reply.
    Replied(T),
}

/// The screen's controls, by what Practice stands at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    Cancel,
    Retry,
    Back,
    PickAnotherChange,
    PickOtherAreas,
    AskAnyway,
}

impl Control {
    fn label(self) -> &'static str {
        match self {
            Self::Cancel => "Cancel",
            Self::Retry => "Retry",
            Self::Back => "Back",
            Self::PickAnotherChange => "Pick another change",
            Self::PickOtherAreas => "Pick other areas",
            Self::AskAnyway => "Ask anyway",
        }
    }
}

impl<T> Request<T> {
    /// Each of `count` requests, none sent yet.
    fn all(count: usize) -> Vec<Self> {
        (0..count).map(|_| Self::Unsent).collect()
    }

    /// Why it failed, if it did.
    fn failure(&self) -> Option<RequestFailure> {
        match self {
            Self::Failed(failure) => Some(*failure),
            Self::Unsent | Self::Sent | Self::Replied(_) => None,
        }
    }

    /// Makes a failed request one to send again.
    fn retry(&mut self) {
        if let Self::Failed(_) = self {
            *self = Self::Unsent;
        }
    }

    /// Its reply, if it came.
    fn replied(self) -> Option<T> {
        match self {
            Self::Replied(reply) => Some(reply),
            Self::Unsent | Self::Sent | Self::Failed(_) => None,
        }
    }
}

/// Sends the request that asks `asking`: stamps it with `sent`, makes it
/// with `request`, starts it as a background job, whose result `output`
/// makes the app's, and waits for it as what it asked (rule 24).
fn send<T: Send + 'static>(
    jobs: &mut Jobs<JobOutput>,
    waiting: &mut Waiting<Asking>,
    sent: &mut SentRequests,
    asking: Asking,
    request: impl FnOnce(Stamp) -> Pending<T>,
    output: impl FnOnce(Result<Reply<T>, RequestFailure>) -> JobOutput + Send + 'static,
) {
    let request = request(sent.stamp(&asking));
    let job = jobs.start_async(async move { output(request.await) });
    waiting.wait_for(job, asking);
}

/// Logs which checks withheld a candidate, with the band of Jev's answer to
/// each under `policy`, or none when it went unanswered: names and bands
/// only, never the candidate's text or Jev's probabilities (rules 43, 44).
fn log_withheld(withheld: &Withheld, policy: &AssessmentPolicy) {
    let failed: Vec<(CandidateCheck, Option<Band>)> = withheld
        .failed()
        .iter()
        .map(|&check| {
            let band = withheld
                .answers()
                .get(check)
                .map(|answer| policy.band(check.job(), answer));
            (check, band)
        })
        .collect();
    tracing::info!(
        area = %withheld.id().area(),
        written = withheld.id().written(),
        failed = ?failed,
        "withheld a candidate"
    );
}

/// Starts the requests about the submitted responses to `questions`, all
/// against `evidence`, as background jobs `waiting` waits for: it stops
/// each evaluation the learner cancelled, keeping it in `sent` as a request
/// with no reply, then asks `ai` for each evaluation, explanation and
/// explanation check that waits to be asked for.
fn ask_about_responses(
    questions: &mut Questions,
    ai: &dyn Ai,
    evidence: &Evidence,
    jobs: &mut Jobs<JobOutput>,
    waiting: &mut Waiting<Asking>,
    sent: &mut SentRequests,
) {
    for saved in questions.take_cancelled() {
        tracing::info!(
            session = %saved.session(),
            attempt = saved.attempt().get(),
            "cancelled assessing the reasoning"
        );
        let asking = Asking::Evaluate(saved);
        for _ in 0..waiting.cancel_for(&asking, jobs) {
            sent.cancelled(&asking);
        }
    }
    questions.evaluate(|saved, question| {
        let key = saved.clone();
        send(
            jobs,
            waiting,
            sent,
            Asking::Evaluate(saved.clone()),
            |stamp| ai.assess_reasoning(stamp, saved, evidence, QuestionWithoutKey::from(question)),
            |reply| JobOutput::Evaluated {
                saved: Box::new(key),
                reply,
            },
        );
    });
    questions.explain(|saved, question, criteria| {
        let key = saved.clone();
        send(
            jobs,
            waiting,
            sent,
            Asking::Explain(saved.clone()),
            |stamp| ai.explain(stamp, saved, evidence, question, criteria),
            |reply| JobOutput::Explained {
                saved: Box::new(key),
                reply,
            },
        );
    });
    questions.check_explanations(|saved, question, paragraphs| {
        let key = saved.clone();
        send(
            jobs,
            waiting,
            sent,
            Asking::CheckExplanation(saved.clone()),
            |stamp| ai.check_explanation(stamp, saved, evidence, question, paragraphs),
            |reply| JobOutput::ExplanationChecked {
                saved: Box::new(key),
                reply,
            },
        );
    });
}

impl Practice {
    /// Practice on the showcase change at `index`, which is `change`, whose
    /// tree is `tree`, about the chosen `areas`, if Practice can ask Jev and
    /// the generator in `context`.
    pub(crate) fn on_showcase(
        index: ChangeIndex,
        change: Change,
        tree: ShowcaseTree,
        areas: Vec<Area>,
        context: &Context<'_>,
    ) -> Option<Self> {
        let ai = Arc::clone(context.ai?);
        let settings = context.settings;
        // The showcase project ships no area files, so its areas are
        // Whetstone's defaults.
        let sent = SentRequests::new(
            settings.models(),
            settings.assessment_policy.clone(),
            area_files::of(&areas, &[]),
            context.request_ids,
        );
        tracing::info!(
            position = index.get(),
            areas = areas.len(),
            "opened Preparing questions on a showcase change"
        );
        Some(Self {
            change_ref: ChangeRef::Showcase(index),
            change,
            areas,
            count: settings.questions.value,
            ai,
            failed_requests: FailedRequests::new(settings.models(), context.refused_models),
            // The showcase project's config sets nothing, which its tests
            // check, so only the learner's limit applies.
            connected_code_limit: settings.connected_code_limit(&RepositoryConfig::default()),
            time_limit: context.time_limit,
            evidence: None,
            gate: Gate::Undecided,
            why_withheld: WhyWithheld::default(),
            stage: Stage::FindingConnectedCode(Lookup::Requested(Arc::new(ShowcaseLookup {
                tree,
                position: index.get(),
                dirs: context.analysis.clone(),
                cache: Arc::clone(context.cache),
            }))),
            waiting: Waiting::new(),
            sent,
            focus: Position::default(),
        })
    }

    /// Practice on the commit `id` from the learner's repository, which is
    /// `change`, with `evidence`, exactly as the learner approved it in the
    /// outbound review, about the areas it holds, if Practice can ask Jev
    /// and the generator in `context`. Jev's relevance gate is asked first.
    pub(crate) fn on_repository(
        id: CommitId,
        change: Change,
        evidence: Evidence,
        context: &Context<'_>,
    ) -> Option<Self> {
        let ai = Arc::clone(context.ai?);
        tracing::info!(
            commit = id.full(),
            areas = evidence.areas().len(),
            files = evidence
                .files()
                .iter()
                .filter(|file| file.sent().is_some())
                .count(),
            connections = evidence
                .connections()
                .iter()
                .filter(|connection| connection.sent().is_some())
                .count(),
            bytes = evidence.size(),
            "opened Preparing questions on a commit, with the evidence approved"
        );
        let repository_files = context
            .repository
            .map_or(&[][..], |repository| repository.area_files());
        let sent = SentRequests::new(
            context.settings.models(),
            context.settings.assessment_policy.clone(),
            area_files::of(evidence.areas(), repository_files),
            context.request_ids,
        );
        Some(Self {
            change_ref: ChangeRef::Commit(id),
            change,
            areas: evidence.areas().to_vec(),
            count: context.settings.questions.value,
            ai,
            failed_requests: FailedRequests::new(context.settings.models(), context.refused_models),
            connected_code_limit: evidence.connected_code_limit(),
            time_limit: context.time_limit,
            evidence: Some(evidence),
            gate: Gate::Undecided,
            why_withheld: WhyWithheld::default(),
            stage: Stage::CheckingAreas(Request::Unsent),
            waiting: Waiting::new(),
            sent,
            focus: Position::default(),
        })
    }

    /// Ends finding connected code with `connections`, or why there are
    /// none, and prepares the evidence from them, for the generator to
    /// write from.
    fn prepare_evidence(&mut self, connections: &Connections) {
        let evidence = Evidence::build(
            &self.change,
            &ExcludePatterns::default(),
            &self.areas,
            connections,
            self.connected_code_limit,
            &ReviewChoices::default(),
        );
        tracing::info!(
            connections = evidence.connections().len(),
            bytes = evidence.size(),
            "prepared the evidence for Preparing questions"
        );
        self.evidence = Some(evidence);
        self.check_areas();
    }

    /// Asks Jev's relevance gate which chosen areas are worth asking about.
    fn check_areas(&mut self) {
        self.stage = Stage::CheckingAreas(Request::Unsent);
        self.focus = Position::default();
    }

    /// Applies Jev's `reply` to the relevance gate, or why the request
    /// failed: the generator then writes about the areas that passed.
    fn areas_checked(&mut self, reply: Result<Vec<(AreaId, Probability)>, RequestFailure>) {
        let Stage::CheckingAreas(gate) = &mut self.stage else {
            return;
        };
        match reply {
            Ok(answers) => {
                let bands: Vec<(AreaId, Band)> = answers
                    .into_iter()
                    .map(|(area, answer)| {
                        (area, self.sent.policy().band(Job::RelevanceGate, answer))
                    })
                    .collect();
                tracing::info!(bands = ?bands, "the relevance gate decided");
                self.gate = Gate::decided(&self.areas, &bands);
                if self.gate.asked_about(&self.areas).is_empty() {
                    self.stage = Stage::NothingToPractice;
                    self.focus = Position::default();
                } else {
                    self.write();
                }
            }
            Err(failure) => {
                tracing::info!(%failure, "checking the areas failed");
                self.failed_requests.note_refusal(Role::Jev, failure);
                *gate = Request::Failed(failure);
            }
        }
    }

    /// Skips the relevance gate, at the learner's word, and asks the
    /// generator about every chosen area. Their questions are still checked.
    fn ask_anyway(&mut self) {
        tracing::info!(
            areas = self.areas.len(),
            "asking anyway about the areas the relevance gate dropped"
        );
        self.gate = Gate::AskedAnyway;
        self.write();
    }

    /// Asks the generator for candidates about every area asked about.
    fn write(&mut self) {
        self.why_withheld = WhyWithheld::default();
        self.stage = Stage::Writing(Request::all(self.gate.asked_about(&self.areas).len()));
        self.focus = Position::default();
    }

    /// Applies the generator's `reply` about `area`: the candidates that
    /// pass the plain checks, or why the request failed. Once every area
    /// has its candidates, Jev checks them.
    fn written(&mut self, area: &AreaId, reply: Result<Vec<WrittenCandidate>, RequestFailure>) {
        let (Stage::Writing(writes), Some(evidence)) = (&mut self.stage, &self.evidence) else {
            return;
        };
        let Some(write) = self
            .gate
            .asked_about(&self.areas)
            .iter()
            .position(|chosen| chosen.id() == area)
            .and_then(|at| writes.get_mut(at))
        else {
            return;
        };
        *write = match reply {
            Ok(candidates) => {
                let written = candidates.len();
                let checked = plain_checks(area, candidates, evidence);
                tracing::info!(
                    area = %area,
                    written,
                    dropped = checked.dropped().len(),
                    "the generator wrote candidates"
                );
                self.why_withheld.written(written, checked.dropped());
                Request::Replied(checked.into_passed())
            }
            Err(failure) => {
                tracing::info!(area = %area, %failure, "writing candidates failed");
                self.failed_requests.note_refusal(Role::Generator, failure);
                Request::Failed(failure)
            }
        };
        if writes
            .iter()
            .all(|write| matches!(write, Request::Replied(_)))
        {
            let candidates: Vec<Candidate> = mem::take(writes)
                .into_iter()
                .filter_map(Request::replied)
                .flatten()
                .collect();
            self.stage = Stage::Checking(
                candidates
                    .into_iter()
                    .map(|candidate| (candidate, Request::Unsent))
                    .collect(),
            );
            self.pick_if_checked();
        }
    }

    /// Applies Jev's `reply` to the question check of `candidate`, or why
    /// the request failed. Once every candidate is checked, picks the
    /// questions to show.
    fn checked(
        &mut self,
        candidate: &CandidateId,
        reply: Result<QuestionCheckAnswers, RequestFailure>,
    ) {
        let Stage::Checking(checks) = &mut self.stage else {
            return;
        };
        let Some((_, check)) = checks
            .iter_mut()
            .find(|(checked, _)| checked.id() == candidate)
        else {
            return;
        };
        *check = match reply {
            Ok(answers) => Request::Replied(answers),
            Err(failure) => {
                tracing::info!(area = %candidate.area(), %failure, "checking a candidate failed");
                self.failed_requests.note_refusal(Role::Jev, failure);
                Request::Failed(failure)
            }
        };
        self.pick_if_checked();
    }

    /// Once every candidate is checked, keeps those the show rule passes
    /// and picks the questions to show from them.
    fn pick_if_checked(&mut self) {
        let Stage::Checking(checks) = &mut self.stage else {
            return;
        };
        if !checks
            .iter()
            .all(|(_, check)| matches!(check, Request::Replied(_)))
        {
            return;
        }
        // The policy the checks went out with: each shown question keeps
        // it, and its outcomes are banded with it.
        let policy = self.sent.policy();
        let checks = mem::take(checks);
        let checked = checks.len();
        let mut passed = Vec::new();
        for (candidate, check) in checks {
            let Some(answers) = check.replied() else {
                continue;
            };
            match show_rule(candidate, answers, policy) {
                Ok(question) => passed.push(question),
                Err(withheld) => {
                    log_withheld(&withheld, policy);
                    self.why_withheld.withheld(withheld);
                }
            }
        }
        let passing = passed.len();
        let hints_dropped = passed
            .iter()
            .filter(|question| question.hint().is_none())
            .count();
        let areas: Vec<AreaId> = self
            .gate
            .asked_about(&self.areas)
            .into_iter()
            .map(|area| area.id().clone())
            .collect();
        let picked = pick(passed, &areas, self.count);
        tracing::info!(
            checked,
            withheld = checked - passing,
            hints_dropped,
            picked = picked.questions().len(),
            asked_for = self.count.get(),
            "picked the questions that passed their checks"
        );
        self.stage = match Questions::new(picked, &self.areas, self.failed_requests.clone()) {
            Some(questions) => Stage::Shown(Box::new(
                questions.not_asking_about(self.gate.dropped(&self.areas)),
            )),
            None => Stage::NonePassed,
        };
        self.focus = Position::default();
    }

    /// Ends the Practice, as the learner chose Finish: the summary of the
    /// questions shows.
    fn finish(&mut self) {
        let subject = self.subject();
        self.stage = match mem::replace(&mut self.stage, Stage::NonePassed) {
            Stage::Shown(questions) => {
                Stage::Finished(Box::new(Summary::new(*questions, subject, self.spending())))
            }
            stage => stage,
        };
    }

    /// What the requests sent cost so far, while those it waits for still
    /// run.
    fn spending(&self) -> Spending {
        self.sent.spending(self.waiting.len())
    }

    /// The failure that stopped the current stage, if one did: the first,
    /// in order.
    fn failure(&self) -> Option<RequestFailure> {
        match &self.stage {
            Stage::CheckingAreas(gate) => gate.failure(),
            Stage::Writing(writes) => writes.iter().find_map(Request::failure),
            Stage::Checking(checks) => checks.iter().find_map(|(_, check)| check.failure()),
            Stage::FindingConnectedCode(_)
            | Stage::NothingToPractice
            | Stage::Shown(_)
            | Stage::NonePassed
            | Stage::Finished(_) => None,
        }
    }

    /// Whether it waits for job `job` to find connected code.
    fn finds_connected_code_with(&self, job: JobId) -> bool {
        matches!(self.stage, Stage::FindingConnectedCode(Lookup::Running(waiting)) if waiting == job)
    }

    /// Asks again only what failed in the current stage, or, when no
    /// question passed, writes and checks new ones.
    fn retry(&mut self) {
        match &mut self.stage {
            Stage::CheckingAreas(gate) => gate.retry(),
            Stage::Writing(writes) => writes.iter_mut().for_each(Request::retry),
            Stage::Checking(checks) => checks.iter_mut().for_each(|(_, check)| check.retry()),
            Stage::NonePassed => self.write(),
            Stage::FindingConnectedCode(_)
            | Stage::NothingToPractice
            | Stage::Shown(_)
            | Stage::Finished(_) => {}
        }
        self.focus = Position::default();
    }

    /// The controls, in focus order, by where Practice stands.
    fn controls(&self) -> &'static [Control] {
        match self.stage {
            Stage::NothingToPractice => &[
                Control::PickAnotherChange,
                Control::PickOtherAreas,
                Control::AskAnyway,
            ],
            Stage::NonePassed => &[Control::Retry, Control::PickAnotherChange],
            _ if self.failure().is_some() => &[Control::Retry, Control::Back],
            _ => &[Control::Cancel],
        }
    }

    fn focused(&self) -> Control {
        let controls = self.controls();
        controls
            .get(self.focus.index())
            .or(controls.last())
            .copied()
            .unwrap_or(Control::Cancel)
    }

    /// Where finding connected code stands.
    fn finding_connected_code(&self) -> StageState {
        if let Stage::FindingConnectedCode(_) = self.stage {
            return StageState::Current;
        }
        match self.evidence.as_ref().map(Evidence::connections_state) {
            None | Some(ConnectionsState::Finding) => StageState::Current,
            Some(ConnectionsState::Found | ConnectionsState::NoneFound) => StageState::Done,
            Some(ConnectionsState::TookTooLong | ConnectionsState::AnalysisFailed) => {
                StageState::Skipped
            }
        }
    }

    /// Practice's stages as it stands, drawn with `theme`, with the current
    /// stage's spinner at its frame for `ticks`. On the learner's repository
    /// the outbound review is done, as Send started Practice; the showcase
    /// project has none.
    fn stage_row(&self, theme: &Theme, ticks: usize) -> StageRow<'static> {
        let finding = self.finding_connected_code();
        let failed = self.failure().is_some();
        let gate = match self.stage {
            Stage::FindingConnectedCode(_) => StageState::Upcoming,
            Stage::CheckingAreas(_) if failed => StageState::Failed,
            Stage::CheckingAreas(_) => StageState::Current,
            _ if self.gate == Gate::AskedAnyway => StageState::Skipped,
            Stage::NothingToPractice
            | Stage::Writing(_)
            | Stage::Checking(_)
            | Stage::Shown(_)
            | Stage::NonePassed
            | Stage::Finished(_) => StageState::Done,
        };
        let (writing, checking) = match self.stage {
            Stage::FindingConnectedCode(_) | Stage::CheckingAreas(_) => {
                (StageState::Upcoming, StageState::Upcoming)
            }
            Stage::NothingToPractice => (StageState::Skipped, StageState::Skipped),
            Stage::Writing(_) if failed => (StageState::Failed, StageState::Upcoming),
            Stage::Writing(_) => (StageState::Current, StageState::Upcoming),
            Stage::Checking(_) if failed => (StageState::Done, StageState::Failed),
            Stage::Checking(_) => (StageState::Done, StageState::Current),
            Stage::Shown(_) | Stage::NonePassed | Stage::Finished(_) => {
                (StageState::Done, StageState::Done)
            }
        };
        let stages = PracticeStage::ALL
            .into_iter()
            .filter_map(|stage| {
                let state = match stage {
                    PracticeStage::FindingConnectedCode => finding,
                    PracticeStage::PreparingEvidence if finding == StageState::Current => {
                        StageState::Upcoming
                    }
                    PracticeStage::ReadingTheChange | PracticeStage::PreparingEvidence => {
                        StageState::Done
                    }
                    PracticeStage::OutboundReview => match self.change_ref {
                        ChangeRef::Showcase(_) => return None,
                        ChangeRef::Commit(_) => StageState::Done,
                    },
                    PracticeStage::CheckingAreas => gate,
                    PracticeStage::WritingQuestions => writing,
                    PracticeStage::CheckingQuestions => checking,
                };
                Some(RowStage {
                    label: stage.label(),
                    state,
                })
            })
            .collect::<Vec<_>>();
        let ticks = if stages
            .iter()
            .any(|stage| stage.state == StageState::Current)
        {
            ticks
        } else {
            0
        };
        StageRow::new(stages, ticks, theme)
    }

    /// The lines under the stage row and the title: what went wrong, if
    /// anything did, or why there is nothing to practice. An empty one is a
    /// blank line.
    fn status(&self) -> Vec<String> {
        match (&self.stage, self.failure()) {
            (Stage::NothingToPractice, _) => self
                .areas
                .iter()
                .map(|area| format!("{}: not worth asking about in this change", area.name()))
                .chain([
                    String::new(),
                    "Ask anyway writes questions about every chosen area all the same, and \
                     each is still checked before it shows."
                        .to_owned(),
                ])
                .collect(),
            (Stage::NonePassed, _) => {
                let mut lines = self.why_withheld.lines(self.sent.policy());
                lines.extend([
                    String::new(),
                    "Retry writes and checks new ones.".to_owned(),
                ]);
                lines
            }
            (stage, Some(failure)) => {
                let (name, role) = match stage {
                    Stage::CheckingAreas(_) => ("Checking areas", Role::Jev),
                    Stage::Checking(_) => ("Checking questions", Role::Jev),
                    _ => ("Writing questions", Role::Generator),
                };
                vec![
                    format!("{name} failed: {failure}."),
                    self.failed_requests.what_to_do(role, failure),
                    "Retry asks again only what failed. Nothing is retried until you choose to."
                        .to_owned(),
                ]
            }
            _ => Vec::new(),
        }
    }

    /// The change and the chosen areas, as the screen names them.
    fn subject(&self) -> String {
        let areas: Vec<&str> = self.areas.iter().map(Area::name).collect();
        format!("{} · {}", self.change.summary(), areas.join(", "))
    }

    /// The reader of why fewer questions passed their checks than were
    /// asked for: the same reasons No questions passed checks gives.
    fn why_withheld_reader(&self) -> Reader {
        let mut lines = self.why_withheld.lines(self.sent.policy()).into_iter();
        let count = lines.next().unwrap_or_default();
        Reader::new(
            Route::Read(Reading::WhyWithheld),
            "Why fewer passed checks".to_owned(),
            vec![Section::plain(
                &count,
                &lines.collect::<Vec<_>>().join("\n"),
            )],
        )
    }

    /// The reader of everything the evidence sent, once it is prepared: the
    /// change's message, what each touched file sends, each area prompt and
    /// each connection.
    fn evidence_reader(&self) -> Option<Reader> {
        let evidence = self.evidence.as_ref()?;
        let mut sections = vec![Section::plain("Message, as it is sent", evidence.message())];
        for file in evidence.files() {
            match file.sent() {
                Some(sent) => {
                    sections.push(Section::numbered(
                        &format!("{}: diff", file.path()),
                        sent.diff,
                    ));
                    sections.push(Section::numbered(
                        &match sent.side {
                            Side::After => format!("{}: full content", file.path()),
                            Side::Before => format!(
                                "{}: full content before the change, which deletes it",
                                file.path()
                            ),
                        },
                        sent.content,
                    ));
                }
                None => sections.push(Section::plain(
                    &format!("{}: nothing is sent", file.path()),
                    &format!("{} is {}.", file.path(), file.status()),
                )),
            }
        }
        for area in evidence.areas() {
            sections.push(Section::plain(
                &format!("Area prompt: {}, as it is sent", area.name()),
                area.prompt(),
            ));
        }
        for connection in evidence.connections() {
            if let Some(sent) = connection.sent() {
                let id = connection.id();
                sections.push(Section::numbered_from(
                    &format!(
                        "{}, {}: connected code, as it is sent",
                        id.path(),
                        id.extent()
                    ),
                    sent.text,
                    sent.first_line,
                ));
            }
        }
        Some(Reader::new(
            Route::Read(Reading::Evidence),
            format!("Evidence: {}", self.change.summary()),
            sections,
        ))
    }
}

impl View for Practice {
    fn route(&self) -> Route {
        Route::Practice(self.change_ref)
    }

    fn name(&self) -> &'static str {
        match &self.stage {
            Stage::Shown(questions) => questions.name(),
            Stage::Finished(_) => "Summary",
            Stage::NothingToPractice => "Nothing to practice in this change",
            Stage::NonePassed => "No questions passed checks",
            Stage::FindingConnectedCode(_)
            | Stage::CheckingAreas(_)
            | Stage::Writing(_)
            | Stage::Checking(_) => "Preparing questions",
        }
    }

    fn keymap(&self) -> Keymap {
        match &self.stage {
            Stage::Shown(questions) => questions.keymap(),
            Stage::Finished(_) => summary::KEYMAP,
            Stage::NothingToPractice | Stage::NonePassed => BACK_KEYMAP,
            _ if self.failure().is_some() => BACK_KEYMAP,
            Stage::FindingConnectedCode(_)
            | Stage::CheckingAreas(_)
            | Stage::Writing(_)
            | Stage::Checking(_) => KEYMAP,
        }
    }

    fn input(&self) -> Input {
        match &self.stage {
            Stage::Shown(questions) => questions.input(),
            _ => Input::Commands,
        }
    }

    fn actions(&self) -> &'static [Action] {
        match &self.stage {
            Stage::Shown(questions) => questions.actions(),
            Stage::Finished(_) => &[],
            Stage::NothingToPractice | Stage::NonePassed => BACK_ACTIONS,
            _ if self.failure().is_some() => BACK_ACTIONS,
            Stage::FindingConnectedCode(_)
            | Stage::CheckingAreas(_)
            | Stage::Writing(_)
            | Stage::Checking(_) => ACTIONS,
        }
    }

    fn start_jobs(&mut self, jobs: &mut Jobs<JobOutput>) {
        if let Stage::FindingConnectedCode(Lookup::Requested(lookup)) = &self.stage {
            let lookup = Arc::clone(lookup);
            // As for the outbound review: the engine can't be stopped once
            // it starts, so analyses take turns, and only the newest waits.
            let job = jobs.start_one_at_a_time(move |_token| {
                let (connections, origin) = lookup.tree.find(&lookup.dirs, &lookup.cache);
                Ok(JobOutput::ShowcaseConnectedCode {
                    position: lookup.position,
                    connections,
                    origin,
                })
            });
            jobs.limit(job, self.time_limit);
            self.stage = Stage::FindingConnectedCode(Lookup::Running(job));
        }
        let Some(evidence) = &self.evidence else {
            return;
        };
        match &mut self.stage {
            Stage::CheckingAreas(gate) => {
                if let Request::Unsent = gate {
                    send(
                        jobs,
                        &mut self.waiting,
                        &mut self.sent,
                        Asking::Gate,
                        |stamp| self.ai.check_areas(stamp, evidence, &self.areas),
                        |reply| JobOutput::AreasChecked { reply },
                    );
                    *gate = Request::Sent;
                }
            }
            Stage::Writing(writes) => {
                let areas = self.gate.asked_about(&self.areas);
                let count = CandidateCount::per_area(self.count, areas.len());
                for (area, write) in areas.into_iter().zip(writes) {
                    if let Request::Unsent = write {
                        let id = area.id().clone();
                        send(
                            jobs,
                            &mut self.waiting,
                            &mut self.sent,
                            Asking::Write(id.clone()),
                            |stamp| self.ai.write_candidates(stamp, evidence, area, count),
                            |reply| JobOutput::Written { area: id, reply },
                        );
                        *write = Request::Sent;
                    }
                }
            }
            Stage::Checking(checks) => {
                for (candidate, check) in checks {
                    if let Request::Unsent = check {
                        let id = candidate.id().clone();
                        send(
                            jobs,
                            &mut self.waiting,
                            &mut self.sent,
                            Asking::Check(id.clone()),
                            |stamp| self.ai.check_question(stamp, evidence, candidate),
                            |reply| JobOutput::Checked {
                                candidate: id,
                                reply,
                            },
                        );
                        *check = Request::Sent;
                    }
                }
            }
            Stage::Shown(questions) => {
                ask_about_responses(
                    questions,
                    self.ai.as_ref(),
                    evidence,
                    jobs,
                    &mut self.waiting,
                    &mut self.sent,
                );
            }
            // Finishing sends no new request: those running run on.
            Stage::FindingConnectedCode(_)
            | Stage::NothingToPractice
            | Stage::NonePassed
            | Stage::Finished(_) => {}
        }
    }

    fn keep_record(&mut self, store: &mut dyn SessionStore) {
        if let (Some(questions), Some(evidence)) = (self.stage.questions_mut(), &self.evidence) {
            questions.keep_record(store, evidence, &mut self.sent);
        }
    }

    fn holds_draft(&self) -> bool {
        matches!(&self.stage, Stage::Shown(questions) if questions.holds_draft())
    }

    fn open(&self, route: Route, _context: &Context<'_>) -> Option<Box<dyn View>> {
        match route {
            Route::Read(Reading::Evidence) => self
                .evidence_reader()
                .map(|reader| Box::new(reader) as Box<dyn View>),
            Route::Read(Reading::Answer) => match &self.stage {
                Stage::Shown(questions) => Some(Box::new(questions.answer_reader())),
                _ => None,
            },
            Route::Read(Reading::JevAnswers) => match &self.stage {
                Stage::Shown(questions) => questions
                    .jev_answers(self.sent.requests())
                    .map(|reader| Box::new(reader) as Box<dyn View>),
                _ => None,
            },
            Route::Read(Reading::WhyWithheld) => match &self.stage {
                Stage::Shown(_) => Some(Box::new(self.why_withheld_reader())),
                _ => None,
            },
            _ => None,
        }
    }

    fn waits_for(&self, job: JobId) -> bool {
        self.finds_connected_code_with(job) || self.waiting.waits_for(job)
    }

    fn finish_job(&mut self, job: JobId, output: JobOutput) {
        match output {
            JobOutput::ShowcaseConnectedCode {
                position,
                connections,
                ..
            } => {
                if self.finds_connected_code_with(job)
                    && self.change_ref == ChangeRef::Showcase(ChangeIndex::new(position))
                {
                    self.prepare_evidence(&connections);
                }
            }
            JobOutput::AreasChecked { reply } => {
                if self.waiting.accept(job, &Asking::Gate) {
                    let reply = self.sent.ended(&Asking::Gate, reply);
                    self.areas_checked(reply);
                }
            }
            JobOutput::Written { area, reply } => {
                let asking = Asking::Write(area.clone());
                if self.waiting.accept(job, &asking) {
                    let reply = self.sent.ended(&asking, reply);
                    self.written(&area, reply);
                }
            }
            JobOutput::Checked { candidate, reply } => {
                let asking = Asking::Check(candidate.clone());
                if self.waiting.accept(job, &asking) {
                    let reply = self.sent.ended(&asking, reply);
                    self.checked(&candidate, reply);
                }
            }
            JobOutput::Evaluated { saved, reply } => {
                let saved = *saved;
                let asking = Asking::Evaluate(saved.clone());
                if self.waiting.accept(job, &asking) {
                    let reply = self.sent.ended(&asking, reply);
                    if let Err(failure) = &reply {
                        self.failed_requests.note_refusal(Role::Jev, *failure);
                    }
                    if let Some(questions) = self.stage.questions_mut() {
                        questions.evaluated(&saved, reply);
                    }
                }
            }
            JobOutput::Explained { saved, reply } => {
                let saved = *saved;
                let asking = Asking::Explain(saved.clone());
                if self.waiting.accept(job, &asking) {
                    let reply = self.sent.ended(&asking, reply);
                    // The generator's request routes with zero retention.
                    if let Err(failure) = &reply {
                        self.failed_requests.note_refusal(Role::Generator, *failure);
                    }
                    if let (Some(questions), Some(evidence)) =
                        (self.stage.questions_mut(), &self.evidence)
                    {
                        questions.explained(&saved, evidence, reply);
                    }
                }
            }
            JobOutput::ExplanationChecked { saved, reply } => {
                let saved = *saved;
                let asking = Asking::CheckExplanation(saved.clone());
                if self.waiting.accept(job, &asking) {
                    let reply = self.sent.ended(&asking, reply);
                    if let Err(failure) = &reply {
                        self.failed_requests.note_refusal(Role::Jev, *failure);
                    }
                    if let Some(questions) = self.stage.questions_mut() {
                        questions.explanation_checked(&saved, reply);
                    }
                }
            }
            JobOutput::ConnectedCode { .. } => {}
        }
    }

    fn time_up(&mut self, job: JobId) {
        if self.finds_connected_code_with(job) {
            tracing::info!(
                limit_s = self.time_limit.as_secs(),
                "gave up finding connected code for Preparing questions"
            );
            self.prepare_evidence(&Connections::TookTooLong);
        }
    }

    fn close(&mut self, jobs: &mut Jobs<JobOutput>) {
        // The learner pays for each request, so none runs on. Each is kept
        // as cancelled, for the session record.
        for asking in self.waiting.cancel(jobs) {
            self.sent.cancelled(&asking);
        }
        // The analysis runs on, to be cached, and its result is ignored.
        if let Stage::FindingConnectedCode(Lookup::Running(job)) = self.stage {
            jobs.forget(job);
        }
    }

    fn paste(&mut self, text: &str) {
        if let Stage::Shown(questions) = &mut self.stage {
            questions.paste(text);
        }
    }

    fn apply(&mut self, command: Command, area: Rect) -> Option<Command> {
        match &mut self.stage {
            Stage::Shown(questions) => {
                return match questions.apply(command, area) {
                    Some(Command::Finish) => {
                        self.finish();
                        None
                    }
                    next => next,
                };
            }
            Stage::Finished(summary) => {
                let spending = self.sent.spending(self.waiting.len());
                return summary.apply(command, area, spending);
            }
            _ => {}
        }
        if command != Command::Activate {
            self.focus = self.focus.after(command, self.controls().len());
            return None;
        }
        match self.focused() {
            Control::Cancel
            | Control::Back
            | Control::PickAnotherChange
            | Control::PickOtherAreas => Some(Command::Back),
            Control::AskAnyway => {
                self.ask_anyway();
                None
            }
            Control::Retry => {
                self.retry();
                None
            }
        }
    }

    fn render(&self, context: &Context<'_>, frame: &mut Frame<'_>, area: Rect) {
        let theme = context.theme;
        match &self.stage {
            Stage::Shown(questions) => return questions.render(theme, frame, area),
            Stage::Finished(summary) => {
                return summary.render(theme, frame, area, self.spending());
            }
            _ => {}
        }
        let width = usize::from(area.width);
        let mut lines = self.stage_row(theme, context.ticks).lines(area.width);
        lines.push(Line::default());
        lines.push(Line::styled(self.name(), theme.title));
        lines.extend(
            columns::wrap(&self.subject(), width)
                .into_iter()
                .map(|line| Line::styled(line, theme.muted)),
        );
        let status = self.status();
        if !status.is_empty() {
            lines.push(Line::default());
            for text in status {
                lines.extend(columns::wrap(&text, width).into_iter().map(Line::raw));
            }
        }
        let controls = self.controls();
        let rows = u16::try_from(controls.len()).unwrap_or(u16::MAX);
        let [body, _, buttons] = Layout::vertical([
            Constraint::Max(u16::try_from(lines.len()).unwrap_or(u16::MAX)),
            Constraint::Length(1),
            Constraint::Length(rows),
        ])
        .areas(area);
        frame.render_widget(Paragraph::new(lines), body);
        frame.render_widget(
            List::new(controls.iter().map(|&control| {
                theme.control(
                    control.label(),
                    ControlState {
                        focused: self.focused() == control,
                        ..ControlState::default()
                    },
                )
            })),
            buttons,
        );
    }
}
