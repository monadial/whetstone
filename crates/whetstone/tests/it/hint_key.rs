//! `h` shows the question's hint from any control, as Hint does, and
//! saves that the hint was used once. While the reasoning is edited, `h`
//! is text, as every key is there (rule 29).

use whetstone::Signal;

use crate::answering::{answering, reasoning};
use crate::help_and_signals::signals;
use crate::preparing_questions::{PAYMENT_LIFECYCLE, writing_about_the_domain};

/// The hint of every question [`writing_about_the_domain`] writes.
const HINT: &str = "Hint: The hint, which stays hidden";

#[test]
fn h_shows_the_hint_and_saves_that_it_was_used_once() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 120, 32);
    assert!(!session.text().contains(HINT));

    session.press("h");
    assert_eq!(session.question().row_with("Hint:"), HINT);
    session.press("j j h");

    assert_eq!(
        signals(&session),
        [("Domain question 1?".to_owned(), Signal::HintUsed)],
        "once, from the options or any other control"
    );
}

#[test]
fn h_is_text_while_the_reasoning_is_edited() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 120, 32);

    session.press("i h");

    assert_eq!(reasoning(&session)[0], "h");
    assert!(!session.text().contains(HINT));
    assert_eq!(signals(&session), []);
}

#[test]
fn the_footer_names_the_hint_key() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let session = answering(&ai, 120, 32);

    assert!(
        session.row_with("i write").contains("h hint"),
        "{}",
        session.text()
    );
}
