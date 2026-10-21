//! The question check: one Jev call per candidate, whose state is the
//! evidence plus the candidate, and whose yes/no questions are the
//! candidate's checks.
//!
//! [`question_check_request`] builds the call, and
//! [`question_check_answers`] reads Jev's reply into whetstone-core's
//! [`QuestionCheckAnswers`] for the show rule.

use serde_json::{Map, Value, json};
use whetstone_core::{
    Candidate, CandidateCheck, Evidence, ModelId, OptionLetter, QuestionCheckAnswers,
};

use crate::answers::Answers;
use crate::evidence::evidence_as_sent;
use crate::failure::AiError;
use crate::jev::{JevQuestion, JevReply, JevRequest};
use crate::prompts::QUESTION_CHECK;

/// The Jev request that checks `candidate` with `model`: one yes/no question
/// per check in [`Candidate::checks`], each named after its check, with its
/// instructions from the base prompts. They are `answerable`, `about_area`,
/// `needs_change` and `no_giveaway`; `criterion_1` and on, one per criterion;
/// `option_a` to `option_c`, the option ratings; `rationale_a` to
/// `rationale_c` and `why_it_matters`; and `hint_points_to_evidence` and
/// `hint_hides_answer_key`. It also scores the candidate's `depth` over
/// three levels: answerable by reading one line, needs reading the change,
/// and needs an understanding of how the change affects the code around it.
///
/// Its state is the evidence as sent, as `evidence`, and the whole candidate
/// as `candidate`: its area's name and description, never its area prompt,
/// its stem, its options with their rationales, its answer key, its "why it
/// matters" line, its rubric, its hint and its citations.
#[must_use]
pub fn question_check_request(
    model: &ModelId,
    evidence: &Evidence,
    candidate: &Candidate,
) -> JevRequest {
    JevRequest {
        model: model.as_str().to_owned(),
        state: json!({
            "evidence": evidence_as_sent(evidence),
            "candidate": candidate_state(evidence, candidate),
        }),
        questions: candidate
            .checks()
            .into_iter()
            .map(|check| (name(check), question(check)))
            .chain([(DEPTH.to_owned(), QUESTION_CHECK.depth.question())])
            .collect(),
    }
}

/// Jev's answers to `candidate`'s question check, from `reply`, exactly as
/// returned: each check's probability of yes, and the depth's distribution
/// and expected value. Jev's `confidence` is never read.
///
/// Fails with [`AiError::InvalidReply`] when an answer is missing, of
/// another kind, not a probability, or to a question the check didn't ask,
/// or when the depth isn't three probabilities with an expected value from
/// 0 to 2.
pub fn question_check_answers(
    candidate: &Candidate,
    reply: &JevReply,
) -> Result<QuestionCheckAnswers, AiError> {
    let mut answers = Answers::of(reply);
    let checks = candidate
        .checks()
        .into_iter()
        .map(|check| Ok((check, answers.yes_no(&name(check))?)))
        .collect::<Result<Vec<_>, AiError>>()?;
    let depth = answers.depth(DEPTH)?;
    answers.done()?;
    Ok(QuestionCheckAnswers::new(checks, depth))
}

/// The name the depth score goes under.
const DEPTH: &str = "depth";

/// The name a check's question goes under, such as `criterion_1` or
/// `option_a`.
fn name(check: CandidateCheck) -> String {
    match check {
        CandidateCheck::Answerable => "answerable".to_owned(),
        CandidateCheck::AboutArea => "about_area".to_owned(),
        CandidateCheck::NeedsChange => "needs_change".to_owned(),
        CandidateCheck::NoGiveaway => "no_giveaway".to_owned(),
        CandidateCheck::Criterion(index) => criterion_name(index),
        CandidateCheck::Option(letter) => format!("option_{}", lowercase(letter)),
        CandidateCheck::Rationale(letter) => format!("rationale_{}", lowercase(letter)),
        CandidateCheck::WhyItMatters => "why_it_matters".to_owned(),
        CandidateCheck::HintPointsToEvidence => "hint_points_to_evidence".to_owned(),
        CandidateCheck::HintHidesAnswerKey => "hint_hides_answer_key".to_owned(),
    }
}

/// A check's question, from the base prompts.
fn question(check: CandidateCheck) -> JevQuestion {
    let prompts = &*QUESTION_CHECK;
    match check {
        CandidateCheck::Answerable => prompts.answerable.question(&[]),
        CandidateCheck::AboutArea => prompts.about_area.question(&[]),
        CandidateCheck::NeedsChange => prompts.needs_change.question(&[]),
        CandidateCheck::NoGiveaway => prompts.no_giveaway.question(&[]),
        CandidateCheck::Criterion(index) => prompts
            .criterion
            .question(&[("criterion", &criterion_name(index))]),
        CandidateCheck::Option(letter) => {
            prompts.option.question(&[("letter", &letter.to_string())])
        }
        CandidateCheck::Rationale(letter) => prompts
            .rationale
            .question(&[("letter", &letter.to_string())]),
        CandidateCheck::WhyItMatters => prompts.why_it_matters.question(&[]),
        CandidateCheck::HintPointsToEvidence => prompts.hint_points_to_evidence.question(&[]),
        CandidateCheck::HintHidesAnswerKey => prompts.hint_hides_answer_key.question(&[]),
    }
}

fn lowercase(letter: OptionLetter) -> String {
    letter.to_string().to_ascii_lowercase()
}

/// The candidate as Jev's state holds it.
fn candidate_state(evidence: &Evidence, candidate: &Candidate) -> Value {
    let mut area = json!({ "id": candidate.area().as_str() });
    // The area's name and description, from the areas the evidence was
    // approved with; its area prompt is never sent to Jev.
    if let Some(found) = evidence
        .areas()
        .iter()
        .find(|area| area.id() == candidate.area())
    {
        area["name"] = json!(found.name());
        area["description"] = json!(found.description());
    }
    let options: Map<String, Value> = OptionLetter::ALL
        .into_iter()
        .map(|letter| {
            (
                letter.to_string(),
                json!({
                    "text": candidate.option(letter),
                    "rationale": candidate.rationale(letter),
                }),
            )
        })
        .collect();
    let rubric: Map<String, Value> = candidate
        .rubric()
        .iter()
        .enumerate()
        .map(|(index, criterion)| {
            (
                criterion_name(index),
                json!({
                    "statement": criterion.statement(),
                    "met": criterion.met(),
                    "not_met": criterion.not_met(),
                }),
            )
        })
        .collect();
    let citations: Vec<Value> = candidate
        .citations()
        .iter()
        .map(|citation| {
            json!({
                "path": citation.path(),
                "first_line": citation.lines().first(),
                "last_line": citation.lines().last(),
            })
        })
        .collect();
    json!({
        "area": area,
        "stem": candidate.stem(),
        "options": options,
        "answer_key": candidate.answer_key().to_string(),
        "why_it_matters": candidate.why_it_matters(),
        "rubric": rubric,
        "hint": candidate.hint().text(),
        "citations": citations,
    })
}

/// The name of the rubric's criterion at `index`, counting from 0:
/// `criterion_1` for the first.
fn criterion_name(index: usize) -> String {
    format!("criterion_{}", index + 1)
}

#[cfg(test)]
mod tests {
    use whetstone_core::{Depth, Probability};

    use super::*;
    use crate::jev::JevAnswer;
    use crate::samples::{candidate, evaluated, evidence, jev_model, pretty};

    const RECORDED: &str = include_str!("../fixtures/question-check.json");

    fn request() -> JevRequest {
        question_check_request(&jev_model(), &evidence(), &candidate())
    }

    #[tokio::test]
    async fn a_question_check_sends_the_evidence_and_the_candidate_with_a_question_per_check() {
        let (_, body) = evaluated(&request(), "{}").await;

        insta::assert_snapshot!(pretty(&body));
    }

    #[tokio::test]
    async fn a_question_check_sends_the_area_name_and_description_never_its_prompt() {
        let (_, body) = evaluated(&request(), "{}").await;

        assert_eq!(
            body["state"]["candidate"]["area"],
            json!({
                "id": "domain",
                "name": "Domain",
                "description": "The rules, concepts and language the work is about",
            })
        );
        assert!(!body.to_string().contains("Focus on the rules"));
    }

    /// The recorded reply to the sample candidate's check.
    async fn recorded() -> JevReply {
        evaluated(&request(), RECORDED)
            .await
            .0
            .expect("the recorded reply decodes")
    }

    fn probability(value: f64) -> Probability {
        Probability::new(value).expect("the value is a probability")
    }

    #[tokio::test]
    async fn recorded_answers_decode_as_returned() {
        let answers = question_check_answers(&candidate(), &recorded().await)
            .expect("the recorded reply answers every check");

        assert_eq!(
            answers.iter().collect::<Vec<_>>(),
            [
                (CandidateCheck::Answerable, 0.96),
                (CandidateCheck::AboutArea, 0.76),
                (CandidateCheck::NeedsChange, 0.57),
                (CandidateCheck::NoGiveaway, 0.13),
                (CandidateCheck::Criterion(0), 0.8),
                (CandidateCheck::Criterion(1), 0.93),
                (CandidateCheck::Option(OptionLetter::A), 0.98),
                (CandidateCheck::Option(OptionLetter::B), 0.01),
                (CandidateCheck::Option(OptionLetter::C), 0.02),
                (CandidateCheck::Rationale(OptionLetter::A), 0.97),
                (CandidateCheck::Rationale(OptionLetter::B), 0.88),
                (CandidateCheck::Rationale(OptionLetter::C), 0.93),
                (CandidateCheck::WhyItMatters, 0.81),
                (CandidateCheck::HintPointsToEvidence, 0.95),
                (CandidateCheck::HintHidesAnswerKey, 0.13),
            ]
            .map(|(check, answer)| (check, probability(answer)))
        );
        assert_eq!(
            answers.depth(),
            Depth::new(
                [probability(0.11), probability(0.3), probability(0.59)],
                1.47
            )
            .expect("the recorded expected depth is from 0 to 2")
        );
    }

    #[tokio::test]
    async fn a_depth_that_is_not_a_score_over_three_levels_is_an_invalid_reply() {
        for depth in [
            JevAnswer::YesNo { yes: 0.5 },
            JevAnswer::Score {
                probabilities: vec![0.4, 0.6],
                expected: 0.6,
            },
        ] {
            let mut reply = recorded().await;
            reply.answers.insert("depth".to_owned(), depth);

            assert_eq!(
                question_check_answers(&candidate(), &reply),
                Err(AiError::InvalidReply)
            );
        }
    }

    #[tokio::test]
    async fn a_missing_answer_is_an_invalid_reply() {
        for missing in ["option_b", "depth"] {
            let mut reply = recorded().await;
            reply.answers.remove(missing);

            assert_eq!(
                question_check_answers(&candidate(), &reply),
                Err(AiError::InvalidReply),
                "{missing} missing"
            );
        }
    }

    #[tokio::test]
    async fn an_answer_of_another_kind_is_an_invalid_reply() {
        let mut reply = recorded().await;
        reply.answers.insert(
            "option_b".to_owned(),
            JevAnswer::Score {
                probabilities: vec![0.5, 0.5],
                expected: 0.5,
            },
        );

        assert_eq!(
            question_check_answers(&candidate(), &reply),
            Err(AiError::InvalidReply)
        );
    }

    #[tokio::test]
    async fn an_answer_to_a_question_not_asked_is_an_invalid_reply() {
        let mut reply = recorded().await;
        reply
            .answers
            .insert("option_d".to_owned(), JevAnswer::YesNo { yes: 0.5 });

        assert_eq!(
            question_check_answers(&candidate(), &reply),
            Err(AiError::InvalidReply)
        );
    }

    #[tokio::test]
    async fn an_answer_that_is_not_a_probability_is_an_invalid_reply() {
        let mut reply = recorded().await;
        reply
            .answers
            .insert("option_b".to_owned(), JevAnswer::YesNo { yes: 1.2 });

        assert_eq!(
            question_check_answers(&candidate(), &reply),
            Err(AiError::InvalidReply)
        );
    }
}
