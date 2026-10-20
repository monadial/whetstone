//! What provenance keeps of a Jev call: each question exactly as it was
//! sent, beside Jev's answer exactly as it was returned.

use whetstone_core::JevExchange;

use crate::answers::Answers;
use crate::failure::AiError;
use crate::jev::{JevQuestion, JevReply, JevRequest};

/// The exchanges of one Jev call, in question-name order: each question in
/// `request`, with its name, its instructions and its meanings of yes and
/// no, or its levels, beside its answer in `reply`, exactly as returned. A
/// score question is the depth score, over three levels.
///
/// Fails with [`AiError::InvalidReply`] when an answer is missing, of
/// another kind, out of range, or to a question that wasn't asked, or when
/// a score isn't over three levels.
pub fn jev_exchanges(request: &JevRequest, reply: &JevReply) -> Result<Vec<JevExchange>, AiError> {
    let mut answers = Answers::of(reply);
    let exchanges = request
        .questions
        .iter()
        .map(|(name, question)| {
            Ok(match question {
                JevQuestion::YesNo {
                    instructions,
                    meanings,
                } => JevExchange::YesNo {
                    name: name.clone(),
                    instructions: instructions.clone(),
                    yes: meanings.as_ref().map(|meanings| meanings.yes.clone()),
                    no: meanings.as_ref().map(|meanings| meanings.no.clone()),
                    answer: answers.yes_no(name)?,
                },
                JevQuestion::Score {
                    instructions,
                    levels,
                } => JevExchange::Score {
                    name: name.clone(),
                    instructions: instructions.clone(),
                    levels: levels.clone(),
                    answer: answers.depth(name)?,
                },
            })
        })
        .collect::<Result<Vec<_>, AiError>>()?;
    answers.done()?;
    Ok(exchanges)
}

#[cfg(test)]
mod tests {
    use whetstone_core::{Depth, Probability};

    use super::*;
    use crate::replay::sample_jev_request;
    use crate::samples::evaluated;

    const RECORDED: &str = include_str!("../fixtures/jev-answers.json");

    async fn recorded() -> JevReply {
        evaluated(&sample_jev_request(), RECORDED)
            .await
            .0
            .expect("the recorded reply decodes")
    }

    fn probability(value: f64) -> Probability {
        Probability::new(value).expect("the value is a probability")
    }

    #[tokio::test]
    async fn each_question_sent_is_kept_with_its_answer_as_returned() {
        let exchanges = jev_exchanges(&sample_jev_request(), &recorded().await)
            .expect("every question is answered");

        assert_eq!(
            exchanges,
            [
                JevExchange::Score {
                    name: "depth".to_owned(),
                    instructions: "How much must a learner understand to explain why the retry \
                                   reuses the key?"
                        .to_owned(),
                    levels: vec![
                        "Answerable by reading one line".to_owned(),
                        "Needs reading the change".to_owned(),
                        "Needs an understanding of how the change affects the code around it"
                            .to_owned(),
                    ],
                    answer: Depth::new(
                        [probability(0.07), probability(0.2), probability(0.73)],
                        1.67
                    )
                    .expect("the recorded depth is one"),
                },
                JevExchange::YesNo {
                    name: "option_a".to_owned(),
                    instructions: "Given the change, reusing the original key means the payment \
                                   provider charges at most once."
                        .to_owned(),
                    yes: Some(
                        "The change shows the reused key makes the provider treat the retry as \
                         the same charge"
                            .to_owned()
                    ),
                    no: Some(
                        "The change doesn't show the reused key preventing a second charge"
                            .to_owned()
                    ),
                    answer: probability(0.6),
                },
                JevExchange::YesNo {
                    name: "worth_asking".to_owned(),
                    instructions: "The change gives a learner something worth answering about \
                                   safe retries."
                        .to_owned(),
                    yes: None,
                    no: None,
                    answer: probability(0.76),
                },
            ]
        );
    }

    #[tokio::test]
    async fn a_missing_answer_is_an_invalid_reply() {
        let mut reply = recorded().await;
        reply.answers.remove("worth_asking");

        assert_eq!(
            jev_exchanges(&sample_jev_request(), &reply),
            Err(AiError::InvalidReply)
        );
    }

    #[tokio::test]
    async fn an_answer_to_a_question_not_asked_is_an_invalid_reply() {
        let mut reply = recorded().await;
        reply.answers.insert(
            "option_b".to_owned(),
            crate::jev::JevAnswer::YesNo { yes: 0.5 },
        );

        assert_eq!(
            jev_exchanges(&sample_jev_request(), &reply),
            Err(AiError::InvalidReply)
        );
    }
}
