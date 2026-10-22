//! What provenance keeps of a Jev call: each question exactly as it was
//! sent, beside Jev's answer exactly as it was returned.

use whetstone_core::JevExchange;

use crate::answers::Answers;
use crate::failure::AiError;
use crate::jev::{JevQuestion, JevReply, JevRequest};

/// The exchanges of one Jev call, in question-name order: each question in
/// `request`, with its name, its instructions and its meanings of yes and
/// no, its levels or its options, beside its answer in `reply`, exactly as
/// returned. A score question is the depth score, over three levels.
///
/// Fails with [`AiError::InvalidReply`] when an answer is missing, of
/// another kind, out of range, or to a question that wasn't asked, when a
/// score isn't over three levels, or when a choice's probabilities aren't
/// for exactly the options asked.
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
                JevQuestion::Choice {
                    instructions,
                    options,
                } => JevExchange::Choice {
                    name: name.clone(),
                    instructions: instructions.clone(),
                    options: options.clone(),
                    answer: options
                        .keys()
                        .cloned()
                        .zip(answers.choice(name, options.keys().map(String::as_str))?)
                        .collect(),
                },
            })
        })
        .collect::<Result<Vec<_>, AiError>>()?;
    answers.done()?;
    Ok(exchanges)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use whetstone_core::{Depth, Probability};

    use super::*;
    use crate::jev::JevAnswer;
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

    /// The sample request with a choice question, `pick`, over options A
    /// and B, and the recorded reply with `answer` to it.
    async fn with_choice(answer: &[(&str, f64)]) -> (JevRequest, JevReply) {
        let mut request = sample_jev_request();
        request.questions.insert(
            "pick".to_owned(),
            JevQuestion::choice("Which option?", [("A", "Option A."), ("B", "Option B.")]),
        );
        let mut reply = recorded().await;
        reply.answers.insert(
            "pick".to_owned(),
            JevAnswer::Choice {
                probabilities: answer
                    .iter()
                    .map(|&(label, answer)| (label.to_owned(), answer))
                    .collect(),
            },
        );
        (request, reply)
    }

    #[tokio::test]
    async fn a_choice_is_kept_with_its_options_and_a_probability_for_each() {
        let (request, reply) = with_choice(&[("A", 0.25), ("B", 0.75)]).await;

        let exchanges = jev_exchanges(&request, &reply).expect("every question is answered");

        assert_eq!(
            exchanges.iter().find(|exchange| exchange.name() == "pick"),
            Some(&JevExchange::Choice {
                name: "pick".to_owned(),
                instructions: "Which option?".to_owned(),
                options: BTreeMap::from([
                    ("A".to_owned(), "Option A.".to_owned()),
                    ("B".to_owned(), "Option B.".to_owned()),
                ]),
                answer: BTreeMap::from([
                    ("A".to_owned(), probability(0.25)),
                    ("B".to_owned(), probability(0.75)),
                ]),
            })
        );
    }

    #[tokio::test]
    async fn a_choice_answered_for_other_options_or_not_with_probabilities_is_an_invalid_reply() {
        for answer in [
            &[("A", 1.0)][..],
            &[("A", 0.5), ("C", 0.5)],
            &[("A", 0.5), ("B", 0.25), ("C", 0.25)],
            &[("A", 1.2), ("B", -0.2)],
        ] {
            let (request, reply) = with_choice(answer).await;

            assert_eq!(
                jev_exchanges(&request, &reply),
                Err(AiError::InvalidReply),
                "{answer:?}"
            );
        }
    }

    #[tokio::test]
    async fn an_answer_to_a_question_not_asked_is_an_invalid_reply() {
        let mut reply = recorded().await;
        reply
            .answers
            .insert("option_b".to_owned(), JevAnswer::YesNo { yes: 0.5 });

        assert_eq!(
            jev_exchanges(&sample_jev_request(), &reply),
            Err(AiError::InvalidReply)
        );
    }
}
