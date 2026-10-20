//! Preparing questions on the showcase project, against a scripted AI:
//! Generate questions finds the change's connected code, then one generator
//! call per chosen area writes candidates, plain checks drop malformed ones,
//! Jev checks each one left, the show rule keeps the clear-cut ones and
//! picking chooses up to `questions` of them.

use crate::session::Session;

#[test]
fn home_says_plainly_what_practice_on_the_showcase_project_sends_and_who_pays() {
    let session = Session::plain(120, 32);

    assert!(
        session.text().contains(
            "Practice sends showcase excerpts and your answers to OpenRouter, paid with your \
             credits."
        ),
        "{}",
        session.text()
    );
}
