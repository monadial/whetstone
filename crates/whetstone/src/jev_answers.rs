//! Inspect Jev's answers, opened from a question's feedback: each of Jev's
//! answers about the question, from its question check, the evaluation of
//! the learner's reasoning and the check of its explanation. Each shows as
//! Jev returned it, a probability or, for depth, a distribution with its
//! expected level, with its band under the assessment policy, then each
//! request's models and the policy's version.
//!
//! It is labelled "Jev's probability, not a measure of correctness", as
//! feedback gives only the words the bands lead to and never a number. The
//! answers show here alone: they are never logged (rules 43 and 44).

use whetstone_core::{
    Assessed, AssessmentPolicy, Band, CandidateCheck, DepthLevel, Job, PolicyVersion, Probability,
    Provenance, Question, Validation,
};

use crate::feedback::{CheckedParagraph, Evaluation, Feedback};
use crate::reader::{Reader, Section};
use crate::sent_requests::{Asking, RequestEnded, SentRequest};
use crate::view::{Reading, Route};

/// What labels Jev's numbers.
const LABEL: &str = "Jev's probability, not a measure of correctness";

/// The reader of Jev's answers about `question`, the one at `place`, as
/// "question 2 of 4" names it, whose feedback is `feedback`, with the models
/// of the `requests` that asked them.
pub(crate) fn reader(
    place: &str,
    question: &Question,
    feedback: &Feedback,
    requests: &[SentRequest],
) -> Reader {
    let saved = feedback.saved();
    let mut sections = vec![label(), question_check(question, requests)];
    sections.push(reasoning(
        question,
        feedback.evaluation(),
        &replied(requests, &Asking::Evaluate(saved.clone())),
    ));
    if !feedback.explanation_checks().is_empty() {
        sections.push(explanation_check(
            feedback.explanation_checks(),
            &replied(requests, &Asking::CheckExplanation(saved.clone())),
        ));
    }
    Reader::new(
        Route::Read(Reading::JevAnswers),
        format!("Jev's answers about {place}"),
        sections,
    )
}

/// What the numbers are, and what they are not.
fn label() -> Section {
    let policy = AssessmentPolicy::current();
    let validation = match policy.validation() {
        Validation::Unvalidated => "unvalidated",
        Validation::Validated => "validated",
    };
    Section::plain(
        LABEL,
        &format!(
            "Each number is Jev's probability that the statement beside it holds, exactly as \
             Jev returned it: it is not a score of your answer. Assessment policy {}, \
             {validation}, bands each one clear yes, unclear or clear no, and feedback says only \
             what the bands lead to.",
            policy.version()
        ),
    )
}

/// Jev's answers to `question`'s check, before it showed, and its depth.
fn question_check(question: &Question, requests: &[SentRequest]) -> Section {
    let policy = AssessmentPolicy::current();
    let mut lines = vec![models_line(
        &replied(requests, &Asking::Check(question.id().clone())),
        question.policy(),
    )];
    lines.extend(question.answers().iter().map(|(check, answer)| {
        answered(&statement(check), answer, policy.band(check.job(), answer))
    }));
    let depth = question.answers().depth();
    let levels: Vec<String> = DepthLevel::ALL
        .into_iter()
        .map(|level| format!("{} {}", depth.probability(level).get(), level_name(level)))
        .collect();
    lines.push(format!(
        "Depth · {} · expected level {} · not banded",
        levels.join(", "),
        depth.expected()
    ));
    Section::plain("Question check", &lines.join("\n"))
}

/// Jev's answers about the learner's reasoning written for `question`, as
/// `evaluation` stands, from the requests whose provenance is `requests`.
fn reasoning(question: &Question, evaluation: &Evaluation, requests: &[&Provenance]) -> Section {
    let text = match evaluation {
        Evaluation::Unsent | Evaluation::Awaiting => {
            "Awaiting evaluation: Jev hasn't answered yet.".to_owned()
        }
        Evaluation::Unavailable(_) => {
            "Evaluation unavailable: Jev gave no answers to show.".to_owned()
        }
        Evaluation::Assessed(outcomes) => {
            let policy = AssessmentPolicy::current();
            let version = outcomes.first().map_or(policy.version(), Assessed::policy);
            let mut lines = vec![models_line(requests, version)];
            lines.extend(question.rubric().iter().zip(outcomes).enumerate().map(
                |(place, (criterion, assessed))| {
                    answered(
                        &format!("Meets criterion {}: {}", place + 1, criterion.statement()),
                        assessed.answer(),
                        policy.band(Job::ReasoningCriterion, assessed.answer()),
                    )
                },
            ));
            lines.join("\n")
        }
    };
    Section::plain("Reasoning", &text)
}

/// Jev's `checks` of the explanation's paragraphs, in the order they came,
/// from the requests whose provenance is `requests`. A paragraph checked
/// more than once, as Retry explanation writes it again, names each check:
/// the last is of the paragraph written last.
fn explanation_check(checks: &[CheckedParagraph], requests: &[&Provenance]) -> Section {
    let policy = AssessmentPolicy::current();
    let version = checks
        .first()
        .map_or(policy.version(), |checked| checked.policy);
    let mut lines = vec![models_line(requests, version)];
    for (at, checked) in checks.iter().enumerate() {
        let same = |other: &&CheckedParagraph| other.criterion == checked.criterion;
        let explanation = match checks.iter().filter(same).count() {
            1 => format!("Criterion {}'s explanation", checked.criterion + 1),
            of => format!(
                "Criterion {}'s explanation, check {} of {of},",
                checked.criterion + 1,
                checks[..=at].iter().filter(same).count()
            ),
        };
        let band = |answer| policy.band(Job::ExplanationCheck, answer);
        let answers = checked.answers;
        lines.push(answered(
            &format!("{explanation} is supported by the evidence"),
            answers.supported,
            band(answers.supported),
        ));
        lines.push(answered(
            &format!("{explanation} agrees with its outcome"),
            answers.agrees,
            band(answers.agrees),
        ));
    }
    Section::plain("Explanation check", &lines.join("\n"))
}

/// The provenance of each request in `requests` that asked `asking` and
/// was answered.
fn replied<'a>(requests: &'a [SentRequest], asking: &Asking) -> Vec<&'a Provenance> {
    requests
        .iter()
        .filter(|sent| sent.asking == *asking && sent.ended == RequestEnded::Replied)
        .map(|sent| &sent.provenance)
        .collect()
}

/// The models each request of `requests` asked and that answered, once
/// each, with the version of the policy that banded their answers.
fn models_line(requests: &[&Provenance], policy: PolicyVersion) -> String {
    let mut models: Vec<String> = Vec::new();
    for provenance in requests {
        let model = match provenance.reported_model() {
            Some(answered) => format!(
                "asked {}, answered by {answered}",
                provenance.requested_model()
            ),
            None => format!(
                "asked {}, which didn't say which model answered",
                provenance.requested_model()
            ),
        };
        if !models.contains(&model) {
            models.push(model);
        }
    }
    let models = if models.is_empty() {
        "The models aren't known".to_owned()
    } else {
        let joined = models.join("; ");
        let mut letters = joined.chars();
        letters
            .next()
            .map(|first| first.to_uppercase().chain(letters).collect())
            .unwrap_or_default()
    };
    format!("{models} · assessment policy {policy}")
}

/// One answer's line: its `statement`, Jev's `answer` as returned, and its
/// `band`.
fn answered(statement: &str, answer: Probability, band: Band) -> String {
    format!("{statement} · {} · {}", answer.get(), band_name(band))
}

/// What a question check's `check` asks Jev, as the learner reads it.
fn statement(check: CandidateCheck) -> String {
    match check {
        CandidateCheck::Answerable => "Answerable from the evidence sent".to_owned(),
        CandidateCheck::AboutArea => "About its area".to_owned(),
        CandidateCheck::NeedsChange => "Needs this change, not general knowledge".to_owned(),
        CandidateCheck::NoGiveaway => "The stem and options don't give the answer away".to_owned(),
        CandidateCheck::Criterion(index) => format!(
            "Criterion {} follows from the evidence and the answer key",
            index + 1
        ),
        CandidateCheck::Option(letter) => format!("Option {letter} answers the question correctly"),
        CandidateCheck::Rationale(letter) => {
            format!("The rationale for {letter} is supported by the evidence")
        }
        CandidateCheck::WhyItMatters => {
            "\"Why it matters\" is supported by the evidence".to_owned()
        }
        CandidateCheck::HintPointsToEvidence => "The hint points to the evidence".to_owned(),
        CandidateCheck::HintHidesAnswerKey => "The hint doesn't reveal the answer key".to_owned(),
    }
}

/// A depth level's name.
fn level_name(level: DepthLevel) -> &'static str {
    match level {
        DepthLevel::OneLine => "one line",
        DepthLevel::TheChange => "the change",
        DepthLevel::CodeAround => "the code around it",
    }
}

/// A band's name.
fn band_name(band: Band) -> &'static str {
    match band {
        Band::ClearYes => "clear yes",
        Band::Unclear => "unclear",
        Band::ClearNo => "clear no",
    }
}
