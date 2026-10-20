//! A paste goes into the focused text entry as one cleaned insertion, and
//! never acts as keys. Anywhere without a focused text entry it does nothing.

use ratatui::crossterm::event::Event;
use whetstone::Flow;

use crate::repo::TestRepo;
use crate::repository::started_in;
use crate::session::Session;
use crate::viewer::{footer, status};

/// From home: open the picker, move past its five areas onto "Preserve
/// payment retry identity", the fifth curated change, and open it in the
/// viewer.
const OPEN_RETRY_IDENTITY: &str = "<Enter> j j j j j j j j j <Enter>";

/// The footer while the search box has focus.
const SEARCH_BOX_FOOTER: &str = "Enter search  Esc cancel";

/// Pastes `text`, as a terminal with bracketed paste reports it.
fn paste(session: &mut Session, text: &str) -> Flow {
    session.handle(&Event::Paste(text.to_owned()))
}

/// The viewer on "Preserve payment retry identity", with its search box
/// open.
fn searching_the_viewer() -> Session {
    let mut session = Session::plain(80, 24);
    session.press(OPEN_RETRY_IDENTITY);
    session.press("/");
    session
}

#[test]
fn a_paste_into_the_viewer_search_box_goes_in_whole_in_one_event() {
    let mut session = searching_the_viewer();

    assert_eq!(paste(&mut session, "operation_key"), Flow::Continue);

    assert_eq!(status(&session), "/operation_key", "{}", session.text());
}

#[test]
fn a_pasted_line_break_does_not_submit_the_search_and_becomes_a_space() {
    let mut session = searching_the_viewer();

    paste(&mut session, "operation\nkey");

    assert_eq!(status(&session), "/operation key", "{}", session.text());
    assert_eq!(footer(&session), SEARCH_BOX_FOOTER, "still searching");
}

#[test]
fn keys_in_a_paste_are_typed_and_never_act() {
    let mut session = searching_the_viewer();

    assert_eq!(paste(&mut session, "q\nj : ? G h"), Flow::Continue);

    assert_eq!(status(&session), "/q j : ? G h", "{}", session.text());
    assert_eq!(footer(&session), SEARCH_BOX_FOOTER, "still searching");
    assert!(
        session.rows()[1]
            .trim()
            .starts_with("Preserve payment retry identity"),
        "still on the viewer, with no overlay:\n{}",
        session.text()
    );
}

#[test]
fn crlf_a_lone_cr_and_a_tab_in_a_paste_each_become_a_space() {
    let mut session = searching_the_viewer();

    paste(&mut session, "one\r\ntwo\rthree\tfour");

    assert_eq!(
        status(&session),
        "/one two three four",
        "{}",
        session.text()
    );
}

#[test]
fn control_characters_in_a_paste_are_dropped() {
    let mut session = searching_the_viewer();

    paste(&mut session, "re\u{1b}[31mtry\u{7}\u{0}\u{7f}");

    assert_eq!(status(&session), "/re[31mtry", "{}", session.text());
}

#[test]
fn a_paste_adds_to_what_was_typed_and_backspace_erases_from_its_end() {
    let mut session = searching_the_viewer();
    session.press("o p");

    paste(&mut session, "eration_key");
    assert_eq!(status(&session), "/operation_key", "{}", session.text());

    session.press("<BS> <Enter>");
    assert_eq!(status(&session), "Match 1 of 4 for \"operation_ke\"");
}

#[test]
fn a_pasted_search_of_the_commits_finds_the_commit_as_one_line() {
    let repo = TestRepo::new("payments");
    repo.write("README.md", "# Payments\n");
    repo.commit("Start the payments service");
    repo.write("src/retry.rs", "pub fn retry() {}\n");
    repo.commit("Retry failed payments");
    let mut session = started_in(repo.root(), 80, 24);
    session.press("<Enter> /");

    paste(&mut session, "failed\r\npayments\r\n");
    assert_eq!(status(&session), "/failed payments", "{}", session.text());
    assert_eq!(footer(&session), SEARCH_BOX_FOOTER, "still searching");

    session.press("<BS> <Enter>");
    assert_eq!(status(&session), "Match 1 of 1 for \"failed payments\"");
}

/// Asserts that pasting text full of keys changes nothing in `session`:
/// not the screen, not the overlay, and nothing quits.
fn assert_paste_is_ignored(session: &mut Session) {
    let before = session.text();

    assert_eq!(paste(session, "q\n:?j G\r\n/x\t"), Flow::Continue);

    assert_eq!(session.text(), before, "the paste changed nothing");
}

#[test]
fn a_paste_on_home_is_ignored() {
    let mut session = Session::plain(80, 24);

    assert_paste_is_ignored(&mut session);
}

#[test]
fn a_paste_on_the_picker_is_ignored() {
    let mut session = Session::plain(80, 24);
    session.press("<Enter> j");

    assert_paste_is_ignored(&mut session);
}

#[test]
fn a_paste_on_the_repository_picker_is_ignored_unless_it_is_searching() {
    let repo = TestRepo::new("payments");
    repo.write("README.md", "# Payments\n");
    repo.commit("Start the payments service");
    let mut session = started_in(repo.root(), 80, 24);
    session.press("<Enter>");

    assert_paste_is_ignored(&mut session);

    session.press("/ <Esc>");
    assert_paste_is_ignored(&mut session);
}

#[test]
fn a_paste_on_the_viewer_is_ignored_unless_it_is_searching() {
    let mut session = Session::plain(80, 24);
    session.press(OPEN_RETRY_IDENTITY);

    assert_paste_is_ignored(&mut session);

    session.press("/ <Esc>");
    assert_paste_is_ignored(&mut session);
}

#[test]
fn a_paste_on_the_outbound_review_is_ignored() {
    let repo = TestRepo::new("payments");
    repo.write("README.md", "# Payments\n");
    repo.commit("Start the payments service");
    let mut session = started_in(repo.root(), 120, 40);
    session.press("<Enter> j j j j j <Space> k k k k k <S-Tab> <Enter>");
    assert!(
        session.text().contains("Review what will be sent"),
        "{}",
        session.text()
    );

    assert_paste_is_ignored(&mut session);
}

#[test]
fn a_paste_on_settings_is_ignored() {
    let mut session = Session::plain(80, 24);
    session.press("j <Enter>");
    assert_eq!(session.rows()[1].trim(), "Settings");

    assert_paste_is_ignored(&mut session);
}

#[test]
fn a_paste_over_key_help_or_the_palette_is_ignored() {
    let mut session = Session::plain(80, 24);
    session.press(OPEN_RETRY_IDENTITY);

    session.press("?");
    assert_paste_is_ignored(&mut session);

    session.press("<Esc> :");
    assert_paste_is_ignored(&mut session);
}

#[test]
fn a_paste_under_the_window_too_small_notice_is_ignored() {
    let mut session = searching_the_viewer();
    session.resize(40, 10);

    assert_paste_is_ignored(&mut session);

    session.resize(80, 24);
    assert_eq!(status(&session), "/", "{}", session.text());
}
