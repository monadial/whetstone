//! The hand check, live: Generate questions on the showcase project through
//! the real Jev and generator, at the app seam. It calls `OpenRouter` with a
//! real key, so it is ignored and runs only when asked, costing a few cents:
//! in the keyed end-to-end job (`.github/workflows/live.yml`), with the live
//! tests of each kind of request in whetstone-ai's `live.rs`, or locally:
//!
//! ```sh
//! OPENROUTER_API_KEY=… cargo nextest run --workspace --all-features --locked --no-fail-fast --run-ignored only live_
//! ```
//!
//! Per-PR tests never call `OpenRouter` (rule 48); the job is one of its
//! stated exceptions.

use crate::preparing_questions::{DOMAIN, PAYMENT_LIFECYCLE, generate_in};
use crate::session::Setup;

/// The maintainer's key, the one thing a test reads from the process's
/// environment, and only here: the app gets it as every test's variables
/// are given (rule 55).
fn live_key() -> String {
    std::env::var("OPENROUTER_API_KEY").expect("set OPENROUTER_API_KEY to run the live tests")
}

#[test]
#[ignore = "calls OpenRouter with the maintainer's key"]
fn live_generate_questions_on_the_showcase_shows_checked_questions_or_a_named_shortfall() {
    let key = live_key();
    // Each reply's models, tokens and cost, and nothing sent or written, go
    // to the test's output, which is how the run reports what it cost.
    drop(
        tracing_subscriber::fmt()
            .with_test_writer()
            .with_ansi(false)
            .try_init(),
    );
    // The fewest questions, so the fewest candidates: one area writes five,
    // and Jev checks each one that passes the plain checks.
    let mut session = generate_in(
        Setup {
            config: Some("questions = 3\n"),
            variables: &[("NO_COLOR", "1"), ("OPENROUTER_API_KEY", &key)],
            live: true,
            ..Setup::default()
        },
        &[DOMAIN],
        PAYMENT_LIFECYCLE,
        120,
        32,
    );

    session.settle();

    let text = session.text();
    let shown = text.contains("Question 1 of ")
        || text.contains("No questions passed checks")
        || text.contains(" passed checks, 3 asked for");
    assert!(
        shown,
        "neither checked questions nor a named shortfall:\n{text}"
    );
    println!("{text}");
}
