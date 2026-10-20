//! Answering a question: `a`, `b` and `c` choose an option while the
//! options have focus, `i` or Enter starts writing reasoning in the
//! reasoning editor and Esc stops, keeping the draft, and Tab reaches
//! Submit response, where Enter submits. Submit stays disabled, saying
//! what's missing, until both are there. The session record starts once
//! questions pass their checks, and Submit saves the response to it before
//! anything else is done with it.

use std::sync::Arc;

use ratatui::crossterm::event::Event;
use whetstone::{Assessment, Flow, StoreEntry};
use whetstone_ai::{Asked, ScriptedAi};
use whetstone_core::{ChoiceOutcome, OptionLetter};

use crate::preparing_questions::{DOMAIN, PAYMENT_LIFECYCLE, generate, writing_about_the_domain};
use crate::session::Session;

/// A session `width` by `height` showing the first of the questions `ai`
/// writes and checks about the domain area of the payment lifecycle change.
pub(crate) fn answering(ai: &Arc<ScriptedAi>, width: u16, height: u16) -> Session {
    let mut session = generate(ai, &[DOMAIN], PAYMENT_LIFECYCLE, width, height);
    session.settle();
    session
}

/// The stems of the questions each session record in `log` started with.
fn recorded_stems(log: &[StoreEntry]) -> Vec<Vec<String>> {
    log.iter()
        .filter_map(|entry| match entry {
            StoreEntry::Started { record, .. } => Some(
                record
                    .questions
                    .iter()
                    .map(|question| question.stem().to_owned())
                    .collect(),
            ),
            _ => None,
        })
        .collect()
}

#[test]
fn the_session_record_starts_once_questions_pass_with_the_questions_and_the_evidence_sent() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = generate(&ai, &[DOMAIN], PAYMENT_LIFECYCLE, 120, 32);
    session.settle_once();
    session.settle_once();
    assert!(
        session.store().log().is_empty(),
        "nothing is recorded while questions are checked"
    );

    session.settle();

    let log = session.store().log();
    let [StoreEntry::Started { record, .. }] = log.as_slice() else {
        panic!("only the start: {log:#?}");
    };
    assert_eq!(
        recorded_stems(&log),
        [[
            "Domain question 1?",
            "Domain question 2?",
            "Domain question 3?",
            "Domain question 4?"
        ]]
    );
    let asked = ai.asked();
    let Some(Asked::CheckQuestion { evidence, .. }) = asked.last() else {
        panic!("Jev checked the questions: {asked:#?}");
    };
    assert_eq!(&record.evidence, evidence, "the evidence exactly as sent");
}

/// Types `text` a key at a time, as a terminal sends it: a space as Space
/// and a line break as Enter.
pub(crate) fn type_text(session: &mut Session, text: &str) {
    for c in text.chars() {
        let key = match c {
            ' ' => "<Space>".to_owned(),
            '\n' => "<Enter>".to_owned(),
            c => c.to_string(),
        };
        session.press(&key);
    }
}

/// Chooses `choice` (`a`, `b` or `c`), writes `reasoning` and submits it,
/// with Tab from the editor to Submit response and Enter.
pub(crate) fn submit(session: &mut Session, choice: &str, reasoning: &str) {
    session.press(choice);
    session.press("i");
    type_text(session, reasoning);
    session.press("<Tab> <Enter>");
}

/// Pastes `text`, as a terminal with bracketed paste reports it.
fn paste(session: &mut Session, text: &str) {
    session.handle(&Event::Paste(text.to_owned()));
}

/// The rows inside the reasoning editor's box, trimmed.
pub(crate) fn reasoning(session: &Session) -> Vec<String> {
    let rows: Vec<Vec<char>> = session
        .rows()
        .iter()
        .map(|row| row.chars().collect())
        .collect();
    let top = rows.iter().enumerate().find_map(|(y, row)| {
        let text: String = row.iter().collect();
        let x = row.iter().position(|&cell| cell == '┌')?;
        text.contains("Your reasoning").then_some((y, x))
    });
    assert!(top.is_some(), "no reasoning editor:\n{}", session.text());
    let (y, x) = top.unwrap_or_default();
    rows.iter()
        .skip(y + 1)
        .take_while(|row| row.get(x) == Some(&'│'))
        .map(|row| {
            row.iter()
                .skip(x + 1)
                .take_while(|&&cell| cell != '│')
                .collect::<String>()
                .trim()
                .to_owned()
        })
        .collect()
}

/// The reasoning editor's text: its rows, without the empty ones below it.
fn written(session: &Session) -> Vec<String> {
    let mut rows = reasoning(session);
    while rows.last().is_some_and(String::is_empty) {
        rows.pop();
    }
    rows
}

/// The footer's hints, trimmed.
fn footer(session: &Session) -> String {
    let rows = session.rows();
    rows[rows.len() - 2].trim().to_owned()
}

/// The row of option `letter`, trimmed.
fn option(session: &Session, letter: &str) -> String {
    session.row_with(&format!("{letter}  Option {letter} of the question"))
}

#[test]
fn a_b_and_c_choose_an_option_while_the_options_have_focus() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 80, 24);
    assert_eq!(option(&session, "A"), "> ( ) A  Option A of the question");
    assert_eq!(option(&session, "B"), "( ) B  Option B of the question");

    session.press("b");
    assert_eq!(option(&session, "B"), "> (x) B  Option B of the question");
    assert_eq!(option(&session, "A"), "( ) A  Option A of the question");

    session.press("c");
    assert_eq!(option(&session, "C"), "> (x) C  Option C of the question");
    assert_eq!(option(&session, "B"), "( ) B  Option B of the question");

    session.press("k <Space>");
    assert_eq!(option(&session, "B"), "> (x) B  Option B of the question");

    // Off the options, on the reasoning, `a` chooses nothing.
    session.press("j j a");
    assert_eq!(option(&session, "B"), "(x) B  Option B of the question");
    assert_eq!(option(&session, "A"), "( ) A  Option A of the question");
}

#[test]
fn i_starts_editing_where_every_key_is_text_and_esc_stops_keeping_the_draft() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 80, 24);
    assert_eq!(written(&session), ["Press i to write your reasoning."]);

    session.press("i");
    assert_eq!(footer(&session), "Esc stop editing  Tab next");
    // Keys that act outside the editor are text in it (rule 29), and Enter
    // adds a line break (rule 58).
    session.press("q a b c i j k ? : h l <Space> G");
    session.press("<Enter>");
    type_text(&mut session, "Second line");
    assert_eq!(written(&session), ["qabcijk?:hl G", "Second line"]);
    assert!(
        !session.text().contains("Keys on"),
        "no key help:\n{}",
        session.text()
    );

    session.press("<Esc>");
    assert_eq!(written(&session), ["qabcijk?:hl G", "Second line"]);
    assert!(
        footer(&session).contains("q back"),
        "editing stopped: {}",
        footer(&session)
    );
    assert!(
        session
            .row_with("Your reasoning")
            .contains("> Your reasoning"),
        "focus stays on the reasoning:\n{}",
        session.text()
    );

    session.press("<Enter>");
    type_text(&mut session, "!");
    assert_eq!(written(&session), ["qabcijk?:hl G", "Second line!"]);
}

#[test]
fn enter_on_the_options_starts_editing_too() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 80, 24);

    session.press("b <Enter>");
    type_text(&mut session, "Because");

    assert_eq!(written(&session), ["Because"]);
    assert_eq!(option(&session, "B"), "(x) B  Option B of the question");
}

#[test]
fn a_paste_goes_into_the_reasoning_as_one_insertion_keeping_its_line_breaks() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 80, 24);

    // Without the editor focused, a paste does nothing (rule 57).
    paste(&mut session, "b\nq");
    assert_eq!(option(&session, "B"), "( ) B  Option B of the question");
    assert_eq!(written(&session), ["Press i to write your reasoning."]);

    session.press("i");
    paste(&mut session, "one\r\ntwo\rthree\u{7}\nq");

    assert_eq!(written(&session), ["one", "two", "three", "q"]);
    assert_eq!(footer(&session), "Esc stop editing  Tab next");
}

#[test]
fn a_paste_past_the_limit_is_cut_at_4000_characters_with_a_notice() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 120, 32);
    session.press("a i");
    type_text(&mut session, "ab");

    paste(&mut session, &"é".repeat(4_000));

    assert!(
        session
            .text()
            .contains("Paste cut at the 4,000-character limit: 2 characters left out."),
        "{}",
        session.text()
    );
    session.press("<Tab> <Enter>");
    let log = session.store().log();
    let Some(StoreEntry::Response { response, .. }) = log.get(1) else {
        panic!("the response is saved: {log:#?}");
    };
    assert_eq!(
        response.reasoning().chars().count(),
        4_000,
        "counted in graphemes"
    );
}

#[test]
fn submit_stays_disabled_saying_whats_missing_until_a_choice_and_reasoning_are_there() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 80, 24);
    assert_eq!(
        session.row_with("Submit response"),
        "Submit response (choose an option and write your reasoning)"
    );

    session.press("b");
    assert_eq!(
        session.row_with("Submit response"),
        "Submit response (write your reasoning)"
    );

    session.press("i <Space> <Enter> <Space> <Tab>");
    assert_eq!(
        session.row_with("Submit response"),
        "> Submit response (write your reasoning)",
        "whitespace isn't reasoning"
    );
    session.press("<Enter>");
    assert_eq!(session.store().log().len(), 1, "nothing is submitted");

    session.press("i");
    type_text(&mut session, "Retries reuse the key.");
    session.press("<Esc>");
    assert_eq!(session.row_with("Submit response"), "Submit response");
}

#[test]
fn reasoning_without_a_choice_says_an_option_is_missing() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 80, 24);

    session.press("i");
    type_text(&mut session, "Retries reuse the key.");
    session.press("<Tab>");

    assert_eq!(
        session.row_with("Submit response"),
        "> Submit response (choose an option)"
    );
    session.press("<Enter>");
    assert_eq!(session.store().log().len(), 1, "nothing is submitted");
}

#[test]
fn tab_from_the_editor_reaches_submit_and_enter_submits_with_no_dialog() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 80, 24);
    session.press("a i");
    type_text(&mut session, "The key dedupes.");

    session.press("<Tab>");
    assert_eq!(session.row_with("Submit response"), "> Submit response");
    session.press("<Enter>");

    assert!(
        session.text().contains("Choice · Supported"),
        "the choice outcome shows at once:\n{}",
        session.text()
    );
}

#[test]
fn submit_saves_the_response_to_the_session_record_then_its_choice_outcome() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 80, 24);
    session.press("b i");
    type_text(&mut session, "The key dedupes.");
    session.press("<Tab>");
    assert_eq!(session.store().log().len(), 1, "only the start, so far");

    session.press("<Enter>");

    let log = session.store().log();
    let [
        StoreEntry::Started {
            session: id,
            record,
        },
        StoreEntry::Response { saved, response },
        StoreEntry::Assessment {
            saved: assessed,
            assessment: Assessment::Choice(outcome),
        },
    ] = log.as_slice()
    else {
        panic!("the start, the response, then its outcome: {log:#?}");
    };
    assert_eq!(saved.session(), *id);
    assert_eq!(saved.question(), record.questions[0].id());
    assert_eq!(saved.attempt().get(), 1);
    assert_eq!(response.choice(), OptionLetter::B);
    assert_eq!(response.reasoning(), "The key dedupes.");
    assert_eq!(assessed, saved);
    assert_eq!(outcome.outcome(), ChoiceOutcome::NotSupported);
}

#[test]
fn q_asks_before_a_draft_would_be_lost() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 80, 24);
    session.press("b");

    session.press("q");
    assert!(
        session.text().contains("Lose your draft?"),
        "{}",
        session.text()
    );
    assert_eq!(
        session.box_rows("Lose your draft?")[3..],
        ["> Keep answering", "Go back, and lose the draft"]
    );
    session.press("<Enter>");
    assert_eq!(option(&session, "B"), "> (x) B  Option B of the question");

    session.press("<Esc>");
    assert!(session.text().contains("Lose your draft?"));
    session.press("<Esc>");
    assert_eq!(option(&session, "B"), "> (x) B  Option B of the question");

    session.press("q j <Enter>");
    assert!(
        session.text().contains("Change to practice on"),
        "{}",
        session.text()
    );
}

#[test]
fn going_home_or_quitting_asks_before_a_draft_would_be_lost_but_ctrl_c_quits() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 80, 24);
    session.press("i");
    type_text(&mut session, "Draft");
    session.press("<Esc>");

    // The palette's Home, then its Quit, after the screen's own two.
    session.press(": j j <Enter>");
    assert_eq!(
        session
            .box_rows("Lose your draft?")
            .last()
            .map(String::as_str),
        Some("Go home, and lose the draft")
    );
    session.press("<Esc> : j j j j j j <Enter>");
    assert_eq!(
        session
            .box_rows("Lose your draft?")
            .last()
            .map(String::as_str),
        Some("Quit, and lose the draft")
    );
    assert_eq!(session.press("j <Enter>"), Flow::Quit);

    let mut session = answering(&ai_with_a_draft(), 80, 24);
    session.press("b");
    assert_eq!(
        session.press("<C-c>"),
        Flow::Quit,
        "reserved, so it never asks"
    );
}

/// A scripted AI writing and checking the questions again, for a second
/// session.
fn ai_with_a_draft() -> Arc<ScriptedAi> {
    writing_about_the_domain(6, PAYMENT_LIFECYCLE)
}

#[test]
fn inspect_source_reads_the_evidence_as_sent_and_back_returns_to_the_draft() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 120, 32);
    session.press("b i");
    type_text(&mut session, "Draft");
    session.press("<Tab> <Tab>");
    assert_eq!(session.row_with("Inspect source"), "> Inspect source");

    session.press("<Enter>");
    let text = session.text();
    assert!(
        text.contains("Evidence: Model the payment lifecycle"),
        "{text}"
    );
    assert!(text.contains("Message, as it is sent"), "{text}");
    assert!(text.contains(": diff"), "{text}");

    session.press("q");
    assert_eq!(option(&session, "B"), "(x) B  Option B of the question");
    assert_eq!(written(&session), ["Draft"]);
}

#[test]
fn the_question_screen_at_120x32() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 120, 32);
    session.press("b i");
    type_text(
        &mut session,
        "Retries reuse the key, so the provider\ndedupes them.",
    );

    insta::assert_debug_snapshot!("question_120x32", session.frame());
}

#[test]
fn the_question_screen_at_80x24() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 80, 24);
    session.press("b i");
    type_text(
        &mut session,
        "Retries reuse the key, so the provider\ndedupes them.",
    );

    insta::assert_debug_snapshot!("question_80x24", session.frame());
}

#[test]
fn asking_before_a_draft_is_lost_at_120x32() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 120, 32);
    session.press("b q");

    insta::assert_debug_snapshot!("lose_your_draft_120x32", session.frame());
}

#[test]
fn asking_before_a_draft_is_lost_at_80x24() {
    let ai = writing_about_the_domain(6, PAYMENT_LIFECYCLE);
    let mut session = answering(&ai, 80, 24);
    session.press("b q");

    insta::assert_debug_snapshot!("lose_your_draft_80x24", session.frame());
}
