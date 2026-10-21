//! A criterion that asks for more than the question needs is dropped: Jev's
//! question check answers `criterion_N_minimum` for each criterion, and the
//! show rule drops a criterion whose answer isn't clear yes, as it drops a
//! failing hint, keeping the question with the other criteria. Reasoning is
//! assessed against the kept criteria alone, and feedback, explanations, the
//! summary and what a full answer covers show only them. The session record
//! keeps the dropped criteria and the answers that dropped them, and Inspect
//! Jev's answers says which were dropped.

use std::sync::Arc;

use whetstone::StoreEntry;
use whetstone_ai::{Asked, ParagraphCheck, ScriptedAi, WrittenParagraph};
use whetstone_core::{
    CandidateCheck, Criterion, CriterionOutcome, Probability, Question, WrittenCitation,
    WrittenCriterion,
};

use crate::answering::{answering, submit};
use crate::help_and_signals::activate;
use crate::preparing_questions::{
    DOMAIN, PAYMENT_LIFECYCLE, answer, area_id, candidate, stems, touched_path,
};
use crate::session::Session;

/// The first question's stem.
const FIRST: &str = "Domain question 1?";

/// The first question's criterion that asks only for what it needs.
const KEPT: &str = "Says the retry carries the payment's own key";

/// The first question's criterion that asks for more: an identifier and a
/// literal condition.
const DROPPED: &str = "Names `ChargeRequest::new` and quotes `key == payment.key`";

/// Jev's answer to whether [`DROPPED`] asks only for what the question
/// needs: clear no.
const ASKS_TOO_MUCH: f64 = 0.15;

/// A terminal height that fits all of the feedback.
const TALL: u16 = 60;

/// A paragraph explaining what reasoning missed of [`KEPT`].
const EXPLAINED: &str = "The retry's key comes from the payment, so no caller can choose it.";

fn probability(value: f64) -> Probability {
    Probability::new(value).expect("from 0 to 1")
}

/// A scripted AI writing four questions about the domain area of the
/// payment lifecycle change, each passing every check, its answer key A.
/// The first's rubric is [`KEPT`] then [`DROPPED`], and Jev answers that
/// [`DROPPED`] asks for more than the question needs. Jev assesses
/// reasoning written for the first as `assessed`, a probability per
/// criterion it is asked about.
fn writing(assessed: &[f64]) -> Arc<ScriptedAi> {
    let ai = Arc::new(ScriptedAi::new());
    let stems = stems("Domain", 4);
    let mut written: Vec<_> = stems
        .iter()
        .map(|stem| candidate(stem, "A", PAYMENT_LIFECYCLE))
        .collect();
    written[0].rubric = [KEPT, DROPPED]
        .iter()
        .map(|&statement| WrittenCriterion {
            statement: statement.to_owned(),
            met: format!("Meeting it: {statement}"),
            not_met: format!("Not meeting it: {statement}"),
        })
        .collect();
    ai.on_write(&area_id(DOMAIN), Ok(written));
    ai.on_check_passing_except(
        FIRST,
        [answer(CandidateCheck::CriterionMinimum(1), ASKS_TOO_MUCH)],
    );
    for stem in &stems[1..] {
        ai.on_check_passing(stem);
    }
    ai.on_assess(
        FIRST,
        Ok(assessed.iter().map(|&value| probability(value)).collect()),
    );
    ai
}

/// A session `width` by `height` on the feedback for a response choosing A
/// to the first question `ai` writes, with `reasoning`, and every result
/// delivered.
fn answered(ai: &Arc<ScriptedAi>, reasoning: &str, width: u16, height: u16) -> Session {
    let mut session = answering(ai, width, height);
    submit(&mut session, "a", reasoning);
    session.settle();
    session
}

/// The questions the session record started with.
fn recorded(session: &Session) -> Vec<Question> {
    session
        .store()
        .log()
        .into_iter()
        .find_map(|entry| match entry {
            StoreEntry::Started { record, .. } => Some(record.questions),
            _ => None,
        })
        .expect("the session record started")
}

#[test]
fn reasoning_is_assessed_against_the_kept_criteria_alone_and_feedback_shows_only_them() {
    let ai = writing(&[0.9]);

    let session = answered(&ai, "The retry reuses the payment's key.", 120, TALL);

    assert_eq!(
        session.question().section_text("Reasoning"),
        format!("Met · {KEPT}")
    );
    let assessed: Vec<Vec<String>> = ai
        .asked()
        .into_iter()
        .filter_map(|asked| match asked {
            Asked::AssessReasoning { rubric, .. } => Some(
                rubric
                    .iter()
                    .map(|criterion| criterion.statement().to_owned())
                    .collect(),
            ),
            _ => None,
        })
        .collect();
    assert_eq!(assessed, [[KEPT]], "Jev is asked about the kept criterion");
    assert!(!session.text().contains(DROPPED), "{}", session.text());
}

#[test]
fn an_explanation_explains_the_kept_criteria_by_their_place_among_them() {
    let ai = writing(&[0.1]);
    ai.on_explain(
        FIRST,
        Ok(vec![WrittenParagraph {
            criterion: 0,
            outcome: CriterionOutcome::NotMet,
            text: EXPLAINED.to_owned(),
            citations: vec![WrittenCitation {
                path: touched_path(PAYMENT_LIFECYCLE),
                first_line: 1,
                last_line: 1,
            }],
        }]),
    );
    ai.on_check_explanation(
        FIRST,
        Ok(vec![ParagraphCheck {
            supported: probability(0.9),
            agrees: probability(0.9),
        }]),
    );

    let session = answered(&ai, "It retries.", 120, TALL);

    let reasoning = session.question().section_text("Reasoning");
    assert!(
        reasoning.starts_with(&format!(
            "Not met · {KEPT} Explanation · generator {EXPLAINED}"
        )),
        "{reasoning}"
    );
    let explained: Vec<Vec<(usize, CriterionOutcome)>> = ai
        .asked()
        .into_iter()
        .filter_map(|asked| match asked {
            Asked::Explain { criteria, .. } => Some(criteria),
            _ => None,
        })
        .collect();
    assert_eq!(explained, [[(0, CriterionOutcome::NotMet)]]);
}

#[test]
fn a_choice_alone_shows_only_the_kept_criteria_as_what_a_full_answer_covers() {
    let ai = writing(&[]);

    let session = answered(&ai, "", 120, TALL);

    assert_eq!(
        session.question().section_text("What a full answer covers"),
        format!("Criterion 1 · {KEPT}")
    );
    assert!(!session.text().contains(DROPPED), "{}", session.text());
}

#[test]
fn the_summary_shows_only_the_kept_criteria() {
    let ai = writing(&[0.9]);
    let mut session = answered(&ai, "The retry reuses the payment's key.", 120, TALL);

    // Finish asks first, as three questions are still unattempted.
    activate(&mut session, "Finish");
    session.press("j <Enter>");

    assert_eq!(
        session.section("Question 1 · Domain"),
        [
            FIRST.to_owned(),
            "Choice · Supported".to_owned(),
            format!("Met · {KEPT}")
        ]
    );
    assert!(!session.text().contains(DROPPED), "{}", session.text());
}

#[test]
fn the_session_record_keeps_the_dropped_criteria_and_the_answers_that_dropped_them() {
    let ai = writing(&[0.9]);

    let session = answered(&ai, "The retry reuses the payment's key.", 120, TALL);

    let questions = recorded(&session);
    let first = &questions[0];
    assert_eq!(first.stem(), FIRST);
    assert_eq!(
        first
            .rubric()
            .iter()
            .map(Criterion::statement)
            .collect::<Vec<_>>(),
        [KEPT]
    );
    assert_eq!(
        first
            .dropped_criteria()
            .map(|(place, criterion)| (place, criterion.statement()))
            .collect::<Vec<_>>(),
        [(1, DROPPED)]
    );
    assert_eq!(
        first.answers().get(CandidateCheck::CriterionMinimum(1)),
        Some(probability(ASKS_TOO_MUCH)),
        "the answer that dropped it"
    );
    assert_eq!(
        questions[1].dropped_criteria().count(),
        0,
        "a question whose criteria all ask only what it needs drops none"
    );
}

#[test]
fn inspect_jevs_answers_says_which_criteria_were_dropped() {
    let ai = writing(&[0.9]);
    let mut session = answered(&ai, "The retry reuses the payment's key.", 120, TALL);

    activate(&mut session, "Inspect Jev's answers");

    // Its rows as one line, so a line is found wherever it wraps.
    let question_check = session.section("Question check").join(" ");
    for line in [
        "Criterion 1 asks only for what the question needs · 0.9 · clear yes",
        "Criterion 2 asks only for what the question needs · 0.15 · clear no",
        &format!(
            "Criterion 2 dropped · {DROPPED} · it asks for more than the question needs, so \
             reasoning isn't assessed against it"
        ),
    ] {
        assert!(
            question_check.contains(line),
            "{line:?} in {question_check:?}"
        );
    }
    assert_eq!(
        session.section("Reasoning")[2..],
        [format!("Meets criterion 1: {KEPT} · 0.9 · clear yes")]
    );
}
