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
    Area, AreaId, Candidate, Cost, Evidence, ModelId, Probability, Provenance,
    QuestionCheckAnswers, Role, WrittenCandidate,
};

use crate::candidates::{CandidatesOutput, candidates_request};
use crate::connection::OpenRouter;
use crate::gate::{gate_answers, gate_request};
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
    fn check_areas(
        &self,
        evidence: &Evidence,
        areas: &[Area],
    ) -> Pending<Vec<(AreaId, Probability)>> {
        let request = gate_request(&self.models.jev, evidence, areas);
        let areas = areas.to_vec();
        let connection = self.connection.clone();
        let model = self.models.jev.clone();
        Box::pin(async move {
            let reply = connection.evaluate(&request, &never_cancelled()).await?;
            let content = gate_answers(&areas, &reply)?;
            let provenance = provenance(
                Provenance::new(Role::Jev, model),
                Some(reply.model),
                reply.usage,
            );
            log_reply(&provenance);
            Ok(Reply {
                content,
                provenance,
            })
        })
    }

    fn write_candidates(
        &self,
        evidence: &Evidence,
        area: &Area,
        count: CandidateCount,
    ) -> Pending<Vec<WrittenCandidate>> {
        let request = candidates_request(
            &self.models.generator,
            evidence,
            area,
            NonZeroUsize::from(count),
        );
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
            log_reply(&provenance);
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
            log_reply(&provenance);
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

/// Logs a reply's `provenance`: its models, token counts and cost, and
/// nothing that was sent or written (rules 43 and 44).
fn log_reply(provenance: &Provenance) {
    tracing::info!(
        role = ?provenance.role(),
        model = %provenance.requested_model(),
        answered = provenance.reported_model().map(ModelId::as_str),
        input_tokens = provenance.usage().input_tokens,
        output_tokens = provenance.usage().output_tokens,
        cost_usd = provenance.cost().map(Cost::usd),
        "OpenRouter replied"
    );
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use serde_json::Value;
    use whetstone_core::{CandidateCheck, Depth, OptionLetter, Probability, QuestionCount, Usage};

    use super::*;
    use crate::port::RequestFailure;
    use crate::replay::{connection, received, server};
    use crate::samples::{
        area, area_file, candidate, evidence, generator_model, jev_model, observability,
    };
    use crate::{AiError, Timeouts};

    const CANDIDATES: &str = include_str!("../fixtures/candidates.json");
    const QUESTION_CHECK: &str = include_str!("../fixtures/question-check.json");
    const GATE: &str = include_str!("../fixtures/gate.json");

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

    /// Two candidates for each area: 4 questions over 3 areas.
    fn two_per_area() -> CandidateCount {
        CandidateCount::per_area(QuestionCount::try_from(4).expect("from 3 to 5"), 3)
    }

    #[tokio::test]
    async fn the_generator_writes_candidates_with_the_models_it_was_asked_and_what_it_cost() {
        let (server, ai) = ai("/chat/completions", 200, CANDIDATES).await;

        let reply = ai
            .write_candidates(&evidence(), &area(), two_per_area())
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
                .with_cost(Cost::new(0.022_528).expect("0.022528 is a cost"))
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

        let candidate = candidate();
        let reply = ai
            .check_question(&evidence(), &candidate)
            .await
            .expect("the recorded reply decodes");

        let answered: Vec<CandidateCheck> = reply.content.iter().map(|(check, _)| check).collect();
        assert_eq!(answered, candidate.checks(), "an answer to every check");
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
            [Some(0.98), Some(0.01), Some(0.02)]
        );
        assert_eq!(
            reply
                .content
                .get(CandidateCheck::NoGiveaway)
                .map(Probability::get),
            Some(0.13)
        );
        let probability = |value| Probability::new(value).expect("from 0 to 1");
        assert_eq!(
            reply.content.depth(),
            Depth::new([0.11, 0.3, 0.59].map(probability), 1.47).expect("from 0 to 2"),
            "the depth's distribution and expected value, as returned"
        );
        assert_eq!(
            reply.provenance,
            Provenance::new(Role::Jev, jev_model())
                .with_reported_model(ModelId::new("typesafe/jev-1.14-20261014".to_owned()))
                .with_usage(Usage {
                    input_tokens: Some(3076),
                    output_tokens: Some(296),
                })
                .with_cost(Cost::new(0.000_129_192).expect("0.000129192 is a cost"))
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
        assert_eq!(
            questions,
            [
                "about_area",
                "answerable",
                "criterion_1",
                "criterion_2",
                "depth",
                "hint_hides_answer_key",
                "hint_points_to_evidence",
                "needs_change",
                "no_giveaway",
                "option_a",
                "option_b",
                "option_c",
                "rationale_a",
                "rationale_b",
                "rationale_c",
                "why_it_matters",
            ],
            "every check and the depth score, in the same call, named per criterion and per \
             rationale"
        );
    }

    #[tokio::test]
    async fn jev_checks_the_chosen_areas_with_the_model_it_was_asked_and_what_it_cost() {
        let (server, ai) = ai("/systemone", 200, GATE).await;
        let areas = [area(), observability()];

        let reply = ai
            .check_areas(&evidence(), &areas)
            .await
            .expect("the recorded reply decodes");

        let answers: Vec<(&str, f64)> = reply
            .content
            .iter()
            .map(|(area, answer)| (area.as_str(), answer.get()))
            .collect();
        assert_eq!(
            answers,
            [("domain", 0.89), ("observability", 0.07)],
            "an answer per area, in the areas' order, as returned"
        );
        assert_eq!(
            reply.provenance,
            Provenance::new(Role::Jev, jev_model())
                .with_reported_model(ModelId::new("typesafe/jev-1.14-20261014".to_owned()))
                .with_usage(Usage {
                    input_tokens: Some(1234),
                    output_tokens: Some(39),
                })
                .with_cost(Cost::new(0.000_051_828).expect("0.000051828 is a cost"))
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
        assert_eq!(state, ["evidence"], "the evidence alone");
        let questions: Vec<&str> = body["questions"]
            .as_object()
            .expect("the questions are an object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            questions,
            ["domain", "observability"],
            "one question per area, named by its ID"
        );
        let sent = body.to_string();
        for area in areas {
            assert!(
                !sent.contains(area.prompt().trim()),
                "{}'s area prompt reached Jev",
                area.id()
            );
        }
    }

    #[tokio::test]
    async fn a_gate_reply_that_leaves_out_an_area_is_an_invalid_reply() {
        let (_server, ai) = ai("/systemone", 200, GATE).await;
        let security = area_file(
            "security.md",
            "+++\nname = \"Security\"\ndescription = \"Who can do what\"\n+++\n\nLook for checks.\n",
        );

        assert_eq!(
            ai.check_areas(&evidence(), &[area(), observability(), security])
                .await
                .err(),
            Some(RequestFailure::InvalidReply)
        );
    }

    #[tokio::test]
    async fn a_refused_key_is_named_by_the_port() {
        let refused = include_str!("../fixtures/key-refused.json");
        let (_server, ai) = ai("/chat/completions", 401, refused).await;

        assert_eq!(
            ai.write_candidates(&evidence(), &area(), two_per_area())
                .await
                .err(),
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

    /// How each of the port's requests fails when `OpenRouter` answers
    /// `status` and `body`: the relevance gate, writing candidates and the
    /// question check, in that order.
    async fn failures(status: u16, body: &str) -> [Option<RequestFailure>; 3] {
        let (_gate, gate) = ai("/systemone", status, body).await;
        let (_write, write) = ai("/chat/completions", status, body).await;
        let (_check, check) = ai("/systemone", status, body).await;
        [
            gate.check_areas(&evidence(), &[area()]).await.err(),
            write
                .write_candidates(&evidence(), &area(), two_per_area())
                .await
                .err(),
            check.check_question(&evidence(), &candidate()).await.err(),
        ]
    }

    #[tokio::test]
    async fn each_failure_openrouter_reports_reaches_the_port_by_its_name() {
        use RequestFailure::{
            InvalidReply, InvalidRequest, KeyRefused, NoZeroRetentionEndpoint, OutOfCredits,
            ProviderUnavailable, RateLimited, TimedOut,
        };
        // Bodies that weren't recorded live follow `OpenRouter`'s documented
        // error shape: https://openrouter.ai/docs/api_reference/errors-and-debugging
        let server_error = |status| {
            format!(r#"{{"error":{{"code":{status},"message":"Internal Server Error"}}}}"#)
        };
        for (status, body, named) in [
            (
                401,
                include_str!("../fixtures/key-refused.json").to_owned(),
                [KeyRefused; 3],
            ),
            (
                402,
                r#"{"error":{"code":402,"message":"Insufficient credits"}}"#.to_owned(),
                [OutOfCredits; 3],
            ),
            (
                429,
                r#"{"error":{"code":429,"message":"Rate limit exceeded"}}"#.to_owned(),
                [RateLimited; 3],
            ),
            (500, server_error(500), [ProviderUnavailable; 3]),
            (502, server_error(502), [ProviderUnavailable; 3]),
            (503, server_error(503), [ProviderUnavailable; 3]),
            // Only the generator's requests ask for zero retention until #69,
            // so only for them is a refused route about it.
            (
                404,
                include_str!("../fixtures/generator-no-zero-retention.json").to_owned(),
                [
                    ProviderUnavailable,
                    NoZeroRetentionEndpoint,
                    ProviderUnavailable,
                ],
            ),
            (
                400,
                r#"{"error":{"code":400,"message":"not a valid model ID"}}"#.to_owned(),
                [InvalidRequest; 3],
            ),
            (
                403,
                r#"{"error":{"code":403,"message":"Input was flagged","metadata":{"reasons":["moderation"]}}}"#
                    .to_owned(),
                [InvalidRequest; 3],
            ),
            (
                408,
                r#"{"error":{"code":408,"message":"Request timed out"}}"#.to_owned(),
                [TimedOut; 3],
            ),
            (200, "not json".to_owned(), [InvalidReply; 3]),
        ] {
            assert_eq!(
                failures(status, &body).await,
                named.map(Some),
                "status {status}: {body}"
            );
        }
    }

    /// The real AI on a local server that answers `route` only after ten
    /// minutes, with the default time limits but for connecting, which can't
    /// fire first: a paused clock jumps to the next timer whenever the
    /// runtime waits on the socket.
    async fn slow(route: &str) -> (wiremock::MockServer, OpenRouterAi) {
        let server = server(route, 200, "{}", Duration::from_secs(600)).await;
        let timeouts = Timeouts {
            connect: Duration::from_secs(3600),
            ..Timeouts::default()
        };
        let ai = OpenRouterAi::new(connection(&server, timeouts), models());
        (server, ai)
    }

    #[tokio::test(start_paused = true)]
    async fn jev_s_requests_time_out_after_30_seconds() {
        let (_gate, gate) = slow("/systemone").await;
        let started = tokio::time::Instant::now();
        assert_eq!(
            gate.check_areas(&evidence(), &[area()]).await.err(),
            Some(RequestFailure::TimedOut)
        );
        assert_eq!(started.elapsed(), Duration::from_secs(30), "the gate");

        let (_check, check) = slow("/systemone").await;
        let started = tokio::time::Instant::now();
        assert_eq!(
            check.check_question(&evidence(), &candidate()).await.err(),
            Some(RequestFailure::TimedOut)
        );
        assert_eq!(
            started.elapsed(),
            Duration::from_secs(30),
            "the question check"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn the_generator_s_requests_time_out_after_120_seconds() {
        let (_server, ai) = slow("/chat/completions").await;
        let started = tokio::time::Instant::now();

        assert_eq!(
            ai.write_candidates(&evidence(), &area(), two_per_area())
                .await
                .err(),
            Some(RequestFailure::TimedOut)
        );
        assert_eq!(started.elapsed(), Duration::from_secs(120));
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

    /// Log lines, as the app's log file gets them.
    #[derive(Debug, Clone, Default)]
    struct Log(Arc<Mutex<Vec<u8>>>);

    impl io::Write for Log {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0
                .lock()
                .expect("no test thread panics holding the log")
                .extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Log {
        fn text(&self) -> String {
            String::from_utf8_lossy(
                &self
                    .0
                    .lock()
                    .expect("no test thread panics holding the log"),
            )
            .into_owned()
        }
    }

    #[tokio::test]
    async fn a_request_logs_its_models_and_cost_and_nothing_it_sends_or_gets_back() {
        let log = Log::default();
        let writer = log.clone();
        // Every target, Rig's included, at the level the app logs at, in
        // plain text as the app writes it.
        let _logging = tracing::subscriber::set_default(
            tracing_subscriber::fmt()
                .with_writer(move || writer.clone())
                .with_max_level(tracing::Level::INFO)
                .with_ansi(false)
                .finish(),
        );

        let (_generator, generator) = ai("/chat/completions", 200, CANDIDATES).await;
        generator
            .write_candidates(&evidence(), &area(), two_per_area())
            .await
            .expect("the recorded reply decodes");
        let (_jev, jev) = ai("/systemone", 200, QUESTION_CHECK).await;
        jev.check_question(&evidence(), &candidate())
            .await
            .expect("the recorded reply decodes");
        let (_gate, gate) = ai("/systemone", 200, GATE).await;
        gate.check_areas(&evidence(), &[area(), observability()])
            .await
            .expect("the recorded reply decodes");

        let log = log.text();
        assert!(log.contains("OpenRouter replied"), "{log}");
        assert!(
            log.contains("cost_usd=0.022528"),
            "the generator's cost: {log}"
        );
        assert!(log.contains("cost_usd=0.000129192"), "Jev's cost: {log}");
        assert!(log.contains("cost_usd=5.1828e-5"), "the gate's cost: {log}");
        for never in [
            // The key.
            crate::replay::TEST_KEY,
            // A base prompt, the area prompt, and the evidence.
            "You write candidate questions for Whetstone",
            "Given the evidence",
            "Focus on the rules the change encodes",
            // The gate's question about an area.
            "worth answering about the area",
            "Reuse the charge's key when retrying after a timeout",
            "let key = original.key.clone();",
            // A candidate sent to Jev, and one the generator wrote.
            "Why does the retry in `charge_with_retry`",
            "Before this change, the retry after a `Timeout`",
        ] {
            assert!(!log.contains(never), "{never:?} reached the log:\n{log}");
        }
    }
}
