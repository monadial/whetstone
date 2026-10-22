//! Named failures: a request to Jev or the generator that fails stops its
//! stage, which names the failure plainly, says what the learner can do,
//! and offers Retry and Back. An invalid reply is one of them, never Needs
//! review. A model `OpenRouter` refuses for having no zero-retention endpoint
//! is marked in Settings for the rest of the run.

use std::io::Read as _;
use std::net::TcpListener;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use whetstone_ai::{QuestionCheckCall, RequestFailure, ScriptedAi, Timeouts};
use whetstone_core::Role;

use crate::preparing_questions::{DOMAIN, PAYMENT_LIFECYCLE, area_id, candidate, generate};
use crate::session::{NO_COLOR, Session, Setup};

/// Every failure a request can end with but cancelling, which only the
/// learner causes, each with how a stage names it after "failed:" and what
/// the learner reads next, as Jev's and as the generator's.
const NAMED: [(RequestFailure, &str, &str, &str); 9] = [
    (
        RequestFailure::NoKey,
        "no OpenRouter key was found",
        "Set OPENROUTER_API_KEY in your shell, then start Whetstone again.",
        "Set OPENROUTER_API_KEY in your shell, then start Whetstone again.",
    ),
    (
        RequestFailure::KeyRefused,
        "OpenRouter refused the key",
        "Check the key in OPENROUTER_API_KEY, which Whetstone reads as it starts.",
        "Check the key in OPENROUTER_API_KEY, which Whetstone reads as it starts.",
    ),
    (
        RequestFailure::OutOfCredits,
        "out of OpenRouter credits",
        "Add credits to your OpenRouter account, then Retry.",
        "Add credits to your OpenRouter account, then Retry.",
    ),
    (
        RequestFailure::RateLimited,
        "rate limited",
        "OpenRouter had too many requests from your key too quickly. Wait a moment, then Retry.",
        "OpenRouter had too many requests from your key too quickly. Wait a moment, then Retry.",
    ),
    (
        RequestFailure::ProviderUnavailable,
        "OpenRouter or the model's provider is unavailable",
        "OpenRouter couldn't be reached, or typesafe/jev-1.14's provider failed. Check your \
         connection, then Retry.",
        "OpenRouter couldn't be reached, or anthropic/claude-sonnet-5.5's provider failed. \
         Check your connection, then Retry.",
    ),
    (
        RequestFailure::NoZeroRetentionEndpoint,
        "no zero-retention endpoint for the model (refused by OpenRouter)",
        "OpenRouter has no endpoint for typesafe/jev-1.14 that keeps no prompts, so nothing \
         went to the model. Choose another model under [jev] in config.toml; Settings marks \
         this one.",
        "OpenRouter has no endpoint for anthropic/claude-sonnet-5.5 that keeps no prompts, so \
         nothing went to the model. Choose another model under [generator] in config.toml; \
         Settings marks this one.",
    ),
    (
        RequestFailure::TimedOut,
        "timed out",
        "Jev didn't reply in time. Retry, or try again later.",
        "The generator didn't reply in time. Retry, or try again later.",
    ),
    (
        RequestFailure::InvalidReply,
        "the reply couldn't be read",
        "Nothing in Jev's reply is used, and none of it is shown as a judgement.",
        "Nothing in the generator's reply is used.",
    ),
    (
        RequestFailure::InvalidRequest,
        "OpenRouter refused the request as invalid",
        "OpenRouter may not know typesafe/jev-1.14, or its moderation blocked the request. \
         Check the model under [jev] in config.toml.",
        "OpenRouter may not know anthropic/claude-sonnet-5.5, or its moderation blocked the \
         request. Check the model under [generator] in config.toml.",
    ),
];

/// The line under every named failure.
const NOTHING_RETRIED: &str =
    "Retry asks again only what failed. Nothing is retried until you choose to.";

/// The screen's text as one line of words, so a sentence is found wherever
/// it wraps.
fn words(session: &Session) -> String {
    session
        .text()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The controls under the screen's text: its last rows, trimmed, from the
/// first that is one, above the footer.
fn controls(session: &Session) -> Vec<String> {
    let rows: Vec<String> = session
        .rows()
        .iter()
        .map(|row| row.trim().to_owned())
        .collect();
    rows.iter()
        .skip_while(|row| !["> Retry", "Retry"].contains(&row.as_str()))
        .take_while(|row| !row.is_empty())
        .cloned()
        .collect()
}

/// The stem of the one candidate the generator writes in these tests.
const STEM: &str = "Why does the payment move on?";

/// A stage a request fails in.
#[derive(Debug)]
struct Failing {
    /// How the stage names itself.
    stage: &'static str,
    /// Who its requests ask.
    asks: Role,
    /// Scripts `ai` to fail the stage with `failure` twice over, so that
    /// Retry fails again.
    fail: fn(&ScriptedAi, RequestFailure),
}

const STAGES: [Failing; 3] = [
    Failing {
        stage: "Checking areas",
        asks: Role::Jev,
        fail: |ai, failure| {
            ai.on_gate_failing(failure);
            ai.on_gate_failing(failure);
        },
    },
    Failing {
        stage: "Writing questions",
        asks: Role::Generator,
        fail: |ai, failure| {
            ai.on_write(&area_id(DOMAIN), Err(failure));
            ai.on_write(&area_id(DOMAIN), Err(failure));
        },
    },
    Failing {
        stage: "Checking questions",
        asks: Role::Jev,
        // One call of the check fails, and the other two answer: a failure
        // of any call fails the check.
        fail: |ai, failure| {
            ai.on_write(
                &area_id(DOMAIN),
                Ok(vec![candidate(STEM, "A", PAYMENT_LIFECYCLE)]),
            );
            ai.on_check_call_failing(STEM, QuestionCheckCall::OptionRatings, failure);
            ai.on_check_call_failing(STEM, QuestionCheckCall::OptionRatings, failure);
            ai.on_check_passing(STEM);
        },
    },
];

#[test]
fn every_named_failure_in_every_stage_says_plainly_what_went_wrong_with_retry_and_back() {
    for Failing { stage, asks, fail } in STAGES {
        for (failure, name, jev, generator) in NAMED {
            let ai = Arc::new(ScriptedAi::new());
            fail(&ai, failure);
            let mut session = generate(&ai, &[DOMAIN], PAYMENT_LIFECYCLE, 120, 32);
            session.settle();
            let asked = ai.asked().len();

            let text = words(&session);
            let what_to_do = match asks {
                Role::Jev => jev,
                Role::Generator => generator,
            };
            for line in [
                &format!("{stage} failed: {name}."),
                what_to_do,
                NOTHING_RETRIED,
            ] {
                assert!(
                    text.contains(line),
                    "{stage}, {failure:?}: {line:?} in {text}"
                );
            }
            assert!(
                controls(&session).starts_with(&["> Retry".to_owned(), "Back".to_owned()]),
                "{stage}, {failure:?}: Retry, with focus, and Back: {text}"
            );

            session.press("<Enter>");
            session.settle();
            assert_eq!(
                ai.asked().len(),
                asked + 1,
                "{stage}, {failure:?}: Retry asks again what failed, once"
            );
            assert!(
                words(&session).contains(&format!("{stage} failed: {name}.")),
                "{stage}, {failure:?}: {}",
                session.text()
            );

            session.press("j <Enter>");
            assert!(
                session.text().contains("Change to practice on"),
                "{stage}, {failure:?}: Back returns to the areas and changes: {}",
                session.text()
            );
        }
    }
}

#[test]
fn an_invalid_reply_is_a_failure_with_retry_never_needs_review() {
    let ai = Arc::new(ScriptedAi::new());
    ai.on_write(
        &area_id(DOMAIN),
        Ok(vec![candidate(STEM, "A", PAYMENT_LIFECYCLE)]),
    );
    // The candidate call's reply can't be read; the other calls answer.
    ai.on_check_call_failing(
        STEM,
        QuestionCheckCall::Candidate,
        RequestFailure::InvalidReply,
    );
    ai.on_check_passing(STEM);
    let mut session = generate(&ai, &[DOMAIN], PAYMENT_LIFECYCLE, 120, 32);
    session.settle();

    let text = words(&session);
    assert!(
        text.contains("Checking questions failed: the reply couldn't be read."),
        "{text}"
    );
    assert!(!text.contains(STEM), "no question shows: {text}");
    assert!(!text.to_lowercase().contains("needs review"), "{text}");

    session.press("<Enter>");
    session.settle();

    assert_eq!(
        session.question().row_with("Question 1"),
        "Question 1 of 1 · Domain",
        "Retry checks the question again, and it shows once it passes"
    );
}

/// The rows from the one showing `needle`, trimmed.
fn rows_from(session: &Session, needle: &str) -> Vec<String> {
    session
        .rows()
        .iter()
        .map(|row| row.trim().to_owned())
        .skip_while(|row| !row.contains(needle))
        .collect()
}

/// The mark Settings puts against a model refused for having no
/// zero-retention endpoint.
const REFUSED: &str = "no zero-retention endpoint (refused by OpenRouter)";

#[test]
fn after_openrouter_refuses_the_generator_model_settings_marks_it_for_the_rest_of_the_run() {
    let ai = Arc::new(ScriptedAi::new());
    ai.on_write(
        &area_id(DOMAIN),
        Err(RequestFailure::NoZeroRetentionEndpoint),
    );
    let mut session = generate(&ai, &[DOMAIN], PAYMENT_LIFECYCLE, 120, 32);
    let mut before = Session::start(
        Setup {
            variables: NO_COLOR,
            ai: Some(&ai),
            ..Setup::default()
        },
        120,
        32,
    );
    before.press("j <Enter>");
    assert!(!before.text().contains(REFUSED), "{}", before.text());

    session.settle();
    // Back to the areas and changes, then home, then Settings.
    session.press("j <Enter> <Esc> j <Enter>");

    let generator = rows_from(&session, "Generator model");
    assert_eq!(
        generator[..2],
        [
            "Generator model       anthropic/claude-sonnet-5.5 (default)",
            REFUSED
        ],
        "{}",
        session.text()
    );
    assert!(
        !session.question().row_with("Jev model").contains(REFUSED),
        "only the model refused is marked"
    );
}

/// A local server that takes every connection and never replies, so a
/// request to it can only time out (rule 48). It stops with the test.
fn never_replies() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a free local port");
    let address = listener.local_addr().expect("a bound address");
    thread::spawn(move || {
        let mut open = Vec::new();
        for mut stream in listener.incoming().flatten() {
            let mut request = [0; 4096];
            let _read = stream.read(&mut request);
            // Kept open, unanswered.
            open.push(stream);
        }
    });
    format!("http://{address}/api/v1")
}

#[test]
fn practice_asks_with_the_time_limits_it_is_given_and_names_a_request_that_runs_out() {
    let server = never_replies();
    let timeouts = Timeouts {
        connect: Duration::from_secs(5),
        jev: Duration::from_millis(200),
        generator: Duration::from_millis(200),
    };
    let mut session = crate::preparing_questions::generate_in(
        Setup {
            variables: &[("NO_COLOR", "1"), ("OPENROUTER_API_KEY", "sk-or-v1-test")],
            local_openrouter: Some((&server, timeouts)),
            ..Setup::default()
        },
        &[DOMAIN],
        PAYMENT_LIFECYCLE,
        120,
        32,
    );

    let started = Instant::now();
    session.settle();

    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the limit given, not Jev's own 30 s, ran out: {:?}",
        started.elapsed()
    );
    let text = words(&session);
    assert!(
        text.contains("Checking areas failed: timed out."),
        "Jev's limit ran out on the real request: {text}"
    );
    assert_eq!(controls(&session), ["> Retry", "Back"], "{text}");
}
