//! Paging through the change list a screen at a time: a page is as many
//! changes as the list shows at the current size. Page Down and Page Up
//! move a page, Ctrl+D and Ctrl+U half of one, and `gg` and `G` go to
//! either end. Next page and Previous page under the list page for anyone
//! who doesn't know the keys, and a line between them says where the
//! learner is.

use crate::commits::{focused, git_log};
use crate::repo::TestRepo;
use crate::repository::started_in;
use crate::session::Session;
use crate::viewer::footer;

/// A repository called `payments` with a history of 250 commits: "Start
/// the payments service", then 249 bumps of a counter.
fn long_history() -> TestRepo {
    history_of(250)
}

/// A repository called `payments` with a history of `commits` commits,
/// the first of them "Start the payments service".
fn history_of(commits: usize) -> TestRepo {
    let repo = TestRepo::new("payments");
    repo.write("README.md", "# Payments\n");
    repo.commit("Start the payments service");
    repo.bump_counter(commits - 1);
    repo
}

/// From home: open the picker, and move from the first area past the
/// other four onto the newest commit.
const TO_THE_NEWEST: &str = "<Enter> j j j j j";

/// The heading above the commits.
const CHANGES: &str = "Change to practice on";

/// The commit rows in view, without the focus marker, as [`git_log`]
/// lists them.
fn in_view(session: &Session) -> Vec<String> {
    session
        .section(CHANGES)
        .into_iter()
        .filter(|row| row.contains("( ) ") || row.contains("(x) "))
        .map(|row| row.trim_start_matches("> ").to_owned())
        .collect()
}

/// The rows of `repo`'s commits as the picker lists them unfocused, newest
/// first: as [`git_log`] lists them, and the first commit, the oldest, saying
/// so.
fn listed(repo: &TestRepo) -> Vec<String> {
    let mut log = git_log(repo);
    if let Some(first) = log.last_mut() {
        first.push_str("  first commit");
    }
    log
}

/// The row under the commits, with Previous page, where the list is, and
/// Next page, trimmed.
fn paging(session: &Session) -> String {
    session.row_with("Next page")
}

#[test]
fn page_down_and_page_up_move_the_focus_by_the_rows_the_list_shows() {
    let repo = long_history();
    let log = listed(&repo);
    assert_eq!(log.len(), 250);
    for (width, height, rows) in [(120, 32, 18), (80, 24, 10)] {
        let mut session = started_in(repo.root(), width, height);
        session.press(TO_THE_NEWEST);
        assert_eq!(in_view(&session), log[..rows], "at {width}x{height}");

        session.press("<PageDown>");
        assert_eq!(focused(&session), log[rows], "at {width}x{height}");
        assert_eq!(
            in_view(&session),
            log[rows..2 * rows],
            "the whole next page shows at {width}x{height}"
        );

        session.press("<PageDown> j <PageUp>");
        assert_eq!(focused(&session), log[rows + 1], "at {width}x{height}");
        assert_eq!(
            in_view(&session),
            log[rows..2 * rows],
            "at {width}x{height}"
        );

        session.press("<PageUp>");
        assert_eq!(focused(&session), log[1], "at {width}x{height}");
        assert_eq!(in_view(&session), log[..rows], "at {width}x{height}");
    }
}

#[test]
fn the_line_under_the_commits_says_which_are_in_view_with_a_plus_while_more_remain_unread() {
    let repo = long_history();
    let mut session = started_in(repo.root(), 120, 32);
    session.press("<Enter>");

    // The first 50 commits are read when the picker opens.
    assert_eq!(
        paging(&session),
        "Previous page (first page)    Commits 1–18 of 50+    Next page"
    );

    // Each page reads the page after it too, so Next page knows there is
    // one.
    session.press("j j j j j <PageDown>");
    assert_eq!(
        paging(&session),
        "Previous page    Commits 19–36 of 100+    Next page"
    );

    session.press("G");
    assert_eq!(
        paging(&session),
        "Previous page    Commits 233–250 of 250    Next page (last page)"
    );
}

#[test]
fn next_page_and_previous_page_page_the_commits_and_keep_focus_for_the_next_enter() {
    let repo = long_history();
    let log = listed(&repo);
    let mut session = started_in(repo.root(), 120, 32);

    // From the first area, Shift+Tab wraps round to Review what will be
    // sent, and again to Next page.
    session.press("<Enter> <S-Tab> <S-Tab>");
    assert_eq!(
        paging(&session),
        "Previous page (first page)    Commits 1–18 of 50+  > Next page"
    );

    session.press("<Enter>");
    assert_eq!(in_view(&session), log[18..36]);
    assert_eq!(
        paging(&session),
        "Previous page    Commits 19–36 of 100+  > Next page"
    );
    session.press("<Enter>");
    assert_eq!(in_view(&session), log[36..54]);

    session.press("<S-Tab>");
    assert_eq!(
        paging(&session),
        "> Previous page    Commits 37–54 of 100+    Next page"
    );
    session.press("<Space>");
    assert_eq!(in_view(&session), log[18..36]);

    // Shift+Tab goes on into the page in view, at its last commit, and Tab
    // comes back out to Previous page.
    session.press("<S-Tab>");
    assert_eq!(focused(&session), log[35]);
    session.press("<Tab>");
    assert_eq!(
        paging(&session),
        "> Previous page    Commits 19–36 of 100+    Next page"
    );
}

#[test]
fn previous_page_and_next_page_are_disabled_on_the_first_and_last_page() {
    let repo = long_history();
    let log = listed(&repo);
    let mut session = started_in(repo.root(), 120, 32);
    session.press("<Enter> <S-Tab> <S-Tab> <S-Tab>");
    assert_eq!(
        paging(&session),
        "> Previous page (first page)    Commits 1–18 of 50+    Next page"
    );
    session.press("<Enter>");
    assert_eq!(in_view(&session), log[..18], "nothing moves");

    session.press("G");
    session.press("<Tab> <Tab>");
    assert_eq!(
        paging(&session),
        "Previous page    Commits 233–250 of 250  > Next page (last page)"
    );
    session.press("<Enter>");
    assert_eq!(in_view(&session), log[232..], "nothing moves");
}

#[test]
fn tab_from_the_last_area_lands_on_the_first_commit_in_view() {
    let repo = long_history();
    let log = listed(&repo);
    let mut session = started_in(repo.root(), 80, 24);
    session.press("<Enter> <S-Tab> <S-Tab> <Enter> <Enter>");
    assert_eq!(in_view(&session), log[20..30]);

    // From Next page round to the areas, and on past the last of them.
    session.press("<Tab> <Tab> <Tab> <Tab> <Tab> <Tab> <Tab>");
    assert_eq!(focused(&session), log[20]);
    assert_eq!(in_view(&session), log[20..30]);
}

#[test]
fn ctrl_d_and_ctrl_u_move_the_focus_by_half_the_rows_the_list_shows() {
    let repo = long_history();
    let log = listed(&repo);
    let mut session = started_in(repo.root(), 80, 24);
    session.press(TO_THE_NEWEST);

    session.press("<C-d>");
    assert_eq!(focused(&session), log[5]);
    assert_eq!(in_view(&session), log[5..15]);
    session.press("<C-d> <C-d>");
    assert_eq!(focused(&session), log[15]);
    assert_eq!(in_view(&session), log[15..25]);

    session.press("<C-u>");
    assert_eq!(focused(&session), log[10]);
    assert_eq!(in_view(&session), log[10..20]);
}

#[test]
fn gg_goes_to_the_newest_commit_and_capital_g_to_the_oldest() {
    let repo = long_history();
    let log = listed(&repo);
    let mut session = started_in(repo.root(), 120, 32);
    session.press(TO_THE_NEWEST);

    session.press("G");
    assert_eq!(focused(&session), log[249]);
    assert_eq!(in_view(&session), log[232..]);

    session.press("g g");
    assert_eq!(focused(&session), log[0]);
    assert_eq!(in_view(&session), log[..18]);
}

#[test]
fn gg_and_capital_g_go_to_either_end_from_anywhere_on_the_picker() {
    let repo = long_history();
    let log = listed(&repo);
    let mut session = started_in(repo.root(), 120, 32);

    // From the first area.
    session.press("<Enter> G");
    assert_eq!(focused(&session), log[249]);
    session.press("<S-Tab> g g");
    assert_eq!(focused(&session), log[0]);
}

#[test]
fn capital_g_reads_at_most_a_thousand_more_commits_before_going_to_the_oldest_read() {
    let repo = history_of(1_200);
    let log = listed(&repo);
    let mut session = started_in(repo.root(), 120, 32);
    session.press(TO_THE_NEWEST);

    session.press("G");
    assert_eq!(focused(&session), log[1_049]);
    assert_eq!(
        paging(&session),
        "Previous page    Commits 1,033–1,050 of 1,100+    Next page"
    );

    session.press("G");
    assert_eq!(focused(&session), log[1_199]);
    assert_eq!(
        paging(&session),
        "Previous page    Commits 1,183–1,200 of 1,200    Next page (last page)"
    );
}

#[test]
fn a_page_past_the_commits_read_so_far_reads_further_and_never_stops_short() {
    let repo = long_history();
    let log = listed(&repo);
    let mut session = started_in(repo.root(), 120, 32);
    session.press(TO_THE_NEWEST);

    // The first 50 are read: the third page goes past them.
    session.press("<PageDown> <PageDown> <PageDown>");
    assert_eq!(focused(&session), log[54]);
    assert_eq!(in_view(&session), log[54..72]);

    // Past the last commit, PageDown stops at it, with the last page whole.
    session.press(&["<PageDown>"; 12].join(" "));
    assert_eq!(focused(&session), log[249]);
    assert_eq!(in_view(&session), log[232..]);
    session.press("<PageDown>");
    assert_eq!(focused(&session), log[249]);
}

#[test]
fn a_resize_keeps_the_focused_and_the_chosen_commit() {
    let repo = long_history();
    let log = listed(&repo);
    let mut session = started_in(repo.root(), 120, 32);
    session.press(TO_THE_NEWEST);
    session.press("<PageDown> <PageDown> j j <Space> <PageDown> j");
    let chosen = log[38].replacen("( )", "(x)", 1);
    assert_eq!(focused(&session), log[57]);
    let before = session.frame();

    session.resize(80, 24);
    assert_eq!(focused(&session), log[57], "{}", session.text());
    session.press("<PageUp> <PageUp>");
    assert!(
        in_view(&session).contains(&chosen),
        "{:#?}",
        in_view(&session)
    );
    session.press("<PageDown> <PageDown>");
    session.resize(120, 32);
    assert_eq!(focused(&session), log[57]);

    session.resize(50, 12);
    session.resize(120, 32);
    assert_eq!(focused(&session), log[57]);
    let mut back = started_in(repo.root(), 120, 32);
    back.press(TO_THE_NEWEST);
    back.press("<PageDown> <PageDown> j j <Space> <PageDown> j");
    assert_eq!(back.frame(), before);
}

#[test]
fn key_help_and_the_footer_list_the_paging_keys() {
    let repo = long_history();
    let mut session = started_in(repo.root(), 120, 32);
    session.press("<Enter>");
    assert!(
        footer(&session).contains("PgDn page"),
        "{}",
        footer(&session)
    );

    session.press("?");
    let rows = session.overlay_rows();
    for (keys, does) in [
        ("PgDn, PgUp", "Move a page down or up the changes"),
        ("Ctrl+D, Ctrl+U", "Move half a page down or up the changes"),
        (
            "gg, G",
            "Go to the newest commit, or the oldest read so far",
        ),
    ] {
        assert!(
            rows.iter()
                .any(|row| row.starts_with(keys) && row.ends_with(does)),
            "{does:?} is {keys:?}:\n{rows:#?}"
        );
    }
}

#[test]
fn the_showcase_project_pages_its_changes_too() {
    let mut session = Session::plain(80, 18);
    // Past the five areas, onto the first change.
    session.press("<Enter> j j j j j");
    assert_eq!(
        paging(&session),
        "Previous page (first page)    Changes 1–5 of 10    Next page"
    );

    session.press("<PageDown>");
    assert!(
        session
            .row_with("> ( )")
            .starts_with("> ( ) Put storage and the provider behind ports"),
        "{}",
        session.text()
    );
    assert_eq!(
        paging(&session),
        "Previous page    Changes 6–10 of 10    Next page (last page)"
    );
    session.press("g g");
    assert!(
        session
            .row_with("> ( )")
            .starts_with("> ( ) Model the payment lifecycle"),
        "{}",
        session.text()
    );
}

#[test]
fn the_commit_picker_paged_at_120x32() {
    let repo = long_history();
    let mut session = started_in(repo.root(), 120, 32);
    session.press(TO_THE_NEWEST);
    session.press("<PageDown> <PageDown> j j <Space> <Tab> <Tab>");

    insta::assert_debug_snapshot!("commit_picker_paged_120x32", session.frame());
}

#[test]
fn the_commit_picker_paged_at_80x24() {
    let repo = long_history();
    let mut session = started_in(repo.root(), 80, 24);
    session.press(TO_THE_NEWEST);
    session.press("<PageDown> <PageDown> j j <Space> <Tab> <Tab>");

    insta::assert_debug_snapshot!("commit_picker_paged_80x24", session.frame());
}
