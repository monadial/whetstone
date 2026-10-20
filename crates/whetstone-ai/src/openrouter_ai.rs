//! The AI port's real implementation: Practice's requests to Jev and the
//! generator, sent through `OpenRouter` on Rig.
//!
//! Each call builds its request from the evidence before it returns, so the
//! request it hands back owns everything it sends ([`Pending`]). The reply
//! is decoded strictly (rule 37), and its provenance is the model asked for,
//! the model that answered, and the usage and cost the reply reports. A
//! failure is one of the port's [`RequestFailure`]s.

use std::num::NonZeroUsize;

use tokio_util::sync::CancellationToken;
use whetstone_core::{
    Area, Candidate, Cost, Evidence, ModelId, Provenance, QuestionCheckAnswers, Role,
    WrittenCandidate,
};

use crate::candidates::{CandidatesOutput, candidates_request};
use crate::connection::OpenRouter;
use crate::port::{Ai, CandidateCount, Pending, Reply};
use crate::question_check::{question_check_answers, question_check_request};
use crate::usage::Usage;

/// The models Practice asks: Jev's, as `[jev] model` names it, and the
/// generator's, as `[generator] model` does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Models {
    /// The model Jev runs on, such as `typesafe/jev-1.14`.
    pub jev: ModelId,
    /// The generator's model, such as `anthropic/claude-sonnet-5.5`.
    pub generator: ModelId,
}

/// Jev and the generator on `OpenRouter`: the [`Ai`] Practice asks when a key
/// is found. Its `Debug` output redacts the key (rule 52).
#[derive(Debug, Clone)]
pub struct OpenRouterAi {
    connection: OpenRouter,
    models: Models,
}

impl OpenRouterAi {
    /// Practice's requests through `connection`, to `models`.
    #[must_use]
    pub fn new(connection: OpenRouter, models: Models) -> Self {
        Self { connection, models }
    }
}

/// A request's token never fires: a job cancels its request by dropping it
/// where it waits, which drops the HTTP request with it
/// (`Jobs::start_async` in the binary). Its time limit still applies.
fn never_cancelled() -> CancellationToken {
    CancellationToken::new()
}

impl Ai for OpenRouterAi {
    fn write_candidates(
        &self,
        evidence: &Evidence,
        area: &Area,
        count: CandidateCount,
    ) -> Pending<Vec<WrittenCandidate>> {
        // A count is at least one, so the fallback is never taken.
        let count = NonZeroUsize::new(count.get()).unwrap_or(NonZeroUsize::MIN);
        let request = candidates_request(&self.models.generator, evidence, area, count);
        let connection = self.connection.clone();
        let model = self.models.generator.clone();
        Box::pin(async move {
            let reply = connection
                .generate::<CandidatesOutput>(&request, &never_cancelled())
                .await?;
            let provenance = provenance(
                Provenance::new(Role::Generator, model),
                reply.model,
                reply.usage,
            );
            Ok(Reply {
                content: reply.output.into_written(),
                provenance,
            })
        })
    }

    fn check_question(
        &self,
        evidence: &Evidence,
        candidate: &Candidate,
    ) -> Pending<QuestionCheckAnswers> {
        let request = question_check_request(&self.models.jev, evidence, candidate);
        let candidate = candidate.clone();
        let connection = self.connection.clone();
        let model = self.models.jev.clone();
        Box::pin(async move {
            let reply = connection.evaluate(&request, &never_cancelled()).await?;
            let content = question_check_answers(&candidate, &reply)?;
            let provenance = provenance(
                Provenance::new(Role::Jev, model),
                Some(reply.model),
                reply.usage,
            );
            Ok(Reply {
                content,
                provenance,
            })
        })
    }
}

/// `requested` with what the reply reports: the model that answered, and its
/// usage and cost. A cost that isn't one (negative, say) counts as not
/// reported, since a cost is never estimated.
fn provenance(requested: Provenance, reported_model: Option<String>, usage: Usage) -> Provenance {
    let provenance = requested.with_usage(whetstone_core::Usage {
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
    });
    let provenance = match reported_model {
        Some(model) => provenance.with_reported_model(ModelId::new(model)),
        None => provenance,
    };
    match usage.cost.and_then(|usd| Cost::new(usd).ok()) {
        Some(cost) => provenance.with_cost(cost),
        None => provenance,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::Value;
    use whetstone_core::{CandidateCheck, OptionLetter, Probability, QuestionCount, Usage};

    use super::*;
    use crate::port::RequestFailure;
    use crate::replay::{connection, received, server};
    use crate::samples::{area, candidate, evidence, generator_model, jev_model};
    use crate::{AiError, Timeouts};

    const CANDIDATES: &str = include_str!("../fixtures/candidates.json");
    const QUESTION_CHECK: &str = include_str!("../fixtures/question-check.json");

    fn models() -> Models {
        Models {
            jev: jev_model(),
            generator: generator_model(),
        }
    }

    /// The real AI on a local server that answers `route` with `status` and
    /// `reply`.
    async fn ai(route: &str, status: u16, reply: &str) -> (wiremock::MockServer, OpenRouterAi) {
        let server = server(route, status, reply, Duration::ZERO).await;
        let ai = OpenRouterAi::new(connection(&server, Timeouts::default()), models());
        (server, ai)
    }

    fn two() -> CandidateCount {
        CandidateCount::per_area(QuestionCount::try_from(4).expect("from 3 to 5"), 3)
    }

    #[tokio::test]
    async fn the_generator_writes_candidates_with_the_models_it_was_asked_and_what_it_cost() {
        let (server, ai) = ai("/chat/completions", 200, CANDIDATES).await;

        let reply = ai
            .write_candidates(&evidence(), &area(), two())
            .await
            .expect("the recorded reply decodes");

        assert_eq!(reply.content.len(), 2);
        assert_eq!(
            reply.content[0].stem,
            "Before this change, the retry after a `Timeout` generated a fresh `OperationKey`. \
             Given how the provider treats keys, what could go wrong with that behaviour that \
             the change prevents?"
        );
        assert_eq!(
            reply.provenance,
            Provenance::new(Role::Generator, generator_model())
                .with_reported_model(ModelId::new("anthropic/claude-sonnet-5.5".to_owned()))
                .with_usage(Usage {
                    input_tokens: Some(3364),
                    output_tokens: Some(1580),
                })
                .with_cost(Cost::new(0.022_528).expect("a cost"))
        );
        let (_, body) = received(&server).await;
        assert_eq!(body["model"], generator_model().as_str());
        assert_eq!(
            body["provider"],
            serde_json::json!({ "zdr": true, "data_collection": "deny" })
        );
        assert_eq!(body.get("tools"), None::<&Value>);
    }

    #[tokio::test]
    async fn jev_checks_a_candidate_with_the_model_it_was_asked_and_what_it_cost() {
        let (server, ai) = ai("/systemone", 200, QUESTION_CHECK).await;

        let reply = ai
            .check_question(&evidence(), &candidate())
            .await
            .expect("the recorded reply decodes");

        let rating = |letter| {
            reply
                .content
                .get(CandidateCheck::Option(letter))
                .map(Probability::get)
        };
        assert_eq!(
            [
                rating(OptionLetter::A),
                rating(OptionLetter::B),
                rating(OptionLetter::C)
            ],
            [Some(0.98), Some(0.01), Some(0.01)]
        );
        assert_eq!(
            reply.provenance,
            Provenance::new(Role::Jev, jev_model())
                .with_reported_model(ModelId::new("typesafe/jev-1.14-20261014".to_owned()))
                .with_usage(Usage {
                    input_tokens: Some(1907),
                    output_tokens: Some(55),
                })
                .with_cost(Cost::new(0.000_080_094).expect("a cost"))
        );
        let (_, body) = received(&server).await;
        assert_eq!(body["model"], jev_model().as_str());
        assert_eq!(body.get("provider"), None::<&Value>);
        let state: Vec<&str> = body["state"]
            .as_object()
            .expect("the state is an object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(state, ["evidence", "candidate"]);
        let questions: Vec<&str> = body["questions"]
            .as_object()
            .expect("the questions are an object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(questions, ["option_a", "option_b", "option_c"]);
    }

    #[tokio::test]
    async fn a_refused_key_is_named_by_the_port() {
        let refused = include_str!("../fixtures/key-refused.json");
        let (_server, ai) = ai("/chat/completions", 401, refused).await;

        assert_eq!(
            ai.write_candidates(&evidence(), &area(), two()).await.err(),
            Some(RequestFailure::KeyRefused)
        );
    }

    #[tokio::test]
    async fn a_request_openrouter_refuses_as_invalid_is_an_invalid_request() {
        let body = r#"{"error":{"code":400,"message":"typesafe/jev-9 is not a valid model ID"}}"#;
        let (_server, ai) = ai("/systemone", 400, body).await;

        assert_eq!(
            ai.check_question(&evidence(), &candidate()).await.err(),
            Some(RequestFailure::InvalidRequest)
        );
    }

    #[tokio::test]
    async fn a_question_check_reply_that_answers_other_questions_is_an_invalid_reply() {
        // Answers to the sample request's questions, none of them a check of
        // this candidate.
        let other = include_str!("../fixtures/jev-answers.json");
        let (_server, ai) = ai("/systemone", 200, other).await;

        assert_eq!(
            ai.check_question(&evidence(), &candidate()).await.err(),
            Some(RequestFailure::InvalidReply)
        );
    }

    #[test]
    fn every_failure_keeps_its_name_across_the_port() {
        for (error, failure) in [
            (AiError::NoKey, RequestFailure::NoKey),
            (AiError::KeyRefused, RequestFailure::KeyRefused),
            (AiError::OutOfCredits, RequestFailure::OutOfCredits),
            (AiError::RateLimited, RequestFailure::RateLimited),
            (
                AiError::ProviderUnavailable { status: Some(503) },
                RequestFailure::ProviderUnavailable,
            ),
            (
                AiError::ProviderUnavailable { status: None },
                RequestFailure::ProviderUnavailable,
            ),
            (
                AiError::NoZeroRetentionEndpoint {
                    model: "cohere/command-r7b-12-2024".to_owned(),
                },
                RequestFailure::NoZeroRetentionEndpoint,
            ),
            (AiError::TimedOut, RequestFailure::TimedOut),
            (AiError::InvalidReply, RequestFailure::InvalidReply),
            (
                AiError::InvalidRequest { status: Some(403) },
                RequestFailure::InvalidRequest,
            ),
            (
                AiError::InvalidRequest { status: None },
                RequestFailure::InvalidRequest,
            ),
            (AiError::Cancelled, RequestFailure::Cancelled),
        ] {
            assert_eq!(RequestFailure::from(error.clone()), failure, "{error:?}");
        }
    }
}
