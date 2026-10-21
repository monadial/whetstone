//! The questions screen: one question at a time, with its place among
//! them, its area, its stem and options, and Next question from each one's
//! feedback. Only questions that pass every check are shown, and a
//! shortfall says so. When none pass, No questions passed checks offers
//! Retry and Pick another change.

use std::sync::Arc;

use whetstone_ai::{Asked, ScriptedAi};
use whetstone_core::{WrittenCandidate, WrittenCitation, WrittenCriterion};

use crate::answering::submit;
use crate::preparing_questions::{
    DOMAIN, PAYMENT_LIFECYCLE, area_id, candidate, generate, ratings, reasoning_met, stems,
    touched_path, writing_about_the_domain,
};
use crate::session::Session;

/// A session that prepared the questions `ai` writes and checks about the
/// domain area of the payment lifecycle change, `width` by `height`.
fn prepared(ai: &Arc<ScriptedAi>, width: u16, height: u16) -> Session {
    let mut session = generate(ai, &[DOMAIN], PAYMENT_LIFECYCLE, width, height);
    session.settle();
    session
}

/// The stems of the questions shown, from the first to the last, answering
/// each and moving on with Next question, after which the last one's
/// feedback shows.
pub(crate) fn every_stem_shown(session: &mut Session, among: &[String]) -> Vec<String> {
    let count = session
        .row_with("Question 1 of ")
        .split(" of ")
        .nth(1)
        .and_then(|rest| rest.split(' ').next())
        .and_then(|count| count.parse::<usize>().ok())
        .expect("Question 1 of n");
    let mut shown = Vec::new();
    for _ in 0..count {
        let text = session.text();
        shown.extend(
            among
                .iter()
                .filter(|stem| text.contains(stem.as_str()))
                .cloned(),
        );
        submit(session, "a", "Because.");
        session.press("<Enter>");
    }
    shown
}

#[test]
fn a_question_shows_its_place_its_area_its_stem_and_its_options_and_nothing_of_its_answer() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let session = prepared(&ai, 120, 32);

    let text = session.text();
    assert_eq!(session.row_with("Question 1"), "Question 1 of 4 · Domain");
    assert_eq!(session.row_with("Domain question 1?"), "Domain question 1?");
    assert_eq!(
        [
            session.row_with("Option A"),
            session.row_with("Option B"),
            session.row_with("Option C")
        ],
        [
            "> ( ) A  Option A of the question",
            "( ) B  Option B of the question",
            "( ) C  Option C of the question"
        ]
    );
    for hidden in [
        "rationale",
        "Why it matters",
        "rubric",
        "hint",
        "answer key",
    ] {
        assert!(!text.contains(hidden), "{hidden} shows:\n{text}");
    }
}

#[test]
fn back_from_the_questions_returns_to_the_areas_and_changes() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = prepared(&ai, 80, 24);

    session.press("q");

    assert!(session.text().contains("Change to practice on"));
    assert!(
        session
            .row_with("Model the payment lifecycle")
            .starts_with("(x) Model the payment lifecycle"),
        "the choices are kept:\n{}",
        session.text()
    );
}

#[test]
fn when_fewer_pass_than_were_asked_for_the_shortfall_heads_the_questions() {
    let ai = writing_about_the_domain(3, PAYMENT_LIFECYCLE);
    let mut session = prepared(&ai, 80, 24);

    assert_eq!(session.rows()[1].trim(), "3 passed checks, 4 asked for");
    assert_eq!(session.row_with("Question 1"), "Question 1 of 3 · Domain");
    for _ in 0..2 {
        submit(&mut session, "a", "Because.");
        session.press("<Enter>");
    }
    assert_eq!(session.rows()[1].trim(), "3 passed checks, 4 asked for");
    assert_eq!(session.row_with("Question 3"), "Question 3 of 3 · Domain");
}

#[test]
fn as_many_as_were_asked_for_are_shown_with_no_shortfall() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let session = prepared(&ai, 80, 24);

    assert!(!session.text().contains("asked for"), "{}", session.text());
}

/// A scripted AI writing the candidate `stem`, whose answer key is A, and
/// whose options Jev rates `rated`, passing its other checks, among three
/// that pass.
fn among_three_that_pass(stem: &str, rated: [f64; 3]) -> (Arc<ScriptedAi>, Vec<String>) {
    let ai = Arc::new(ScriptedAi::new());
    let mut written = vec![stem.to_owned()];
    written.extend(stems("Domain", 3));
    ai.on_write(
        &area_id(DOMAIN),
        Ok(written
            .iter()
            .map(|stem| candidate(stem, "A", PAYMENT_LIFECYCLE))
            .collect()),
    );
    let [a, b, c] = rated;
    ai.on_check_passing_except(stem, ratings(a, b, c));
    for passing in &written[1..] {
        ai.on_check_passing(passing);
    }
    for stem in &written {
        reasoning_met(&ai, stem);
    }
    (ai, written)
}

/// Whether the candidate `stem`, whose answer key is A and whose options Jev
/// rates `ratings`, is shown among three that pass.
fn shown(stem: &str, ratings: [f64; 3]) -> bool {
    let (ai, written) = among_three_that_pass(stem, ratings);
    let mut session = prepared(&ai, 120, 32);
    every_stem_shown(&mut session, &written).contains(&stem.to_owned())
}

#[test]
fn a_question_with_two_options_supported_is_never_shown() {
    assert!(!shown("Two supported?", [0.9, 0.85, 0.1]));
}

#[test]
fn a_question_with_no_option_supported_is_never_shown() {
    assert!(!shown("None supported?", [0.1, 0.15, 0.05]));
}

#[test]
fn a_question_with_an_unclear_option_is_never_shown() {
    assert!(!shown("Unclear B?", [0.9, 0.5, 0.1]));
    assert!(!shown("Unclear answer key?", [0.79, 0.1, 0.1]));
}

#[test]
fn a_question_whose_answer_key_jev_disagrees_with_is_never_shown() {
    assert!(!shown("Key disagrees?", [0.1, 0.9, 0.1]));
}

#[test]
fn a_question_whose_only_supported_option_is_its_answer_key_is_shown_even_at_the_cut_offs() {
    assert!(shown("At the cut-offs?", [0.80, 0.20, 0.20]));
}

/// `stem`'s candidate, well formed but for what `malform` changes.
fn malformed(stem: &str, malform: impl FnOnce(&mut WrittenCandidate)) -> WrittenCandidate {
    let mut written = candidate(stem, "A", PAYMENT_LIFECYCLE);
    malform(&mut written);
    written
}

#[test]
fn plain_checks_drop_malformed_candidates_before_jev_sees_them() {
    let criterion = || WrittenCriterion {
        statement: "A criterion".to_owned(),
        met: "Met".to_owned(),
        not_met: "Not met".to_owned(),
    };
    let candidates = vec![
        malformed("Only two options?", |written| {
            written.options.pop();
        }),
        malformed("A fourth option?", |written| {
            let mut d = written.options[0].clone();
            d.letter = "D".to_owned();
            written.options.push(d);
        }),
        malformed("Answer key D?", |written| {
            "D".clone_into(&mut written.answer_key);
        }),
        malformed("No criteria?", |written| written.rubric.clear()),
        malformed("Four criteria?", |written| {
            written.rubric = vec![criterion(), criterion(), criterion(), criterion()];
        }),
        malformed("Cites a file not sent?", |written| {
            written.citations = vec![WrittenCitation {
                path: "src/not_in_the_change.rs".to_owned(),
                first_line: 1,
                last_line: 1,
            }];
        }),
        malformed("Cites lines past the end?", |written| {
            written.citations = vec![WrittenCitation {
                path: touched_path(PAYMENT_LIFECYCLE),
                first_line: 1,
                last_line: 100_000,
            }];
        }),
        malformed("Cites nothing?", |written| written.citations.clear()),
        malformed("", |_| {}),
        malformed("No hint?", |written| written.hint.clear()),
        candidate("Well formed?", "A", PAYMENT_LIFECYCLE),
    ];
    let ai = Arc::new(ScriptedAi::new());
    ai.on_write(&area_id(DOMAIN), Ok(candidates));
    // Jev is scripted for the well-formed one only: checking any other
    // fails the test.
    ai.on_check_passing("Well formed?");

    let session = prepared(&ai, 120, 32);

    let checked: Vec<String> = ai
        .asked()
        .iter()
        .filter_map(|asked| match asked {
            Asked::CheckQuestion { candidate, .. } => Some(candidate.stem().to_owned()),
            Asked::CheckAreas { .. }
            | Asked::WriteCandidates { .. }
            | Asked::AssessReasoning { .. } => None,
        })
        .collect();
    assert_eq!(checked, ["Well formed?"]);
    assert_eq!(session.rows()[1].trim(), "1 passed checks, 4 asked for");
    assert!(session.text().contains("Well formed?"));
}

/// A session where no candidate passed its checks: the generator wrote two
/// about the domain area, and Jev rated both with no option supported. The
/// generator and Jev are scripted to pass two more on a Retry.
fn none_passed(width: u16, height: u16) -> (Session, Arc<ScriptedAi>) {
    let ai = Arc::new(ScriptedAi::new());
    let first = stems("First try", 2);
    let second = stems("Second try", 2);
    for stems in [&first, &second] {
        ai.on_write(
            &area_id(DOMAIN),
            Ok(stems
                .iter()
                .map(|stem| candidate(stem, "A", PAYMENT_LIFECYCLE))
                .collect()),
        );
    }
    for stem in &first {
        ai.on_check_passing_except(stem, ratings(0.1, 0.1, 0.1));
    }
    for stem in &second {
        ai.on_check_passing(stem);
    }
    (prepared(&ai, width, height), ai)
}

#[test]
fn when_none_pass_no_questions_passed_checks_offers_retry_and_pick_another_change() {
    let (session, _) = none_passed(80, 24);

    let text = session.text();
    assert!(text.contains("No questions passed checks"), "{text}");
    let controls: Vec<String> = session
        .rows()
        .iter()
        .map(|row| row.trim().to_owned())
        .filter(|row| ["> Retry", "Pick another change"].contains(&row.as_str()))
        .collect();
    assert_eq!(controls, ["> Retry", "Pick another change"], "{text}");
    assert!(!text.contains("First try"), "no question shows: {text}");
}

#[test]
fn retry_when_none_pass_writes_and_checks_new_candidates() {
    let (mut session, ai) = none_passed(80, 24);
    let asked_before = ai.asked().len();

    session.press("<Enter>");
    assert!(
        session.text().contains("Preparing questions"),
        "{}",
        session.text()
    );
    session.settle();

    let again = &ai.asked()[asked_before..];
    assert!(
        matches!(again.first(), Some(Asked::WriteCandidates { area, .. }) if area.id() == &area_id(DOMAIN)),
        "the generator writes again: {again:#?}"
    );
    assert_eq!(again.len(), 3, "one write, then a check for each of two");
    assert_eq!(session.row_with("Question 1"), "Question 1 of 2 · Domain");
    assert!(session.text().contains("Second try question 1?"));
}

#[test]
fn pick_another_change_returns_to_the_areas_and_changes() {
    let (mut session, _) = none_passed(80, 24);

    session.press("j <Enter>");

    assert!(session.text().contains("Change to practice on"));
}

#[test]
fn the_questions_screen_at_120x32() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let session = prepared(&ai, 120, 32);

    insta::assert_debug_snapshot!("questions_120x32", session.frame());
}

#[test]
fn the_questions_screen_at_80x24() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let session = prepared(&ai, 80, 24);

    insta::assert_debug_snapshot!("questions_80x24", session.frame());
}

#[test]
fn the_questions_screen_with_a_shortfall_at_80x24() {
    let ai = writing_about_the_domain(3, PAYMENT_LIFECYCLE);
    let mut session = prepared(&ai, 80, 24);
    submit(&mut session, "a", "Because.");
    session.press("<Enter>");

    insta::assert_debug_snapshot!("questions_with_a_shortfall_80x24", session.frame());
}

#[test]
fn no_questions_passed_checks_at_120x32() {
    let (session, _) = none_passed(120, 32);

    insta::assert_debug_snapshot!("no_questions_passed_checks_120x32", session.frame());
}

#[test]
fn no_questions_passed_checks_at_80x24() {
    let (session, _) = none_passed(80, 24);

    insta::assert_debug_snapshot!("no_questions_passed_checks_80x24", session.frame());
}
