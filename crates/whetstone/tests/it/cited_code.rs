//! The code a question is about, beside it: a header naming the change,
//! then each citation's lines as the evidence sent them, under its path and
//! line range, numbered as in their file, with three lines around them. In
//! a touched file, the lines the change added or removed are marked `+` and
//! `-`; in connected code, the heading says why it is there. At 120 columns
//! it is on the left, and narrower, above the question. Tab gives it focus,
//! and it then scrolls on its own; `z` shows it on the whole screen, and
//! back; and Inspect source opens the whole evidence at the first citation.
//! Its feedback keeps it.

use std::sync::Arc;

use ratatui::style::Color;
use whetstone_ai::ScriptedAi;
use whetstone_core::{WrittenCandidate, WrittenCitation};

use crate::answering::{reasoning, submit};
use crate::help_and_signals::activate;
use crate::preparing_questions::{
    DOMAIN, PAYMENT_LIFECYCLE, area_id, candidate, generate_in, reasoning_met, stems,
};
use crate::send_starts_practice::{REVIEW_ABOUT_THE_DOMAIN, press_send, reviewing};
use crate::session::{NO_COLOR, Session, Setup};

/// The showcase change "Preserve payment retry identity", whose diff
/// changes `ChargeRequest::new` in `src/provider.rs`, and whose connected
/// code includes lines 54–68 of `src/http.rs`, which call `pay`.
const PRESERVE_RETRY_IDENTITY: usize = 4;

/// Line 29 of `src/provider.rs`, the new `ChargeRequest::new`.
const NEW_CHARGE: (&str, usize, usize) = ("src/provider.rs", 29, 29);

/// Lines 60–61 of `src/http.rs`, in connected code that calls `pay`.
const HTTP_PAY: (&str, usize, usize) = ("src/http.rs", 60, 61);

/// The code around [`NEW_CHARGE`], as the pane shows it on the whole
/// screen: lines 26–32, with the line the change removed before each line
/// it added in its place.
const NEW_CHARGE_CODE: [&str; 11] = [
    "src/provider.rs, line 29",
    "26   impl ChargeRequest {",
    "   -     pub fn new(payment: &Payment, operation_key: OperationKey) -> Self {",
    "27 +     /// Builds the charge for a payment. It always carries the operation key",
    "28 +     /// the payment was created with, so every retry is the same charge.",
    "29 +     pub fn new(payment: &Payment) -> Self {",
    "30           Self {",
    "   -             operation_key,",
    "31 +             operation_key: payment.operation_key().clone(),",
    "32               reference: payment.id().to_string(),",
    "",
];

/// A scripted AI whose generator writes four questions about the domain
/// area of the showcase change at `change`, each citing `citations`, and
/// each passing every check.
fn citing(change: usize, citations: &[(&str, usize, usize)]) -> Arc<ScriptedAi> {
    let ai = Arc::new(ScriptedAi::new());
    let stems = stems("Domain", 4);
    ai.on_write(
        &area_id(DOMAIN),
        Ok(stems
            .iter()
            .map(|stem| WrittenCandidate {
                citations: citations
                    .iter()
                    .map(|&(path, first_line, last_line)| WrittenCitation {
                        path: path.to_owned(),
                        first_line,
                        last_line,
                    })
                    .collect(),
                ..candidate(stem, "A", change)
            })
            .collect()),
    );
    for stem in &stems {
        ai.on_check_passing(stem);
        reasoning_met(&ai, stem);
    }
    ai
}

/// A session `width` by `height`, without colour, on the first of the
/// questions `ai` writes about the showcase change at `change`.
fn questions(ai: &Arc<ScriptedAi>, change: usize, width: u16, height: u16) -> Session {
    questions_in(
        Setup {
            variables: NO_COLOR,
            ai: Some(ai),
            ..Setup::default()
        },
        change,
        width,
        height,
    )
}

/// The same as [`questions`], in a session started as `setup` says.
fn questions_in(setup: Setup<'_>, change: usize, width: u16, height: u16) -> Session {
    let mut session = generate_in(setup, &[DOMAIN], change, width, height);
    session.settle();
    assert!(
        session.text().contains("Question 1 of 4"),
        "{}",
        session.text()
    );
    session
}

/// A session `width` by `height` on the first question about "Preserve
/// payment retry identity", which cites [`NEW_CHARGE`], then [`HTTP_PAY`].
fn citing_both(width: u16, height: u16) -> Session {
    let ai = citing(PRESERVE_RETRY_IDENTITY, &[NEW_CHARGE, HTTP_PAY]);
    questions(&ai, PRESERVE_RETRY_IDENTITY, width, height)
}

/// Where the question's place ("Question 1 of 4") starts: its column and
/// its row.
fn question_at(session: &Session) -> (usize, usize) {
    let rows = session.rows();
    let found = rows.iter().enumerate().find_map(|(y, row)| {
        let x = row.find("Question ")?;
        Some((row.char_indices().take_while(|&(at, _)| at < x).count(), y))
    });
    assert!(found.is_some(), "no question shows:\n{}", session.text());
    found.unwrap_or_default()
}

/// The code pane's rows, without the screen's margin, trimmed at the end:
/// left of the question at 120 columns, and above it narrower, without the
/// blank row between them.
fn code(session: &Session) -> Vec<String> {
    let (x, y) = question_at(session);
    let rows = session.rows();
    let pane: Vec<String> = if x > 2 {
        rows.iter()
            .skip(1)
            .take(rows.len() - 3)
            .map(|row| row.chars().skip(2).take(x - 4).collect())
            .collect()
    } else {
        rows.iter()
            .skip(1)
            .take(y - 2)
            .map(|row| row.chars().skip(2).collect())
            .collect()
    };
    pane.into_iter()
        .map(|row: String| row.trim_end().to_owned())
        .collect()
}

/// The screen's rows inside its margins, trimmed at the end, as the code
/// shows on the whole screen: the header, then the code.
fn whole_screen(session: &Session) -> Vec<String> {
    let rows = session.rows();
    rows.iter()
        .skip(1)
        .take(rows.len() - 3)
        .map(|row| {
            row.chars()
                .skip(2)
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect()
}

#[test]
fn the_code_beside_a_showcase_question_is_headed_by_the_changes_title() {
    let session = citing_both(120, 32);

    assert_eq!(code(&session)[0], "  Preserve payment retry identity");
}

#[test]
fn the_code_beside_a_question_on_a_commit_is_headed_by_its_short_id_and_summary() {
    let repo = crate::connected_code::rust_payments();
    repo.write(
        "src/pay.rs",
        crate::connected_code::rust_pay("validate(amount * 2)"),
    );
    repo.commit("Double the charge");
    let head = repo.head();
    let ai = citing(PAYMENT_LIFECYCLE, &[("src/pay.rs", 1, 1)]);
    let mut session = reviewing(&repo, &ai, REVIEW_ABOUT_THE_DOMAIN, 120, 32);
    press_send(&mut session);
    session.settle();

    let header = &code(&session)[0];
    assert!(
        header.starts_with(&format!("  {}", head.chars().take(7).collect::<String>()))
            && header.ends_with("  Double the charge"),
        "{header:?}"
    );
}

#[test]
fn cited_lines_show_with_their_numbers_the_lines_around_them_and_the_diffs_marks() {
    let mut session = citing_both(120, 32);

    session.press("z");

    assert_eq!(
        whole_screen(&session)[1..=NEW_CHARGE_CODE.len()],
        NEW_CHARGE_CODE,
        "lines 26–32, the removed lines unnumbered before the lines that took their place"
    );
}

#[test]
fn added_and_removed_lines_take_the_themes_tokens() {
    let ai = citing(PRESERVE_RETRY_IDENTITY, &[NEW_CHARGE]);
    let mut session = questions_in(
        Setup {
            variables: &[("COLORTERM", "truecolor")],
            ai: Some(&ai),
            ..Setup::default()
        },
        PRESERVE_RETRY_IDENTITY,
        120,
        32,
    );
    session.press("z");

    let frame = session.frame();
    let rows = session.rows();
    let colour = |text: &str| {
        let y = rows.iter().position(|row| row.contains(text));
        assert!(y.is_some(), "no row shows {text:?}:\n{}", session.text());
        let y = y.unwrap_or_default();
        let x = rows[y].chars().position(|c| c == '+' || c == '-');
        frame[(
            u16::try_from(x.unwrap_or_default()).unwrap_or_default(),
            u16::try_from(y).unwrap_or_default(),
        )]
            .fg
    };
    assert_eq!(
        colour("pub fn new(payment: &Payment) -> Self {"),
        Color::Rgb(0x8B, 0xD2, 0xB5),
        "an added line takes `supported`"
    );
    assert_eq!(
        colour("pub fn new(payment: &Payment, operation_key"),
        Color::Rgb(0xFF, 0xA6, 0xAD),
        "a removed line takes `error`"
    );
}

#[test]
fn a_citation_into_connected_code_shows_its_relation_and_only_the_lines_sent() {
    let ai = citing(PRESERVE_RETRY_IDENTITY, &[HTTP_PAY]);
    let mut session = questions(&ai, PRESERVE_RETRY_IDENTITY, 120, 32);

    session.press("z");

    let shown = whole_screen(&session);
    assert_eq!(shown[1], "src/http.rs, lines 60–61 · calls `pay`");
    let numbers: Vec<&str> = shown[2..10]
        .iter()
        .map(|row| row.split(' ').next().unwrap_or_default())
        .collect();
    assert_eq!(
        numbers,
        ["57", "58", "59", "60", "61", "62", "63", "64"],
        "three lines either side, numbered as in the file, and no marks"
    );
    assert!(
        shown[2..10]
            .iter()
            .all(|row| row.chars().nth(3) != Some('+') && row.chars().nth(3) != Some('-')),
        "{shown:#?}"
    );
}

#[test]
fn tab_gives_the_code_focus_and_it_scrolls_on_its_own() {
    let mut session = citing_both(80, 24);
    let before = code(&session);
    assert_eq!(before[0], "  Preserve payment retry identity");
    assert_eq!(before[1], NEW_CHARGE_CODE[0]);

    // Shift+Tab from option A, where focus starts.
    session.press("<S-Tab>");
    assert_eq!(code(&session)[0], "> Preserve payment retry identity");
    assert!(
        session.question().row_with("( ) A").starts_with("( ) A"),
        "option A no longer has focus"
    );

    // The code's first row, a `j` at a time, until it scrolls no further.
    let mut tops = vec![code(&session)[1].clone()];
    loop {
        let pane = code(&session);
        session.press("j");
        if code(&session) == pane {
            break;
        }
        tops.push(code(&session)[1].clone());
    }
    assert_eq!(tops[1], "26   impl ChargeRequest {", "a row at a time");
    assert!(
        tops.contains(&"src/http.rs, lines 60–61 · calls `pay`".to_owned()),
        "down to the second citation: {tops:#?}"
    );
    let at_the_bottom = code(&session);
    let page = at_the_bottom.len() - 1;

    session.press("g g");
    assert_eq!(code(&session)[1], tops[0]);
    session.press("<PageDown>");
    assert_eq!(code(&session)[1], tops[page], "a page of {page} rows");
    session.press("<PageUp>");
    assert_eq!(code(&session)[1], tops[0]);
    session.press("G");
    assert_eq!(code(&session), at_the_bottom);
    session.press("g g");

    // Tab goes on to the first control.
    session.press("<Tab>");
    assert_eq!(code(&session)[0], "  Preserve payment retry identity");
    assert_eq!(
        session.question().row_with("( ) A"),
        "> ( ) A  Option A of the question"
    );
}

#[test]
fn tab_from_the_last_control_gives_the_code_focus() {
    let mut session = citing_both(120, 32);

    session.press(&"<Tab> ".repeat(10));
    assert!(
        session.question().row_with("Report").ends_with("> Report"),
        "{}",
        session.text()
    );
    session.press("<Tab>");

    assert_eq!(code(&session)[0], "> Preserve payment retry identity");
}

#[test]
fn z_shows_the_code_on_the_whole_screen_and_the_question_again() {
    let mut session = citing_both(120, 32);

    for back in ["z", "q", "<Esc>"] {
        session.press("z");
        assert!(
            !session.text().contains("Domain question 1?"),
            "{}",
            session.text()
        );
        assert_eq!(
            whole_screen(&session)[0],
            "> Preserve payment retry identity"
        );
        assert_eq!(whole_screen(&session)[1..12], NEW_CHARGE_CODE);

        session.press(back);
        assert_eq!(
            session.question().row_with("Domain question 1?"),
            "Domain question 1?",
            "{back} shows the question again"
        );
    }
}

#[test]
fn z_works_at_any_width_and_on_feedback() {
    let mut session = citing_both(80, 24);
    session.press("z");
    // Long lines wrap at 80 columns.
    assert_eq!(whole_screen(&session)[1..3], NEW_CHARGE_CODE[..2]);
    session.press("z");

    submit(&mut session, "a", "Because.");
    session.settle();
    assert!(session.text().contains("Choice · Supported"));
    session.press("z");
    assert!(!session.text().contains("Choice · Supported"));
    assert_eq!(whole_screen(&session)[1], NEW_CHARGE_CODE[0]);
}

#[test]
fn inspect_source_opens_the_evidence_at_the_first_citation() {
    let mut session = citing_both(120, 32);

    activate(&mut session, "Inspect source");

    let text = session.text();
    assert!(
        text.contains("Evidence: Preserve payment retry identity"),
        "{text}"
    );
    // Three lines lead up to it.
    let rows = session.rows();
    let first = rows.iter().find(|row| row.trim().starts_with("26 "));
    assert_eq!(
        first.map(|row| row.trim()),
        Some("26 impl ChargeRequest {"),
        "{text}"
    );
    assert!(
        text.contains("29     pub fn new(payment: &Payment) -> Self {"),
        "{text}"
    );
    assert!(
        !text.contains("Message, as it is sent"),
        "not at the top: {text}"
    );
}

#[test]
fn at_120_columns_the_code_is_on_the_left_of_the_question() {
    let session = citing_both(120, 32);

    let (x, y) = question_at(&session);
    assert_eq!(y, 1, "the question's place heads the right");
    assert!((60..=72).contains(&x), "about 55% of the width: {x}");
    assert_eq!(code(&session)[1..3], NEW_CHARGE_CODE[..2]);
}

#[test]
fn narrower_the_code_is_above_the_question_in_at_most_two_fifths_of_the_height() {
    let session = citing_both(80, 24);

    let (x, y) = question_at(&session);
    assert_eq!(x, 2, "the question has the whole width");
    let pane = code(&session);
    assert_eq!(pane[0], "  Preserve payment retry identity");
    assert_eq!(pane[1], NEW_CHARGE_CODE[0]);
    assert!(y <= 1 + 24 * 2 / 5 + 1, "{}", session.text());
}

#[test]
fn the_reasoning_editor_starts_at_eight_rows_and_grows_while_it_has_focus() {
    let mut session = citing_both(120, 32);
    assert_eq!(reasoning(&session).len(), 6, "eight rows with its border");

    session.press("i");

    assert!(
        reasoning(&session).len() > 6,
        "it takes the rows left: {}",
        session.text()
    );
    session.press("<Esc> j j j j");
    assert_eq!(reasoning(&session).len(), 6, "back once focus moves on");
}

#[test]
fn feedback_keeps_the_code_beside_it() {
    let mut session = citing_both(120, 32);

    submit(&mut session, "a", "Because.");
    session.settle();

    assert_eq!(session.question().row_with("Choice"), "Choice · Supported");
    assert_eq!(code(&session)[0], "  Preserve payment retry identity");
    assert_eq!(code(&session)[1..3], NEW_CHARGE_CODE[..2]);
}

#[test]
fn tab_gives_the_code_beside_feedback_focus_and_it_scrolls() {
    let mut session = citing_both(80, 24);
    submit(&mut session, "a", "Because.");
    session.settle();

    // Round from Next question, where focus starts, past Finish.
    session.press(&"<Tab> ".repeat(4));
    assert_eq!(session.question().row_with("Finish"), "> Finish");
    session.press("<Tab>");
    assert_eq!(code(&session)[0], "> Preserve payment retry identity");
    session.press("j");
    assert_eq!(code(&session)[1], "26   impl ChargeRequest {");

    // Shift+Tab goes back to the last control.
    session.press("<S-Tab>");
    assert_eq!(session.question().row_with("Finish"), "> Finish");
}

#[test]
fn the_question_screen_with_its_code_at_120x32() {
    let session = citing_both(120, 32);

    insta::assert_debug_snapshot!("question_with_its_code_120x32", session.frame());
}

#[test]
fn the_question_screen_with_its_code_at_80x24() {
    let session = citing_both(80, 24);

    insta::assert_debug_snapshot!("question_with_its_code_80x24", session.frame());
}

#[test]
fn the_feedback_screen_with_its_code_at_120x32() {
    let mut session = citing_both(120, 32);
    submit(&mut session, "a", "Because.");
    session.settle();

    insta::assert_debug_snapshot!("feedback_with_its_code_120x32", session.frame());
}

#[test]
fn the_feedback_screen_with_its_code_at_80x24() {
    let mut session = citing_both(80, 24);
    submit(&mut session, "a", "Because.");
    session.settle();

    insta::assert_debug_snapshot!("feedback_with_its_code_80x24", session.frame());
}

#[test]
fn the_code_on_the_whole_screen_at_80x24() {
    let mut session = citing_both(80, 24);
    session.press("z");

    insta::assert_debug_snapshot!("code_on_the_whole_screen_80x24", session.frame());
}
