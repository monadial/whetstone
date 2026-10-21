//! Finish and the summary. Finish, on feedback or in the palette, asks first
//! while questions are still unattempted. The summary counts what happened
//! to the questions, lists each one's choice outcome and criterion outcomes
//! separately, with no combined grade, and gives the session's cost as each
//! reply reported it, for Jev and for the generator, never estimated. It
//! leads back to home.

use std::sync::Arc;

use whetstone_ai::{Asked, RequestFailure, ScriptedAi};
use whetstone_core::{Cost, Probability, Role};

use crate::answering::{answering, submit, type_text};
use crate::help_and_signals::activate;
use crate::preparing_questions::{
    DOMAIN, PAYMENT_LIFECYCLE, area_id, candidate, reasoning_met, stems, writing_about_the_domain,
};
use crate::session::{Session, columns};

/// The number the summary counts beside `label`.
fn count(session: &Session, label: &str) -> usize {
    let rows = session.rows();
    let row = rows.iter().map(|row| row.trim()).find_map(|row| {
        let rest = row.strip_prefix(label)?;
        let number = rest.trim_start();
        (rest.len() > number.len()).then(|| number.parse().ok())?
    });
    assert!(row.is_some(), "no count of {label:?}:\n{}", session.text());
    row.unwrap_or_default()
}

/// Submits a response to each of the `count` questions showing in turn,
/// moving on with Next question, and lets every evaluation come.
fn answer_every_question(session: &mut Session, count: usize) {
    for place in 1..=count {
        submit(session, "a", "Because.");
        if place < count {
            session.press("<Enter>");
        }
    }
    session.settle();
}

#[test]
fn finish_once_every_question_is_attempted_shows_the_summary_at_once() {
    let ai = writing_about_the_domain(3, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 120, 32);
    answer_every_question(&mut session, 3);

    activate(&mut session, "Finish");

    assert_eq!(session.rows()[1].trim(), "Summary", "{}", session.text());
    assert_eq!(count(&session, "Answered"), 3);
    assert_eq!(count(&session, "Skipped"), 0);
}

/// The rows of the box asking whether to finish, trimmed.
fn finish_rows(session: &Session) -> Vec<String> {
    session.box_rows("Finish this Practice?")
}

#[test]
fn finish_on_feedback_asks_first_while_questions_are_still_unattempted() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 80, 24);
    submit(&mut session, "a", "Because.");
    session.settle();

    activate(&mut session, "Finish");
    assert_eq!(
        finish_rows(&session),
        [
            "3 of 4 questions are still unattempted. If you finish now,",
            "they count as skipped.",
            "",
            "> Keep answering",
            "Finish",
        ]
    );
    session.press("<Enter>");
    assert!(!session.text().contains("Finish this Practice?"));
    assert_eq!(session.row_with("Question 1"), "Question 1 of 4 · Domain");
    assert_eq!(session.row_with("Finish"), "> Finish", "focus stays");

    session.press("<Enter> j <Enter>");

    assert_eq!(session.rows()[1].trim(), "Summary", "{}", session.text());
    assert_eq!(count(&session, "Answered"), 1);
    assert_eq!(count(&session, "Skipped"), 3);
}

#[test]
fn finish_in_the_palette_asks_first_too_and_says_a_draft_isnt_kept() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 120, 32);
    session.press("b");

    // The palette's Finish, after Inspect source.
    session.press(": j <Enter>");

    assert_eq!(
        finish_rows(&session)[..3],
        [
            "4 of 4 questions are still unattempted. If you finish now,",
            "they count as skipped. Any draft you started isn't kept.",
            "",
        ]
    );
    session.press("<Esc>");
    assert!(!session.text().contains("Finish this Practice?"));
    assert_eq!(
        session.row_with("Option B"),
        "> (x) B  Option B of the question",
        "the draft stays"
    );

    session.press(": j <Enter> j <Enter>");

    assert_eq!(session.rows()[1].trim(), "Summary", "{}", session.text());
    assert_eq!(count(&session, "Skipped"), 4);
    assert!(
        !session.text().contains("Lose your draft?"),
        "finishing asked already"
    );
}

#[test]
fn finish_on_feedback_in_the_palette_asks_first_while_questions_are_still_unattempted() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 80, 24);
    submit(&mut session, "a", "Because.");

    // The palette's Finish, after Inspect source.
    session.press(": j <Enter>");

    assert_eq!(finish_rows(&session)[3..], ["> Keep answering", "Finish"]);
    session.press("j <Enter>");
    assert_eq!(session.rows()[1].trim(), "Summary", "{}", session.text());
}

#[test]
fn the_summary_leads_back_to_home_without_asking_about_drafts_finishing_dropped() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 80, 24);
    session.press("b");
    session.press(": j <Enter> j <Enter>");
    assert_eq!(session.row_with("Home"), "> Home");

    session.press("<Enter>");

    assert!(
        session.text().contains("Practice on the showcase project"),
        "{}",
        session.text()
    );
    assert!(!session.text().contains("Lose your draft?"));
}

/// A session showing the summary of four questions about the domain area,
/// each kind of ending at least once: the first an assisted attempt,
/// answered and assessed; the second reported, then skipped; the third one
/// that can't be answered from this context; and the fourth answered, its
/// reasoning still awaiting evaluation as the learner finished.
fn finished_every_way(width: u16, height: u16) -> Session {
    finished_every_way_with(
        &writing_about_the_domain(6, PAYMENT_LIFECYCLE),
        width,
        height,
    )
}

/// The same as [`finished_every_way`], with the questions `ai` writes and
/// checks.
fn finished_every_way_with(ai: &Arc<ScriptedAi>, width: u16, height: u16) -> Session {
    let mut session = answering(ai, width, height);
    session.press("a");
    activate(&mut session, "Show answer");
    session.press("q i");
    type_text(&mut session, "Because.");
    session.press("<Tab> <Enter>");
    session.settle();
    session.press("<Enter>");

    activate(&mut session, "Report");
    session.press("<Space> <Tab> <Tab> <Tab> <Tab> <Enter>");
    activate(&mut session, "Skip");
    activate(&mut session, "Cannot answer from this context");
    submit(&mut session, "b", "Because.");

    activate(&mut session, "Finish");
    session.press("j <Enter>");
    session
}

#[test]
fn the_summary_counts_answered_skipped_cannot_answer_awaiting_evaluation_assisted_and_reported() {
    let session = finished_every_way(120, 32);

    assert_eq!(
        [
            "Answered",
            "Skipped",
            "Cannot answer from this context",
            "Awaiting evaluation",
            "Assisted",
            "Reported",
        ]
        .map(|label| count(&session, label)),
        [2, 1, 1, 1, 1, 1],
        "{}",
        session.text()
    );
}

#[test]
fn an_evaluation_still_awaited_as_the_learner_finished_comes_into_the_summary() {
    let mut session = finished_every_way(120, 32);

    session.settle();

    assert_eq!(count(&session, "Awaiting evaluation"), 0);
    assert_eq!(count(&session, "Answered"), 2);
}

/// Tall enough for the whole summary of four questions.
const TALL: u16 = 48;

#[test]
fn the_summary_lists_each_questions_choice_outcome_and_criterion_outcomes_separately() {
    let session = finished_every_way(120, TALL);

    assert_eq!(
        session.section("Question 1 · Domain · Assisted"),
        [
            "Domain question 1?",
            "Choice · Supported · Assisted",
            "Met · Assisted · The rubric's criterion, which stays hidden",
        ]
    );
    assert_eq!(
        session.section("Question 2 · Domain · Reported"),
        ["Domain question 2?", "Skipped"]
    );
    assert_eq!(
        session.section("Question 3 · Domain"),
        ["Domain question 3?", "Cannot answer from this context"]
    );
    assert_eq!(
        session.section("Question 4 · Domain"),
        [
            "Domain question 4?",
            "Choice · Not supported",
            "Reasoning · Awaiting evaluation",
        ]
    );
}

/// The heading of the summary's cost.
const COST: &str = "Cost, as each reply reported it";

/// The summary's cost, a row a line, split into its columns.
fn cost(session: &Session) -> Vec<Vec<String>> {
    columns(&session.section(COST))
        .into_iter()
        .map(|row| row.into_iter().map(str::to_owned).collect())
        .collect()
}

/// Scripts each of `ai`'s replies from `role` to report `usd`, or no cost.
fn costing(ai: &ScriptedAi, role: Role, usd: Option<f64>) {
    ai.report_cost(role, usd.map(|usd| Cost::new(usd).expect("a cost")));
}

/// Answers each of the `count` questions `ai` writes and checks about the
/// domain area, lets every request reply, and finishes, at 120 by
/// [`TALL`].
fn answered_and_finished(ai: &Arc<ScriptedAi>, count: usize) -> Session {
    let mut session = answering(ai, 120, TALL);
    answer_every_question(&mut session, count);
    activate(&mut session, "Finish");
    session
}

#[test]
fn the_cost_adds_up_what_each_reply_reported_for_jev_and_for_the_generator() {
    let ai = writing_about_the_domain(3, PAYMENT_LIFECYCLE);
    costing(&ai, Role::Jev, Some(0.000_012));
    costing(&ai, Role::Generator, Some(0.0774));

    let session = answered_and_finished(&ai, 3);

    // Jev's gate, three checks and three evaluations, and the generator's
    // one write.
    assert_eq!(
        cost(&session),
        [
            ["Jev", "$0.000084"],
            ["Generator", "$0.0774"],
            ["Total", "$0.077484"],
        ]
    );
}

#[test]
fn the_cost_says_how_many_requests_reported_none_a_failed_one_too_and_never_estimates_them() {
    // Three questions, and Jev's evaluation of the first one's reasoning
    // fails.
    let ai = Arc::new(ScriptedAi::new());
    let stems = stems("Domain", 3);
    ai.on_write(
        &area_id(DOMAIN),
        Ok(stems
            .iter()
            .map(|stem| candidate(stem, "A", PAYMENT_LIFECYCLE))
            .collect()),
    );
    for stem in &stems {
        ai.on_check_passing(stem);
    }
    ai.on_assess(&stems[0], Err(RequestFailure::TimedOut));
    for stem in &stems[1..] {
        reasoning_met(&ai, stem);
    }
    costing(&ai, Role::Jev, Some(0.000_012));
    costing(&ai, Role::Generator, None);

    let session = answered_and_finished(&ai, 3);

    // Jev's gate, three checks and two evaluations reported theirs; the
    // generator's write reported none, and the failed evaluation had no
    // reply.
    assert_eq!(
        cost(&session),
        [
            vec!["Jev", "$0.000072"],
            vec!["Generator", "$0.00"],
            vec!["Total", "$0.000072"],
            vec!["Not reported for 2 requests"],
        ]
    );
    assert_eq!(
        session.section("Question 1 · Domain")[1..],
        ["Choice · Supported", "Reasoning · Evaluation unavailable"]
    );
}

#[test]
fn an_evaluation_the_learner_cancelled_counts_as_a_request_that_reported_no_cost() {
    let ai = writing_about_the_domain(3, PAYMENT_LIFECYCLE);
    costing(&ai, Role::Jev, Some(0.25));
    costing(&ai, Role::Generator, Some(1.5));
    let mut session = answering(&ai, 120, TALL);
    submit(&mut session, "a", "Because.");
    activate(&mut session, "Cancel evaluation");
    session.settle();
    activate(&mut session, "Next question");
    answer_every_question(&mut session, 2);

    activate(&mut session, "Finish");

    // The gate, three checks and two evaluations, and the write: the
    // cancelled evaluation's cost, if it had one, never came.
    assert_eq!(
        cost(&session),
        [
            vec!["Jev", "$1.50"],
            vec!["Generator", "$1.50"],
            vec!["Total", "$3.00"],
            vec!["Not reported for 1 request"],
        ]
    );
}

#[test]
fn finishing_sends_no_new_request_but_one_still_running_comes_in() {
    // One question, whose reasoning Jev finds not met: on feedback, the
    // generator would then be asked to explain it.
    let ai = Arc::new(ScriptedAi::new());
    let stem = "Domain question 1?";
    ai.on_write(
        &area_id(DOMAIN),
        Ok(vec![candidate(stem, "A", PAYMENT_LIFECYCLE)]),
    );
    ai.on_check_passing(stem);
    ai.on_assess(stem, Ok(vec![Probability::new(0.1).expect("from 0 to 1")]));
    let mut session = answering(&ai, 120, TALL);
    submit(&mut session, "a", "Because.");

    activate(&mut session, "Finish");
    session.settle();

    assert_eq!(
        session.section("Question 1 · Domain")[1..],
        [
            "Choice · Supported",
            "Not met · The rubric's criterion, which stays hidden",
        ],
        "the evaluation running as the learner finished came in"
    );
    assert!(
        !ai.asked()
            .iter()
            .any(|asked| matches!(asked, Asked::Explain { .. })),
        "nothing is asked once the learner finished"
    );
}

/// [`finished_every_way`], with every reply reporting its cost.
fn summary_with_its_cost(width: u16, height: u16) -> Session {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    costing(&ai, Role::Jev, Some(0.000_012));
    costing(&ai, Role::Generator, Some(0.0774));
    finished_every_way_with(&ai, width, height)
}

#[test]
fn the_summary_at_120x32() {
    let session = summary_with_its_cost(120, 32);

    insta::assert_debug_snapshot!("summary_120x32", session.frame());
}

#[test]
fn the_summary_at_80x24() {
    let session = summary_with_its_cost(80, 24);

    insta::assert_debug_snapshot!("summary_80x24", session.frame());
}

#[test]
fn the_summary_scrolled_to_its_cost_at_80x24() {
    let mut session = summary_with_its_cost(80, 24);

    session.press("G");

    assert_eq!(session.row_with("Total"), "Total      $0.077496");
    insta::assert_debug_snapshot!("summary_scrolled_to_its_cost_80x24", session.frame());
}

/// A session asking whether to finish, `width` by `height`, with the first
/// of four questions answered.
fn asking_to_finish(width: u16, height: u16) -> Session {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, width, height);
    submit(&mut session, "a", "Because.");
    session.settle();
    activate(&mut session, "Finish");
    session
}

#[test]
fn asking_whether_to_finish_at_120x32() {
    let session = asking_to_finish(120, 32);

    insta::assert_debug_snapshot!("asking_whether_to_finish_120x32", session.frame());
}

#[test]
fn asking_whether_to_finish_at_80x24() {
    let session = asking_to_finish(80, 24);

    insta::assert_debug_snapshot!("asking_whether_to_finish_80x24", session.frame());
}
