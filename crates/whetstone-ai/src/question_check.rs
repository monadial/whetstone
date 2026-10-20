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
/// per check in [`Candidate::checks`], each named after its check, such as
/// `option_a`, with its instructions from the base prompts.
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
            .collect(),
    }
}

/// Jev's answers to `candidate`'s question check, from `reply`, exactly as
/// returned.
///
/// Fails with [`AiError::InvalidReply`] when an answer is missing, of
/// another kind, not a probability, or to a question the check didn't ask.
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
    answers.done()?;
    Ok(QuestionCheckAnswers::new(checks))
}

/// The name a check's question goes under, such as `option_a`.
fn name(check: CandidateCheck) -> String {
    match check {
        CandidateCheck::Option(letter) => format!("option_{}", lowercase(letter)),
    }
}

/// A check's question, from the base prompts.
fn question(check: CandidateCheck) -> JevQuestion {
    match check {
        CandidateCheck::Option(letter) => QUESTION_CHECK
            .option
            .question(&[("letter", &letter.to_string())]),
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
    use whetstone_core::Probability;

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

    fn probability(value: f64) -> Option<Probability> {
        Probability::new(value).ok()
    }

    #[tokio::test]
    async fn recorded_answers_decode_as_returned() {
        let answers = question_check_answers(&candidate(), &recorded().await)
            .expect("the recorded reply answers every check");

        assert_eq!(
            [
                answers.get(CandidateCheck::Option(OptionLetter::A)),
                answers.get(CandidateCheck::Option(OptionLetter::B)),
                answers.get(CandidateCheck::Option(OptionLetter::C)),
            ],
            [probability(0.98), probability(0.01), probability(0.01)]
        );
    }

    #[tokio::test]
    async fn a_missing_answer_is_an_invalid_reply() {
        let mut reply = recorded().await;
        reply.answers.remove("option_b");

        assert_eq!(
            question_check_answers(&candidate(), &reply),
            Err(AiError::InvalidReply)
        );
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
