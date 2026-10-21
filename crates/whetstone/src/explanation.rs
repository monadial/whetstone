//! The explanation of what a response's reasoning missed. Once Jev has
//! assessed the reasoning, one request to the generator writes a paragraph,
//! with citations, for each criterion not met or needing review. There is
//! no request when every criterion is met.
//!
//! Plain code checks each paragraph first: it needs text and a citation,
//! and every citation must name lines the evidence sends. One request to
//! Jev then checks the paragraphs left: whether each is supported by the
//! evidence and agrees with its criterion's outcome. A paragraph shows,
//! under its criterion's outcome and labelled as the generator's, only when
//! both answers are clear yes under the assessment policy. Otherwise it is
//! withheld. A failed request leaves its paragraphs unavailable. Either
//! way, Retry explanation asks again for those paragraphs alone, once no
//! request is running.
//!
//! The outcomes are Jev's and never change: the explanation only explains
//! them.

use std::mem;

use ratatui::style::Style;
use ratatui::text::{Line, Span};
use whetstone_ai::{ParagraphCheck, RequestFailure, WrittenParagraph};
use whetstone_core::{
    Assessed, AssessmentPolicy, Band, Citation, CriterionOutcome, Evidence, Extent, Job,
};

use crate::columns;
use crate::session_record::{Assessment, ExplanationCheck, SavedResponse};
use crate::theme::Theme;

/// The explanation of one response's reasoning: where each criterion's
/// paragraph stands.
#[derive(Debug, Default)]
pub(crate) struct Explanation {
    /// Each criterion to explain, in rubric order: none when every
    /// criterion is met, or before Jev's outcomes come.
    criteria: Vec<Explained>,
}

/// A criterion to explain, and where its paragraph stands.
#[derive(Debug)]
struct Explained {
    /// Its place in the rubric, counting from 0.
    criterion: usize,
    /// Its outcome: not met, or needs review.
    outcome: CriterionOutcome,
    paragraph: Paragraph,
}

/// Where one criterion's paragraph stands.
#[derive(Debug)]
enum Paragraph {
    /// To ask the generator for, as soon as the screen starts its jobs.
    Unsent,
    /// Asked of the generator.
    Writing,
    /// Written, and through the plain checks: to ask Jev to check, as soon
    /// as the screen starts its jobs.
    Unchecked(Written),
    /// Asked of Jev's check.
    Checking(Written),
    /// It passed its checks, and shows.
    Shown(Written),
    /// It didn't pass its checks, and waits for the learner's Retry.
    Withheld,
    /// A request failed, and it waits for the learner's Retry.
    Unavailable(Failed),
}

/// A paragraph through the plain checks, with its citations as checked.
#[derive(Debug)]
struct Written {
    paragraph: WrittenParagraph,
    citations: Vec<Citation>,
}

/// Which request failed, and why.
#[derive(Debug, Clone, Copy)]
enum Failed {
    /// The generator's, writing the paragraph.
    Writing(RequestFailure),
    /// Jev's, checking it.
    Checking(RequestFailure),
}

impl Paragraph {
    /// Whether a request for it is still to send or running.
    fn is_pending(&self) -> bool {
        matches!(
            self,
            Self::Unsent | Self::Writing | Self::Unchecked(_) | Self::Checking(_)
        )
    }

    /// Whether it waits for the learner's Retry.
    fn is_retryable(&self) -> bool {
        matches!(self, Self::Withheld | Self::Unavailable(_))
    }
}

impl Explanation {
    /// The explanation of the criteria whose `outcomes`, in rubric order,
    /// are not met or need review, none asked for yet. It explains nothing
    /// when every criterion is met.
    pub(crate) fn of(outcomes: &[Assessed<CriterionOutcome>]) -> Self {
        Self {
            criteria: outcomes
                .iter()
                .enumerate()
                .filter_map(|(criterion, assessed)| match assessed.outcome() {
                    CriterionOutcome::Met => None,
                    outcome @ (CriterionOutcome::NotMet | CriterionOutcome::NeedsReview) => {
                        Some(Explained {
                            criterion,
                            outcome,
                            paragraph: Paragraph::Unsent,
                        })
                    }
                })
                .collect(),
        }
    }

    /// The criteria to ask the generator to explain, each with its
    /// outcome, if any wait to be asked for. They are then being written.
    pub(crate) fn take_request(&mut self) -> Option<Vec<(usize, CriterionOutcome)>> {
        let asked: Vec<_> = self
            .criteria
            .iter_mut()
            .filter(|explained| matches!(explained.paragraph, Paragraph::Unsent))
            .map(|explained| {
                explained.paragraph = Paragraph::Writing;
                (explained.criterion, explained.outcome)
            })
            .collect();
        (!asked.is_empty()).then_some(asked)
    }

    /// The paragraphs to ask Jev to check, in rubric order, if any wait to
    /// be checked. They are then being checked.
    pub(crate) fn take_check(&mut self) -> Option<Vec<WrittenParagraph>> {
        let mut asked = Vec::new();
        for explained in &mut self.criteria {
            if let Paragraph::Unchecked(_) = explained.paragraph
                && let Paragraph::Unchecked(written) =
                    mem::replace(&mut explained.paragraph, Paragraph::Writing)
            {
                asked.push(written.paragraph.clone());
                explained.paragraph = Paragraph::Checking(written);
            }
        }
        (!asked.is_empty()).then_some(asked)
    }

    /// Applies the generator's `reply` to the paragraphs being written for
    /// the response `saved` acknowledges: a paragraph for each, in rubric
    /// order, or why the request failed. Plain code checks each paragraph
    /// against `evidence`, withholding one that fails, and the rest wait
    /// for Jev's check. A reply that doesn't explain exactly the criteria
    /// asked about is a reply that could not be read (rule 37).
    ///
    /// Returns what to save to the session record: each paragraph withheld.
    pub(crate) fn written(
        &mut self,
        saved: &SavedResponse,
        evidence: &Evidence,
        reply: Result<Vec<WrittenParagraph>, RequestFailure>,
    ) -> Vec<Assessment> {
        let writing: Vec<(usize, CriterionOutcome)> = self
            .criteria
            .iter()
            .filter(|explained| matches!(explained.paragraph, Paragraph::Writing))
            .map(|explained| (explained.criterion, explained.outcome))
            .collect();
        // Only paragraphs being written take a result (rule 24).
        if writing.is_empty() {
            return Vec::new();
        }
        let session = saved.session();
        let attempt = saved.attempt().get();
        let paragraphs = match reply {
            Ok(paragraphs)
                if paragraphs
                    .iter()
                    .map(|paragraph| (paragraph.criterion, paragraph.outcome))
                    .eq(writing.iter().copied()) =>
            {
                paragraphs
            }
            Ok(paragraphs) => {
                tracing::warn!(
                    %session,
                    attempt,
                    written = paragraphs.len(),
                    asked = writing.len(),
                    "the generator's explanation didn't explain each criterion asked about once"
                );
                self.unavailable(Failed::Writing(RequestFailure::InvalidReply));
                return Vec::new();
            }
            Err(failure) => {
                tracing::info!(%session, attempt, %failure, "explaining the reasoning failed");
                self.unavailable(Failed::Writing(failure));
                return Vec::new();
            }
        };
        let mut withheld = Vec::new();
        for paragraph in paragraphs {
            let Some(explained) = self
                .criteria
                .iter_mut()
                .find(|explained| explained.criterion == paragraph.criterion)
            else {
                continue;
            };
            explained.paragraph = if let Some(citations) = plain_checks(&paragraph, evidence) {
                Paragraph::Unchecked(Written {
                    paragraph,
                    citations,
                })
            } else {
                withheld.push(Assessment::Explanation {
                    paragraph,
                    check: ExplanationCheck::FailedPlainChecks,
                });
                Paragraph::Withheld
            };
        }
        tracing::info!(
            %session,
            attempt,
            written = writing.len(),
            failed_plain_checks = withheld.len(),
            "the generator explained the reasoning"
        );
        withheld
    }

    /// Applies Jev's `reply` to the paragraphs being checked for the
    /// response `saved` acknowledges: its answers about each, in rubric
    /// order, which the assessment policy bands, or why the request failed.
    /// A paragraph shows only when both answers are clear yes. A reply that
    /// doesn't answer each paragraph once is a reply that could not be read
    /// (rule 37).
    ///
    /// Returns what to save to the session record: each paragraph checked,
    /// shown or withheld.
    pub(crate) fn checked(
        &mut self,
        saved: &SavedResponse,
        reply: Result<Vec<ParagraphCheck>, RequestFailure>,
    ) -> Vec<Assessment> {
        let checking = self
            .criteria
            .iter()
            .filter(|explained| matches!(explained.paragraph, Paragraph::Checking(_)))
            .count();
        // Only paragraphs being checked take a result (rule 24).
        if checking == 0 {
            return Vec::new();
        }
        let session = saved.session();
        let attempt = saved.attempt().get();
        let answers = match reply {
            Ok(answers) if answers.len() == checking => answers,
            Ok(answers) => {
                tracing::warn!(
                    %session,
                    attempt,
                    answered = answers.len(),
                    paragraphs = checking,
                    "Jev's check of the explanation didn't answer each paragraph once"
                );
                self.unavailable(Failed::Checking(RequestFailure::InvalidReply));
                return Vec::new();
            }
            Err(failure) => {
                tracing::info!(%session, attempt, %failure, "checking the explanation failed");
                self.unavailable(Failed::Checking(failure));
                return Vec::new();
            }
        };
        let policy = AssessmentPolicy::current();
        let mut checked = Vec::new();
        let paragraphs = self
            .criteria
            .iter_mut()
            .filter(|explained| matches!(explained.paragraph, Paragraph::Checking(_)));
        for (explained, answers) in paragraphs.zip(answers) {
            let Paragraph::Checking(written) =
                mem::replace(&mut explained.paragraph, Paragraph::Withheld)
            else {
                continue;
            };
            let clear = |answer| policy.band(Job::ExplanationCheck, answer) == Band::ClearYes;
            let check = if clear(answers.supported) && clear(answers.agrees) {
                ExplanationCheck::Passed {
                    answers,
                    policy: policy.version(),
                }
            } else {
                ExplanationCheck::Failed {
                    answers,
                    policy: policy.version(),
                }
            };
            checked.push(Assessment::Explanation {
                paragraph: written.paragraph.clone(),
                check,
            });
            if let ExplanationCheck::Passed { .. } = check {
                explained.paragraph = Paragraph::Shown(written);
            }
        }
        tracing::info!(
            %session,
            attempt,
            checked = checked.len(),
            shown = checked
                .iter()
                .filter(|assessment| matches!(
                    assessment,
                    Assessment::Explanation {
                        check: ExplanationCheck::Passed { .. },
                        ..
                    }
                ))
                .count(),
            "Jev checked the explanation"
        );
        checked
    }

    /// Makes every paragraph whose request is running unavailable, as
    /// `failed` says.
    fn unavailable(&mut self, failed: Failed) {
        for explained in &mut self.criteria {
            let running = match failed {
                Failed::Writing(_) => matches!(explained.paragraph, Paragraph::Writing),
                Failed::Checking(_) => matches!(explained.paragraph, Paragraph::Checking(_)),
            };
            if running {
                explained.paragraph = Paragraph::Unavailable(failed);
            }
        }
    }

    /// Whether Retry explanation is offered: a paragraph is withheld or
    /// unavailable, and no request for the explanation is running.
    pub(crate) fn can_retry(&self) -> bool {
        self.criteria
            .iter()
            .any(|explained| explained.paragraph.is_retryable())
            && !self
                .criteria
                .iter()
                .any(|explained| explained.paragraph.is_pending())
    }

    /// Asks again, once the screen starts its jobs, for each paragraph
    /// withheld or unavailable. Each paragraph that shows stays.
    pub(crate) fn retry(&mut self) {
        for explained in &mut self.criteria {
            if explained.paragraph.is_retryable() {
                explained.paragraph = Paragraph::Unsent;
            }
        }
    }

    /// The lines under the outcome of the criterion at `criterion` in the
    /// rubric, `width` cells wide, drawn with `theme`: where its paragraph
    /// stands, or the paragraph itself, labelled as the generator's, with
    /// its citations. None when it isn't explained.
    pub(crate) fn lines(
        &self,
        criterion: usize,
        theme: &Theme,
        width: usize,
    ) -> Vec<Line<'static>> {
        let Some(explained) = self
            .criteria
            .iter()
            .find(|explained| explained.criterion == criterion)
        else {
            return Vec::new();
        };
        let indented = |text: &str, style| {
            columns::wrap(text, width.saturating_sub(INDENT.len()))
                .into_iter()
                .map(move |line| Line::styled(format!("{INDENT}{line}"), style))
        };
        match &explained.paragraph {
            Paragraph::Unsent | Paragraph::Writing => indented(
                "The generator is explaining this. Jev checks it before it shows.",
                theme.muted,
            )
            .collect(),
            Paragraph::Unchecked(_) | Paragraph::Checking(_) => {
                indented("Jev is checking the generator's explanation.", theme.muted).collect()
            }
            Paragraph::Shown(written) => {
                let mut lines = vec![Line::from(vec![
                    Span::raw(INDENT),
                    Span::styled("Explanation", theme.title),
                    Span::styled(" · generator", theme.muted),
                ])];
                lines.extend(indented(&written.paragraph.text, Style::new()));
                for citation in &written.citations {
                    lines.extend(indented(
                        &format!(
                            "Cited {}, {}",
                            citation.path(),
                            Extent::Lines(citation.lines())
                        ),
                        theme.muted,
                    ));
                }
                lines
            }
            Paragraph::Withheld => {
                indented("Explanation withheld: it didn't pass checks", Style::new()).collect()
            }
            Paragraph::Unavailable(failed) => indented(
                &match failed {
                    Failed::Writing(failure) => {
                        format!(
                            "Explanation unavailable: the generator's request failed: {failure}."
                        )
                    }
                    Failed::Checking(failure) => {
                        format!("Explanation unavailable: Jev's check failed: {failure}.")
                    }
                },
                Style::new(),
            )
            .collect(),
        }
    }
}

/// How far the lines under a criterion's outcome are indented.
const INDENT: &str = "  ";

/// The citations of `paragraph`, as checked against `evidence`, if it
/// passes the plain checks: it has text, it cites the evidence, and every
/// citation names lines the evidence sends.
fn plain_checks(paragraph: &WrittenParagraph, evidence: &Evidence) -> Option<Vec<Citation>> {
    if paragraph.text.trim().is_empty() || paragraph.citations.is_empty() {
        return None;
    }
    paragraph
        .citations
        .iter()
        .map(|citation| Citation::in_evidence(citation.clone(), evidence))
        .collect()
}
