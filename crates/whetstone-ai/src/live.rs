//! Live tests: each kind of request the AI port makes, sent through the real
//! [`OpenRouterAi`] to Jev and the generator, so the recorded replies the
//! contract tests replay stay true. They call `OpenRouter` with a real key,
//! so they are ignored and run only when asked for, for a few cents: in the
//! keyed end-to-end job (`.github/workflows/live.yml`), or locally with the
//! maintainer's own key, as `CODING_STANDARDS.md` says under "The keyed
//! end-to-end job". Per-PR tests never call `OpenRouter` (rule 48); these are
//! one of its stated exceptions.
//!
//! # One test for each kind of request
//!
//! Each kind of request has one test here, named `live_` and the port's
//! call, such as `live_write_candidates`. It:
//!
//! 1. asks [`live_ai`] with the samples its fixture was recorded with
//!    (`samples.rs`), at the smallest size that still gives the reply's whole
//!    shape;
//! 2. asserts the reply holds what the recorded one holds, such as an answer
//!    to every question asked;
//! 3. checks its provenance with [`assert_reported`].
//!
//! A ticket that adds a kind of request to the port adds its test here the
//! same way, next to its recorder in `record.rs` and its contract tests.
//! The job runs every ignored test whose name starts with `live_`, so the
//! new test needs nothing else.

use whetstone_core::{CriterionOutcome, DepthLevel, ModelId, Provenance, QuestionCount};

use crate::replay::live_key;
use crate::samples::{
    Acknowledged, OUTCOMES, REASONING, area, candidate, evidence, generator_model, jev_model,
    observability, paragraphs, question,
};
use crate::{Ai, CandidateCount, Models, OpenRouter, OpenRouterAi, QuestionWithoutKey};

/// The real AI on `OpenRouter`, with the key from `OPENROUTER_API_KEY`, the
/// models the fixtures were recorded with, and the app's time limits.
fn live_ai() -> OpenRouterAi {
    let connection = OpenRouter::new(Some(live_key())).expect("the key isn't blank");
    OpenRouterAi::new(
        connection,
        Models {
            jev: jev_model(),
            generator: generator_model(),
        },
    )
}

/// Each reply's models, tokens and cost, as the port logs them, go to the
/// test's output, which is how a run reports what it cost. Nothing sent or
/// written reaches that log (rules 43 and 44).
fn log_replies() -> tracing::subscriber::DefaultGuard {
    tracing::subscriber::set_default(
        tracing_subscriber::fmt()
            .with_test_writer()
            .with_max_level(tracing::Level::INFO)
            .with_ansi(false)
            .finish(),
    )
}

/// What every recorded reply reports, so every live one must too: an answer
/// from the model asked for (`OpenRouter` may name a dated version of it,
/// such as `typesafe/jev-1.14-20261014`), and its tokens and cost.
fn assert_reported(provenance: &Provenance) {
    let asked = provenance.requested_model().as_str();
    let answered = provenance.reported_model().map(ModelId::as_str);
    assert!(
        answered.is_some_and(|model| model.starts_with(asked)),
        "{asked} was asked for, and {answered:?} answered"
    );
    let usage = provenance.usage();
    assert!(
        usage.input_tokens.is_some() && usage.output_tokens.is_some(),
        "no token counts reported: {usage:?}"
    );
    assert!(provenance.cost().is_some(), "no cost reported");
}

#[tokio::test]
#[ignore = "calls OpenRouter with a real key"]
async fn live_check_areas() {
    let _log = log_replies();
    // Two areas, as recorded: one the sample change is about, and one it
    // hardly touches.
    let areas = [area(), observability()];

    let reply = live_ai()
        .check_areas(&evidence(), &areas)
        .await
        .expect("Jev checks the areas");

    let answered: Vec<&str> = reply
        .content
        .iter()
        .map(|(area, _)| area.as_str())
        .collect();
    assert_eq!(
        answered,
        ["domain", "observability"],
        "one per area, in order"
    );
    let [(_, domain), (_, observability)] = reply.content.as_slice() else {
        unreachable!("two answers, as just asserted");
    };
    assert!(
        domain.get() > observability.get(),
        "yes means worth asking: the change is about its domain ({}), not its observability ({})",
        domain.get(),
        observability.get()
    );
    assert_reported(&reply.provenance);
}

#[tokio::test]
#[ignore = "calls OpenRouter with a real key"]
async fn live_write_candidates() {
    let _log = log_replies();
    // One candidate, the fewest the port asks for: 3 questions over 5 areas.
    let one = CandidateCount::per_area(QuestionCount::try_from(3).expect("from 3 to 5"), 5);
    assert_eq!(one.get(), 1);

    let reply = live_ai()
        .write_candidates(&evidence(), &area(), one)
        .await
        .expect("the generator writes candidates");

    assert!(
        !reply.content.is_empty(),
        "the generator wrote no candidate"
    );
    assert_reported(&reply.provenance);
}

#[tokio::test]
#[ignore = "calls OpenRouter with a real key"]
async fn live_check_question() {
    let _log = log_replies();
    let candidate = candidate();

    let reply = live_ai()
        .check_question(&evidence(), &candidate)
        .await
        .expect("Jev checks the candidate");

    for check in candidate.checks() {
        assert!(
            reply.content.get(check).is_some(),
            "Jev didn't answer {check:?}"
        );
    }
    // Depth is kept as returned, so its expected value must be what Depth
    // takes it for: the levels' places, counting from 0, weighted by their
    // probabilities. Rounding each to hundredths moves the two apart by at
    // most 0.02, while places counted from 1 would move them apart by 1.
    let depth = reply.content.depth();
    let weighted: f64 = DepthLevel::ALL
        .into_iter()
        .zip(0..)
        .map(|(level, place)| f64::from(place) * depth.probability(level).get())
        .sum();
    assert!(
        (weighted - depth.expected()).abs() <= 0.05,
        "the depth's expected value, {}, isn't its levels' places from 0, weighted: {weighted}",
        depth.expected()
    );
    assert_reported(&reply.provenance);
}

#[tokio::test]
#[ignore = "calls OpenRouter with a real key"]
async fn live_assess_reasoning() {
    let _log = log_replies();
    // The sample question, with its two criteria, and the reasoning saved
    // for it, as recorded.
    let question = question();

    let reply = live_ai()
        .assess_reasoning(
            &Acknowledged(REASONING),
            &evidence(),
            QuestionWithoutKey::from(&question),
        )
        .await
        .expect("Jev assesses the reasoning");

    assert_eq!(
        reply.content.len(),
        question.rubric().len(),
        "an answer per criterion"
    );
    assert_reported(&reply.provenance);
}

#[tokio::test]
#[ignore = "calls OpenRouter with a real key"]
async fn live_explain() {
    let _log = log_replies();
    // The sample question's two criteria, as recorded: the first not met,
    // the second needing review.
    let criteria: Vec<(usize, CriterionOutcome)> = OUTCOMES.into_iter().enumerate().collect();

    let reply = live_ai()
        .explain(
            &Acknowledged(REASONING),
            &evidence(),
            &question(),
            &criteria,
        )
        .await
        .expect("the generator explains the criteria");

    let explained: Vec<(usize, CriterionOutcome)> = reply
        .content
        .iter()
        .map(|paragraph| (paragraph.criterion, paragraph.outcome))
        .collect();
    assert_eq!(
        explained, criteria,
        "one paragraph per criterion asked, in rubric order"
    );
    for paragraph in &reply.content {
        assert!(
            !paragraph.text.trim().is_empty(),
            "the paragraph about criterion {} says nothing",
            paragraph.criterion
        );
    }
    assert_reported(&reply.provenance);
}

#[tokio::test]
#[ignore = "calls OpenRouter with a real key"]
async fn live_check_explanation() {
    let _log = log_replies();
    // The explanation recorded for the sample question, two paragraphs.
    let paragraphs = paragraphs();

    let reply = live_ai()
        .check_explanation(
            &Acknowledged(REASONING),
            &evidence(),
            &question(),
            &paragraphs,
        )
        .await
        .expect("Jev checks the explanation");

    assert_eq!(
        reply.content.len(),
        paragraphs.len(),
        "an answer to both questions per paragraph"
    );
    assert_reported(&reply.provenance);
}
