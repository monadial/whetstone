//! Settings says how far to trust the outcomes and the models: the
//! assessment policy's version and whether it is validated, a note when
//! `[jev] model` isn't the model the policy was checked against, and a mark
//! against a model `OpenRouter` refused during this run for having no
//! zero-retention endpoint.

use std::sync::Arc;

use whetstone_ai::{RequestFailure, ScriptedAi};

use crate::preparing_questions::{DOMAIN, PAYMENT_LIFECYCLE, area_id, generate_in};
use crate::session::{NO_COLOR, Session, Setup};

/// Opens Settings from home.
fn settings(setup: Setup<'_>, width: u16, height: u16) -> Session {
    let mut session = Session::start(setup, width, height);
    session.press("j <Enter>");
    session
}

/// The screen's text as one line of words, so a phrase is found wherever
/// it wraps.
fn words(session: &Session) -> String {
    session
        .text()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// A config that asks Jev on a model other than the release's default.
const ANOTHER_JEV: &str = "[jev]\nmodel = \"typesafe/jev-1.13\"\n";

/// What Settings says when `[jev] model` isn't the release's default.
const POLICY_NOTE: &str =
    "Assessment policy checked against typesafe/jev-1.14; you're using typesafe/jev-1.13";

#[test]
fn settings_shows_the_assessment_policy_s_version_and_that_it_isn_t_validated() {
    let session = settings(
        Setup {
            variables: NO_COLOR,
            ..Setup::default()
        },
        80,
        24,
    );

    assert_eq!(
        session.row_with("Assessment policy"),
        "Assessment policy     version 0, not validated"
    );
    assert!(
        !session.text().contains("checked against"),
        "no note with the default Jev model: {}",
        session.text()
    );
}

#[test]
fn the_focused_policy_row_says_what_the_policy_is_and_what_validated_means() {
    let mut session = settings(
        Setup {
            variables: NO_COLOR,
            ..Setup::default()
        },
        120,
        32,
    );

    session.press("j j j j j j j");

    assert!(
        session
            .row_with("Assessment policy")
            .starts_with("> Assessment policy"),
        "{}",
        session.text()
    );
    let text = words(&session);
    assert!(
        text.contains("turn Jev's probabilities into clear yes, unclear or clear no"),
        "{text}"
    );
    assert!(text.contains("checked against labelled examples"), "{text}");
    assert!(text.contains("can't be configured"), "{text}");
}

#[test]
fn with_another_jev_model_settings_notes_the_policy_was_checked_against_the_default() {
    let session = settings(
        Setup {
            config: Some(ANOTHER_JEV),
            variables: NO_COLOR,
            ..Setup::default()
        },
        120,
        32,
    );

    assert!(words(&session).contains(POLICY_NOTE), "{}", session.text());
}

/// Settings after `OpenRouter` refused the generator model for having no
/// zero-retention endpoint, while Jev runs on another model than the
/// default, in a `width` by `height` terminal.
fn after_a_refused_generator_model(width: u16, height: u16) -> Session {
    let ai = Arc::new(ScriptedAi::new());
    ai.on_write(
        &area_id(DOMAIN),
        Err(RequestFailure::NoZeroRetentionEndpoint),
    );
    let mut session = generate_in(
        Setup {
            config: Some(ANOTHER_JEV),
            variables: &[("NO_COLOR", "1"), ("OPENROUTER_API_KEY", "sk-or-v1-test")],
            ai: Some(&ai),
            ..Setup::default()
        },
        &[DOMAIN],
        PAYMENT_LIFECYCLE,
        width,
        height,
    );
    session.settle();
    // Back to the areas and changes, then home, then Settings.
    session.press("j <Enter> <Esc> j <Enter>");
    session
}

#[test]
fn settings_with_the_policy_note_and_a_refused_model_at_120x32() {
    let session = after_a_refused_generator_model(120, 32);

    insta::assert_debug_snapshot!(
        "settings_with_the_policy_note_and_a_refused_model_120x32",
        session.frame()
    );
}

#[test]
fn settings_with_the_policy_note_and_a_refused_model_at_80x24() {
    let session = after_a_refused_generator_model(80, 24);

    insta::assert_debug_snapshot!(
        "settings_with_the_policy_note_and_a_refused_model_80x24",
        session.frame()
    );
}

#[test]
fn every_note_and_mark_shows_whole_at_80x24() {
    let session = after_a_refused_generator_model(80, 24);

    let text = words(&session);
    assert!(text.contains(POLICY_NOTE), "{text}");
    assert!(
        text.contains(
            "anthropic/claude-sonnet-5.5 (default) no zero-retention endpoint (refused by \
             OpenRouter)"
        ),
        "the mark under the generator model: {text}"
    );
    assert!(text.contains("Back"), "{text}");
}
