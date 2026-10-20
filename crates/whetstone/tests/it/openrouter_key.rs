//! The `OpenRouter` key decides whether Practice can start on the showcase
//! project. With `OPENROUTER_API_KEY` set, Generate questions waits only for
//! the learner's choices. Without a key it stays disabled, saying it needs
//! one. No test here sends a request (rule 48).

use crate::preparing_questions::{DOMAIN, PAYMENT_LIFECYCLE, generate_in};
use crate::session::{Session, Setup};

/// A key that looks like `OpenRouter`'s and opens nothing. It never leaves
/// the machine: the harness gives a key without a scripted AI one with
/// nothing scripted.
const MADE_UP_KEY: &str = "sk-or-v1-made-up";

/// The showcase picker, opened in a session whose environment sets
/// `OPENROUTER_API_KEY` to `key`.
fn picker_with_key(key: &str) -> Session {
    let mut session = Session::start(
        Setup {
            variables: &[("NO_COLOR", "1"), ("OPENROUTER_API_KEY", key)],
            ..Setup::default()
        },
        80,
        24,
    );
    session.press("<Enter>");
    session
}

#[test]
fn with_a_key_generate_questions_waits_only_for_a_change() {
    let mut session = picker_with_key(MADE_UP_KEY);
    assert_eq!(
        session.row_with("Generate questions"),
        "Generate questions (choose a change)"
    );

    // Past the five areas to the first change, and choose it.
    session.press("j j j j j <Space>");
    assert_eq!(
        session.row_with("Generate questions"),
        "Generate questions",
        "with a change chosen and every area selected, it is enabled"
    );
}

#[test]
fn an_empty_or_blank_key_is_no_key() {
    for key in ["", "   "] {
        let session = picker_with_key(key);

        assert_eq!(
            session.row_with("Generate questions"),
            "Generate questions (needs OPENROUTER_API_KEY)",
            "{key:?}"
        );
    }
}

#[test]
#[should_panic(expected = "no write scripted for the area")]
fn with_a_key_but_no_scripted_ai_a_request_fails_the_test_rather_than_reach_openrouter() {
    let mut session = generate_in(
        Setup {
            variables: &[("NO_COLOR", "1"), ("OPENROUTER_API_KEY", MADE_UP_KEY)],
            ..Setup::default()
        },
        &[DOMAIN],
        PAYMENT_LIFECYCLE,
        80,
        24,
    );

    session.settle();
}
