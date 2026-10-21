//! Jev assesses a submitted response's reasoning, criterion by criterion,
//! without knowing the choice. After Submit, the choice outcome shows at
//! once and the reasoning awaits evaluation. Then each criterion shows met,
//! not met or needs review. Cancel evaluation stops the request and keeps
//! the response submitted; a failure or a cancellation shows Evaluation
//! unavailable, with Retry evaluation. A result that is no longer current
//! changes nothing (rule 24), and every outcome is saved to the session
//! record.

use std::sync::Arc;

use whetstone::{Assessment, Durability, SavedResponse, StoreEntry};
use whetstone_ai::{Asked, RequestFailure, ScriptedAi};
use whetstone_core::{
    Criterion, CriterionOutcome, OptionLetter, Probability, WrittenCandidate, WrittenCriterion,
};

use crate::answering::{answering, answering_with, submit};
use crate::preparing_questions::{DOMAIN, PAYMENT_LIFECYCLE, area_id, candidate, stems};
use crate::session::{Refusing, Session, TestStore};

/// The first question's stem.
const FIRST: &str = "Domain question 1?";

/// The second question's stem.
const SECOND: &str = "Domain question 2?";

/// The reasoning the tests submit.
const REASONING: &str = "Retries reuse the key, so the provider dedupes them.";

/// A terminal height that fits all of the feedback, for the tests that
/// read the outcomes whole.
const TALL: u16 = 40;

/// What shows under a criterion not met or needing review while the
/// generator writes its explanation, which these tests never deliver.
const EXPLAINING: &str = "The generator is explaining this. Jev checks it before it shows.";

/// The first question's rubric, in order.
const CRITERIA: [&str; 3] = [
    "Names the key the retry reuses",
    "Says why the retry can't charge twice",
    "Says what the timed-out charge may already have done",
];

type Assessing = Result<Vec<Probability>, RequestFailure>;

fn probabilities(values: &[f64]) -> Vec<Probability> {
    values
        .iter()
        .map(|&value| Probability::new(value).expect("from 0 to 1"))
        .collect()
}

/// A scripted AI writing four questions about the domain area of the
/// payment lifecycle change, each passing every check, its answer key A, the
/// first with `criteria` as its rubric. Jev assesses the reasoning written
/// for the first as `assessments`, one per request, in turn. An explanation
/// of it is scripted, for outcomes that ask for one, but never delivered.
fn writing(criteria: &[&str], assessments: Vec<Assessing>) -> Arc<ScriptedAi> {
    let ai = Arc::new(ScriptedAi::new());
    let stems = stems("Domain", 4);
    let mut written: Vec<WrittenCandidate> = stems
        .iter()
        .map(|stem| candidate(stem, "A", PAYMENT_LIFECYCLE))
        .collect();
    written[0].rubric = criteria
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
    for assessment in assessments {
        ai.on_assess(FIRST, assessment);
    }
    ai.on_explain(FIRST, Err(RequestFailure::TimedOut));
    ai
}

/// A session `width` by `height` on the feedback for a response choosing
/// `choice` to the first question that `ai` writes. Jev's assessment isn't
/// delivered yet.
fn submitted(ai: &Arc<ScriptedAi>, choice: &str, width: u16, height: u16) -> Session {
    let mut session = answering(ai, width, height);
    submit(&mut session, choice, REASONING);
    session
}

/// The same as [`submitted`], with Jev's assessment of the three
/// [`CRITERIA`] scripted as `answers` and delivered. The explanation it
/// asks for, if it asks for one, is being written.
fn assessed(answers: &[f64], width: u16, height: u16) -> Session {
    let ai = writing(&CRITERIA, vec![Ok(probabilities(answers))]);
    let mut session = submitted(&ai, "a", width, height);
    session.settle_once();
    session
}

/// Every reasoning evaluation `ai` was asked for, in order.
fn assessments(ai: &ScriptedAi) -> Vec<Asked> {
    ai.asked()
        .into_iter()
        .filter(|asked| matches!(asked, Asked::AssessReasoning { .. }))
        .collect()
}

/// Every response the store saved, in order.
fn saved_responses(log: &[StoreEntry]) -> Vec<SavedResponse> {
    log.iter()
        .filter_map(|entry| match entry {
            StoreEntry::Response { saved, .. } => Some(saved.clone()),
            _ => None,
        })
        .collect()
}

/// Every criterion outcome the store saved, with the response it is of, in
/// order.
fn criterion_outcomes(log: &[StoreEntry]) -> Vec<(SavedResponse, usize, CriterionOutcome)> {
    log.iter()
        .filter_map(|entry| match entry {
            StoreEntry::Assessment {
                saved,
                assessment: Assessment::Criterion { criterion, outcome },
            } => Some((saved.clone(), *criterion, outcome.outcome())),
            _ => None,
        })
        .collect()
}

/// Moves focus from Next question up to the evaluation's control and
/// activates it.
fn activate_the_evaluations_control(session: &mut Session) {
    session.press("k <Enter>");
}

#[test]
fn after_submit_the_reasoning_awaits_evaluation_and_the_choice_outcome_already_shows() {
    let ai = writing(&CRITERIA[..1], vec![Ok(probabilities(&[0.9]))]);
    let session = submitted(&ai, "a", 120, TALL);

    assert_eq!(
        session.question().section_text("Choice · Supported"),
        ["Jev supports A, the option you chose."].join(" ")
    );
    assert_eq!(
        session
            .question()
            .section_text("Reasoning · Awaiting evaluation"),
        ["Jev is assessing it against each criterion. Your response is submitted."].join(" ")
    );
    assert_eq!(
        session.question().row_with("Cancel evaluation"),
        "Cancel evaluation"
    );
    assert_eq!(assessments(&ai).len(), 1, "one Jev call per response");
}

#[test]
fn each_criterion_shows_met_not_met_or_needs_review_in_words_only() {
    let session = assessed(&[0.9, 0.1, 0.5], 120, TALL);

    assert_eq!(
        session.question().section_text("Reasoning"),
        [
            "Met · Names the key the retry reuses",
            "Not met · Says why the retry can't charge twice",
            EXPLAINING,
            "Needs review · Says what the timed-out charge may already have done",
            "Jev couldn't tell whether your reasoning covers this.",
            EXPLAINING,
        ]
        .join(" ")
    );
    let text = session.text();
    assert!(!text.contains("Awaiting evaluation"), "{text}");
    assert!(!text.contains("Cancel evaluation"), "{text}");
    for number in ["0.9", "0.1", "0.5", "%"] {
        assert!(!text.contains(number), "{number} shows:\n{text}");
    }
}

#[test]
fn the_cut_offs_decide_met_and_not_met_and_anything_between_needs_review() {
    let session = assessed(&[0.80, 0.20, 0.79], 120, TALL);

    assert_eq!(
        session.question().section_text("Reasoning"),
        [
            "Met · Names the key the retry reuses",
            "Not met · Says why the retry can't charge twice",
            EXPLAINING,
            "Needs review · Says what the timed-out charge may already have done",
            "Jev couldn't tell whether your reasoning covers this.",
            EXPLAINING,
        ]
        .join(" ")
    );
}

#[test]
fn jev_is_asked_with_the_saved_response_and_never_the_chosen_option_or_the_answer_key() {
    let ai = writing(&CRITERIA, vec![Ok(probabilities(&[0.9, 0.9, 0.9]))]);
    let session = submitted(&ai, "c", 120, 32);

    let asked = ai.asked();
    let Some(Asked::CheckQuestion {
        evidence: checked_with,
        ..
    }) = asked
        .iter()
        .find(|asked| matches!(asked, Asked::CheckQuestion { .. }))
    else {
        panic!("Jev checked the questions: {asked:#?}");
    };
    let [saved] = &saved_responses(&session.store().log())[..] else {
        panic!("one response saved: {:#?}", session.store().log());
    };
    // Every field the request held: none has room for the chosen option or
    // the answer key.
    let [
        Asked::AssessReasoning {
            saved: asked_with,
            evidence,
            stem,
            options,
            rubric,
            reasoning,
        },
    ] = &assessments(&ai)[..]
    else {
        panic!("one reasoning evaluation: {asked:#?}");
    };
    assert_eq!(
        asked_with,
        &format!("{saved:?}"),
        "asked with the saved response"
    );
    assert_eq!(evidence, checked_with, "the evidence, exactly as sent");
    assert_eq!(stem, FIRST);
    assert_eq!(
        options,
        &OptionLetter::ALL.map(|letter| format!("Option {letter} of the question"))
    );
    assert_eq!(
        rubric.iter().map(Criterion::statement).collect::<Vec<_>>(),
        CRITERIA
    );
    assert_eq!(reasoning, REASONING);
    let logged = format!("{:?}", assessments(&ai));
    for hidden in [
        "The rationale for",
        "Why it matters, which stays hidden",
        "answer_key",
        "choice",
    ] {
        assert!(!logged.contains(hidden), "{hidden:?} was sent: {logged}");
    }
}

#[test]
fn a_response_the_store_did_not_save_is_never_evaluated() {
    let ai = writing(&CRITERIA, Vec::new());
    let store = TestStore::new(Durability::InMemory, Refusing::Responses);
    let mut session = answering_with(&ai, &store, 120, 32);

    submit(&mut session, "a", REASONING);
    session.settle();

    assert!(
        session
            .text()
            .contains("Your response wasn't saved, so it isn't submitted:"),
        "{}",
        session.text()
    );
    assert_eq!(
        assessments(&ai),
        [],
        "no evaluation without a saved response"
    );
}

#[test]
fn every_criterion_outcome_is_saved_to_the_session_record_after_the_choice_outcome() {
    let session = assessed(&[0.9, 0.1, 0.5], 120, 32);

    let log = session.store().log();
    let [saved] = &saved_responses(&log)[..] else {
        panic!("one response saved: {log:#?}");
    };
    assert!(
        matches!(
            &log[..3],
            [
                StoreEntry::Started { .. },
                StoreEntry::Response { saved: response, .. },
                StoreEntry::Assessment {
                    assessment: Assessment::Choice(_),
                    ..
                },
            ] if response == saved
        ),
        "the start, the response, then its choice outcome: {log:#?}"
    );
    assert_eq!(
        criterion_outcomes(&log),
        [
            (saved.clone(), 0, CriterionOutcome::Met),
            (saved.clone(), 1, CriterionOutcome::NotMet),
            (saved.clone(), 2, CriterionOutcome::NeedsReview),
        ]
    );
    assert!(
        matches!(
            log.last(),
            Some(StoreEntry::Assessment {
                assessment: Assessment::Criterion { criterion: 2, .. },
                ..
            })
        ),
        "the criterion outcomes come last: {log:#?}"
    );
}

#[test]
fn cancel_stops_the_evaluation_and_keeps_the_response_submitted() {
    let ai = writing(&CRITERIA, vec![Ok(probabilities(&[0.9, 0.9, 0.9]))]);
    let mut session = submitted(&ai, "a", 120, TALL);

    activate_the_evaluations_control(&mut session);
    session.settle();

    assert_eq!(
        session
            .question()
            .section_text("Reasoning · Evaluation unavailable"),
        [
            "You cancelled the evaluation.",
            "Your response stays submitted. Retry evaluation asks Jev again.",
        ]
        .join(" ")
    );
    assert!(
        session
            .rows()
            .iter()
            .any(|row| row.trim() == "> Retry evaluation"),
        "Retry evaluation has focus:\n{}",
        session.text()
    );
    assert_eq!(
        session.question().section_text("Choice · Supported"),
        ["Jev supports A, the option you chose."].join(" ")
    );
    let log = session.store().log();
    assert_eq!(saved_responses(&log).len(), 1, "still submitted");
    assert_eq!(
        criterion_outcomes(&log),
        [],
        "the cancelled result is never applied"
    );
}

#[test]
fn a_failure_shows_evaluation_unavailable_and_retry_asks_jev_again() {
    let ai = writing(
        &CRITERIA,
        vec![
            Err(RequestFailure::RateLimited),
            Ok(probabilities(&[0.9, 0.9, 0.9])),
        ],
    );
    let mut session = submitted(&ai, "a", 120, TALL);
    session.settle();
    assert_eq!(
        session
            .question()
            .section_text("Reasoning · Evaluation unavailable"),
        [
            "Jev's evaluation failed: rate limited.",
            "OpenRouter had too many requests from your key too quickly. Wait a moment, then \
             Retry.",
            "Your response stays submitted. Retry evaluation asks Jev again.",
        ]
        .join(" ")
    );
    assert_eq!(criterion_outcomes(&session.store().log()), []);

    activate_the_evaluations_control(&mut session);
    assert_eq!(
        session
            .question()
            .section_text("Reasoning · Awaiting evaluation"),
        ["Jev is assessing it against each criterion. Your response is submitted."].join(" ")
    );
    session.settle();

    assert_eq!(
        session.question().section_text("Reasoning"),
        CRITERIA
            .map(|criterion| format!("Met · {criterion}"))
            .join(" ")
    );
    let asked = assessments(&ai);
    assert_eq!(asked.len(), 2, "the Retry asked once more: {asked:#?}");
    assert_eq!(asked[0], asked[1], "the same request again");
    let log = session.store().log();
    assert_eq!(saved_responses(&log).len(), 1, "nothing else is submitted");
    assert_eq!(criterion_outcomes(&log).len(), 3);
}

#[test]
fn a_reply_that_doesnt_answer_each_criterion_is_unavailable_never_needs_review() {
    let ai = writing(&CRITERIA, vec![Ok(probabilities(&[0.9]))]);
    let mut session = submitted(&ai, "a", 120, TALL);
    session.settle();

    assert_eq!(
        session
            .question()
            .section_text("Reasoning · Evaluation unavailable"),
        [
            "Jev's evaluation failed: the reply couldn't be read.",
            "Nothing in Jev's reply is used, and none of it is shown as a judgement.",
            "Your response stays submitted. Retry evaluation asks Jev again.",
        ]
        .join(" ")
    );
    assert_eq!(criterion_outcomes(&session.store().log()), []);
}

#[test]
fn a_result_from_a_cancelled_request_is_ignored_once_retry_asks_again() {
    let ai = writing(
        &CRITERIA,
        vec![
            Ok(probabilities(&[0.9, 0.9, 0.9])),
            Ok(probabilities(&[0.1, 0.1, 0.1])),
        ],
    );
    let mut session = submitted(&ai, "a", 120, TALL);

    activate_the_evaluations_control(&mut session);
    activate_the_evaluations_control(&mut session);
    session.settle_once();

    assert_eq!(
        session.question().section_text("Reasoning"),
        CRITERIA
            .map(|criterion| [format!("Not met · {criterion}"), EXPLAINING.to_owned()])
            .concat()
            .join(" "),
        "only the current request's result applies"
    );
    let outcomes: Vec<CriterionOutcome> = criterion_outcomes(&session.store().log())
        .into_iter()
        .map(|(_, _, outcome)| outcome)
        .collect();
    assert_eq!(outcomes, [CriterionOutcome::NotMet; 3]);
}

#[test]
fn a_result_after_leaving_the_practice_changes_nothing() {
    let ai = writing(&CRITERIA, vec![Ok(probabilities(&[0.9, 0.9, 0.9]))]);
    let mut session = submitted(&ai, "a", 120, 32);

    session.press("q");
    let left = session.text();
    session.settle();

    assert_eq!(session.text(), left, "the late result changes nothing");
    assert_eq!(criterion_outcomes(&session.store().log()), []);
}

#[test]
fn each_result_applies_to_its_own_response_even_once_another_question_shows() {
    let ai = writing(&CRITERIA[..1], vec![Ok(probabilities(&[0.9]))]);
    ai.on_assess(SECOND, Ok(probabilities(&[0.1])));
    ai.on_explain(SECOND, Err(RequestFailure::TimedOut));
    let mut session = submitted(&ai, "a", 120, TALL);

    session.press("<Enter>");
    submit(&mut session, "b", "The key is reused.");
    session.settle_once();

    assert_eq!(
        session.question().section_text("Reasoning"),
        [
            "Not met · The rubric's criterion, which stays hidden",
            EXPLAINING
        ]
        .join(" ")
    );
    let log = session.store().log();
    let [first, second] = &saved_responses(&log)[..] else {
        panic!("two responses saved: {log:#?}");
    };
    assert_eq!(
        criterion_outcomes(&log),
        [
            (first.clone(), 0, CriterionOutcome::Met),
            (second.clone(), 0, CriterionOutcome::NotMet),
        ]
    );
}

#[test]
fn reasoning_awaiting_evaluation_at_80x24() {
    let ai = writing(&CRITERIA, vec![Ok(probabilities(&[0.9, 0.1, 0.5]))]);
    let mut session = submitted(&ai, "a", 80, 24);
    session.press("G");

    insta::assert_debug_snapshot!("reasoning_awaiting_evaluation_80x24", session.frame());
}

#[test]
fn reasoning_awaiting_evaluation_at_120x32() {
    let ai = writing(&CRITERIA, vec![Ok(probabilities(&[0.9, 0.1, 0.5]))]);
    let session = submitted(&ai, "a", 120, 32);

    insta::assert_debug_snapshot!("reasoning_awaiting_evaluation_120x32", session.frame());
}

#[test]
fn reasoning_met_not_met_and_needs_review_at_80x24() {
    let mut session = assessed(&[0.9, 0.1, 0.5], 80, 24);
    session.press("G");

    insta::assert_debug_snapshot!("reasoning_assessed_80x24", session.frame());
}

#[test]
fn reasoning_met_not_met_and_needs_review_at_120x32() {
    let session = assessed(&[0.9, 0.1, 0.5], 120, 32);

    insta::assert_debug_snapshot!("reasoning_assessed_120x32", session.frame());
}

#[test]
fn evaluation_unavailable_at_120x32() {
    let ai = writing(&CRITERIA, vec![Err(RequestFailure::TimedOut)]);
    let mut session = submitted(&ai, "a", 120, 32);
    session.settle();

    insta::assert_debug_snapshot!("reasoning_evaluation_unavailable_120x32", session.frame());
}

#[test]
fn evaluation_unavailable_at_80x24() {
    let ai = writing(&CRITERIA, vec![Err(RequestFailure::TimedOut)]);
    let mut session = submitted(&ai, "a", 80, 24);
    session.settle();
    session.press("G");

    insta::assert_debug_snapshot!("reasoning_evaluation_unavailable_80x24", session.frame());
}
