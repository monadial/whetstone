//! Reasoning is optional: a learner can answer with a choice alone. Submit
//! is enabled once an option is chosen, and the reasoning editor says
//! writing reasoning is optional. Reasoning that is blank or only
//! whitespace counts as not written. Without reasoning, the response is
//! saved before its choice outcome shows, as with it, but Jev isn't asked
//! about reasoning and no explanation is written: feedback shows the choice
//! outcome as it always does, then the rubric as what a full answer covers,
//! with no outcome. With reasoning, everything works as it did. The summary
//! counts the questions answered with reasoning and those answered with a
//! choice only separately.

use std::sync::Arc;

use whetstone::{Assessment, StoreEntry};
use whetstone_ai::{Asked, ScriptedAi};
use whetstone_core::{Probability, Reasoning, WrittenCriterion};

use crate::answering::{EMPTY_EDITOR, answering, reasoning, submit};
use crate::help_and_signals::activate;
use crate::preparing_questions::{DOMAIN, PAYMENT_LIFECYCLE, area_id, candidate, stems};
use crate::session::Session;

/// The first question's stem.
const FIRST: &str = "Domain question 1?";

/// The second question's stem.
const SECOND: &str = "Domain question 2?";

/// The third question's stem.
const THIRD: &str = "Domain question 3?";

/// The first question's rubric, in order.
const CRITERIA: [&str; 2] = [
    "Names the key the retry reuses",
    "Says why the retry can't charge twice",
];

/// A terminal height that fits all of the feedback.
const TALL: u16 = 40;

/// A scripted AI writing four questions about the domain area of the
/// payment lifecycle change, each passing every check, its answer key A,
/// the first with [`CRITERIA`] as its rubric. Jev meets every criterion of
/// any reasoning written for them, so no explanation is asked for.
fn writing() -> Arc<ScriptedAi> {
    let ai = Arc::new(ScriptedAi::new());
    let stems = stems("Domain", 4);
    let mut written: Vec<_> = stems
        .iter()
        .map(|stem| candidate(stem, "A", PAYMENT_LIFECYCLE))
        .collect();
    written[0].rubric = CRITERIA
        .iter()
        .map(|&statement| WrittenCriterion {
            statement: statement.to_owned(),
            met: format!("Meeting it: {statement}"),
            not_met: format!("Not meeting it: {statement}"),
        })
        .collect();
    ai.on_write(&area_id(DOMAIN), Ok(written));
    for stem in &stems {
        ai.on_check_passing(stem);
    }
    ai.on_assess(FIRST, Ok(probabilities(&[0.9, 0.9])));
    for stem in &stems[1..] {
        ai.on_assess(stem, Ok(probabilities(&[0.9])));
    }
    ai
}

fn probabilities(values: &[f64]) -> Vec<Probability> {
    values
        .iter()
        .map(|&value| Probability::new(value).expect("from 0 to 1"))
        .collect()
}

/// Every request `ai` was asked about a response: reasoning evaluations,
/// explanations and explanation checks, in order.
fn about_responses(ai: &ScriptedAi) -> Vec<Asked> {
    ai.asked()
        .into_iter()
        .filter(|asked| {
            matches!(
                asked,
                Asked::AssessReasoning { .. }
                    | Asked::Explain { .. }
                    | Asked::CheckExplanation { .. }
            )
        })
        .collect()
}

/// What the store saved after the record started: each response, with its
/// choice and whether its reasoning is written, and each outcome, named.
fn saved_after_the_start(session: &Session) -> Vec<String> {
    session
        .store()
        .log()
        .into_iter()
        .filter_map(|entry| match entry {
            StoreEntry::Started { .. } | StoreEntry::Signal { .. } | StoreEntry::Request { .. } => {
                None
            }
            StoreEntry::Response { response, .. } => Some(format!(
                "response {} {}",
                response.choice(),
                match response.reasoning() {
                    Reasoning::Written(_) => "with reasoning",
                    Reasoning::NotWritten => "with no reasoning",
                }
            )),
            StoreEntry::Assessment { assessment, .. } => Some(
                match assessment {
                    Assessment::Choice(_) => "choice outcome",
                    Assessment::Criterion { .. } => "criterion outcome",
                    Assessment::Explanation { .. } => "explanation",
                }
                .to_owned(),
            ),
        })
        .collect()
}

#[test]
fn submit_is_enabled_once_an_option_is_chosen_with_reasoning_optional() {
    let ai = writing();
    let mut session = answering(&ai, 80, 24);
    assert_eq!(
        session.row_with("Submit response"),
        "Submit response (choose an option)"
    );
    assert_eq!(reasoning(&session)[..2], EMPTY_EDITOR);

    session.press("b");
    assert_eq!(session.row_with("Submit response"), "Submit response");
    assert_eq!(reasoning(&session)[..2], EMPTY_EDITOR, "the note stays");

    session.press("i <Tab> <Enter>");
    assert_eq!(
        session.section("Choice · Not supported"),
        ["Jev doesn't support B, the option you chose. It supports A, the answer key."]
    );
    assert_eq!(
        saved_after_the_start(&session),
        ["response B with no reasoning", "choice outcome"],
        "saved before its choice outcome, which shows at once"
    );
}

#[test]
fn a_choice_alone_shows_its_outcome_and_the_rubric_and_asks_jev_nothing() {
    let ai = writing();
    let mut session = answering(&ai, 120, TALL);

    submit(&mut session, "b", "");
    session.settle();

    assert_eq!(
        session.section("Choice · Not supported"),
        ["Jev doesn't support B, the option you chose. It supports A, the answer key."]
    );
    assert_eq!(
        session.section("You chose B: Option B of the question"),
        ["The rationale for B, which stays hidden"]
    );
    assert_eq!(
        session.section("The answer key is A: Option A of the question"),
        ["The rationale for A, which stays hidden"]
    );
    assert_eq!(
        session.section("Why it matters"),
        ["Why it matters, which stays hidden"]
    );
    assert_eq!(session.section("Cited").len(), 1);
    assert_eq!(
        session.row_with("Reasoning not written"),
        "Reasoning not written: not assessed"
    );
    assert_eq!(
        session.section("What a full answer covers"),
        [
            "Criterion 1 · Names the key the retry reuses",
            "Criterion 2 · Says why the retry can't charge twice",
        ]
    );
    let text = session.text();
    for absent in [
        "Your reasoning",
        "Met",
        "Not met",
        "Needs review",
        "Awaiting evaluation",
        "Cancel evaluation",
        "Retry evaluation",
        "Explanation",
    ] {
        assert!(!text.contains(absent), "{absent} shows:\n{text}");
    }
    assert_eq!(
        about_responses(&ai),
        [],
        "no reasoning evaluation, explanation or check"
    );
    assert_eq!(
        saved_after_the_start(&session),
        ["response B with no reasoning", "choice outcome"]
    );
}

#[test]
fn reasoning_of_only_whitespace_counts_as_not_written() {
    let ai = writing();
    let mut session = answering(&ai, 120, TALL);

    submit(&mut session, "a", " \n\t \n");
    session.settle();

    assert_eq!(
        session.section("Choice · Supported"),
        ["Jev supports A, the option you chose."]
    );
    assert_eq!(
        session.row_with("Reasoning not written"),
        "Reasoning not written: not assessed"
    );
    assert_eq!(about_responses(&ai), []);
    assert_eq!(
        saved_after_the_start(&session),
        ["response A with no reasoning", "choice outcome"]
    );
}

#[test]
fn with_reasoning_jev_assesses_it_as_before() {
    let ai = writing();
    let mut session = answering(&ai, 120, TALL);

    submit(&mut session, "a", "Retries reuse the key.");
    assert_eq!(
        session.section("Reasoning · Awaiting evaluation"),
        ["Jev is assessing it against each criterion. Your response is submitted."]
    );
    session.settle();

    assert_eq!(
        session.section("Your reasoning"),
        ["Retries reuse the key."]
    );
    assert_eq!(
        session.section("Reasoning"),
        [
            "Met · Names the key the retry reuses",
            "Met · Says why the retry can't charge twice",
        ]
    );
    let [Asked::AssessReasoning { reasoning, .. }] = &about_responses(&ai)[..] else {
        panic!("one reasoning evaluation: {:#?}", ai.asked());
    };
    assert_eq!(reasoning, "Retries reuse the key.");
    assert!(!session.text().contains("What a full answer covers"));
    assert_eq!(
        saved_after_the_start(&session),
        [
            "response A with reasoning",
            "choice outcome",
            "criterion outcome",
            "criterion outcome",
        ]
    );
}

/// The number the summary counts beside `label`.
fn count(session: &Session, label: &str) -> usize {
    let row = session.row_with(label);
    let number = row.strip_prefix(label).map(str::trim_start);
    assert!(
        number.is_some(),
        "no count of {label:?}:\n{}",
        session.text()
    );
    number
        .and_then(|number| number.parse().ok())
        .unwrap_or_default()
}

#[test]
fn the_summary_counts_answers_with_reasoning_and_choices_only_separately() {
    let ai = writing();
    let mut session = answering(&ai, 120, TALL);
    submit(&mut session, "b", "");
    session.press("<Enter>");
    submit(&mut session, "a", "Retries reuse the key.");
    session.press("<Enter>");
    submit(&mut session, "a", "  ");
    session.press("<Enter>");
    submit(&mut session, "c", "The provider dedupes.");
    session.settle();

    activate(&mut session, "Finish");

    assert_eq!(count(&session, "Answered with reasoning"), 2);
    assert_eq!(count(&session, "Choice only"), 2);
    assert_eq!(count(&session, "Skipped"), 0);
    assert_eq!(count(&session, "Awaiting evaluation"), 0);
    assert_eq!(
        session.section("Question 1 · Domain"),
        [
            FIRST,
            "Choice · Not supported",
            "Reasoning not written: not assessed",
        ]
    );
    assert_eq!(
        session.section("Question 2 · Domain"),
        [
            SECOND,
            "Choice · Supported",
            "Met · The rubric's criterion, which stays hidden",
        ]
    );
    assert_eq!(
        session.section("Question 3 · Domain"),
        [
            THIRD,
            "Choice · Supported",
            "Reasoning not written: not assessed",
        ]
    );
    assert_eq!(
        session.section("Question 4 · Domain"),
        [
            "Domain question 4?",
            "Choice · Not supported",
            "Met · The rubric's criterion, which stays hidden",
        ]
    );
    let summary = session.text();
    for absent in CRITERIA {
        assert!(!summary.contains(absent), "{absent} shows:\n{summary}");
    }
}

#[test]
fn an_assisted_choice_alone_is_marked_assisted_and_sends_nothing_more() {
    let ai = writing();
    let mut session = answering(&ai, 120, TALL);
    let asked = ai.asked().len();
    session.press("a");
    activate(&mut session, "Show answer");
    session.press("q i <Tab> <Enter>");
    session.settle();

    assert_eq!(
        session.section("Choice · Supported · Assisted"),
        ["Jev supports A, the option you chose."]
    );
    assert_eq!(
        session.row_with("Reasoning not written"),
        "Reasoning not written: not assessed"
    );
    activate(&mut session, "Inspect Jev's answers");
    assert_eq!(
        session.section("Reasoning"),
        ["Reasoning not written: Jev wasn't asked about it."]
    );
    session.press("q");
    activate(&mut session, "Finish");
    session.press("j <Enter>");

    assert_eq!(count(&session, "Choice only"), 1);
    assert_eq!(count(&session, "Assisted"), 1);
    assert_eq!(
        session.section("Question 1 · Domain · Assisted"),
        [
            FIRST,
            "Choice · Supported · Assisted",
            "Reasoning not written: not assessed",
        ]
    );
    assert_eq!(ai.asked().len(), asked, "nothing more is sent");
    assert!(
        !session
            .store()
            .log()
            .iter()
            .any(|entry| matches!(entry, StoreEntry::Request { .. })),
        "no request ended after the record started: {:#?}",
        session.store().log()
    );
}

#[test]
fn the_question_screen_with_a_choice_and_no_reasoning_at_120x32() {
    let ai = writing();
    let mut session = answering(&ai, 120, 32);
    session.press("b");

    insta::assert_debug_snapshot!("choice_only_question_120x32", session.frame());
}

#[test]
fn the_question_screen_with_a_choice_and_no_reasoning_at_80x24() {
    let ai = writing();
    let mut session = answering(&ai, 80, 24);
    session.press("b");

    insta::assert_debug_snapshot!("choice_only_question_80x24", session.frame());
}

#[test]
fn choice_only_feedback_scrolled_to_the_rubric_at_120x32() {
    let ai = writing();
    let mut session = answering(&ai, 120, 32);
    submit(&mut session, "b", "");
    session.settle();

    session.press("G");

    insta::assert_debug_snapshot!("choice_only_feedback_120x32", session.frame());
}

#[test]
fn choice_only_feedback_scrolled_to_the_rubric_at_80x24() {
    let ai = writing();
    let mut session = answering(&ai, 80, 24);
    submit(&mut session, "b", "");
    session.settle();

    session.press("G");

    assert_eq!(
        session.section("What a full answer covers"),
        [
            "Criterion 1 · Names the key the retry reuses",
            "Criterion 2 · Says why the retry can't charge twice",
        ]
    );
    insta::assert_debug_snapshot!("choice_only_feedback_80x24", session.frame());
}
