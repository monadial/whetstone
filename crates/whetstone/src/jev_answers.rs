//! Inspect Jev's answers, opened from a question's feedback: each of Jev's
//! answers about the question, from its question check, the evaluation of
//! the learner's reasoning and the check of its explanation. Each shows as
//! Jev returned it, a probability or, for depth, a distribution with its
//! expected level, and for the answer probe a probability for each option,
//! with its band under the assessment policy, then each request's models,
//! the policy's version and the cut-offs that banded the answers, each
//! job's marked as the policy's or from the learner's config.
//!
//! It is labelled "Jev's probability, not a measure of correctness", as
//! feedback gives only the words the bands lead to and never a number. The
//! answers show here alone: they are never logged (rules 43 and 44).

use whetstone_ai::QuestionCheckCall;
use whetstone_core::{
    AppliedCutOffs, Assessed, AssessmentPolicy, Band, CandidateCheck, CutOffsOrigin, DepthLevel,
    Job, PolicyVersion, Probability, Provenance, Question, Validation,
};

use crate::feedback::{CheckedParagraph, Evaluation, Feedback};
use crate::reader::{Reader, Section};
use crate::sent_requests::{Asking, RequestEnded, SentRequest};
use crate::view::{Reading, Route};

/// What labels Jev's numbers.
pub(crate) const LABEL: &str = "Jev's probability, not a measure of correctness";

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
    let mut sections = vec![label(question.policy()), question_check(question, requests)];
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

/// What the numbers are, and what they are not, under `policy`, the one
/// the question was checked with.
fn label(policy: &AssessmentPolicy) -> Section {
    let validation = match policy.validation() {
        Validation::Unvalidated => "unvalidated",
        Validation::Validated => "validated",
    };
    let comparable = match policy.origin() {
        CutOffsOrigin::Default => String::new(),
        CutOffsOrigin::Learner => format!(
            " Your config changes some of its cut-offs, so these bands aren't comparable with \
             policy {}'s.",
            policy.version()
        ),
    };
    Section::plain(
        LABEL,
        &format!(
            "Each number is Jev's probability that the statement beside it holds, exactly as \
             Jev returned it: it is not a score of your answer. Assessment policy {}, \
             {validation}, bands each one clear yes, unclear or clear no, and feedback says only \
             what the bands lead to.{comparable}",
            policy.version()
        ),
    )
}

/// Jev's answers to `question`'s check, before it showed, and its depth.
fn question_check(question: &Question, requests: &[SentRequest]) -> Section {
    let policy = question.policy();
    let mut lines = vec![
        models_line(
            &QuestionCheckCall::ALL
                .into_iter()
                .flat_map(|call| replied(requests, &Asking::Check(question.id().clone(), call)))
                .collect::<Vec<_>>(),
            policy.version(),
        ),
        cut_offs_line(Job::QuestionCheck, policy.cut_offs(Job::QuestionCheck)),
        cut_offs_line(Job::AnswerProbe, policy.cut_offs(Job::AnswerProbe)),
        cut_offs_line(Job::OptionRating, policy.cut_offs(Job::OptionRating)),
        cut_offs_line(
            Job::RationaleAndHintChecks,
            policy.cut_offs(Job::RationaleAndHintChecks),
        ),
    ];
    let yes_no = |(check, answer): (CandidateCheck, Probability)| {
        answered(&statement(check), answer, policy.band(check.job(), answer))
    };
    // The answer probe, check 3, is a choice, so each option has Jev's
    // probability that it answers the question, judged without the change,
    // and only the answer key's is banded.
    let (before, after): (Vec<_>, Vec<_>) = question
        .answers()
        .iter()
        .partition(|&(check, _)| check < CandidateCheck::AnswerProbe);
    lines.extend(before.into_iter().map(yes_no));
    if let Some(probe) = question.answers().probe() {
        let check = CandidateCheck::AnswerProbe;
        lines.extend(probe.iter().map(|(letter, answer)| {
            if letter == question.answer_key() {
                answered(
                    &format!("Without the change, option {letter}, the answer key, answers it"),
                    answer,
                    policy.band(check.job(), answer),
                )
            } else {
                format!(
                    "Without the change, option {letter} answers it · {}",
                    answer.get()
                )
            }
        }));
    }
    lines.extend(after.into_iter().map(yes_no));
    // The answers number the criteria as the generator wrote them, and the
    // reasoning only those the show rule kept. The show rule drops a
    // criterion only on a clear no, or on no answer at all.
    lines.extend(question.dropped_criteria().map(|(place, criterion)| {
        let why = match question
            .answers()
            .get(CandidateCheck::CriterionMinimum(place))
        {
            Some(_) => "Jev clearly said it asks for more than the question needs",
            None => "Jev didn't answer whether it asks only for what the question needs",
        };
        format!(
            "Criterion {} dropped · {} · {why}, so reasoning isn't assessed against it",
            place + 1,
            criterion.statement()
        )
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
        Evaluation::NotWritten => "Reasoning not written: Jev wasn't asked about it.".to_owned(),
        Evaluation::Unsent | Evaluation::Awaiting => {
            "Awaiting evaluation: Jev hasn't answered yet.".to_owned()
        }
        Evaluation::Unavailable(_) => {
            "Evaluation unavailable: Jev gave no answers to show.".to_owned()
        }
        Evaluation::Assessed(outcomes) => {
            let cut_offs = outcomes.first().map_or_else(
                || question.policy().cut_offs(Job::ReasoningCriterion),
                Assessed::cut_offs,
            );
            let mut lines = vec![
                models_line(requests, cut_offs.policy()),
                cut_offs_line(Job::ReasoningCriterion, cut_offs),
            ];
            lines.extend(question.rubric().iter().zip(outcomes).enumerate().map(
                |(place, (criterion, assessed))| {
                    answered(
                        &format!("Meets criterion {}: {}", place + 1, criterion.statement()),
                        assessed.answer(),
                        assessed.cut_offs().band(assessed.answer()),
                    )
                },
            ));
            lines.join("\n")
        }
    };
    Section::plain("Reasoning", &text)
}

/// Jev's `checks` of the explanation's paragraphs, in the order they came,
/// from the requests whose provenance is `requests`, each banded with the
/// cut-offs it records. A paragraph checked more than once, as Retry
/// explanation writes it again, names each check: the last is of the
/// paragraph written last.
fn explanation_check(checks: &[CheckedParagraph], requests: &[&Provenance]) -> Section {
    let mut lines = Vec::new();
    if let Some(first) = checks.first() {
        lines.push(models_line(requests, first.cut_offs.policy()));
        lines.push(cut_offs_line(Job::ExplanationCheck, first.cut_offs));
    }
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
        let band = |answer| checked.cut_offs.band(answer);
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

/// The line naming the cut-offs, `applied`, that banded `job`'s answers,
/// and whose they are.
fn cut_offs_line(job: Job, applied: AppliedCutOffs) -> String {
    let whose = match applied.origin() {
        CutOffsOrigin::Default => format!("policy {}'s", applied.policy()),
        CutOffsOrigin::Learner => "from your config".to_owned(),
    };
    format!("Cut-offs for the {job} · {} · {whose}", applied.cut_offs())
}

/// One answer's line: its `statement`, Jev's `answer` as returned, and its
/// `band`.
fn answered(statement: &str, answer: Probability, band: Band) -> String {
    format!("{statement} · {} · {}", answer.get(), band_name(band))
}

/// What a question check's `check` asks Jev, as the learner reads it.
pub(crate) fn statement(check: CandidateCheck) -> String {
    match check {
        CandidateCheck::Answerable => "Answerable from the evidence sent".to_owned(),
        CandidateCheck::AboutArea => "About its area".to_owned(),
        CandidateCheck::AnswerProbe => "Jev chose the answer without reading the change".to_owned(),
        CandidateCheck::Criterion(index) => format!(
            "Criterion {} follows from the evidence and the answer key",
            index + 1
        ),
        CandidateCheck::CriterionMinimum(index) => {
            format!(
                "Criterion {} asks only for what the question needs",
                index + 1
            )
        }
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
pub(crate) fn band_name(band: Band) -> &'static str {
    match band {
        Band::ClearYes => "clear yes",
        Band::Unclear => "unclear",
        Band::ClearNo => "clear no",
    }
}
