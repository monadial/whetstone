//! Test support: the sample requests the fixtures were recorded with, and a
//! local server that replays a recorded `OpenRouter` reply (rule 48).
//!
//! Every fixture under `fixtures/` is a reply body recorded live by
//! `record.rs`, with no headers, except where a test says it follows
//! `OpenRouter`'s documented error shape instead.

use std::collections::BTreeMap;
use std::time::Duration;

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::{GeneratorRequest, JevQuestion, JevRequest, OpenRouter, SecretString, Timeouts};

/// The key the tests' connections send. It never leaves the machine.
pub(crate) const TEST_KEY: &str = "sk-or-v1-test";

/// The real key, which the recorder and the live tests send: the
/// maintainer's, or the keyed end-to-end job's. Nothing else reads it, and
/// the app reads the environment in `main.rs` alone (rule 55).
pub(crate) fn live_key() -> SecretString {
    SecretString::from(
        std::env::var("OPENROUTER_API_KEY")
            .expect("set OPENROUTER_API_KEY to record the fixtures or run the live tests"),
    )
}

/// The path prefix of `OpenRouter`'s API, kept on the local server so that
/// requests go to the same paths they go to live.
pub(crate) const API: &str = "/api/v1";

pub(crate) const JEV_MODEL: &str = "typesafe/jev-1.14";
pub(crate) const GENERATOR_MODEL: &str = "anthropic/claude-sonnet-5.5";

/// The made-up change the samples are about. It is not repository code.
pub(crate) fn sample_state() -> Value {
    json!({
        "change": {
            "summary": "Retry a card charge after a timeout.",
            "diff": "-    let key = OperationKey::new();\n+    let key = original.key.clone();\n     client.charge(amount, key).await?;",
        },
    })
}

/// A Jev request with each kind of question Practice asks: a yes/no question,
/// one with its meanings spelled out, and a score.
pub(crate) fn sample_jev_request() -> JevRequest {
    JevRequest {
        model: JEV_MODEL.to_owned(),
        state: sample_state(),
        questions: BTreeMap::from([
            (
                "worth_asking".to_owned(),
                JevQuestion::yes_no(
                    "The change gives a learner something worth answering about safe retries.",
                ),
            ),
            (
                "option_a".to_owned(),
                JevQuestion::yes_no_with_meanings(
                    "Given the change, reusing the original key means the payment provider charges at most once.",
                    "The change shows the reused key makes the provider treat the retry as the same charge",
                    "The change doesn't show the reused key preventing a second charge",
                ),
            ),
            (
                "depth".to_owned(),
                JevQuestion::score(
                    "How much must a learner understand to explain why the retry reuses the key?",
                    [
                        "Answerable by reading one line",
                        "Needs reading the change",
                        "Needs an understanding of how the change affects the code around it",
                    ],
                ),
            ),
        ]),
    }
}

/// The structured output the sample generator request asks for.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
pub(crate) struct SampleQuestion {
    /// The question.
    pub(crate) stem: String,
    /// Three options, A to C.
    pub(crate) options: Vec<String>,
    /// The correct option's letter.
    pub(crate) answer_key: String,
}

pub(crate) fn sample_generator_request() -> GeneratorRequest {
    GeneratorRequest {
        model: GENERATOR_MODEL.to_owned(),
        preamble: "You write one multiple-choice question about the change you are given, \
                   with three options and the letter of the correct one."
            .to_owned(),
        input: sample_state().to_string(),
    }
}

/// A local server that answers every `POST` to `route` (such as
/// `/systemone`) with `status` and `body`, after `delay`.
pub(crate) async fn server(route: &str, status: u16, body: &str, delay: Duration) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("{API}{route}")))
        .respond_with(
            ResponseTemplate::new(status)
                .set_body_raw(body.to_owned(), "application/json")
                .set_delay(delay),
        )
        .mount(&server)
        .await;
    server
}

/// A connection to `server` with `timeouts`.
pub(crate) fn connection(server: &MockServer, timeouts: Timeouts) -> OpenRouter {
    OpenRouter::with_settings(
        Some(SecretString::from(TEST_KEY)),
        format!("{}{API}", server.uri()),
        timeouts,
    )
    .expect("the test key isn't blank")
}

/// The one request `server` received: its auth header and its JSON body,
/// which never holds the key.
pub(crate) async fn received(server: &MockServer) -> (String, Value) {
    let requests = server
        .received_requests()
        .await
        .expect("the server records requests");
    let [request] = requests.as_slice() else {
        panic!("expected one request, got {}", requests.len());
    };
    let auth = request
        .headers
        .get("authorization")
        .expect("the request has an auth header")
        .to_str()
        .expect("the auth header is text")
        .to_owned();
    // The key goes in the auth header and nowhere else (rule 41).
    assert!(!String::from_utf8_lossy(&request.body).contains(TEST_KEY));
    (auth, request.body_json().expect("the body is JSON"))
}
