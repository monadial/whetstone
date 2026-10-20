//! Practice on a change: Preparing questions, then the questions.
//!
//! On the showcase project, Generate questions opens it. Preparing
//! questions finds the change's connected code the way the outbound review
//! finds a commit's, and prepares the evidence from it. Then the generator
//! writes candidates, one request per chosen area, all at once; plain
//! checks drop the malformed ones; Jev checks each candidate left, all at
//! once; the show rule keeps the clear-cut ones; and picking chooses up to
//! `questions` of them for the questions screen. The stage row shows where
//! it is.
//!
//! A failed request stops its stage, which names itself and offers Retry,
//! which asks again only what failed, and Back. Nothing is retried
//! automatically. Cancel, Back or going home cancels every request still
//! running, and a result that comes after changes nothing (rule 24). Finding
//! connected code runs on instead, as it does for the outbound review, so
//! its graph is cached.

use std::mem;
use std::sync::Arc;
use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::{List, Paragraph};
use whetstone_ai::{Ai, CandidateCount, Reply, RequestFailure};
use whetstone_core::{
    Area, AreaId, AssessmentPolicy, Candidate, CandidateId, Change, ConnectedCodeLimit,
    Connections, ConnectionsState, Evidence, ExcludePatterns, QuestionCheckAnswers, QuestionCount,
    RepositoryConfig, ReviewChoices, WrittenCandidate, pick, plain_checks, show_rule,
};

use crate::columns;
use crate::connected_code::{AnalysisDirs, GraphCache, ShowcaseTree};
use crate::focus::Position;
use crate::job::{JobId, JobOutput, Jobs, Waiting};
use crate::keymap::{self, Binding, Command, Keymap};
use crate::palette::Action;
use crate::questions::{self, Questions};
use crate::stage_row::{PracticeStage, Stage as RowStage, StageRow, StageState};
use crate::theme::{ControlState, Theme};
use crate::view::{ChangeIndex, ChangeRef, Context, Route, View};

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

/// The keys once no question passed: `q` and Esc go back.
const NONE_PASSED_KEYMAP: Keymap = &[
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

/// The questions screen's own action in the palette, and No questions
/// passed checks'.
const BACK_ACTIONS: &[Action] = &[Action {
    label: "Back",
    does: "Go back to the areas and changes",
    command: Command::Back,
}];

/// Practice on one change, from Preparing questions to the questions.
#[derive(Debug)]
pub(crate) struct Practice {
    /// The change, as its route names it.
    at: ChangeRef,
    change: Change,
    /// The chosen areas, in the order offered.
    areas: Vec<Area>,
    /// How many questions the learner asked for.
    count: QuestionCount,
    ai: Arc<dyn Ai>,
    connected_code_limit: ConnectedCodeLimit,
    /// How long it waits for connected code.
    time_limit: Duration,
    lookup: Lookup,
    /// The evidence, once connected code is found or given up on.
    evidence: Option<Evidence>,
    stage: Stage,
    /// The requests to Jev and the generator it waits for, each with what
    /// it asked (rule 24).
    waiting: Waiting<Asking>,
    focus: Position,
}

/// Finding the change's connected code.
#[derive(Debug)]
enum Lookup {
    /// Not started yet: the job starts as the screen opens.
    Requested(Box<ShowcaseLookup>),
    /// The job finding it, which the screen waits for.
    Running(JobId),
    /// Found, or given up on.
    Done,
}

/// What the job that finds a showcase change's connected code needs.
#[derive(Debug)]
struct ShowcaseLookup {
    tree: ShowcaseTree,
    position: usize,
    dirs: AnalysisDirs,
    cache: Arc<GraphCache>,
}

/// Where Practice stands once the evidence is prepared.
#[derive(Debug)]
enum Stage {
    /// The generator writes candidates about each chosen area, in area
    /// order: what passed the plain checks.
    Writing(Vec<Request<Vec<Candidate>>>),
    /// Jev checks each candidate that passed the plain checks, in the order
    /// written, area by area.
    Checking(Vec<(Candidate, Request<QuestionCheckAnswers>)>),
    /// The questions picked.
    Shown(Questions),
    /// No question passed its checks.
    NonePassed,
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

/// What a request asked, which its result must carry (rule 24).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Asking {
    /// The generator, to write candidates about this area.
    Write(AreaId),
    /// Jev, to check this candidate.
    Check(CandidateId),
}

/// The screen's controls, by what Practice stands at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    Cancel,
    Retry,
    Back,
    PickAnotherChange,
}

impl Control {
    fn label(self) -> &'static str {
        match self {
            Self::Cancel => "Cancel",
            Self::Retry => "Retry",
            Self::Back => "Back",
            Self::PickAnotherChange => "Pick another change",
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
        tracing::info!(
            position = index.get(),
            areas = areas.len(),
            "opened Preparing questions on a showcase change"
        );
        Some(Self {
            at: ChangeRef::Showcase(index),
            change,
            areas,
            count: settings.questions.value,
            ai,
            // The showcase project's config sets nothing, which its tests
            // check, so only the learner's limit applies.
            connected_code_limit: settings.connected_code_limit(&RepositoryConfig::default()),
            time_limit: context.time_limit,
            lookup: Lookup::Requested(Box::new(ShowcaseLookup {
                tree,
                position: index.get(),
                dirs: context.analysis.clone(),
                cache: Arc::clone(context.cache),
            })),
            evidence: None,
            stage: Stage::Writing(Vec::new()),
            waiting: Waiting::new(),
            focus: Position::default(),
        })
    }

    /// Ends finding connected code with `connections`, or why there are
    /// none, and prepares the evidence from them, for the generator to
    /// write from.
    fn prepare_evidence(&mut self, connections: &Connections) {
        self.lookup = Lookup::Done;
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
        self.write();
    }

    /// Asks the generator for candidates about every chosen area.
    fn write(&mut self) {
        self.stage = Stage::Writing(Request::all(self.areas.len()));
        self.focus = Position::default();
    }

    /// Applies the generator's `reply` about `area`: the candidates that
    /// pass the plain checks, or why the request failed. Once every area
    /// has its candidates, Jev checks them.
    fn written(
        &mut self,
        area: &AreaId,
        reply: Result<Reply<Vec<WrittenCandidate>>, RequestFailure>,
    ) {
        let (Stage::Writing(writes), Some(evidence)) = (&mut self.stage, &self.evidence) else {
            return;
        };
        let Some(write) = self
            .areas
            .iter()
            .position(|chosen| chosen.id() == area)
            .and_then(|at| writes.get_mut(at))
        else {
            return;
        };
        *write = match reply {
            Ok(reply) => {
                let written = reply.content.len();
                let checked = plain_checks(area, reply.content, evidence);
                tracing::info!(
                    area = %area,
                    written,
                    dropped = checked.dropped().len(),
                    "the generator wrote candidates"
                );
                Request::Replied(checked.into_passed())
            }
            Err(failure) => {
                tracing::info!(area = %area, %failure, "writing candidates failed");
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
        reply: Result<Reply<QuestionCheckAnswers>, RequestFailure>,
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
            Ok(reply) => Request::Replied(reply.content),
            Err(failure) => {
                tracing::info!(area = %candidate.area(), %failure, "checking a candidate failed");
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
        let policy = AssessmentPolicy::current();
        let checks = mem::take(checks);
        let checked = checks.len();
        let (passed, withheld): (Vec<_>, Vec<_>) = checks
            .into_iter()
            .filter_map(|(candidate, check)| Some((candidate, check.replied()?)))
            .map(|(candidate, answers)| show_rule(candidate, answers, policy))
            .partition(Result::is_ok);
        let areas: Vec<AreaId> = self.areas.iter().map(|area| area.id().clone()).collect();
        let picked = pick(
            passed.into_iter().filter_map(Result::ok).collect(),
            &areas,
            self.count,
        );
        tracing::info!(
            checked,
            withheld = withheld.len(),
            picked = picked.questions().len(),
            asked_for = self.count.get(),
            "picked the questions that passed their checks"
        );
        self.stage = match Questions::new(picked, &self.areas) {
            Some(questions) => Stage::Shown(questions),
            None => Stage::NonePassed,
        };
        self.focus = Position::default();
    }

    /// The failure that stopped the current stage, if one did: the first,
    /// in order.
    fn failure(&self) -> Option<RequestFailure> {
        match &self.stage {
            Stage::Writing(writes) => writes.iter().find_map(Request::failure),
            Stage::Checking(checks) => checks.iter().find_map(|(_, check)| check.failure()),
            Stage::Shown(_) | Stage::NonePassed => None,
        }
    }

    /// Asks again only what failed in the current stage, or, when no
    /// question passed, writes and checks new ones.
    fn retry(&mut self) {
        match &mut self.stage {
            Stage::Writing(writes) => writes.iter_mut().for_each(Request::retry),
            Stage::Checking(checks) => checks.iter_mut().for_each(|(_, check)| check.retry()),
            Stage::NonePassed => self.write(),
            Stage::Shown(_) => {}
        }
        self.focus = Position::default();
    }

    /// The controls, in focus order, by where Practice stands.
    fn controls(&self) -> &'static [Control] {
        match self.stage {
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
        match self.evidence.as_ref().map(Evidence::connections_state) {
            None | Some(ConnectionsState::Finding) => StageState::Current,
            Some(ConnectionsState::Found | ConnectionsState::NoneFound) => StageState::Done,
            Some(ConnectionsState::TookTooLong | ConnectionsState::AnalysisFailed) => {
                StageState::Skipped
            }
        }
    }

    /// Practice's stages as it stands, drawn with `theme`, with the current
    /// stage's spinner at its frame for `ticks`. The showcase project has no
    /// outbound review, and the relevance gate is skipped until it arrives.
    fn stage_row(&self, theme: &Theme, ticks: usize) -> StageRow<'static> {
        let finding = self.finding_connected_code();
        let failed = self.failure().is_some();
        let (writing, checking) = match (&self.stage, finding) {
            (_, StageState::Current) => (StageState::Upcoming, StageState::Upcoming),
            (Stage::Writing(_), _) if failed => (StageState::Failed, StageState::Upcoming),
            (Stage::Writing(_), _) => (StageState::Current, StageState::Upcoming),
            (Stage::Checking(_), _) if failed => (StageState::Done, StageState::Failed),
            (Stage::Checking(_), _) => (StageState::Done, StageState::Current),
            (Stage::Shown(_) | Stage::NonePassed, _) => (StageState::Done, StageState::Done),
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
                    PracticeStage::OutboundReview => return None,
                    // The relevance gate arrives later.
                    PracticeStage::CheckingAreas => StageState::Skipped,
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
    /// anything did.
    fn status(&self) -> Vec<String> {
        match (&self.stage, self.failure()) {
            (Stage::NonePassed, _) => vec![
                "None of the questions written passed their checks, so there is nothing to \
                 show. Retry writes and checks new ones."
                    .to_owned(),
            ],
            (stage, Some(failure)) => vec![
                format!(
                    "{} failed: {failure}.",
                    match stage {
                        Stage::Checking(_) => "Checking questions",
                        _ => "Writing questions",
                    }
                ),
                "Retry asks again only what failed. Nothing is retried until you choose to."
                    .to_owned(),
            ],
            _ => Vec::new(),
        }
    }

    /// The change and the chosen areas, as the screen names them.
    fn subject(&self) -> String {
        let areas: Vec<&str> = self.areas.iter().map(Area::name).collect();
        format!("{} · {}", self.change.summary(), areas.join(", "))
    }
}

impl View for Practice {
    fn route(&self) -> Route {
        Route::Practice(self.at)
    }

    fn name(&self) -> &'static str {
        match self.stage {
            Stage::Shown(_) => "Questions",
            Stage::NonePassed => "No questions passed checks",
            Stage::Writing(_) | Stage::Checking(_) => "Preparing questions",
        }
    }

    fn keymap(&self) -> Keymap {
        match self.stage {
            Stage::Writing(_) | Stage::Checking(_) => KEYMAP,
            Stage::Shown(_) => questions::KEYMAP,
            Stage::NonePassed => NONE_PASSED_KEYMAP,
        }
    }

    fn actions(&self) -> &'static [Action] {
        match self.stage {
            Stage::Writing(_) | Stage::Checking(_) => ACTIONS,
            Stage::Shown(_) | Stage::NonePassed => BACK_ACTIONS,
        }
    }

    fn start_jobs(&mut self, jobs: &mut Jobs<JobOutput>) {
        if let Lookup::Requested(_) = self.lookup {
            let Lookup::Requested(lookup) = mem::replace(&mut self.lookup, Lookup::Done) else {
                return;
            };
            let ShowcaseLookup {
                tree,
                position,
                dirs,
                cache,
            } = *lookup;
            // As for the outbound review: the engine can't be stopped once
            // it starts, so analyses take turns, and only the newest waits.
            let job = jobs.start_one_at_a_time(move |_token| {
                let (connections, origin) = tree.find(&dirs, &cache);
                Ok(JobOutput::ShowcaseConnectedCode {
                    position,
                    connections,
                    origin,
                })
            });
            jobs.limit(job, self.time_limit);
            self.lookup = Lookup::Running(job);
        }
        let Some(evidence) = &self.evidence else {
            return;
        };
        match &mut self.stage {
            Stage::Writing(writes) => {
                let count = CandidateCount::per_area(self.count, self.areas.len());
                for (area, write) in self.areas.iter().zip(writes) {
                    if let Request::Unsent = write {
                        let request = self.ai.write_candidates(evidence, area, count);
                        let id = area.id().clone();
                        let job = jobs.start_async(async move {
                            JobOutput::Written {
                                area: id,
                                reply: request.await,
                            }
                        });
                        self.waiting.wait_for(job, Asking::Write(area.id().clone()));
                        *write = Request::Sent;
                    }
                }
            }
            Stage::Checking(checks) => {
                for (candidate, check) in checks {
                    if let Request::Unsent = check {
                        let request = self.ai.check_question(evidence, candidate);
                        let id = candidate.id().clone();
                        let job = jobs.start_async(async move {
                            JobOutput::Checked {
                                candidate: id,
                                reply: request.await,
                            }
                        });
                        self.waiting
                            .wait_for(job, Asking::Check(candidate.id().clone()));
                        *check = Request::Sent;
                    }
                }
            }
            Stage::Shown(_) | Stage::NonePassed => {}
        }
    }

    fn waits_for(&self, job: JobId) -> bool {
        matches!(self.lookup, Lookup::Running(waiting) if waiting == job)
            || self.waiting.waits_for(job)
    }

    fn finish_job(&mut self, job: JobId, output: JobOutput) {
        match output {
            JobOutput::ShowcaseConnectedCode { connections, .. } => {
                if matches!(self.lookup, Lookup::Running(waiting) if waiting == job) {
                    self.prepare_evidence(&connections);
                }
            }
            JobOutput::Written { area, reply } => {
                if self.waiting.accept(job, &Asking::Write(area.clone())) {
                    self.written(&area, reply);
                }
            }
            JobOutput::Checked { candidate, reply } => {
                if self.waiting.accept(job, &Asking::Check(candidate.clone())) {
                    self.checked(&candidate, reply);
                }
            }
            JobOutput::ConnectedCode { .. } => {}
        }
    }

    fn time_up(&mut self, job: JobId) {
        if matches!(self.lookup, Lookup::Running(waiting) if waiting == job) {
            tracing::info!(
                limit_s = self.time_limit.as_secs(),
                "gave up finding connected code for Preparing questions"
            );
            self.prepare_evidence(&Connections::TookTooLong);
        }
    }

    fn close(&mut self, jobs: &mut Jobs<JobOutput>) {
        // The learner pays for each request, so none runs on.
        self.waiting.cancel(jobs);
        // The analysis runs on, to be cached, and its result is ignored.
        if let Lookup::Running(job) = self.lookup {
            jobs.forget(job);
        }
    }

    fn apply(&mut self, command: Command, _area: Rect) -> Option<Command> {
        if let Stage::Shown(questions) = &mut self.stage {
            return questions.apply(command);
        }
        if command != Command::Activate {
            self.focus = self.focus.after(command, self.controls().len());
            return None;
        }
        match self.focused() {
            Control::Cancel | Control::Back | Control::PickAnotherChange => Some(Command::Back),
            Control::Retry => {
                self.retry();
                None
            }
        }
    }

    fn render(&self, context: &Context<'_>, frame: &mut Frame<'_>, area: Rect) {
        let theme = context.theme;
        if let Stage::Shown(questions) = &self.stage {
            questions.render(theme, frame, area);
            return;
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
