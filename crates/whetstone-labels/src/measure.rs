//! The keyed run: each prepared call sent to Jev a set number of times, a
//! few at a time, through the same `OpenRouter` connection Practice uses,
//! and each label given Jev's answers to its question.
//!
//! Nothing is retried (rule 23): a call that fails is named in the
//! measurements, and its labels have one answer fewer.

use std::collections::BTreeSet;
use std::num::NonZeroUsize;

use futures_util::stream::{self, StreamExt as _};
use tokio_util::sync::CancellationToken;
use whetstone_ai::OpenRouter;

use crate::prepare::Prepared;
use crate::report::{Failure, Measured, Measurements};

/// How many calls are in flight at once: enough that a few hundred calls
/// take minutes, few enough not to be rate limited.
const IN_FLIGHT: usize = 4;

/// Sends each of `prepared`'s calls `runs` times through `connection`, all of
/// the first run's before the second's, and measures every label against
/// Jev's answers, read by Practice's strict decoders.
///
/// A call that fails, or whose reply doesn't decode, is a [`Failure`]. A
/// reply's cost is counted even when its answers don't decode, since it was
/// paid for; a call that fails reports no cost, so it counts as not
/// reported.
pub async fn measure(
    connection: &OpenRouter,
    prepared: &Prepared,
    runs: NonZeroUsize,
) -> Measurements {
    let cancel = CancellationToken::new();
    let attempts = (1..=runs.get()).flat_map(|run| {
        prepared
            .calls()
            .iter()
            .enumerate()
            .map(move |(index, call)| (run, index, call))
    });
    let replies: Vec<_> = stream::iter(attempts)
        .map(|(run, index, call)| {
            let cancel = &cancel;
            async move {
                let reply = connection.evaluate(call.request(), cancel).await;
                (run, index, reply)
            }
        })
        .buffered(IN_FLIGHT)
        .collect()
        .await;

    let mut labels: Vec<Measured> = prepared
        .items()
        .map(|item| Measured {
            name: item.name().to_owned(),
            job: item.job(),
            expected: item.label().expected(),
            answers: Vec::new(),
        })
        .collect();
    let starts: Vec<usize> = prepared
        .calls()
        .iter()
        .scan(0, |start, call| {
            let this = *start;
            *start += call.items().len();
            Some(this)
        })
        .collect();
    let mut measurements = Measurements {
        model: prepared.model().clone(),
        reported_models: BTreeSet::new(),
        runs: runs.get(),
        calls: replies.len(),
        failures: Vec::new(),
        cost: 0.0,
        cost_not_reported: 0,
        labels: Vec::new(),
        by_agent: prepared.by_agent(),
        skipped: prepared.skipped().to_vec(),
    };
    for (run, index, reply) in replies {
        let call = &prepared.calls()[index];
        let answers = match reply {
            Ok(reply) => {
                measurements.reported_models.insert(reply.model.clone());
                match reply.usage.cost {
                    Some(cost) => measurements.cost += cost,
                    None => measurements.cost_not_reported += 1,
                }
                call.answers(&reply)
            }
            Err(error) => {
                measurements.cost_not_reported += 1;
                Err(error)
            }
        };
        match answers {
            Ok(answers) => {
                let start = starts[index];
                let labelled = &mut labels[start..start + call.items().len()];
                for (label, answer) in labelled.iter_mut().zip(answers) {
                    label.answers.push(answer);
                }
            }
            Err(error) => measurements.failures.push(Failure {
                example: call.example().to_owned(),
                run,
                error,
            }),
        }
    }
    measurements.labels = labels;
    measurements
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::{Map, Value, json};
    use whetstone_ai::{AiError, SecretString, Timeouts};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

    use crate::samples::{CANDIDATE, GATE, prepared};

    const REPORTED_MODEL: &str = "typesafe/jev-1.14-20261014";

    /// A stand-in for Jev on `OpenRouter`'s `/systemone`, which answers every
    /// question a request asks, in the shape Jev replies in: a yes/no
    /// question with the probability `answers` gives its name, or 0.9, and a
    /// score with a fixed distribution. Each reply costs 0.0001 USD.
    struct Jev {
        answers: fn(&str) -> Option<f64>,
    }

    impl Respond for Jev {
        fn respond(&self, request: &Request) -> ResponseTemplate {
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            let mut answers = Map::new();
            for (name, question) in body["questions"].as_object().unwrap() {
                let answer = if question["type"] == "score" {
                    let levels = question["criteria"].as_array().unwrap();
                    let legend: Map<String, Value> = levels
                        .iter()
                        .enumerate()
                        .map(|(level, text)| (level.to_string(), text.clone()))
                        .collect();
                    json!({
                        "type": "score",
                        "score": 1.3,
                        "legend": legend,
                        "probabilities": { "0": 0.1, "1": 0.5, "2": 0.4 },
                        "confidence": 0.3,
                    })
                } else {
                    match (self.answers)(name) {
                        Some(yes) => json!({ "type": "noul", "noul": yes }),
                        None => continue,
                    }
                };
                answers.insert(name.clone(), answer);
            }
            ResponseTemplate::new(200).set_body_json(json!({
                "model": REPORTED_MODEL,
                "answers": answers,
                "usage": { "input_tokens": 1000, "output_tokens": 40, "cost": 0.0001 },
                "id": "gen-dec-test",
                "provider": "TypeSafe",
            }))
        }
    }

    async fn server(jev: impl Respond + 'static) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v1/systemone"))
            .respond_with(jev)
            .mount(&server)
            .await;
        server
    }

    fn connection(server: &MockServer) -> OpenRouter {
        OpenRouter::with_settings(
            Some(SecretString::from("sk-or-v1-test")),
            format!("{}/api/v1", server.uri()),
            Timeouts::default(),
        )
        .unwrap()
    }

    fn three() -> NonZeroUsize {
        NonZeroUsize::new(3).unwrap()
    }

    fn answers(measurements: &Measurements, name: &str) -> Vec<f64> {
        measurements
            .labels
            .iter()
            .find(|label| label.name == name)
            .unwrap_or_else(|| panic!("no label {name}"))
            .answers
            .iter()
            .map(|answer| answer.get())
            .collect()
    }

    #[tokio::test]
    async fn each_call_is_made_three_times_and_each_label_gets_jev_s_answer_to_its_question() {
        let server = server(Jev {
            answers: |name| match name {
                "observability" => Some(0.1),
                "option_b" => Some(0.15),
                "criterion_2_minimum" => Some(0.3),
                _ => Some(0.9),
            },
        })
        .await;
        let prepared = prepared(&format!("{GATE}{CANDIDATE}")).unwrap();

        let measurements = measure(&connection(&server), &prepared, three()).await;

        assert_eq!(server.received_requests().await.unwrap().len(), 6);
        assert_eq!((measurements.runs, measurements.calls), (3, 6));
        assert_eq!(measurements.failures, []);
        assert_eq!(answers(&measurements, "gate-retry/observability"), [0.1; 3]);
        assert_eq!(answers(&measurements, "gate-retry/domain"), [0.9; 3]);
        assert_eq!(answers(&measurements, "retry-key/option_b"), [0.15; 3]);
        assert_eq!(
            answers(&measurements, "retry-key/criterion_2_minimum"),
            [0.3; 3]
        );
        assert_eq!(
            answers(&measurements, "retry-key/hint_hides_answer_key"),
            [0.9; 3]
        );
        assert_eq!(measurements.labels.len(), 2 + 17);
        assert_eq!(measurements.by_agent, 2 + 17);
        assert!((measurements.cost - 0.0006).abs() < 1e-12);
        assert_eq!(measurements.cost_not_reported, 0);
        assert_eq!(
            measurements.reported_models,
            BTreeSet::from([REPORTED_MODEL.to_owned()])
        );
    }

    #[tokio::test]
    async fn a_call_that_fails_is_named_and_the_other_calls_are_still_measured() {
        let server = MockServer::start().await;
        // The gate's call asks about the domain area; the candidate's doesn't.
        Mock::given(method("POST"))
            .and(path("/api/v1/systemone"))
            .and(|request: &Request| {
                String::from_utf8_lossy(&request.body).contains(r#""domain":{"#)
            })
            .respond_with(ResponseTemplate::new(429).set_body_json(
                json!({ "error": { "code": 429, "message": "Rate limit exceeded" } }),
            ))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/v1/systemone"))
            .respond_with(Jev {
                answers: |_| Some(0.9),
            })
            .mount(&server)
            .await;
        let prepared = prepared(&format!("{GATE}{CANDIDATE}")).unwrap();

        let measurements = measure(&connection(&server), &prepared, three()).await;

        let failed: Vec<(&str, usize, &AiError)> = measurements
            .failures
            .iter()
            .map(|failure| (failure.example.as_str(), failure.run, &failure.error))
            .collect();
        assert_eq!(
            failed,
            [
                ("gate-retry", 1, &AiError::RateLimited),
                ("gate-retry", 2, &AiError::RateLimited),
                ("gate-retry", 3, &AiError::RateLimited),
            ]
        );
        assert_eq!(answers(&measurements, "gate-retry/domain"), [0.0; 0]);
        assert_eq!(answers(&measurements, "retry-key/option_a"), [0.9; 3]);
        assert!((measurements.cost - 0.0003).abs() < 1e-12);
        assert_eq!(measurements.cost_not_reported, 3);
    }

    #[tokio::test]
    async fn a_reply_that_leaves_a_question_unanswered_is_an_invalid_reply() {
        let server = server(Jev {
            answers: |name| (name != "option_c").then_some(0.9),
        })
        .await;
        let prepared = prepared(CANDIDATE).unwrap();

        let measurements = measure(&connection(&server), &prepared, three()).await;

        assert_eq!(measurements.failures.len(), 3);
        assert!(
            measurements
                .failures
                .iter()
                .all(|failure| failure.error == AiError::InvalidReply)
        );
        assert_eq!(answers(&measurements, "retry-key/option_a"), [0.0; 0]);
        // Rig rejects the whole reply, so its cost never arrives.
        assert_eq!(measurements.cost_not_reported, 3);
    }
}
