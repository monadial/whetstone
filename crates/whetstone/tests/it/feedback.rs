//! Feedback at Submit: the choice outcome shows at once, from the option
//! ratings, as Supported or Not supported, never Needs review, with its
//! citations, the chosen option's rationale, the answer key's rationale and
//! "why it matters". The reasoning shows as it was submitted, then its
//! outcome (`reasoning_evaluation.rs`). A submitted response is final, and
//! feedback offers Next question, Inspect source, Report and Finish.

use whetstone::Durability;

use crate::answering::{EMPTY_EDITOR, answering, answering_with, reasoning, submit};
use crate::preparing_questions::{PAYMENT_LIFECYCLE, touched_path, writing_about_the_domain};
use crate::session::{Refusing, Session, TestStore};

/// A session `width` by `height` on the feedback for a response choosing
/// `choice` to the first question, whose answer key is A.
fn submitted(choice: &str, width: u16, height: u16) -> Session {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, width, height);
    submit(
        &mut session,
        choice,
        "Retries reuse the key, so the provider dedupes them.",
    );
    session
}

#[test]
fn choosing_the_answer_key_is_supported_at_once_with_its_rationale_why_it_matters_and_citations() {
    let session = submitted("a", 120, 32);

    assert_eq!(
        session.question().section("Choice · Supported"),
        ["Jev supports A, the option you chose."]
    );
    assert_eq!(
        session
            .question()
            .section("You chose A: Option A of the question"),
        ["The rationale for A, which stays hidden"]
    );
    assert!(
        !session.text().contains("The answer key is"),
        "the answer key is the choice:\n{}",
        session.text()
    );
    assert_eq!(
        session.question().section("Why it matters"),
        ["Why it matters, which stays hidden"]
    );
    assert_eq!(
        session.question().section("Cited"),
        [format!("{}, line 1", touched_path(PAYMENT_LIFECYCLE))]
    );
}

#[test]
fn choosing_another_option_is_not_supported_with_its_rationale_and_the_answer_keys() {
    let session = submitted("c", 120, 32);

    assert_eq!(
        session.question().section_text("Choice · Not supported"),
        "Jev doesn't support C, the option you chose. It supports A, the answer key."
    );
    assert_eq!(
        session
            .question()
            .section("You chose C: Option C of the question"),
        ["The rationale for C, which stays hidden"]
    );
    assert_eq!(
        session
            .question()
            .section("The answer key is A: Option A of the question"),
        ["The rationale for A, which stays hidden"]
    );
    assert_eq!(
        session.question().section("Why it matters"),
        ["Why it matters, which stays hidden"]
    );
    assert_eq!(
        session.question().section("Cited"),
        [format!("{}, line 1", touched_path(PAYMENT_LIFECYCLE))]
    );
}

#[test]
fn a_choice_outcome_is_never_needs_review_and_shows_no_numbers() {
    for choice in ["a", "b", "c"] {
        let session = submitted(choice, 120, 32);

        let text = session.text();
        assert!(!text.contains("Needs review"), "{text}");
        for number in ["0.9", "0.1", "0.05", "%"] {
            assert!(!text.contains(number), "{number} shows:\n{text}");
        }
    }
}

#[test]
fn the_reasoning_section_shows_it_as_submitted() {
    let session = submitted("a", 120, 32);

    assert_eq!(
        session.question().section_text("Your reasoning"),
        "Retries reuse the key, so the provider dedupes them."
    );
    assert!(
        !session.text().contains("Saved"),
        "the in-memory store never shows Saved"
    );
}

#[test]
fn saved_shows_only_once_a_durable_store_has_saved_the_response() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let store = TestStore::new(Durability::Durable, Refusing::Nothing);
    let mut session = answering_with(&ai, &store, 120, 32);
    assert!(!session.text().contains("Saved"), "{}", session.text());

    submit(&mut session, "a", "Because.");
    // Saved is the feedback's last line.
    session.press("G");

    assert_eq!(session.question().row_with("Saved"), "Saved");
}

#[test]
fn a_submitted_response_is_final() {
    let mut session = submitted("c", 120, 32);
    let before = session.text();
    let log = session.store().log();

    session.press("a b c i x");

    assert_eq!(session.text(), before, "nothing changes it");
    assert_eq!(session.store().log(), log, "nothing more is saved");
    assert!(
        !before.contains("Submit response") && !before.contains("Press i to write"),
        "no way back to the response:\n{before}"
    );
}

#[test]
fn next_question_shows_the_next_one_with_a_fresh_draft() {
    let mut session = submitted("c", 80, 24);
    assert_eq!(
        session.question().row_with("Next question"),
        "> Next question"
    );

    session.press("<Enter>");

    assert_eq!(
        session.question().row_with("Question 2"),
        "Question 2 of 4 · Domain"
    );
    assert!(session.text().contains("Domain question 2?"));
    assert_eq!(
        session.question().row_with("Option A"),
        "> ( ) A  Option A of the question"
    );
    assert_eq!(reasoning(&session)[..2], EMPTY_EDITOR);
    assert_eq!(
        session.question().row_with("Submit response"),
        "Submit response (choose an option)"
    );
}

#[test]
fn after_the_last_question_next_question_is_disabled_and_finish_has_focus() {
    let ai = writing_about_the_domain(3, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 80, 24);
    for _ in 0..2 {
        submit(&mut session, "a", "Because.");
        session.press("<Enter>");
    }
    submit(&mut session, "a", "Because.");

    assert_eq!(
        session.question().row_with("Next question"),
        "Next question (this is the last)"
    );
    assert_eq!(session.question().row_with("Finish"), "> Finish");
    session.press("k k k k <Enter>");
    assert_eq!(
        session.question().row_with("Question 3"),
        "Question 3 of 3 · Domain"
    );
}

#[test]
fn finish_returns_home() {
    let mut session = submitted("a", 80, 24);

    // Finish asks first, as three questions are unattempted, then the
    // summary leads home.
    session.press("j j j j <Enter> j <Enter> <Enter>");

    assert!(
        session.text().contains("Practice on the showcase project"),
        "{}",
        session.text()
    );
    assert!(!session.text().contains("Lose your draft?"));
}

#[test]
fn inspect_source_from_feedback_reads_the_evidence_and_back_returns_to_it() {
    let mut session = submitted("a", 120, 32);

    session.press("j <Enter>");
    assert!(
        session
            .text()
            .contains("Evidence: Model the payment lifecycle"),
        "{}",
        session.text()
    );
    assert!(
        session.text().contains(&format!(
            "{}: full content",
            touched_path(PAYMENT_LIFECYCLE)
        )),
        "{}",
        session.text()
    );

    session.press("q");
    assert_eq!(session.question().row_with("Choice"), "Choice · Supported");
}

#[test]
fn feedback_too_long_for_the_screen_scrolls_above_its_controls() {
    let mut session = submitted("c", 80, 24);
    let assessed = "Your response is submitted.";
    let scrolled = "Reasoning · Awaiting evaluation";
    assert!(!session.text().contains(scrolled), "{}", session.text());

    // Below the code the question cites, half a page is a few rows.
    session.press("<C-d> <C-d> <C-d> <C-d>");
    assert!(session.text().contains(scrolled), "{}", session.text());
    assert_eq!(
        session.question().row_with("Next question"),
        "> Next question"
    );

    session.press("g g");
    assert!(
        session.text().contains("Domain question 1?"),
        "{}",
        session.text()
    );
    session.press("G");
    assert!(session.text().contains(assessed), "{}", session.text());
}

#[test]
fn the_feedback_screen_at_120x32() {
    let session = submitted("c", 120, 32);

    insta::assert_debug_snapshot!("feedback_120x32", session.frame());
}

#[test]
fn the_feedback_screen_at_80x24() {
    let session = submitted("c", 80, 24);

    insta::assert_debug_snapshot!("feedback_80x24", session.frame());
}

#[test]
fn supported_feedback_at_80x24() {
    let session = submitted("a", 80, 24);

    insta::assert_debug_snapshot!("feedback_supported_80x24", session.frame());
}
