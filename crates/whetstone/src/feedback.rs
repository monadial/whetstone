//! Feedback on a submitted response: the choice outcome at once, from the
//! option ratings, with the chosen option's rationale, the answer key's
//! rationale, "why it matters" and the citations, then the reasoning and its
//! outcome. The choice outcome is Supported or Not supported, never Needs
//! review. Outcomes are in words only: Jev's probabilities stay out of
//! them.
//!
//! The reasoning awaits evaluation until Jev's answers come, and then each
//! criterion of the rubric shows met, not met or needs review. Cancel
//! evaluation stops the request, and the response stays submitted. A
//! failure or a cancellation shows Evaluation unavailable, with Retry
//! evaluation, which asks Jev again. A failure is named, then what the
//! learner can do about it, as on Preparing questions. Each outcome is
//! saved to the session record once it is known.
//!
//! Under each criterion not met or needing review, the generator's
//! explanation of it shows once Jev has checked it, or says it was withheld
//! or is unavailable, with Retry explanation (see [`crate::explanation`]).
//! The outcomes always stay. Each paragraph checked is saved to the session
//! record.
//!
//! A submitted response is final, so feedback offers no way back to it. Its
//! controls are the evaluation's, while it awaits or failed, and the
//! explanation's, once Retry explanation is offered, then Next question,
//! Inspect source, Inspect Jev's answers, Report and Finish. An assisted
//! attempt's outcomes are each marked Assisted. Feedback shows no number:
//! Jev's answers, with their probabilities, are under Inspect Jev's answers
//! (see [`crate::jev_answers`]), which keeps them as they came.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, Paragraph};
use whetstone_ai::{ParagraphCheck, RequestFailure, WrittenParagraph};
use whetstone_core::{
    Assessed, AssessmentPolicy, ChoiceOutcome, CriterionOutcome, Evidence, Extent, OptionLetter,
    PolicyVersion, Probability, Question, Role,
};

use crate::columns;
use crate::explanation::Explanation;
use crate::failures::FailedRequests;
use crate::focus::Position;
use crate::keymap::{self, Binding, Command, Keymap};
use crate::session_record::{
    Assessment, Durability, ExplanationCheck, SavedResponse, SessionStore,
};
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

/// Feedback's controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    /// Stops the evaluation while it awaits.
    CancelEvaluation,
    /// Asks Jev again once the evaluation failed or was cancelled.
    RetryEvaluation,
    /// Asks again for each paragraph of the explanation withheld or
    /// unavailable.
    RetryExplanation,
    NextQuestion,
    InspectSource,
    /// Shows each of Jev's answers about the question, with its number.
    InspectJevAnswers,
    /// Reports the question, which changes no outcome.
    Report,
    Finish,
}

impl Control {
    /// The controls feedback always has, in focus order, after the
    /// evaluation's.
    const ALWAYS: [Self; 5] = [
        Self::NextQuestion,
        Self::InspectSource,
        Self::InspectJevAnswers,
        Self::Report,
        Self::Finish,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::CancelEvaluation => "Cancel evaluation",
            Self::RetryEvaluation => "Retry evaluation",
            Self::RetryExplanation => "Retry explanation",
            Self::NextQuestion => "Next question",
            Self::InspectSource => "Inspect source",
            Self::InspectJevAnswers => "Inspect Jev's answers",
            Self::Report => "Report",
            Self::Finish => "Finish",
        }
    }
}

/// Where Jev's evaluation of the reasoning stands.
#[derive(Debug)]
pub(crate) enum Evaluation {
    /// To ask for, as soon as the screen starts its jobs.
    Unsent,
    /// Asked for: the reasoning awaits evaluation.
    Awaiting,
    /// It failed, or the learner cancelled it, and waits for their Retry.
    Unavailable(RequestFailure),
    /// Each criterion's outcome, in rubric order.
    Assessed(Vec<Assessed<CriterionOutcome>>),
}

/// What the learner does next from feedback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// Answer the next question.
    NextQuestion,
    /// Read the evidence the question rests on.
    InspectSource,
    /// Read each of Jev's answers about the question.
    InspectJevAnswers,
    /// Report the question.
    Report,
    /// End the Practice.
    Finish,
    /// Stop the evaluation of the reasoning, which awaits.
    CancelEvaluation,
}

/// Where the question is among the questions shown, which decides whether
/// there is a next one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Place {
    /// Another question follows it: the next, or after the last, the first,
    /// while another question is still unattempted.
    BeforeTheLast,
    /// It is the last, and no other question is unattempted.
    Last,
}

/// What follows the question's place, and each outcome, of an assisted
/// attempt.
pub(crate) const ASSISTED: &str = " · Assisted";

/// Whose work an attempt is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum AttemptKind {
    /// The learner's own, with or without the hint.
    #[default]
    Own,
    /// An assisted attempt: the learner chose Show answer first, so its
    /// assessment is not independent, and its outcomes are marked Assisted.
    Assisted,
}

/// One of Jev's checks of a paragraph of the explanation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CheckedParagraph {
    /// The place in the rubric of the criterion the paragraph explains,
    /// counting from 0.
    pub(crate) criterion: usize,
    /// Jev's answers, as returned.
    pub(crate) answers: ParagraphCheck,
    /// The version of the policy that banded them.
    pub(crate) policy: PolicyVersion,
}

/// Feedback on one submitted response.
#[derive(Debug)]
pub(crate) struct Feedback {
    outcome: Assessed<ChoiceOutcome>,
    /// Whether the attempt is the learner's own or assisted.
    kind: AttemptKind,
    /// The store's acknowledgement of the response, which names its
    /// session, question and attempt and holds the response as saved: Jev
    /// is asked about the reasoning with it, and its result is waited for
    /// by it (rules 24 and 39).
    saved: SavedResponse,
    /// Whether the store that saved the response keeps it after Whetstone
    /// closes: only then does "Saved" show.
    durability: Durability,
    /// Where Jev's evaluation of the reasoning stands.
    evaluation: Evaluation,
    /// Where the explanation of what the reasoning missed stands, once the
    /// outcomes are known.
    explanation: Explanation,
    /// The criterion outcomes and explanation paragraphs known but not yet
    /// saved to the session record.
    unrecorded: Vec<Assessment>,
    /// Each of Jev's checks of a paragraph of the explanation, in the order
    /// they came.
    explanation_checks: Vec<CheckedParagraph>,
    /// The control with focus. If it is gone, as Cancel evaluation goes once
    /// the outcomes come, focus is where it started.
    focus: Control,
    /// The control focus starts on.
    start: Control,
    /// The row at the top of the feedback.
    top: Position,
}

impl Feedback {
    /// Feedback on the response a store of `durability` saved as `saved`,
    /// whose choice's outcome is `outcome`, in an attempt of `kind`, for a
    /// question at `place`. Focus starts on Next question, or on Finish
    /// after the last.
    pub(crate) fn new(
        outcome: Assessed<ChoiceOutcome>,
        saved: SavedResponse,
        kind: AttemptKind,
        durability: Durability,
        place: Place,
    ) -> Self {
        let start = match place {
            Place::BeforeTheLast => Control::NextQuestion,
            Place::Last => Control::Finish,
        };
        Self {
            outcome,
            kind,
            saved,
            durability,
            evaluation: Evaluation::Unsent,
            explanation: Explanation::default(),
            unrecorded: Vec::new(),
            explanation_checks: Vec::new(),
            focus: start,
            start,
            top: Position::default(),
        }
    }

    /// The store's acknowledgement of the response.
    pub(crate) fn saved(&self) -> &SavedResponse {
        &self.saved
    }

    /// The choice's outcome.
    pub(crate) fn outcome(&self) -> Assessed<ChoiceOutcome> {
        self.outcome
    }

    /// Where Jev's evaluation of the reasoning stands.
    pub(crate) fn evaluation(&self) -> &Evaluation {
        &self.evaluation
    }

    /// Each of Jev's checks of a paragraph of the explanation, in the order
    /// they came, a Retry's included.
    pub(crate) fn explanation_checks(&self) -> &[CheckedParagraph] {
        &self.explanation_checks
    }

    /// The saved response whose reasoning to ask Jev about, if its
    /// evaluation waits to be asked for. It then awaits.
    pub(crate) fn take_evaluation(&mut self) -> Option<&SavedResponse> {
        let Evaluation::Unsent = self.evaluation else {
            return None;
        };
        self.evaluation = Evaluation::Awaiting;
        Some(&self.saved)
    }

    /// Applies Jev's `reply` about the reasoning written for `question`: a
    /// probability for each criterion, in rubric order, which the assessment
    /// policy turns into its outcome, or why the request failed. A reply
    /// that doesn't answer each criterion once is a reply that could not be
    /// read (rule 37).
    pub(crate) fn evaluated(
        &mut self,
        question: &Question,
        reply: Result<Vec<Probability>, RequestFailure>,
    ) {
        let session = self.saved.session();
        let attempt = self.saved.attempt().get();
        // Only an evaluation it awaits takes a result (rule 24).
        let Evaluation::Awaiting = self.evaluation else {
            return;
        };
        self.evaluation = match reply {
            Ok(answers) if answers.len() == question.rubric().len() => {
                let policy = AssessmentPolicy::current();
                let outcomes: Vec<_> = answers
                    .into_iter()
                    .map(|answer| policy.criterion_outcome(answer))
                    .collect();
                let count = |outcome| {
                    outcomes
                        .iter()
                        .filter(|assessed| assessed.outcome() == outcome)
                        .count()
                };
                tracing::info!(
                    %session,
                    attempt,
                    met = count(CriterionOutcome::Met),
                    not_met = count(CriterionOutcome::NotMet),
                    needs_review = count(CriterionOutcome::NeedsReview),
                    "Jev assessed the reasoning"
                );
                self.unrecorded = outcomes
                    .iter()
                    .enumerate()
                    .map(|(criterion, &outcome)| Assessment::Criterion { criterion, outcome })
                    .collect();
                self.explanation = Explanation::of(&outcomes);
                Evaluation::Assessed(outcomes)
            }
            Ok(answers) => {
                tracing::warn!(
                    %session,
                    attempt,
                    answered = answers.len(),
                    criteria = question.rubric().len(),
                    "Jev's assessment of the reasoning didn't answer each criterion once"
                );
                Evaluation::Unavailable(RequestFailure::InvalidReply)
            }
            Err(failure) => {
                tracing::info!(%session, attempt, %failure, "assessing the reasoning failed");
                Evaluation::Unavailable(failure)
            }
        };
    }

    /// The saved response whose reasoning the generator is to explain, and
    /// the criteria to explain, each with its outcome, if any wait to be
    /// asked for. They are then being written.
    pub(crate) fn take_explanation(
        &mut self,
    ) -> Option<(&SavedResponse, Vec<(usize, CriterionOutcome)>)> {
        let criteria = self.explanation.take_request()?;
        Some((&self.saved, criteria))
    }

    /// The saved response whose explanation Jev is to check, and the
    /// paragraphs to check, if any wait to be. They are then being checked.
    pub(crate) fn take_explanation_check(
        &mut self,
    ) -> Option<(&SavedResponse, Vec<WrittenParagraph>)> {
        let paragraphs = self.explanation.take_check()?;
        Some((&self.saved, paragraphs))
    }

    /// Applies the generator's `reply` explaining the reasoning: a
    /// paragraph for each criterion asked about, whose citations plain code
    /// checks against `evidence`, or why the request failed.
    pub(crate) fn explained(
        &mut self,
        evidence: &Evidence,
        reply: Result<Vec<WrittenParagraph>, RequestFailure>,
    ) {
        let withheld = self.explanation.written(&self.saved, evidence, reply);
        self.unrecorded.extend(withheld);
    }

    /// Applies Jev's `reply` checking the explanation: its answers about
    /// each paragraph, or why the request failed.
    pub(crate) fn explanation_checked(
        &mut self,
        reply: Result<Vec<ParagraphCheck>, RequestFailure>,
    ) {
        let checked = self.explanation.checked(&self.saved, reply);
        self.explanation_checks
            .extend(checked.iter().filter_map(|assessment| match assessment {
                Assessment::Explanation {
                    paragraph,
                    check:
                        ExplanationCheck::Passed { answers, policy }
                        | ExplanationCheck::Failed { answers, policy },
                } => Some(CheckedParagraph {
                    criterion: paragraph.criterion,
                    answers: *answers,
                    policy: *policy,
                }),
                Assessment::Choice(_)
                | Assessment::Criterion { .. }
                | Assessment::Explanation { .. } => None,
            }));
        self.unrecorded.extend(checked);
    }

    /// Saves to `store` each criterion outcome and explanation paragraph
    /// known since the last save. A save that fails loses nothing the
    /// learner wrote, and the outcome still shows.
    pub(crate) fn keep_record(&mut self, store: &mut dyn SessionStore) {
        for assessment in self.unrecorded.drain(..) {
            if let Err(error) = store.save_assessment(&self.saved, assessment) {
                tracing::warn!(%error, "a criterion outcome or explanation wasn't saved");
            }
        }
    }

    /// The controls, in focus order: the evaluation's, while it awaits or
    /// is unavailable, Retry explanation, while it is offered, then the
    /// rest.
    fn controls(&self) -> Vec<Control> {
        let evaluation = match self.evaluation {
            Evaluation::Unsent | Evaluation::Awaiting => Some(Control::CancelEvaluation),
            Evaluation::Unavailable(_) => Some(Control::RetryEvaluation),
            Evaluation::Assessed(_) => None,
        };
        let explanation = self
            .explanation
            .can_retry()
            .then_some(Control::RetryExplanation);
        evaluation
            .into_iter()
            .chain(explanation)
            .chain(Control::ALWAYS)
            .collect()
    }

    fn focused(&self) -> Control {
        if self.controls().contains(&self.focus) {
            self.focus
        } else {
            self.start
        }
    }

    /// Why the control `control` is disabled at `place`, if it is.
    fn disabled(control: Control, place: Place) -> Option<&'static str> {
        match (control, place) {
            (Control::NextQuestion, Place::Last) => Some("this is the last"),
            _ => None,
        }
    }

    /// Applies `command` to the feedback on `question`, at `place`, drawn
    /// in `area` with a failed request worded by `failed`, and returns what
    /// the learner does next, if they chose.
    pub(crate) fn apply(
        &mut self,
        command: Command,
        area: Rect,
        question: &Question,
        place: Place,
        failed: &FailedRequests,
    ) -> Option<Step> {
        match command {
            Command::Activate => {
                let control = self.focused();
                if Self::disabled(control, place).is_some() {
                    return None;
                }
                return match control {
                    Control::CancelEvaluation => {
                        self.evaluation = Evaluation::Unavailable(RequestFailure::Cancelled);
                        self.focus = Control::RetryEvaluation;
                        Some(Step::CancelEvaluation)
                    }
                    Control::RetryEvaluation => {
                        self.evaluation = Evaluation::Unsent;
                        self.focus = Control::CancelEvaluation;
                        None
                    }
                    Control::RetryExplanation => {
                        self.explanation.retry();
                        None
                    }
                    Control::NextQuestion => Some(Step::NextQuestion),
                    Control::InspectSource => Some(Step::InspectSource),
                    Control::InspectJevAnswers => Some(Step::InspectJevAnswers),
                    Control::Report => Some(Step::Report),
                    Control::Finish => Some(Step::Finish),
                };
            }
            Command::HalfPageDown | Command::HalfPageUp | Command::Top | Command::Bottom => {
                // Styles do not change how the text wraps.
                let lines = self.lines(
                    question,
                    &Theme::new(ColorSupport::NoColor),
                    area.width,
                    failed,
                );
                let [body, ..] = layout(lines.len(), self.controls().len(), area);
                let last = lines.len().saturating_sub(usize::from(body.height));
                self.top = self.top.scrolled(command, last, usize::from(body.height));
            }
            command => {
                let controls = self.controls();
                let focused = self.focused();
                let at = controls
                    .iter()
                    .position(|&control| control == focused)
                    .unwrap_or_default();
                self.focus = controls[Position::at(at).after(command, controls.len()).index()];
            }
        }
        None
    }

    /// The feedback's text about `question`, drawn with `theme`, `width`
    /// cells wide, with a failed request worded by `failed`.
    fn lines(
        &self,
        question: &Question,
        theme: &Theme,
        width: u16,
        failed: &FailedRequests,
    ) -> Vec<Line<'static>> {
        let width = usize::from(width);
        let mut lines = Vec::new();
        let wrapped = |lines: &mut Vec<Line<'static>>, text: &str| {
            lines.extend(columns::wrap(text, width).into_iter().map(Line::raw));
        };
        wrapped(&mut lines, question.stem());
        lines.push(Line::default());
        if self.kind == AttemptKind::Assisted {
            lines.extend(
                columns::wrap(
                    "Assisted attempt: you chose Show answer before submitting, so its \
                     outcomes aren't independent.",
                    width,
                )
                .into_iter()
                .map(|line| Line::styled(line, theme.muted)),
            );
            lines.push(Line::default());
        }
        let chosen = self.saved.response().choice();
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
            Span::styled(self.assisted_mark(), theme.title),
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
        for line in self.saved.response().reasoning().lines() {
            if line.trim().is_empty() {
                lines.push(Line::default());
            } else {
                wrapped(&mut lines, line);
            }
        }
        lines.push(Line::default());
        self.reasoning_outcome(&mut lines, question, theme, width, failed);
        if self.durability == Durability::Durable {
            lines.push(Line::default());
            lines.push(Line::styled("Saved", theme.muted));
        }
        lines
    }

    /// Adds the reasoning's outcome to `lines`, `width` cells wide, drawn
    /// with `theme`: awaiting evaluation, unavailable, why and what to do as
    /// `failed` words it, or each criterion of `question`'s rubric with its
    /// outcome.
    fn reasoning_outcome(
        &self,
        lines: &mut Vec<Line<'static>>,
        question: &Question,
        theme: &Theme,
        width: usize,
        failed: &FailedRequests,
    ) {
        let wrapped = |lines: &mut Vec<Line<'static>>, text: &str| {
            lines.extend(columns::wrap(text, width).into_iter().map(Line::raw));
        };
        match &self.evaluation {
            Evaluation::Unsent | Evaluation::Awaiting => {
                lines.push(Line::styled("Reasoning · Awaiting evaluation", theme.title));
                wrapped(
                    lines,
                    "Jev is assessing it against each criterion. Your response is submitted.",
                );
            }
            Evaluation::Unavailable(failure) => {
                lines.push(Line::styled(
                    "Reasoning · Evaluation unavailable",
                    theme.title,
                ));
                match failure {
                    RequestFailure::Cancelled => wrapped(lines, "You cancelled the evaluation."),
                    &failure => {
                        wrapped(lines, &format!("Jev's evaluation failed: {failure}."));
                        wrapped(lines, &failed.what_to_do(Role::Jev, failure));
                    }
                }
                wrapped(
                    lines,
                    "Your response stays submitted. Retry evaluation asks Jev again.",
                );
            }
            Evaluation::Assessed(outcomes) => {
                lines.push(Line::styled("Reasoning", theme.title));
                for (place, (criterion, assessed)) in
                    question.rubric().iter().zip(outcomes).enumerate()
                {
                    let outcome = format!(
                        "{}{}",
                        match assessed.outcome() {
                            CriterionOutcome::Met => "Met",
                            CriterionOutcome::NotMet => "Not met",
                            CriterionOutcome::NeedsReview => "Needs review",
                        },
                        self.assisted_mark()
                    );
                    for (index, text) in
                        columns::wrap(&format!("{outcome} · {}", criterion.statement()), width)
                            .into_iter()
                            .enumerate()
                    {
                        lines.push(match text.strip_prefix(outcome.as_str()) {
                            Some(rest) if index == 0 => Line::from(vec![
                                Span::styled(outcome.clone(), theme.title),
                                Span::raw(rest.to_owned()),
                            ]),
                            _ => Line::raw(text),
                        });
                    }
                    if assessed.outcome() == CriterionOutcome::NeedsReview {
                        lines.extend(
                            columns::wrap(
                                "Jev couldn't tell whether your reasoning covers this.",
                                width.saturating_sub(2),
                            )
                            .into_iter()
                            .map(|text| Line::styled(format!("  {text}"), theme.muted)),
                        );
                    }
                    lines.extend(self.explanation.lines(place, theme, width, failed));
                }
            }
        }
    }

    /// What follows each outcome of an assisted attempt.
    fn assisted_mark(&self) -> &'static str {
        match self.kind {
            AttemptKind::Own => "",
            AttemptKind::Assisted => ASSISTED,
        }
    }

    /// Draws the feedback on `question`, at `place`, in `area` with `theme`,
    /// with a failed request worded by `failed`.
    pub(crate) fn render(
        &self,
        theme: &Theme,
        frame: &mut Frame<'_>,
        area: Rect,
        question: &Question,
        place: Place,
        failed: &FailedRequests,
    ) {
        let lines = self.lines(question, theme, area.width, failed);
        let controls = self.controls();
        let [body, _, buttons] = layout(lines.len(), controls.len(), area);
        let last = lines.len().saturating_sub(usize::from(body.height));
        let top = self.top.index().min(last);
        frame.render_widget(
            Paragraph::new(lines).scroll((u16::try_from(top).unwrap_or(u16::MAX), 0)),
            body,
        );
        frame.render_widget(
            List::new(controls.into_iter().map(|control| {
                theme.control(
                    control.label(),
                    ControlState {
                        focused: self.focused() == control,
                        disabled: Self::disabled(control, place),
                        ..ControlState::default()
                    },
                )
            })),
            buttons,
        );
    }
}

/// Where the feedback's text of `lines` rows, a blank row and its
/// `controls` go in `area`. The controls keep their rows on a short screen,
/// and the text scrolls above them.
fn layout(lines: usize, controls: usize, area: Rect) -> [Rect; 3] {
    let rows = u16::try_from(controls).unwrap_or(u16::MAX);
    Layout::vertical([
        Constraint::Max(u16::try_from(lines).unwrap_or(u16::MAX)),
        Constraint::Length(1),
        Constraint::Length(rows),
    ])
    .areas(area)
}
