//! The recorder: re-records the fixtures from live `OpenRouter` replies.
//!
//! Each recording runs the real call through a local server that forwards
//! the request to `OpenRouter` and saves the reply's body, never its headers,
//! under `fixtures/`. The recordings are ignored, so they run only when asked
//! for, and need the maintainer's key, which costs a few cents:
//!
//! ```sh
//! OPENROUTER_API_KEY=… cargo test -p whetstone-ai --locked record:: -- --ignored --test-threads 1
//! ```
//!
//! A re-recorded reply changes values the contract tests assert, so update
//! them in the same change. One check here runs on every PR instead: every
//! fixture is JSON, with no key or auth header in it.
//!
//! Per-PR tests never call `OpenRouter` (rule 48); the recordings are one of
//! its stated exceptions, with the keyed end-to-end job's live tests
//! (`live.rs`).

use std::path::PathBuf;
use std::time::Duration;

use rig::rig_reqwest::reqwest;
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

use crate::replay::{API, SampleQuestion, sample_generator_request, sample_jev_request};
use crate::samples::{area, candidate, evidence, generator_model, jev_model};
use crate::{
    AiError, CandidatesOutput, GeneratorRequest, OpenRouter, SecretString, Timeouts,
    candidates_request, question_check_request,
};

const UPSTREAM: &str = "https://openrouter.ai";

/// A model with no zero-retention endpoint on `OpenRouter`, so that the
/// generator's routing is refused.
const MODEL_WITHOUT_ZERO_RETENTION: &str = "cohere/command-r7b-12-2024";

/// Forwards each request to `OpenRouter` and saves the reply's body as the
/// fixture `name`.
struct Forward {
    name: &'static str,
}

impl Respond for Forward {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let url = format!("{UPSTREAM}{}", request.url.path());
        let headers = request.headers.clone();
        let body = request.body.clone();
        // The server answers synchronously, so the forwarded call runs on a
        // runtime of its own.
        let (status, reply) = std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("a runtime can be built")
                .block_on(async move {
                    let mut forwarded = reqwest::Client::new().post(url).body(body);
                    for name in ["authorization", "content-type"] {
                        if let Some(value) = headers.get(name) {
                            forwarded = forwarded.header(name, value.as_bytes());
                        }
                    }
                    let response = forwarded.send().await.expect("OpenRouter replies");
                    let status = response.status().as_u16();
                    (
                        status,
                        response.bytes().await.expect("the reply has a body"),
                    )
                })
        })
        .join()
        .expect("the forwarding thread ends");
        let path = fixture(self.name);
        std::fs::write(&path, &reply).expect("the fixture is written");
        println!("{}: HTTP {status}, saved to {}", self.name, path.display());
        ResponseTemplate::new(status).set_body_raw(reply.to_vec(), "application/json")
    }
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(format!("{name}.json"))
}

/// A connection with `key` through a local server that records the reply
/// to `route` as the fixture `name`.
async fn recording(name: &'static str, route: &str, key: &str) -> (MockServer, OpenRouter) {
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::path(format!("{API}{route}")))
        .respond_with(Forward { name })
        .mount(&server)
        .await;
    let connection = OpenRouter::with_settings(
        Some(SecretString::from(key)),
        format!("{}{API}", server.uri()),
        // Live calls get the real limits, with room for a slow generator.
        Timeouts {
            generator: Duration::from_secs(300),
            ..Timeouts::default()
        },
    )
    .expect("the key isn't blank");
    (server, connection)
}

/// The maintainer's key, or the keyed end-to-end job's. Only the recorder
/// and the live tests read it: the app reads the environment in `main.rs`
/// alone (rule 55).
pub(crate) fn live_key() -> String {
    std::env::var("OPENROUTER_API_KEY")
        .expect("set OPENROUTER_API_KEY to record the fixtures or run the live tests")
}

#[tokio::test]
#[ignore = "calls OpenRouter with the maintainer's key"]
async fn record_jev_answers() {
    let (_server, connection) = recording("jev-answers", "/systemone", &live_key()).await;

    let reply = connection
        .evaluate(&sample_jev_request(), &CancellationToken::new())
        .await;

    println!("{reply:#?}");
    assert!(reply.is_ok());
}

#[tokio::test]
#[ignore = "calls OpenRouter with the maintainer's key"]
async fn record_generator_output() {
    let (_server, connection) =
        recording("generator-output", "/chat/completions", &live_key()).await;

    let reply = connection
        .generate::<SampleQuestion>(&sample_generator_request(), &CancellationToken::new())
        .await;

    println!("{reply:#?}");
    assert!(reply.is_ok());
}

#[tokio::test]
#[ignore = "calls OpenRouter with the maintainer's key"]
async fn record_generator_refused_route() {
    let (_server, connection) = recording(
        "generator-no-zero-retention",
        "/chat/completions",
        &live_key(),
    )
    .await;
    let request = GeneratorRequest {
        model: MODEL_WITHOUT_ZERO_RETENTION.to_owned(),
        ..sample_generator_request()
    };

    let reply = connection
        .generate::<SampleQuestion>(&request, &CancellationToken::new())
        .await;

    assert_eq!(
        reply.err(),
        Some(AiError::NoZeroRetentionEndpoint {
            model: MODEL_WITHOUT_ZERO_RETENTION.to_owned()
        })
    );
}

#[tokio::test]
#[ignore = "calls OpenRouter"]
async fn record_key_refused() {
    // A made-up key: OpenRouter refuses it, and the call costs nothing.
    let (_server, connection) =
        recording("key-refused", "/chat/completions", "sk-or-v1-refused").await;

    let reply = connection
        .generate::<SampleQuestion>(&sample_generator_request(), &CancellationToken::new())
        .await;

    assert_eq!(reply.err(), Some(AiError::KeyRefused));
}

#[tokio::test]
#[ignore = "calls OpenRouter with the maintainer's key"]
async fn record_candidates() {
    let (_server, connection) = recording("candidates", "/chat/completions", &live_key()).await;
    let request = candidates_request(
        &generator_model(),
        &evidence(),
        &area(),
        std::num::NonZeroUsize::new(2).expect("2 is not 0"),
    );

    let reply = connection
        .generate::<CandidatesOutput>(&request, &CancellationToken::new())
        .await;

    println!("{reply:#?}");
    assert!(reply.is_ok());
}

#[tokio::test]
#[ignore = "calls OpenRouter with the maintainer's key"]
async fn record_question_check() {
    let (_server, connection) = recording("question-check", "/systemone", &live_key()).await;
    let request = question_check_request(&jev_model(), &evidence(), &candidate());

    let reply = connection
        .evaluate(&request, &CancellationToken::new())
        .await;

    println!("{reply:#?}");
    assert!(reply.is_ok());
}

/// What the recorder saved: every fixture is JSON, so a failed recording
/// can't save something else unnoticed, and none holds a key or an auth
/// header (rule 41). It reads only the fixtures, so it runs on every PR.
#[test]
fn every_fixture_is_json_with_no_key_or_auth_header() {
    let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let mut checked = 0;
    for entry in std::fs::read_dir(&folder).expect("the fixtures folder can be read") {
        let path = entry.expect("the fixtures folder can be read").path();
        let text = std::fs::read_to_string(&path).expect("a fixture is text");
        if let Err(error) = serde_json::from_str::<Value>(&text) {
            panic!("{} isn't JSON: {error}", path.display());
        }
        let text = text.to_lowercase();
        // A key, OpenRouter's or made up, and an auth header saved with
        // the body.
        for never in ["sk-or-", "\"authorization\"", "bearer "] {
            assert!(!text.contains(never), "{} holds {never:?}", path.display());
        }
        checked += 1;
    }
    assert!(checked > 0, "no fixture in {}", folder.display());
}
