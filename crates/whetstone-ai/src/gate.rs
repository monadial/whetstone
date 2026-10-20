//! The relevance gate: one Jev call, before any question is written, that
//! asks for each chosen area whether the change is worth asking about.
//!
//! [`gate_request`] builds the call, and [`gate_answers`] reads Jev's reply
//! into a probability per area, for the assessment policy to band.

use serde_json::json;
use whetstone_core::{Area, AreaId, Evidence, ModelId, Probability};

use crate::answers::Answers;
use crate::evidence::evidence_as_sent;
use crate::failure::AiError;
use crate::jev::{JevReply, JevRequest};
use crate::prompts::GATE;

/// The Jev request that asks, with `model`, whether the change in `evidence`
/// is worth asking about in each of `areas`, the chosen areas, each once.
///
/// It asks one yes/no question per area, named by the area's ID, from the
/// base prompts' template filled with the area's name and description, with
/// the same meanings of yes and no for every area. Its state is the
/// evidence as sent, as `evidence`. Area prompts are never sent: the gate
/// judges the change against each area's description.
#[must_use]
pub fn gate_request(model: &ModelId, evidence: &Evidence, areas: &[Area]) -> JevRequest {
    JevRequest {
        model: model.as_str().to_owned(),
        state: json!({ "evidence": evidence_as_sent(evidence) }),
        questions: areas
            .iter()
            .map(|area| {
                (
                    area.id().as_str().to_owned(),
                    GATE.area
                        .question(&[("name", area.name()), ("description", area.description())]),
                )
            })
            .collect(),
    }
}

/// Jev's answer for each of `areas`, from `reply`, in their order: the
/// probability that the change is worth asking about in it, exactly as
/// returned.
///
/// Fails with [`AiError::InvalidReply`] when an answer is missing, of
/// another kind, not a probability, or for an area that wasn't asked about.
pub fn gate_answers(
    areas: &[Area],
    reply: &JevReply,
) -> Result<Vec<(AreaId, Probability)>, AiError> {
    let mut answers = Answers::of(reply);
    let answered = areas
        .iter()
        .map(|area| Ok((area.id().clone(), answers.yes_no(area.id().as_str())?)))
        .collect::<Result<Vec<_>, AiError>>()?;
    answers.done()?;
    Ok(answered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jev::JevAnswer;
    use crate::samples::{area, evaluated, evidence, jev_model, observability, pretty};

    const RECORDED: &str = include_str!("../fixtures/gate.json");

    /// The chosen areas: domain, and observability, which the sample change
    /// hardly touches.
    fn areas() -> [Area; 2] {
        [area(), observability()]
    }

    fn request() -> JevRequest {
        gate_request(&jev_model(), &evidence(), &areas())
    }

    async fn recorded() -> JevReply {
        evaluated(&request(), RECORDED)
            .await
            .0
            .expect("the recorded reply decodes")
    }

    #[tokio::test]
    async fn a_gate_request_sends_the_evidence_with_a_question_per_area() {
        let (_, body) = evaluated(&request(), "{}").await;

        insta::assert_snapshot!(pretty(&body));
    }

    #[tokio::test]
    async fn a_gate_request_sends_no_area_prompt() {
        let (_, body) = evaluated(&request(), "{}").await;
        let body = body.to_string();

        for area in areas() {
            assert!(body.contains(area.description()));
            assert!(!body.contains(area.prompt().trim()));
        }
    }

    #[tokio::test]
    async fn recorded_answers_decode_as_a_probability_per_area_in_their_order() {
        let answers = gate_answers(&areas(), &recorded().await).expect("every area is answered");

        assert_eq!(
            answers,
            [("domain", 0.89), ("observability", 0.07)].map(|(id, answer)| (
                id.parse().expect("a valid area ID"),
                Probability::new(answer).expect("a probability")
            ))
        );
    }

    #[tokio::test]
    async fn a_missing_answer_is_an_invalid_reply() {
        let mut reply = recorded().await;
        reply.answers.remove("observability");

        assert_eq!(gate_answers(&areas(), &reply), Err(AiError::InvalidReply));
    }

    #[tokio::test]
    async fn an_answer_of_another_kind_is_an_invalid_reply() {
        let mut reply = recorded().await;
        reply.answers.insert(
            "domain".to_owned(),
            JevAnswer::Score {
                probabilities: vec![0.5, 0.5],
                expected: 0.5,
            },
        );

        assert_eq!(gate_answers(&areas(), &reply), Err(AiError::InvalidReply));
    }

    #[tokio::test]
    async fn an_answer_for_an_area_not_asked_about_is_an_invalid_reply() {
        let mut reply = recorded().await;
        reply
            .answers
            .insert("security".to_owned(), JevAnswer::YesNo { yes: 0.5 });

        assert_eq!(gate_answers(&areas(), &reply), Err(AiError::InvalidReply));
    }
}
