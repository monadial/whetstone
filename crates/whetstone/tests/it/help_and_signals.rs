//! The learner's help controls and signals on a question: Hint, Show
//! answer, Skip, Cannot answer from this context and Report. They are
//! reached by focus, with Tab or `j`/`k`, never by bare letters, so the
//! reasoning editor keeps every key as text (rule 29). Each is saved to the
//! session record, and none is held against the learner.

use std::sync::Arc;

use ratatui::crossterm::event::Event;
use whetstone::{Assessment, Report, ReportKind, Signal, StoreEntry};
use whetstone_ai::{Asked, RequestFailure, ScriptedAi};
use whetstone_core::{CandidateCheck, Probability};

use crate::answering::{answering, reasoning, submit, type_text};
use crate::preparing_questions::{
    DOMAIN, PAYMENT_LIFECYCLE, answer, area_id, candidate, reasoning_met, stems,
    writing_about_the_domain,
};
use crate::session::Session;

/// The first question's stem.
const FIRST: &str = "Domain question 1?";

/// A scripted AI writing four questions about the domain area of the
/// payment lifecycle change, their answer keys A, and checking each but the
/// first as passing every check. The test scripts the rest.
fn writing_four() -> (Arc<ScriptedAi>, Vec<String>) {
    let ai = Arc::new(ScriptedAi::new());
    let stems = stems("Domain", 4);
    ai.on_write(
        &area_id(DOMAIN),
        Ok(stems
            .iter()
            .map(|stem| candidate(stem, "A", PAYMENT_LIFECYCLE))
            .collect()),
    );
    for stem in &stems[1..] {
        ai.on_check_passing(stem);
    }
    (ai, stems)
}

/// Four questions, each passing every check but the first's hint, which
/// fails its check, so the first has no hint, and each one's reasoning
/// meeting its criterion once submitted.
fn first_without_a_hint() -> Arc<ScriptedAi> {
    let (ai, stems) = writing_four();
    ai.on_check_passing_except(FIRST, [answer(CandidateCheck::HintHidesAnswerKey, 0.5)]);
    for stem in &stems {
        reasoning_met(&ai, stem);
    }
    ai
}

/// Moves focus with Tab to the control labelled `label`, then activates it
/// with Enter.
pub(crate) fn activate(session: &mut Session, label: &str) {
    focus(session, label);
    session.press("<Enter>");
}

/// Moves focus with Tab to the control labelled `label`.
pub(crate) fn focus(session: &mut Session, label: &str) {
    let focused = format!("> {label}");
    for _ in 0..20 {
        if session.text().contains(&focused) {
            break;
        }
        session.press("<Tab>");
    }
    assert!(
        session.text().contains(&focused),
        "Tab never reaches {label:?}:\n{}",
        session.text()
    );
}

/// Writes `reasoning`, starting with `i` from any control but the
/// options, and submits it, with Tab from the editor to Submit response and
/// Enter.
fn write_and_submit(session: &mut Session, reasoning: &str) {
    session.press("i");
    type_text(session, reasoning);
    session.press("<Tab> <Enter>");
}

/// The signals saved to the session record so far, with the stem of the
/// question each is about, in order.
pub(crate) fn signals(session: &Session) -> Vec<(String, Signal)> {
    let log = session.store().log();
    let stem = |question| {
        log.iter()
            .find_map(|entry| match entry {
                StoreEntry::Started { record, .. } => record
                    .questions
                    .iter()
                    .find(|asked| asked.id() == question)
                    .map(|asked| asked.stem().to_owned()),
                _ => None,
            })
            .expect("a question the record asks")
    };
    log.iter()
        .filter_map(|entry| match entry {
            StoreEntry::Signal {
                question, signal, ..
            } => Some((stem(question), signal.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn the_help_controls_follow_submit_and_inspect_source_and_tab_reaches_them_from_the_editor() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 80, 24);
    assert_eq!(
        session.row_with("Hint"),
        "Hint    Show answer    Skip    Cannot answer from this context    Report"
    );

    // In the editor, every key is text, so Tab, not a letter, leaves it.
    session.press("i");
    type_text(&mut session, "h s r");
    session.press("<Tab> <Tab> <Tab>");

    assert_eq!(
        session.row_with("Hint"),
        "> Hint    Show answer    Skip    Cannot answer from this context    Report"
    );
    assert!(signals(&session).is_empty(), "nothing is signalled yet");
}

#[test]
fn hint_shows_the_questions_checked_hint_and_records_that_it_was_used() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 80, 24);
    assert!(
        !session.text().contains("The hint, which stays hidden"),
        "{}",
        session.text()
    );

    activate(&mut session, "Hint");

    assert_eq!(
        session.row_with("The hint"),
        "Hint: The hint, which stays hidden"
    );
    assert_eq!(signals(&session), [(FIRST.to_owned(), Signal::HintUsed)]);
    assert!(
        !session.row_with("Question 1").contains("Assisted"),
        "the attempt stays the learner's own:\n{}",
        session.text()
    );

    session.press("<Enter>");
    assert_eq!(signals(&session).len(), 1, "used once, saved once");
}

#[test]
fn a_question_whose_hint_failed_its_check_says_no_hint_for_this_question() {
    let ai = first_without_a_hint();
    let mut session = answering(&ai, 80, 24);

    activate(&mut session, "Hint");

    assert_eq!(session.row_with("No hint"), "No hint for this question");
    assert!(
        !session.text().contains("The hint, which stays hidden"),
        "{}",
        session.text()
    );
    assert!(signals(&session).is_empty(), "no hint was used");
}

#[test]
fn show_answer_reveals_the_answer_key_the_rationales_and_the_rubric_and_makes_the_attempt_assisted()
{
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 120, 32);
    session.press("b");

    activate(&mut session, "Show answer");

    assert_eq!(
        session.row_with("Answer to question"),
        "Answer to question 1 of 4"
    );
    assert_eq!(
        session.section("Answer key"),
        ["A: Option A of the question"]
    );
    for letter in ["A", "B", "C"] {
        assert_eq!(
            session.section(&format!("Rationale for {letter}")),
            [format!("The rationale for {letter}, which stays hidden")]
        );
    }
    assert_eq!(
        session.section("Why it matters"),
        ["Why it matters, which stays hidden"]
    );
    assert_eq!(
        session.section("Rubric, criterion 1 of 1"),
        [
            "The rubric's criterion, which stays hidden",
            "Met: What meeting it looks like",
            "Not met: What not meeting it looks like",
        ]
    );
    assert_eq!(signals(&session), [(FIRST.to_owned(), Signal::ShowAnswer)]);

    session.press("q");
    assert_eq!(
        session.row_with("Question 1"),
        "Question 1 of 4 · Domain · Assisted"
    );
    assert_eq!(
        session.row_with("Option B"),
        "(x) B  Option B of the question",
        "the draft stays"
    );

    session.press("<Enter>");
    assert!(session.text().contains("Answer to question 1 of 4"));
    assert_eq!(signals(&session).len(), 1, "shown once, saved once");
}

#[test]
fn an_assisted_attempt_is_still_submitted_and_assessed_with_both_outcomes_marked_assisted() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 120, 32);
    session.press("a");
    activate(&mut session, "Show answer");
    session.press("q");

    write_and_submit(&mut session, "The key dedupes retries.");
    session.settle();

    assert_eq!(
        session.row_with("Assisted attempt"),
        "Assisted attempt: you chose Show answer before submitting, so its outcomes aren't \
         independent."
    );
    assert_eq!(
        session.section("Choice · Supported · Assisted"),
        ["Jev supports A, the option you chose."]
    );
    assert_eq!(
        session.section("Reasoning"),
        ["Met · Assisted · The rubric's criterion, which stays hidden"]
    );
    let log = session.store().log();
    assert!(
        matches!(
            log.as_slice(),
            [
                StoreEntry::Started { .. },
                StoreEntry::Signal {
                    signal: Signal::ShowAnswer,
                    ..
                },
                StoreEntry::Response { .. },
                StoreEntry::Assessment {
                    assessment: Assessment::Choice(_),
                    ..
                },
                StoreEntry::Assessment {
                    assessment: Assessment::Criterion { .. },
                    ..
                },
            ]
        ),
        "Show answer, then the response and its outcomes, assessed as usual: {log:#?}"
    );
}

#[test]
fn an_attempt_with_a_hint_alone_is_not_marked_assisted() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 120, 32);
    session.press("a");
    activate(&mut session, "Hint");

    write_and_submit(&mut session, "The key dedupes retries.");
    session.settle();

    assert!(!session.text().contains("Assisted"), "{}", session.text());
    assert_eq!(
        session.section("Reasoning"),
        ["Met · The rubric's criterion, which stays hidden"]
    );
}

/// The question that shows: its place, from "Question 1 of 4 · Domain".
fn showing(session: &Session) -> String {
    session
        .row_with("Question ")
        .split(" · ")
        .next()
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn skip_keeps_the_draft_and_the_skipped_question_comes_round_again_after_the_last() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 80, 24);
    session.press("b i");
    type_text(&mut session, "Half a thought");
    session.press("<Esc>");

    activate(&mut session, "Skip");

    assert_eq!(showing(&session), "Question 2 of 4");
    assert_eq!(
        session.row_with("Option B"),
        "( ) B  Option B of the question",
        "the next question has its own draft"
    );
    assert_eq!(reasoning(&session)[0], "Press i to write your reasoning.");
    assert_eq!(signals(&session), [(FIRST.to_owned(), Signal::Skip)]);
    assert!(
        !session.store().log().iter().any(|entry| matches!(
            entry,
            StoreEntry::Response { .. } | StoreEntry::Assessment { .. }
        )),
        "a skip leaves the question unattempted"
    );

    for place in 2..=4 {
        assert_eq!(showing(&session), format!("Question {place} of 4"));
        submit(&mut session, "a", "Because.");
        if place < 4 {
            session.press("<Enter>");
        }
    }
    assert_eq!(
        session.row_with("Next question"),
        "> Next question",
        "the skipped question is still to come round"
    );
    session.press("<Enter>");

    assert_eq!(showing(&session), "Question 1 of 4");
    assert_eq!(
        session.row_with("Option B"),
        "(x) B  Option B of the question"
    );
    assert_eq!(reasoning(&session)[0], "Half a thought");
}

#[test]
fn skip_moves_to_the_next_unattempted_question_past_those_answered() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 120, 32);
    submit(&mut session, "a", "Because.");
    session.press("<Enter>");

    for (from, to) in [(2, 3), (3, 4), (4, 2)] {
        assert_eq!(showing(&session), format!("Question {from} of 4"));
        activate(&mut session, "Skip");
        assert_eq!(showing(&session), format!("Question {to} of 4"));
    }
    assert_eq!(
        signals(&session)
            .into_iter()
            .map(|(stem, _)| stem)
            .collect::<Vec<_>>(),
        [
            "Domain question 2?",
            "Domain question 3?",
            "Domain question 4?"
        ]
    );
}

#[test]
fn the_only_question_cant_be_skipped() {
    let ai = writing_about_the_domain(1, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 120, 32);

    focus(&mut session, "Skip");
    session.press("<Enter>");

    assert_eq!(showing(&session), "Question 1 of 1");
    assert!(
        session
            .row_with("Skip")
            .contains("> Skip (it's the only question)"),
        "{}",
        session.text()
    );
    assert_eq!(signals(&session), []);
}

#[test]
fn cannot_answer_needs_no_response_records_the_signal_and_moves_on() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 80, 24);

    activate(&mut session, "Cannot answer from this context");

    assert_eq!(showing(&session), "Question 2 of 4");
    assert_eq!(
        signals(&session),
        [(FIRST.to_owned(), Signal::CannotAnswer)]
    );
    assert!(
        !session.store().log().iter().any(|entry| matches!(
            entry,
            StoreEntry::Response { .. } | StoreEntry::Assessment { .. }
        )),
        "no response, so no outcome"
    );
}

#[test]
fn cannot_answer_counts_as_neither_incorrect_nor_skipped() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 80, 24);
    session.press("b");
    activate(&mut session, "Cannot answer from this context");

    for place in 2..=4 {
        assert_eq!(showing(&session), format!("Question {place} of 4"));
        submit(&mut session, "a", "Because.");
        if place < 4 {
            session.press("<Enter>");
        }
    }

    assert_eq!(
        session.row_with("Next question"),
        "Next question (this is the last)",
        "it doesn't come round as a skipped question would"
    );
    assert_eq!(session.row_with("Finish"), "> Finish");
    session.settle();
    let outcomes = session
        .store()
        .log()
        .iter()
        .filter(|entry| matches!(entry, StoreEntry::Assessment { .. }))
        .count();
    assert_eq!(
        outcomes,
        3 * 2,
        "a choice and a criterion outcome for each answered"
    );
    session.press("q");
    assert!(
        session.text().contains("Change to practice on"),
        "its draft isn't held, so leaving doesn't ask:\n{}",
        session.text()
    );
}

#[test]
fn a_question_that_cannot_be_answered_says_so_when_it_comes_round() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 120, 32);
    activate(&mut session, "Cannot answer from this context");
    activate(&mut session, "Skip");
    submit(&mut session, "a", "Because.");
    session.press("<Enter>");
    submit(&mut session, "a", "Because.");

    session.press("<Enter>");

    assert_eq!(showing(&session), "Question 1 of 4");
    assert_eq!(
        session.section("Cannot answer from this context").join(" "),
        "You said the evidence isn't enough to answer this question. It needs no response, \
         and counts as neither incorrect nor skipped."
    );
    assert!(!session.text().contains("Submit response"));
    assert_eq!(session.row_with("Next question"), "> Next question");
    session.press("<Enter>");
    assert_eq!(showing(&session), "Question 2 of 4", "the skipped one");
}

#[test]
fn the_last_question_left_that_cannot_be_answered_stays_with_finish() {
    let ai = writing_about_the_domain(1, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 80, 24);

    activate(&mut session, "Cannot answer from this context");

    assert_eq!(showing(&session), "Question 1 of 1");
    assert_eq!(
        session.row_with("Next question"),
        "Next question (this is the last)"
    );
    assert_eq!(session.row_with("Finish"), "> Finish");
    session.press("<Enter>");
    assert!(
        session.text().contains("Practice on the showcase project"),
        "{}",
        session.text()
    );
}

/// The rows of the open report, trimmed.
fn report_rows(session: &Session) -> Vec<String> {
    session.box_rows("Report this question")
}

#[test]
fn report_from_the_question_takes_a_kind_and_a_note_and_changes_nothing_else() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 80, 24);
    session.press("b");

    activate(&mut session, "Report");
    assert_eq!(
        report_rows(&session),
        [
            "What may be wrong with this question?",
            "",
            "> ( ) Hallucination: it states something the evidence doesn't hold",
            "( ) Mistake: it is wrong",
            "( ) Ambiguity: it can be read more than one way",
            "",
            "Note (optional):",
            "",
            "Report (choose what may be wrong)",
            "Cancel",
            "",
            "A report is kept in your session record. It never changes an outcome.",
        ]
    );

    session.press("j j <Enter>");
    session.press("<Tab>");
    // In the note every key is text, `q` and `j` too (rule 29).
    type_text(&mut session, "B and C agree, just quoted");
    assert_eq!(
        report_rows(&session)[4..=6],
        [
            "(x) Ambiguity: it can be read more than one way",
            "",
            "> Note (optional): B and C agree, just quoted",
        ]
    );
    session.press("<Tab>");
    assert_eq!(report_rows(&session)[8], "> Report");
    session.press("<Enter>");

    assert!(!session.text().contains("Report this question"));
    assert_eq!(
        signals(&session),
        [(
            FIRST.to_owned(),
            Signal::Report(Report::new(
                ReportKind::Ambiguity,
                Some("B and C agree, just quoted".to_owned())
            ))
        )]
    );
    assert_eq!(
        session.row_with("Question 1"),
        "Question 1 of 4 · Domain · Reported"
    );
    assert_eq!(
        session.row_with("Option B"),
        "(x) B  Option B of the question",
        "the draft stays"
    );
}

#[test]
fn a_report_needs_a_kind_but_no_note() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 80, 24);
    activate(&mut session, "Report");

    session.press("<Tab> <Tab> <Tab> <Tab> <Enter>");
    assert_eq!(
        report_rows(&session)[8],
        "> Report (choose what may be wrong)"
    );
    assert_eq!(signals(&session), [], "nothing is reported yet");

    session.press("<S-Tab> <S-Tab> <S-Tab> <S-Tab> <Space> <Tab> <Tab> <Tab> <Tab> <Enter>");

    assert_eq!(
        signals(&session),
        [(
            FIRST.to_owned(),
            Signal::Report(Report::new(ReportKind::Hallucination, None))
        )]
    );
}

#[test]
fn esc_or_cancel_closes_the_report_keeping_nothing() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 80, 24);
    activate(&mut session, "Report");
    session.press("<Enter> <Tab> <Tab> <Tab>");
    type_text(&mut session, "Not sure");

    session.press("<Esc>");
    assert!(!session.text().contains("Report this question"));
    assert_eq!(
        showing(&session),
        "Question 1 of 4",
        "still on the question"
    );

    activate(&mut session, "Report");
    assert_eq!(
        report_rows(&session)[6],
        "Note (optional):",
        "a new report starts empty"
    );
    session.press("q");
    assert!(!session.text().contains("Report this question"));

    activate(&mut session, "Report");
    session.press("<Enter>");
    focus(&mut session, "Cancel");
    session.press("<Enter>");
    assert!(!session.text().contains("Report this question"));
    assert_eq!(signals(&session), []);
    assert!(!session.row_with("Question 1").contains("Reported"));
}

#[test]
fn a_paste_into_the_note_goes_in_on_one_line() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 80, 24);
    activate(&mut session, "Report");
    session.press("j <Enter>");
    session.handle(&Event::Paste("one\ntwo".to_owned()));
    assert_eq!(signals(&session), [], "nowhere to paste yet");

    session.press("<Tab> <Tab>");
    session.handle(&Event::Paste("two\r\nlines\tand a tab".to_owned()));
    session.press("<Tab> <Enter>");

    assert_eq!(
        signals(&session),
        [(
            FIRST.to_owned(),
            Signal::Report(Report::new(
                ReportKind::Mistake,
                Some("two lines and a tab".to_owned())
            ))
        )]
    );
}

#[test]
fn report_from_feedback_changes_no_outcome() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 120, 32);
    submit(&mut session, "c", "Because.");
    session.settle();
    let choice = session.section("Choice · Not supported");
    let reasoning = session.section("Reasoning");
    let saved = session.store().log();

    activate(&mut session, "Report");
    session.press("j <Enter> <Tab> <Tab> <Tab> <Enter>");

    assert_eq!(session.section("Choice · Not supported"), choice);
    assert_eq!(session.section("Reasoning"), reasoning);
    let log = session.store().log();
    assert_eq!(log[..saved.len()], saved, "nothing saved is changed");
    assert!(
        matches!(
            &log[saved.len()..],
            [StoreEntry::Signal {
                signal: Signal::Report(report),
                ..
            }] if report.kind() == ReportKind::Mistake && report.note().is_none()
        ),
        "only the report is added: {log:#?}"
    );
    assert_eq!(
        session.row_with("Question 1"),
        "Question 1 of 4 · Domain · Reported"
    );
    assert_eq!(session.row_with("Next question"), "Next question");
}

#[test]
fn report_from_a_question_that_cannot_be_answered() {
    let ai = writing_about_the_domain(1, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 80, 24);
    activate(&mut session, "Cannot answer from this context");

    activate(&mut session, "Report");
    session.press("<Enter> <Tab> <Tab> <Tab> <Tab> <Enter>");

    assert_eq!(
        signals(&session)
            .into_iter()
            .map(|(_, signal)| signal)
            .collect::<Vec<_>>(),
        [
            Signal::CannotAnswer,
            Signal::Report(Report::new(ReportKind::Hallucination, None))
        ]
    );
}

/// A session `width` by `height` writing a report on the first question:
/// ambiguity is chosen, and the note is being typed.
fn reporting(width: u16, height: u16) -> Session {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, width, height);
    session.press("b");
    activate(&mut session, "Report");
    session.press("j j <Enter> <Tab>");
    type_text(&mut session, "B and C say the same");
    session
}

#[test]
fn report_at_120x32() {
    let session = reporting(120, 32);

    insta::assert_debug_snapshot!("report_120x32", session.frame());
}

#[test]
fn report_at_80x24() {
    let session = reporting(80, 24);

    insta::assert_debug_snapshot!("report_80x24", session.frame());
}

/// A session `width` by `height` on the assessed feedback for an assisted
/// attempt at the first question, choosing C, after Show answer, scrolled
/// to its end, where the criterion's outcome is.
fn assisted_feedback(width: u16, height: u16) -> Session {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, width, height);
    session.press("c");
    activate(&mut session, "Show answer");
    session.press("q");
    write_and_submit(
        &mut session,
        "Retries reuse the key, so the provider dedupes them.",
    );
    session.settle();
    session.press("G");
    session
}

#[test]
fn assisted_feedback_at_120x32() {
    let session = assisted_feedback(120, 32);

    insta::assert_debug_snapshot!("assisted_feedback_120x32", session.frame());
}

#[test]
fn assisted_feedback_at_80x24() {
    let session = assisted_feedback(80, 24);

    insta::assert_debug_snapshot!("assisted_feedback_80x24", session.frame());
}

/// Four questions, each passing every check, whose reasoning meets its
/// criterion once submitted, but the first's: Jev's evaluation of it fails,
/// then, asked again, finds it met.
fn first_evaluation_failing() -> Arc<ScriptedAi> {
    let (ai, stems) = writing_four();
    ai.on_check_passing(FIRST);
    ai.on_assess(FIRST, Err(RequestFailure::TimedOut));
    for stem in &stems {
        reasoning_met(&ai, stem);
    }
    ai
}

/// Four questions, each passing every check, whose reasoning meets its
/// criterion once submitted, but the first's, which doesn't: explaining it
/// fails, and fails again when asked again.
fn first_explanation_failing() -> Arc<ScriptedAi> {
    let (ai, stems) = writing_four();
    ai.on_check_passing(FIRST);
    ai.on_assess(FIRST, Ok(vec![Probability::new(0.1).expect("from 0 to 1")]));
    for _ in 0..2 {
        ai.on_explain(FIRST, Err(RequestFailure::RateLimited));
    }
    for stem in &stems[1..] {
        reasoning_met(&ai, stem);
    }
    ai
}

/// How many times the AI was asked to assess the reasoning written for the
/// question `stem`, and to explain it.
fn asked_about(ai: &ScriptedAi, stem: &str) -> (usize, usize) {
    let asked = ai.asked();
    let count = |explain: bool| {
        asked
            .iter()
            .filter(|asked| match asked {
                Asked::AssessReasoning { stem: about, .. } => !explain && about == stem,
                Asked::Explain { stem: about, .. } => explain && about == stem,
                _ => false,
            })
            .count()
    };
    (count(false), count(true))
}

#[test]
fn a_submitted_question_that_comes_round_shows_its_feedback_with_retry_for_a_failed_evaluation() {
    let ai = first_evaluation_failing();
    let mut session = answering(&ai, 120, 32);
    submit(&mut session, "a", "Because.");
    session.settle();
    assert!(
        session
            .text()
            .contains("Reasoning · Evaluation unavailable")
    );
    session.press("<Enter>");
    activate(&mut session, "Skip");
    for _ in 3..=4 {
        submit(&mut session, "a", "Because.");
        session.settle();
        session.press("<Enter>");
    }

    assert_eq!(showing(&session), "Question 1 of 4", "it comes round");
    assert!(
        session
            .text()
            .contains("Reasoning · Evaluation unavailable"),
        "{}",
        session.text()
    );
    activate(&mut session, "Retry evaluation");
    session.settle();

    assert_eq!(
        session.section("Reasoning"),
        ["Met · The rubric's criterion, which stays hidden"]
    );
    assert_eq!(asked_about(&ai, FIRST), (2, 0));
    session.press("<Enter>");
    assert_eq!(showing(&session), "Question 2 of 4", "then the skipped one");
}

#[test]
fn a_submitted_question_that_comes_round_offers_retry_for_a_failed_explanation() {
    let ai = first_explanation_failing();
    let mut session = answering(&ai, 120, 40);
    submit(&mut session, "a", "Because.");
    session.settle();
    session.press("<Enter>");
    activate(&mut session, "Skip");
    for _ in 3..=4 {
        submit(&mut session, "a", "Because.");
        session.settle();
        session.press("<Enter>");
    }

    assert_eq!(showing(&session), "Question 1 of 4", "it comes round");
    activate(&mut session, "Retry explanation");
    session.settle();

    assert_eq!(asked_about(&ai, FIRST), (1, 2));
}

#[test]
fn every_signal_is_saved_to_the_session_record_in_order() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 120, 32);
    activate(&mut session, "Hint");
    activate(&mut session, "Show answer");
    session.press("q");
    activate(&mut session, "Skip");
    activate(&mut session, "Report");
    session.press("<Enter> <Tab> <Tab> <Tab>");
    type_text(&mut session, "Two options agree");
    session.press("<Tab> <Enter>");
    activate(&mut session, "Cannot answer from this context");

    assert_eq!(
        signals(&session),
        [
            (FIRST.to_owned(), Signal::HintUsed),
            (FIRST.to_owned(), Signal::ShowAnswer),
            (FIRST.to_owned(), Signal::Skip),
            (
                "Domain question 2?".to_owned(),
                Signal::Report(Report::new(
                    ReportKind::Hallucination,
                    Some("Two options agree".to_owned())
                ))
            ),
            ("Domain question 2?".to_owned(), Signal::CannotAnswer),
        ]
    );
    assert_eq!(showing(&session), "Question 3 of 4");
}

#[test]
fn key_help_on_the_report_lists_its_own_keys() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 120, 32);
    activate(&mut session, "Report");

    session.press("?");
    let help = session.text();
    assert!(help.contains("Keys on Report"), "{help}");
    assert!(help.contains("Close the report, keeping nothing"), "{help}");

    session.press("<Esc> <Tab> <Tab> <Tab>");
    type_text(&mut session, "?");
    assert!(
        !session.text().contains("Keys on"),
        "in the note, ? is text:\n{}",
        session.text()
    );
    assert!(
        session
            .box_rows("Report this question")
            .contains(&"> Note (optional): ?".to_owned()),
        "{}",
        session.text()
    );
}
