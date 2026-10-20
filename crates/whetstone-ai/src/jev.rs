//! Jev evaluations: one call to `OpenRouter`'s `/systemone`, through
//! `rig-typesafeai`, with a JSON state and named questions.
//!
//! A question is a yes/no question or a score with ordered levels. Practice
//! never asks Jev's choice questions: their answers sum to one and always
//! pick an option, so each option gets a yes/no question of its own.
//!
//! Jev requests carry no `provider` object. Rig 0.45 can't send one, so this
//! is ADR 0001's temporary exception until #69: `TypeSafe` is Jev's only
//! provider on `OpenRouter`, and keeps no prompts.

use std::collections::BTreeMap;

use rig::typesafeai::types::{Answer, Question};
use rig::typesafeai::{DynamicQuery, Evaluate, EvaluationResult, JevConfig};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::connection::{OpenRouter, bounded};
use crate::failure::{AiError, Routing};
use crate::usage::Usage;

/// One Jev evaluation: the model to ask, the state every question is about,
/// and the questions by name.
#[derive(Debug, Clone, PartialEq)]
pub struct JevRequest {
    /// The `OpenRouter` model, such as `typesafe/jev-1.14`.
    pub model: String,
    /// The JSON state every question is evaluated against: an object, array
    /// or string, never null.
    pub state: Value,
    /// The questions, by the name their answers come back under. Names are
    /// unique and not empty, and there is at least one question.
    pub questions: BTreeMap<String, JevQuestion>,
}

/// One question to Jev.
#[derive(Debug, Clone, PartialEq)]
pub enum JevQuestion {
    /// A statement Jev answers with the probability that it holds.
    YesNo {
        /// The statement.
        instructions: String,
        /// What yes and no mean, when the statement needs them spelled out.
        meanings: Option<YesNoMeanings>,
    },
    /// A rating on ordered levels, lowest first. Jev answers with a
    /// probability for each level and their expected position.
    Score {
        /// What to rate.
        instructions: String,
        /// The levels, lowest first: two to ten of them.
        levels: Vec<String>,
    },
}

/// What yes and no mean for a yes/no question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YesNoMeanings {
    /// What a yes looks like.
    pub yes: String,
    /// What a no looks like.
    pub no: String,
}

impl JevQuestion {
    /// A yes/no question about `instructions`.
    pub fn yes_no(instructions: impl Into<String>) -> Self {
        Self::YesNo {
            instructions: instructions.into(),
            meanings: None,
        }
    }

    /// A yes/no question about `instructions`, with what `yes` and `no` look
    /// like.
    pub fn yes_no_with_meanings(
        instructions: impl Into<String>,
        yes: impl Into<String>,
        no: impl Into<String>,
    ) -> Self {
        Self::YesNo {
            instructions: instructions.into(),
            meanings: Some(YesNoMeanings {
                yes: yes.into(),
                no: no.into(),
            }),
        }
    }

    /// A score question about `instructions` on `levels`, lowest first.
    pub fn score(
        instructions: impl Into<String>,
        levels: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self::Score {
            instructions: instructions.into(),
            levels: levels.into_iter().map(Into::into).collect(),
        }
    }

    /// The question as `rig-typesafeai` sends it.
    fn definition(&self) -> Question {
        match self {
            Self::YesNo {
                instructions,
                meanings,
            } => Question::Noul {
                instructions: Value::from(instructions.as_str()),
                criteria: meanings.as_ref().map(|meanings| {
                    BTreeMap::from([
                        ("true".to_owned(), Value::from(meanings.yes.as_str())),
                        ("false".to_owned(), Value::from(meanings.no.as_str())),
                    ])
                }),
            },
            Self::Score {
                instructions,
                levels,
            } => Question::Score {
                instructions: Value::from(instructions.as_str()),
                criteria: levels
                    .iter()
                    .map(|level| Value::from(level.as_str()))
                    .collect(),
            },
        }
    }
}

/// Jev's reply: its answers as returned, by question name, with the model
/// that answered and what the call used.
#[derive(Debug, Clone, PartialEq)]
pub struct JevReply {
    /// One answer per question, under the question's name.
    pub answers: BTreeMap<String, JevAnswer>,
    /// The model `OpenRouter` reports answered, such as
    /// `typesafe/jev-1.14-20261014`.
    pub model: String,
    /// The tokens and cost the reply reported.
    pub usage: Usage,
}

/// One of Jev's answers, as returned: nothing is rounded or renormalised
/// (rule 38). Jev's `confidence` is never read.
#[derive(Debug, Clone, PartialEq)]
pub enum JevAnswer {
    /// A yes/no question's answer.
    YesNo {
        /// The probability that the answer is yes.
        yes: f64,
    },
    /// A score question's answer.
    Score {
        /// The probability of each level, in the order the levels were
        /// given.
        probabilities: Vec<f64>,
        /// The probability-weighted mean of the levels' zero-based
        /// positions.
        expected: f64,
    },
}

impl JevAnswer {
    fn from_rig(answer: Answer) -> Result<Self, AiError> {
        match answer {
            Answer::Noul { noul } => Ok(Self::YesNo { yes: noul }),
            Answer::Score {
                score,
                probabilities,
                ..
            } => {
                // Rig has checked the levels are exactly 0 to n - 1.
                let probabilities = (0..probabilities.len())
                    .map(|level| probabilities.get(&level.to_string()).copied())
                    .collect::<Option<Vec<_>>>()
                    .ok_or(AiError::InvalidReply)?;
                Ok(Self::Score {
                    probabilities,
                    expected: score,
                })
            }
            Answer::Choice { .. } => Err(AiError::InvalidReply),
        }
    }
}

impl OpenRouter {
    /// Sends `request` to Jev in one call, within the Jev time limit, and
    /// returns its answers. It ends early as [`AiError::Cancelled`] when
    /// `cancel` is cancelled.
    ///
    /// The whole reply is rejected, as [`AiError::InvalidReply`], when an
    /// answer is missing, unexpected, of another kind or out of range. A
    /// request Rig can't build (no questions, an empty name, a score without
    /// two to ten levels, a null state) is [`AiError::InvalidRequest`], and
    /// nothing is sent.
    #[tracing::instrument(name = "jev", skip_all, fields(model = %request.model))]
    pub async fn evaluate(
        &self,
        request: &JevRequest,
        cancel: &CancellationToken,
    ) -> Result<JevReply, AiError> {
        let model = request.model.as_str();
        let questions = DynamicQuery::new(
            request
                .questions
                .iter()
                .map(|(name, question)| (name.clone(), question.definition()))
                .collect(),
        )
        .map_err(|error| AiError::from_provider(&error, model, Routing::Any))?;
        let jev = JevConfig::new(self.key_for_auth_header())
            .model(model)
            .with_endpoint(self.url("/systemone"))
            .connect(self.jev_http())
            .evaluation();
        let result = bounded(self.timeouts().jev, cancel, async {
            jev.evaluate(&request.state, questions)
                .await
                .map_err(|error| AiError::from_provider(&error, model, Routing::Any))
        })
        .await?;
        JevReply::from_rig(result)
    }
}

impl JevReply {
    fn from_rig(result: EvaluationResult<BTreeMap<String, Answer>>) -> Result<Self, AiError> {
        Ok(Self {
            answers: result
                .answers
                .into_iter()
                .map(|(name, answer)| Ok((name, JevAnswer::from_rig(answer)?)))
                .collect::<Result<_, AiError>>()?,
            model: result.model,
            usage: result
                .usage
                .as_ref()
                .map(Usage::from_rig)
                .unwrap_or_default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;

    use super::*;
    use crate::Timeouts;
    use crate::replay::{
        JEV_MODEL, connection, received, sample_jev_request, sample_state, server,
    };

    async fn evaluate(request: &JevRequest, reply: &str) -> (Result<JevReply, AiError>, Value) {
        let server = server("/systemone", 200, reply, Duration::ZERO).await;
        let result = connection(&server, Timeouts::default())
            .evaluate(request, &CancellationToken::new())
            .await;
        let (auth, body) = received(&server).await;
        assert_eq!(auth, "Bearer sk-or-v1-test");
        (result, body)
    }

    #[tokio::test]
    async fn a_jev_request_carries_the_model_state_and_questions_and_no_provider() {
        let (_, body) = evaluate(
            &sample_jev_request(),
            include_str!("../fixtures/jev-answers.json"),
        )
        .await;

        // No `provider` object until Rig can send one (#69).
        assert_eq!(
            body,
            json!({
                "model": "typesafe/jev-1.14",
                "state": {
                    "change": {
                        "summary": "Retry a card charge after a timeout.",
                        "diff": "-    let key = OperationKey::new();\n+    let key = original.key.clone();\n     client.charge(amount, key).await?;",
                    },
                },
                "questions": {
                    "depth": {
                        "type": "score",
                        "instructions": "How much must a learner understand to explain why the retry reuses the key?",
                        "criteria": [
                            "Answerable by reading one line",
                            "Needs reading the change",
                            "Needs an understanding of how the change affects the code around it",
                        ],
                    },
                    "option_a": {
                        "type": "noul",
                        "instructions": "Given the change, reusing the original key means the payment provider charges at most once.",
                        "criteria": {
                            "true": "The change shows the reused key makes the provider treat the retry as the same charge",
                            "false": "The change doesn't show the reused key preventing a second charge",
                        },
                    },
                    "worth_asking": {
                        "type": "noul",
                        "instructions": "The change gives a learner something worth answering about safe retries.",
                    },
                },
            })
        );
    }

    #[tokio::test]
    async fn jev_answers_decode_as_returned_with_the_model_and_usage() {
        let (reply, _) = evaluate(
            &sample_jev_request(),
            include_str!("../fixtures/jev-answers.json"),
        )
        .await;

        assert_eq!(
            reply,
            Ok(JevReply {
                answers: BTreeMap::from([
                    (
                        "depth".to_owned(),
                        JevAnswer::Score {
                            probabilities: vec![0.07, 0.2, 0.73],
                            expected: 1.67,
                        },
                    ),
                    ("option_a".to_owned(), JevAnswer::YesNo { yes: 0.6 }),
                    ("worth_asking".to_owned(), JevAnswer::YesNo { yes: 0.76 }),
                ]),
                model: "typesafe/jev-1.14-20261014".to_owned(),
                usage: Usage {
                    input_tokens: Some(474),
                    output_tokens: Some(53),
                    cost: Some(0.000_019_908),
                },
            })
        );
    }

    #[tokio::test]
    async fn a_reply_whose_answer_is_of_another_kind_is_invalid() {
        // The spike's recorded reply rates `options` as a choice question,
        // where this request asks a yes/no question.
        let request = JevRequest {
            model: JEV_MODEL.to_owned(),
            state: sample_state(),
            questions: ["gate", "options", "criterion"]
                .into_iter()
                .map(|name| (name.to_owned(), JevQuestion::yes_no("A statement.")))
                .collect(),
        };

        let (reply, _) = evaluate(
            &request,
            include_str!("../fixtures/jev-answers-with-choice.json"),
        )
        .await;

        assert_eq!(reply, Err(AiError::InvalidReply));
    }

    #[tokio::test]
    async fn a_request_rig_cannot_build_is_invalid_and_sends_nothing() {
        let server = server("/systemone", 200, "{}", Duration::ZERO).await;
        let request = JevRequest {
            questions: BTreeMap::new(),
            ..sample_jev_request()
        };

        let reply = connection(&server, Timeouts::default())
            .evaluate(&request, &CancellationToken::new())
            .await;

        assert_eq!(reply, Err(AiError::InvalidRequest { status: None }));
        assert_eq!(
            server.received_requests().await.map(|sent| sent.len()),
            Some(0)
        );
    }

    #[tokio::test]
    async fn a_reply_that_answers_other_questions_is_invalid() {
        let request = JevRequest {
            questions: BTreeMap::from([("gate".to_owned(), JevQuestion::yes_no("A statement."))]),
            ..sample_jev_request()
        };

        let (reply, _) = evaluate(&request, include_str!("../fixtures/jev-answers.json")).await;

        assert_eq!(reply, Err(AiError::InvalidReply));
    }
}
