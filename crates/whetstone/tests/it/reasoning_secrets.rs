//! The secret check on reasoning: Submit response checks the reasoning with
//! the secret rules, blocking and warning alike, before it is saved or sent.
//! A possible secret stops that Submit and shows under the editor. Submitting
//! again with the reasoning unchanged saves it and sends it as it is, which
//! is the learner's "Not a secret", and editing it clears the notice.

use whetstone::StoreEntry;
use whetstone_ai::{Asked, ScriptedAi};
use whetstone_core::WrittenReasoning;

use crate::answering::{answering, submit, type_text};
use crate::preparing_questions::{PAYMENT_LIFECYCLE, writing_about_the_domain};
use crate::session::Session;

/// A fake GitHub token, built from pieces so that no secret scanner finds
/// a whole one in the source.
const GITHUB_TOKEN: &str = concat!("ghp_", "FAKEfakeFAKEfakeFAKEfakeFAKEfakeFAKE");

/// What shows under the editor for reasoning that holds [`GITHUB_TOKEN`].
const NOTICE: &str = "Possible secret in your reasoning: GitHub token, ghp_********";

/// What the learner can do about it.
const WHAT_TO_DO: &str = "Edit your reasoning, or submit again to send it as it is.";

/// Reasoning that holds [`GITHUB_TOKEN`].
fn holding_a_key() -> String {
    format!("Retries send {GITHUB_TOKEN} again.")
}

/// Every reasoning `ai` was asked to assess or explain, in order.
fn sent(ai: &ScriptedAi) -> Vec<String> {
    ai.asked()
        .into_iter()
        .filter_map(|asked| match asked {
            Asked::AssessReasoning { reasoning, .. } | Asked::Explain { reasoning, .. } => {
                Some(reasoning)
            }
            _ => None,
        })
        .collect()
}

/// The written reasoning of every response the store saved, in order.
fn saved(session: &Session) -> Vec<String> {
    session
        .store()
        .log()
        .iter()
        .filter_map(|entry| match entry {
            StoreEntry::Response { response, .. } => response
                .reasoning()
                .written()
                .map(|written: &WrittenReasoning| written.as_str().to_owned()),
            _ => None,
        })
        .collect()
}

/// The question's rows, trimmed, as one line, so a sentence reads whole
/// wherever it wraps.
fn words(session: &Session) -> String {
    session.question().words()
}

#[test]
fn reasoning_that_holds_a_key_is_neither_saved_nor_sent_on_the_first_submit() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 120, 32);

    submit(&mut session, "a", &holding_a_key());
    session.settle();

    assert!(words(&session).contains(NOTICE), "{}", session.text());
    assert!(words(&session).contains(WHAT_TO_DO), "{}", session.text());
    assert_eq!(saved(&session), [] as [String; 0]);
    assert_eq!(session.store().log().len(), 1, "only the start");
    assert_eq!(sent(&ai), [] as [String; 0]);
    assert!(!session.text().contains("Choice ·"), "{}", session.text());
    assert_eq!(
        session.question().row_with("Submit response"),
        "> Submit response",
        "focus stays on Submit, for a second Submit"
    );
}

#[test]
fn submitting_again_with_the_reasoning_unchanged_saves_it_and_sends_it() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 120, 32);
    submit(&mut session, "a", &holding_a_key());

    session.press("<Enter>");
    session.settle();

    assert_eq!(saved(&session), [holding_a_key()]);
    assert_eq!(sent(&ai)[..1], [holding_a_key()]);
    assert!(session.text().contains("Choice ·"), "{}", session.text());
}

#[test]
fn editing_the_reasoning_clears_the_notice_and_the_next_submit_checks_it_again() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 120, 32);
    submit(&mut session, "a", &holding_a_key());
    assert!(words(&session).contains(NOTICE), "{}", session.text());

    // Moving in the editor changes nothing, so the notice stays.
    session.press("i <Left>");
    assert!(words(&session).contains(NOTICE), "{}", session.text());

    session.press("<Right>");
    type_text(&mut session, " Then");
    assert!(!words(&session).contains(NOTICE), "{}", session.text());
    assert!(!words(&session).contains(WHAT_TO_DO), "{}", session.text());

    session.press("<Tab> <Enter>");
    session.settle();

    assert!(words(&session).contains(NOTICE), "{}", session.text());
    assert_eq!(saved(&session), [] as [String; 0]);
    assert_eq!(sent(&ai), [] as [String; 0]);
}

#[test]
fn reasoning_edited_to_hold_no_secret_is_submitted_at_once() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 120, 32);
    submit(
        &mut session,
        "a",
        &format!("Retries send the key {GITHUB_TOKEN}"),
    );
    assert!(words(&session).contains(NOTICE), "{}", session.text());

    session.press("i");
    session.press(&vec!["<BS>"; GITHUB_TOKEN.len() + 1].join(" "));
    type_text(&mut session, " again.");
    session.press("<Tab> <Enter>");
    session.settle();

    assert_eq!(saved(&session), ["Retries send the key again."]);
}

/// Reasoning that holds a password in a URL, which the secret check warns
/// about. Snapshots use it rather than [`holding_a_key`], since the editor
/// shows the reasoning whole, and no snapshot holds a whole fake key that a
/// secret scanner would find.
fn holding_a_password() -> String {
    format!(
        "It connects to {} first.",
        concat!("postgres://app:", "hunter2@db.internal/app")
    )
}

#[test]
fn a_warning_stops_the_first_submit_too() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 120, 32);

    submit(&mut session, "a", &holding_a_password());

    assert!(
        words(&session)
            .contains("Possible secret in your reasoning: Password in a URL, post********"),
        "{}",
        session.text()
    );
    assert_eq!(saved(&session), [] as [String; 0]);
}

#[test]
fn reasoning_with_a_possible_secret_at_120x32() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 120, 32);
    submit(&mut session, "a", &holding_a_password());

    insta::assert_debug_snapshot!("reasoning_with_a_possible_secret_120x32", session.frame());
}

#[test]
fn reasoning_with_a_possible_secret_at_80x24() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 80, 24);
    submit(&mut session, "a", &holding_a_password());

    insta::assert_debug_snapshot!("reasoning_with_a_possible_secret_80x24", session.frame());
}
