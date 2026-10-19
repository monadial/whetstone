//! The change viewer: Enter on a change in the picker opens it, showing the
//! diff and the code before and after the change for each file it touches.

use whetstone::Flow;
use whetstone_core::{Change, ShowcaseProject, TouchedFile};

use crate::session::{NO_COLOR, Session, Setup};

/// "Preserve payment retry identity", the fifth curated change. It modifies
/// `src/provider.rs`, `src/service.rs` and `tests/api.rs`.
const RETRY_IDENTITY: usize = 4;

/// "Persist payments in an append-only file store", the second curated
/// change. Its last file, `src/store.rs`, is one it creates.
const FILE_STORE: usize = 1;

/// "Serve payments over HTTP", the fourth curated change. It touches twelve
/// files.
const HTTP: usize = 3;

/// The keys from home that open the curated change at `index`: open the
/// picker, move past its five areas onto that change, then press Enter.
fn open(index: usize) -> String {
    let mut keys = "<Enter>".to_owned();
    for _ in 0..5 + index {
        keys.push_str(" j");
    }
    keys.push_str(" <Enter>");
    keys
}

/// Whether some row of `session` shows `needle`.
fn shows(session: &Session, needle: &str) -> bool {
    session.rows().iter().any(|row| row.contains(needle))
}

/// The rows of the code pane, the second of the two side by side, trimmed.
fn code(session: &Session) -> Vec<String> {
    session
        .rows()
        .iter()
        .filter_map(|row| {
            let columns: Vec<&str> = row.split('│').collect();
            (columns.len() == 5).then(|| columns[3].trim().to_owned())
        })
        .collect()
}

/// The keys `key` pressed `times` times.
fn times(key: &str, times: usize) -> String {
    vec![key; times].join(" ")
}

#[test]
fn enter_on_a_change_opens_it_on_the_diff_of_the_first_file_it_touches() {
    let mut session = Session::plain(120, 32);

    session.press(&open(RETRY_IDENTITY));

    assert!(
        session.rows()[1]
            .trim()
            .starts_with("Preserve payment retry identity"),
        "{}",
        session.text()
    );
    for file in ["src/provider.rs", "src/service.rs", "tests/api.rs"] {
        assert!(shows(&session, file), "{file}:\n{}", session.text());
    }
    for line in [
        "diff --git a/src/provider.rs b/src/provider.rs",
        "@@ -24,9 +24,11 @@ pub struct ChargeRequest {",
        "-    pub fn new(payment: &Payment, operation_key: OperationKey) -> Self {",
    ] {
        assert!(shows(&session, line), "{line}:\n{}", session.text());
    }
}

#[test]
fn q_and_esc_return_to_the_picker_with_its_areas_the_chosen_change_and_focus_intact() {
    for back in ["q", "<Esc>"] {
        let mut session = Session::plain(80, 24);
        // Deselect infrastructure, then open the change from there.
        session.press("<Enter> j <Space> j j j j j j j j <Enter>");
        assert!(
            session.rows()[1]
                .trim()
                .starts_with("Preserve payment retry identity"),
            "{}",
            session.text()
        );

        assert_eq!(session.press(back), Flow::Continue);

        let text = session.text();
        assert!(
            text.contains("  ( ) Infrastructure"),
            "after {back}:\n{text}"
        );
        assert!(text.contains("  (x) Domain"), "after {back}:\n{text}");
        assert!(
            text.contains("> (x) Preserve payment retry identity"),
            "after {back}:\n{text}"
        );
    }
}

#[test]
fn h_and_l_switch_between_the_files_diff_before_and_after_panes_stopping_at_either_end() {
    let mut session = Session::plain(120, 32);
    session.press(&open(RETRY_IDENTITY));
    assert!(shows(&session, "> (x) Diff"), "{}", session.text());

    session.press("l");
    assert!(shows(&session, "> (x) Before"), "{}", session.text());
    assert!(
        shows(
            &session,
            "1 //! Charges payments through the external payment provider."
        ),
        "the code before the change, numbered:\n{}",
        session.text()
    );

    session.press("l l");
    assert!(
        shows(&session, "> (x) After"),
        "l stops at After:\n{}",
        session.text()
    );

    session.press("h h h h");
    assert!(
        shows(&session, "> (x) src/provider.rs"),
        "h stops at the Files pane, its chosen file focused:\n{}",
        session.text()
    );
    assert!(
        shows(&session, "  (x) Diff"),
        "the code pane still shows the diff:\n{}",
        session.text()
    );
}

#[test]
fn the_arrows_tab_and_shift_tab_switch_panes_too_and_tab_wraps_around() {
    let mut session = Session::plain(120, 32);
    session.press(&open(RETRY_IDENTITY));

    session.press("<Right>");
    assert!(shows(&session, "> (x) Before"), "{}", session.text());
    session.press("<Left> <Left>");
    assert!(
        shows(&session, "> (x) src/provider.rs"),
        "{}",
        session.text()
    );

    session.press("<S-Tab>");
    assert!(shows(&session, "> (x) After"), "{}", session.text());
    session.press("<Tab>");
    assert!(
        shows(&session, "> (x) src/provider.rs"),
        "{}",
        session.text()
    );
    session.press("<Tab>");
    assert!(shows(&session, "> (x) Diff"), "{}", session.text());
}

#[test]
fn in_the_files_pane_j_and_k_choose_the_file_the_code_pane_shows() {
    let mut session = Session::plain(120, 32);
    session.press(&open(RETRY_IDENTITY));

    session.press("h j");
    assert!(
        shows(&session, "> (x) src/service.rs"),
        "{}",
        session.text()
    );
    assert!(
        shows(&session, "diff --git a/src/service.rs b/src/service.rs"),
        "{}",
        session.text()
    );
    assert!(
        !shows(&session, "diff --git a/src/provider.rs"),
        "only the chosen file's diff:\n{}",
        session.text()
    );

    session.press("<Down> j j");
    assert!(
        shows(&session, "> (x) tests/api.rs"),
        "j stops at the last file:\n{}",
        session.text()
    );
    session.press("k <Up> <Up>");
    assert!(
        shows(&session, "> (x) src/provider.rs"),
        "k stops at the first file:\n{}",
        session.text()
    );
}

#[test]
fn a_file_the_change_creates_has_no_code_before_and_the_pane_says_so() {
    let mut session = Session::plain(120, 32);
    session.press(&open(FILE_STORE));

    session.press("h j j j j l l");

    assert!(shows(&session, "> (x) Before"), "{}", session.text());
    assert!(
        shows(
            &session,
            "This change creates src/store.rs, so it has no code before."
        ),
        "{}",
        session.text()
    );
    session.press("l");
    assert!(
        shows(
            &session,
            "1 //! Stores payments in an append-only log file. Every save appends the"
        ),
        "{}",
        session.text()
    );
}

#[test]
fn j_and_k_scroll_the_focused_code_pane_a_line_at_a_time() {
    let mut session = Session::plain(80, 24);
    session.press(&open(RETRY_IDENTITY));
    session.press("l l");
    assert!(
        code(&session)[0].starts_with("1 //! Charges"),
        "{}",
        session.text()
    );

    session.press("j <Down> j");
    assert!(
        code(&session)[0].starts_with("4 //! exponential"),
        "{}",
        session.text()
    );

    session.press("k <Up>");
    assert!(code(&session)[0].starts_with("2 //!"), "{}", session.text());
}

#[test]
fn scrolling_stops_with_the_last_line_at_the_bottom_and_k_moves_straight_back() {
    let mut session = Session::plain(80, 24);
    session.press(&open(RETRY_IDENTITY));
    session.press("l l");

    session.press(&times("j", 260));
    let end = code(&session);
    assert_eq!(
        end.last().map(String::as_str),
        Some("250 }"),
        "{}",
        session.text()
    );

    session.press("k");
    let rows = code(&session);
    assert_ne!(rows, end, "one k scrolls:\n{}", session.text());
    assert!(!rows.contains(&"250 }".to_owned()), "{}", session.text());
}

#[test]
fn each_pane_keeps_its_own_scroll_position_for_each_file() {
    let mut session = Session::plain(80, 24);
    session.press(&open(RETRY_IDENTITY));

    // The diff down two lines, the code after the change down six.
    session.press("j j l l j j j j j j");
    assert_eq!(
        code(&session)[0],
        "7 use std::future::Future;",
        "{}",
        session.text()
    );
    session.press("h h");
    assert!(
        code(&session)[0].starts_with("--- a/src/provider.rs"),
        "the diff where it was left:\n{}",
        session.text()
    );
    session.press("l");
    assert!(
        code(&session)[0].starts_with("1 "),
        "the code before, never scrolled:\n{}",
        session.text()
    );

    // Another file, scrolled, then back.
    session.press("h h j l j j j h k l l l");
    assert_eq!(
        code(&session)[0],
        "7 use std::future::Future;",
        "the first file's code after, where it was left:\n{}",
        session.text()
    );
}

#[test]
fn gg_and_capital_g_go_to_the_top_and_bottom_of_the_focused_pane() {
    let mut session = Session::plain(80, 24);
    session.press(&open(RETRY_IDENTITY));
    session.press("l l");

    session.press("G");
    assert_eq!(
        code(&session).last().map(String::as_str),
        Some("250 }"),
        "{}",
        session.text()
    );
    session.press("g g");
    assert!(
        code(&session)[0].starts_with("1 //! Charges"),
        "{}",
        session.text()
    );

    // In the Files pane they choose the last and the first file.
    session.press("h h h G");
    assert!(shows(&session, "> (x) tests/api.rs"), "{}", session.text());
    session.press("g g");
    assert!(
        shows(&session, "> (x) src/provider.rs"),
        "{}",
        session.text()
    );
}

#[test]
fn a_g_that_another_key_follows_is_dropped_and_the_other_key_acts_alone() {
    let mut session = Session::plain(80, 24);
    session.press(&open(RETRY_IDENTITY));
    session.press("l l j j j j j j");

    session.press("g j");
    assert_eq!(
        code(&session)[0],
        "8 use std::num::NonZeroU32;",
        "{}",
        session.text()
    );

    session.press("g k g g");
    assert!(
        code(&session)[0].starts_with("1 //! Charges"),
        "{}",
        session.text()
    );
}

#[test]
fn ctrl_d_and_ctrl_u_scroll_half_the_panes_height() {
    // At 80x24 the code pane shows 16 lines, so half a page is 8.
    let mut session = Session::plain(80, 24);
    session.press(&open(RETRY_IDENTITY));
    session.press("l l");

    session.press("<C-d>");
    assert_eq!(
        code(&session)[0],
        "9 use std::time::Duration;",
        "{}",
        session.text()
    );
    session.press("<C-d> <C-u>");
    assert_eq!(
        code(&session)[0],
        "9 use std::time::Duration;",
        "{}",
        session.text()
    );
    session.press("<C-u> <C-u>");
    assert!(
        code(&session)[0].starts_with("1 //! Charges"),
        "{}",
        session.text()
    );
    session.press("G <C-d>");
    assert_eq!(
        code(&session).last().map(String::as_str),
        Some("250 }"),
        "{}",
        session.text()
    );

    // At 120x32 it shows 24, so half a page is 12.
    let mut session = Session::plain(120, 32);
    session.press(&open(RETRY_IDENTITY));
    session.press("l l <C-d>");
    assert_eq!(
        code(&session)[..2],
        [
            "13",
            "14 use crate::payment::{ChargeOutcome, OperationKey, Payment};"
        ],
        "{}",
        session.text()
    );
}

#[test]
fn in_the_files_pane_ctrl_d_and_ctrl_u_move_half_a_page_of_files() {
    let mut session = Session::plain(80, 24);
    session.press(&open(HTTP));

    session.press("h <C-d>");
    assert!(
        shows(&session, "> (x) src/payment.rs"),
        "{}",
        session.text()
    );
    session.press("<C-u>");
    assert!(shows(&session, "> (x) .gitignore"), "{}", session.text());
}

/// The row under the panes, where the search box and what a search found
/// show, trimmed.
fn status(session: &Session) -> String {
    let rows = session.rows();
    rows[rows.len() - 3].trim().to_owned()
}

/// The footer's hints, trimmed.
fn footer(session: &Session) -> String {
    let rows = session.rows();
    rows[rows.len() - 2].trim().to_owned()
}

#[test]
fn slash_opens_a_search_box_where_every_key_but_ctrl_c_is_text() {
    let mut session = Session::plain(80, 24);
    session.press(&open(RETRY_IDENTITY));
    assert_eq!(status(&session), "", "{}", session.text());

    session.press("/");
    assert_eq!(status(&session), "/", "{}", session.text());
    assert_eq!(footer(&session), "Enter search  Esc cancel");

    assert_eq!(session.press("q ? : j G <Space> h"), Flow::Continue);
    assert_eq!(status(&session), "/q?:jG h", "{}", session.text());
    assert!(
        session.rows()[1]
            .trim()
            .starts_with("Preserve payment retry identity"),
        "still on the viewer, with no overlay:\n{}",
        session.text()
    );
    assert!(shows(&session, "> (x) Diff"), "{}", session.text());

    session.press("<Esc>");
    assert_eq!(status(&session), "", "{}", session.text());
    assert_eq!(
        footer(&session),
        "? keys  : actions  h/l panes  / search  q back"
    );
    assert!(
        shows(&session, "> (x) Diff"),
        "focus is where it was:\n{}",
        session.text()
    );

    session.press("/ x");
    assert_eq!(session.press("<C-c>"), Flow::Quit);
}

/// The keys that type `text` into the search box.
fn typing(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c == ' ' {
                "<Space>".to_owned()
            } else {
                c.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn enter_searches_the_focused_pane_from_its_top_and_says_which_match_shows() {
    let mut session = Session::plain(80, 24);
    session.press(&open(RETRY_IDENTITY));
    // Past the diff's first match, `operation_key` on its ninth line.
    session.press("j j j j j j j j j");

    session.press(&format!("/ {} <Enter>", typing("operation_key")));

    // Four lines of the diff hold `operation_key`: its 9th, 14th, 15th and
    // 24th. The first from the top shown is the 14th, already in view.
    assert_eq!(status(&session), "Match 2 of 4 for \"operation_key\"");
    let rows = code(&session);
    assert!(
        rows[0].starts_with("+    /// Builds the charge"),
        "nothing scrolls:\n{}",
        session.text()
    );
    assert!(
        rows.contains(&"-            operation_key,".to_owned()),
        "{}",
        session.text()
    );
    assert_eq!(
        footer(&session),
        "? keys  : actions  h/l panes  / search  q back"
    );
}

/// Whether a row of the code pane starts with `start`.
fn code_shows(session: &Session, start: &str) -> bool {
    code(session).iter().any(|row| row.starts_with(start))
}

#[test]
fn n_and_capital_n_move_to_the_next_and_previous_match_wrapping_around() {
    let mut session = Session::plain(80, 24);
    session.press(&open(RETRY_IDENTITY));
    session.press(&format!("/ {} <Enter>", typing("operation_key")));
    assert_eq!(status(&session), "Match 1 of 4 for \"operation_key\"");

    session.press("n n");
    assert_eq!(status(&session), "Match 3 of 4 for \"operation_key\"");
    assert!(
        code_shows(&session, "+            operation_key:"),
        "{}",
        session.text()
    );

    // The 24th line was out of view: the diff scrolls to it.
    session.press("n");
    assert_eq!(status(&session), "Match 4 of 4 for \"operation_key\"");
    assert!(
        code_shows(&session, "-        ChargeRequest::new(&payment,"),
        "{}",
        session.text()
    );

    // Past the last match, back to the first.
    session.press("n");
    assert_eq!(status(&session), "Match 1 of 4 for \"operation_key\"");
    assert!(
        code_shows(&session, "-    pub fn new(payment: &Payment,"),
        "{}",
        session.text()
    );

    session.press("N");
    assert_eq!(status(&session), "Match 4 of 4 for \"operation_key\"");
    session.press("N");
    assert_eq!(status(&session), "Match 3 of 4 for \"operation_key\"");
}

#[test]
fn when_nothing_matches_the_viewer_says_so_and_nothing_moves() {
    let mut session = Session::plain(80, 24);
    session.press(&open(RETRY_IDENTITY));
    session.press("j j");
    let rows = code(&session);

    session.press(&format!("/ {} <Enter>", typing("refund")));

    assert_eq!(status(&session), "No match for \"refund\" in Diff");
    session.press("n N");
    assert_eq!(code(&session), rows, "{}", session.text());
    assert_eq!(status(&session), "No match for \"refund\" in Diff");
}

#[test]
fn a_search_in_the_files_pane_chooses_the_files_whose_paths_match() {
    let mut session = Session::plain(80, 24);
    session.press(&open(HTTP));

    session.press(&format!("h / {} <Enter>", typing("src/")));
    // Seven of the twelve files are under src/, the first src/config.rs.
    assert!(shows(&session, "> (x) src/config.rs"), "{}", session.text());
    assert_eq!(status(&session), "Match 1 of 7 for \"src/\"");

    session.press("n");
    assert!(shows(&session, "> (x) src/http.rs"), "{}", session.text());
    session.press("N N");
    assert!(shows(&session, "> (x) src/store.rs"), "{}", session.text());
    assert_eq!(status(&session), "Match 7 of 7 for \"src/\"");

    session.press(&format!("/ {} <Enter>", typing("docs/")));
    assert_eq!(status(&session), "No match for \"docs/\" in Files");
    assert!(shows(&session, "> (x) src/store.rs"), "{}", session.text());
}

#[test]
fn the_last_search_carries_to_the_pane_focus_moves_to_and_n_searches_it() {
    let mut session = Session::plain(80, 24);
    session.press(&open(RETRY_IDENTITY));
    session.press(&format!("/ {} <Enter>", typing("ChargeRequest")));

    // The code before the change holds it on more lines than are in view.
    session.press("l");
    let found = status(&session);
    assert!(
        found.ends_with(" matches for \"ChargeRequest\""),
        "{found:?}:\n{}",
        session.text()
    );
    session.press("n");
    assert!(
        status(&session).starts_with("Match 1 of "),
        "{}",
        session.text()
    );
    let at = code(&session)
        .iter()
        .position(|row| row == "18 pub struct ChargeRequest {");
    assert!(
        at.is_some_and(|at| at >= 4),
        "its 18th line, scrolled to the middle of the pane:\n{}",
        session.text()
    );
}

#[test]
fn backspace_erases_in_the_search_box_esc_keeps_the_last_search_and_empty_enter_searches_nothing() {
    let mut session = Session::plain(80, 24);
    session.press(&open(RETRY_IDENTITY));

    session.press(&format!(
        "/ {} <BS> <BS> <BS> <Enter>",
        typing("operation_keyxyz")
    ));
    assert_eq!(status(&session), "Match 1 of 4 for \"operation_key\"");

    session.press(&format!("/ {}", typing("refund")));
    session.press("<BS>");
    assert_eq!(status(&session), "/refun", "{}", session.text());
    session.press("<Esc>");
    assert_eq!(status(&session), "Match 1 of 4 for \"operation_key\"");

    session.press("/ <Enter>");
    assert_eq!(status(&session), "Match 1 of 4 for \"operation_key\"");
    session.press("/ <BS> <BS> x <BS> <Enter>");
    assert_eq!(status(&session), "Match 1 of 4 for \"operation_key\"");
}

#[test]
fn backspace_erases_a_whole_character_even_one_typed_as_two_code_points() {
    let mut session = Session::plain(80, 24);
    session.press(&open(RETRY_IDENTITY));

    // An `e` and a combining acute accent: one character on screen, `é`.
    session.press("/ c a f e \u{301}");
    assert_eq!(status(&session), "/cafe\u{301}", "{}", session.text());

    session.press("<BS>");
    assert_eq!(status(&session), "/caf", "{}", session.text());
}

#[test]
fn a_search_longer_than_the_row_shows_its_end_where_the_typing_is() {
    let mut session = Session::plain(50, 12);
    session.press(&open(RETRY_IDENTITY));

    session.press(&format!("/ {} {}", times("x", 60), typing("end")));

    let typed = status(&session);
    assert!(
        typed.starts_with("/xx") && typed.ends_with("xxend"),
        "the prompt, then the end of the search:\n{}",
        session.text()
    );
    assert!(typed.len() < "/".len() + 60 + "end".len(), "{typed:?}");
}

#[test]
fn key_help_on_the_viewer_lists_its_keys() {
    let mut session = Session::plain(120, 32);
    session.press(&open(RETRY_IDENTITY));
    session.press("?");

    let rows = session.box_rows("Keys on Change");
    for (keys, does) in [
        ("h, Left, l, Right", "Move to the pane on the left or right"),
        (
            "j, Down, k, Up",
            "Scroll, or choose the next or previous file",
        ),
        ("gg, G", "Go to the top or bottom"),
        ("Ctrl+D, Ctrl+U", "Scroll down or up half a page"),
        ("/", "Search the focused pane"),
        ("n, N", "Next or previous match"),
        ("q, Esc", "Back"),
        ("Tab, Shift+Tab", "Next or previous control"),
        ("Ctrl+C", "Quit from anywhere"),
    ] {
        assert!(
            rows.iter()
                .any(|row| row.starts_with(keys) && row.ends_with(does)),
            "{does:?} is {keys:?}:\n{rows:#?}"
        );
    }
}

#[test]
fn the_palette_on_the_viewer_offers_back_the_panes_search_and_the_matches_and_runs_them() {
    let mut session = Session::plain(120, 32);
    session.press(&open(RETRY_IDENTITY));
    session.press(":");

    let rows = session.box_rows("Actions on Change");
    for (index, (label, does)) in [
        ("> Back", "Return to the picker"),
        (
            "Next pane",
            "Move to the pane on the right, or back to Files",
        ),
        (
            "Previous pane",
            "Move to the pane on the left, or round to After",
        ),
        ("Search", "Search the focused pane"),
        ("Next match", "Go to the next match of the last search"),
        (
            "Previous match",
            "Go to the previous match of the last search",
        ),
        ("Home", "Go to the home screen"),
    ]
    .into_iter()
    .enumerate()
    {
        assert!(
            rows[index].starts_with(label) && rows[index].ends_with(does),
            "{label:?}: {rows:#?}"
        );
    }

    // The panes, from the palette.
    session.press("j <Enter>");
    assert!(shows(&session, "> (x) Before"), "{}", session.text());
    session.press(": j j <Enter> : j j <Enter>");
    assert!(
        shows(&session, "> (x) src/provider.rs"),
        "{}",
        session.text()
    );
    session.press(": j <Enter>");

    // Search, from the palette: the box opens, and n from the palette moves on.
    session.press(&format!(
        ": j j j <Enter> {} <Enter>",
        typing("operation_key")
    ));
    assert_eq!(status(&session), "Match 1 of 4 for \"operation_key\"");
    session.press(": j j j j <Enter>");
    assert_eq!(status(&session), "Match 2 of 4 for \"operation_key\"");
    session.press(": j j j j j <Enter>");
    assert_eq!(status(&session), "Match 1 of 4 for \"operation_key\"");

    // Back, to the picker.
    session.press(": <Enter>");
    assert!(
        shows(&session, "> (x) Preserve payment retry identity"),
        "{}",
        session.text()
    );
}

#[test]
fn resizing_keeps_the_scroll_positions_focus_and_last_search() {
    let mut session = Session::plain(80, 24);
    session.press(&open(RETRY_IDENTITY));
    // Scroll the diff, search it, then scroll the code after and keep focus there.
    session.press(&format!(
        "j j / {} <Enter> n l l j j j j",
        typing("operation_key")
    ));
    let before = session.frame();

    session.resize(40, 10);
    assert!(shows(&session, "Window too small"), "{}", session.text());
    session.resize(80, 24);
    assert_eq!(session.frame(), before);

    session.resize(120, 32);
    session.resize(100, 28);
    session.resize(80, 24);
    assert_eq!(session.frame(), before);

    session.press("h h");
    assert_eq!(status(&session), "Match 2 of 4 for \"operation_key\"");
}

#[test]
fn resizing_keeps_a_search_being_typed() {
    let mut session = Session::plain(80, 24);
    session.press(&open(RETRY_IDENTITY));
    session.press("/ o p e");

    session.resize(30, 8);
    session.resize(80, 24);

    assert_eq!(status(&session), "/ope", "{}", session.text());
    session.press(&format!("{} <Enter>", typing("ration_key")));
    assert_eq!(status(&session), "Match 1 of 4 for \"operation_key\"");
}

#[test]
fn after_a_window_grows_the_end_stays_at_the_bottom_and_k_scrolls_at_once() {
    let mut session = Session::plain(80, 24);
    session.press(&open(RETRY_IDENTITY));
    session.press("l l G");

    session.resize(120, 32);
    assert_eq!(
        code(&session).last().map(String::as_str),
        Some("250 }"),
        "{}",
        session.text()
    );
    session.press("k");
    assert_eq!(
        code(&session).last().map(String::as_str),
        Some("249     }"),
        "{}",
        session.text()
    );
}

#[test]
fn the_viewer_at_120x32() {
    let mut session = Session::plain(120, 32);
    session.press(&open(RETRY_IDENTITY));

    insta::assert_debug_snapshot!("viewer_120x32", session.frame());
}

#[test]
fn the_viewer_at_80x24() {
    let mut session = Session::plain(80, 24);
    session.press(&open(RETRY_IDENTITY));

    insta::assert_debug_snapshot!("viewer_80x24", session.frame());
}

/// From the viewer: the code after the change, searched for
/// `ChargeRequest`, on its second match.
const SEARCHED: &str = "l l / C h a r g e R e q u e s t <Enter> n";

#[test]
fn without_colour_focus_the_matches_and_the_current_match_show_by_markers_and_attributes_at_120x32()
{
    let mut session = Session::plain(120, 32);
    session.press(&open(RETRY_IDENTITY));
    session.press(SEARCHED);

    insta::assert_debug_snapshot!("viewer_searched_120x32", session.frame());
}

#[test]
fn without_colour_focus_the_matches_and_the_current_match_show_by_markers_and_attributes_at_80x24()
{
    let mut session = Session::plain(80, 24);
    session.press(&open(RETRY_IDENTITY));
    session.press(SEARCHED);

    insta::assert_debug_snapshot!("viewer_searched_80x24", session.frame());
}

#[test]
fn the_files_pane_focused_on_a_file_the_change_creates_at_80x24() {
    let mut session = Session::plain(80, 24);
    session.press(&open(FILE_STORE));
    session.press("h j j j j");

    insta::assert_debug_snapshot!("viewer_files_pane_80x24", session.frame());
}

#[test]
fn the_code_before_a_file_the_change_creates_at_80x24() {
    let mut session = Session::plain(80, 24);
    session.press(&open(FILE_STORE));
    session.press("h j j j j l l");

    insta::assert_debug_snapshot!("viewer_with_no_code_before_80x24", session.frame());
}

#[test]
fn the_search_box_at_80x24() {
    let mut session = Session::plain(80, 24);
    session.press(&open(RETRY_IDENTITY));
    session.press("/ o p e r");

    insta::assert_debug_snapshot!("viewer_search_box_80x24", session.frame());
}

#[test]
fn the_viewer_saying_nothing_matches_at_80x24() {
    let mut session = Session::plain(80, 24);
    session.press(&open(RETRY_IDENTITY));
    session.press(&format!("/ {} <Enter>", typing("refund")));

    insta::assert_debug_snapshot!("viewer_with_no_match_80x24", session.frame());
}

#[test]
fn key_help_on_the_viewer_at_120x32() {
    let mut session = Session::plain(120, 32);
    session.press(&open(RETRY_IDENTITY));
    session.press("?");

    insta::assert_debug_snapshot!("key_help_on_the_viewer_120x32", session.frame());
}

#[test]
fn key_help_on_the_viewer_at_80x24() {
    let mut session = Session::plain(80, 24);
    session.press(&open(RETRY_IDENTITY));
    session.press("?");

    insta::assert_debug_snapshot!("key_help_on_the_viewer_80x24", session.frame());
}

#[test]
fn the_palette_on_the_viewer_at_80x24() {
    let mut session = Session::plain(80, 24);
    session.press(&open(RETRY_IDENTITY));
    session.press(":");

    insta::assert_debug_snapshot!("palette_on_the_viewer_80x24", session.frame());
}

#[test]
fn the_viewer_in_colour_at_80x24() {
    let mut session = Session::start(
        Setup {
            variables: &[("COLORTERM", "truecolor")],
            ..Setup::default()
        },
        80,
        24,
    );
    session.press(&open(RETRY_IDENTITY));
    session.press(SEARCHED);

    insta::assert_debug_snapshot!("viewer_in_colour_80x24", session.frame());
}

#[test]
fn in_a_narrow_window_the_title_keeps_a_gap_between_the_summary_and_its_areas() {
    let mut session = Session::plain(50, 12);

    // "Put storage and the provider behind ports", in System design.
    session.press(&open(5));

    assert_eq!(
        session.rows()[1].trim(),
        "Put storage and the provider behind ports  Sys",
        "{}",
        session.text()
    );
}

#[test]
fn before_and_after_are_disabled_for_a_file_the_change_creates_saying_why() {
    let mut session = Session::plain(80, 24);
    session.press(&open(FILE_STORE));
    assert!(
        shows(&session, "> (x) Diff   ( ) Before   ( ) After"),
        "{}",
        session.text()
    );

    session.press("h j j j j l");

    assert!(
        shows(&session, "> (x) Diff   ( ) Before (new file)   ( ) After"),
        "{}",
        session.text()
    );
}

#[test]
fn long_lines_wrap_so_their_ends_show_at_80x24() {
    let mut session = Session::plain(80, 24);
    session.press(&open(RETRY_IDENTITY));

    // `-    pub fn new(payment: &Payment, operation_key: OperationKey) -> Self {`
    // is wider than the code pane at 80x24.
    assert!(
        code_shows(&session, "-    pub fn new(payment: &Payment,"),
        "{}",
        session.text()
    );
    assert!(
        code_shows(&session, "operation_key: OperationKey) -> Self {"),
        "its end, on the next row:\n{}",
        session.text()
    );
}

#[test]
fn a_search_matches_whole_characters_only() {
    // A change whose code holds `café`, its `é` an `e` and a combining accent.
    let showcase = ShowcaseProject::new(vec![Change::new(
        "Name the café".to_owned(),
        vec!["domain".parse().expect("an area ID")],
            "diff --git a/menu.txt b/menu.txt\n--- a/menu.txt\n+++ b/menu.txt\n@@ -1 +1 @@\n-cafe\n+cafe\u{301}\n"
                .to_owned(),
            vec![TouchedFile::modified(
                "menu.txt".to_owned(),
                "cafe\n".to_owned(),
                "cafe\u{301}\n".to_owned(),
            )],
    )]);
    let mut session = Session::start(
        Setup {
            variables: NO_COLOR,
            showcase: Some(&showcase),
            ..Setup::default()
        },
        80,
        24,
    );
    // Past the five areas, onto the change.
    session.press("<Enter> j j j j j <Enter>");

    session.press(&format!("/ {} <Enter>", typing("cafe")));
    assert_eq!(
        status(&session),
        "Match 1 of 1 for \"cafe\"",
        "{}",
        session.text()
    );

    session.press("l l");
    assert_eq!(
        status(&session),
        "No match for \"cafe\" in After",
        "the e of the é is not a whole character:\n{}",
        session.text()
    );
    session.press(&format!("/ {} <Enter>", typing("caf")));
    assert_eq!(
        status(&session),
        "Match 1 of 1 for \"caf\"",
        "{}",
        session.text()
    );
}

#[test]
fn the_palette_on_the_viewer_at_120x32() {
    let mut session = Session::plain(120, 32);
    session.press(&open(RETRY_IDENTITY));
    session.press(":");

    insta::assert_debug_snapshot!("palette_on_the_viewer_120x32", session.frame());
}

#[test]
fn the_viewer_saying_nothing_matches_at_120x32() {
    let mut session = Session::plain(120, 32);
    session.press(&open(RETRY_IDENTITY));
    session.press(&format!("/ {} <Enter>", typing("refund")));

    insta::assert_debug_snapshot!("viewer_with_no_match_120x32", session.frame());
}
