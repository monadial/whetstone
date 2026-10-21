//! Filtering the commits: typing in the picker's search box narrows the
//! change list, as the learner types, to the commits whose message or ID
//! matches, reading older commits in the background for more matches.
//! Enter keeps the filter, and paging, choosing and opening work within it;
//! Esc, or clearing the box, shows every commit again.

use crate::commit_paging::{CHANGES, TO_THE_NEWEST, history_of, in_view, listed, paging, position};
use crate::commits::focused;
use crate::repo::TestRepo;
use crate::repository::started_in;
use crate::viewer::{footer, status, typing};

/// A repository of 250 commits: "Start the payments service", then "Bump
/// the counter to 1" to "Bump the counter to 249", newest first in the
/// picker. The first 50 read when the picker opens are the bumps to 249
/// down to 200.
fn long_history() -> TestRepo {
    history_of(250)
}

/// The rows of `log` whose bump is one of `bumps`, as the picker lists them,
/// newest first.
fn bumps(log: &[String], bumps: impl IntoIterator<Item = usize>) -> Vec<String> {
    bumps
        .into_iter()
        .map(|bump| log[249 - bump].clone())
        .collect()
}

#[test]
fn typing_narrows_the_list_to_the_matching_commits_as_the_learner_types() {
    let repo = long_history();
    let log = listed(&repo);
    let mut session = started_in(repo.root(), 120, 32);
    session.press("<Enter> /");
    assert_eq!(footer(&session), "Enter keep  Esc clear");

    session.press(&typing("to 24"));
    assert_eq!(status(&session), "/to 24");
    // The bumps to 249 down to 240 are read already, and older commits are
    // read in the background for more.
    assert_eq!(in_view(&session), bumps(&log, (240..=249).rev()));
    assert_eq!(
        position(&session),
        "10+ matching “to 24” · 1–10 · searching older commits…"
    );

    session.settle();
    let mut matches = bumps(&log, (240..=249).rev());
    matches.extend(bumps(&log, [24]));
    assert_eq!(in_view(&session), matches);
    assert_eq!(position(&session), "11 matching “to 24” · 1–11");

    // One more character narrows it again.
    session.press("9");
    assert_eq!(in_view(&session), bumps(&log, [249]));
    assert_eq!(position(&session), "1 matching “to 249” · 1");
}

#[test]
fn the_filter_matches_as_search_did_ignoring_case_unless_a_capital_is_typed() {
    let repo = long_history();
    let mut session = started_in(repo.root(), 120, 32);
    session.press("<Enter>");

    session.press(&typing("/BUMP THE COUNTER TO 1"));
    session.settle();
    assert_eq!(in_view(&session), Vec::<String>::new());
    assert_eq!(
        session.section(CHANGES)[0],
        "No commits match “BUMP THE COUNTER TO 1”"
    );

    // Erasing the capitals' search and typing it small matches.
    session.press(&vec!["<BS>"; 22].join(" "));
    session.press(&typing("bump the counter to 1"));
    // The search for capitals read every commit, so all its matches show.
    session.settle();
    assert_eq!(
        position(&session),
        "111 matching “bump the counter to 1” · 1–18"
    );
}

#[test]
fn matches_older_than_the_commits_read_are_found_in_the_background() {
    let repo = long_history();
    let log = listed(&repo);
    let mut session = started_in(repo.root(), 120, 32);
    session.press("<Enter>");

    session.press(&typing("/payments service"));
    // None of the first 50 matches.
    assert_eq!(in_view(&session), Vec::<String>::new());
    assert_eq!(position(&session), "Searching older commits…");

    session.settle();
    assert_eq!(in_view(&session), [log[249].clone()]);
    assert_eq!(position(&session), "1 matching “payments service” · 1");
}

#[test]
fn enter_keeps_the_filter_and_paging_choosing_and_opening_work_within_it() {
    let repo = long_history();
    let log = listed(&repo);
    let mut session = started_in(repo.root(), 120, 32);
    session.press("<Enter>");
    // "to 1" matches the bumps to 199 down to 100, to 19 down to 10, and to
    // 1, but none of the first 50 read.
    session.press(&typing("/to 1"));
    session.settle();
    session.press("<Enter>");

    assert_eq!(status(&session), "");
    assert_eq!(focused(&session), log[249 - 199]);
    assert_eq!(in_view(&session), bumps(&log, (182..=199).rev()));

    session.press("<PageDown>");
    session.settle();
    assert_eq!(focused(&session), log[249 - 181]);
    assert_eq!(in_view(&session), bumps(&log, (164..=181).rev()));
    assert_eq!(position(&session), "54+ matching “to 1” · 19–36");
    assert_eq!(paging(&session), "Previous page    Next page");

    session.press("j <Space>");
    assert_eq!(focused(&session), log[249 - 180].replacen("( )", "(x)", 1));
    assert_eq!(
        session.row_with("Review what will be sent"),
        "Review what will be sent"
    );

    // Enter opens it, and back on the picker the filter is still on.
    session.press("<Enter>");
    assert!(
        session.rows()[1]
            .trim()
            .ends_with("Bump the counter to 180"),
        "{}",
        session.text()
    );
    session.press("q");
    assert_eq!(position(&session), "54+ matching “to 1” · 19–36");
}

#[test]
fn esc_shows_every_commit_again_with_the_focused_commit_kept_in_view() {
    let repo = long_history();
    let log = listed(&repo);
    let mut session = started_in(repo.root(), 120, 32);
    session.press("<Enter>");
    session.press(&typing("/to 24"));
    session.settle();
    session.press("<Enter> G");
    assert_eq!(focused(&session), log[249 - 24]);

    // Esc on the picker clears the kept filter, and q still goes back.
    session.press("<Esc>");
    assert_eq!(focused(&session), log[249 - 24]);
    assert!(in_view(&session).contains(&log[249 - 24]));
    assert_eq!(
        position(&session),
        format!("Commits {}–{} of 250", 249 - 24 + 1, 249 - 24 + 18)
    );

    // In the search box, Esc does the same, and so does clearing it.
    for clear in ["<Esc>", "<BS> <BS> <BS> <BS> <BS>"] {
        session.press(&typing("/to 24"));
        session.settle();
        session.press("<Enter> G k");
        assert_eq!(focused(&session), log[249 - 240]);
        session.press(&format!("/ {clear}"));
        assert_eq!(focused(&session), log[249 - 240], "after {clear}");
        assert!(in_view(&session).contains(&log[249 - 240]), "after {clear}");
        assert!(position(&session).starts_with("Commits "), "after {clear}");
    }
}

#[test]
fn with_no_matches_the_list_says_so_and_offers_to_clear_the_filter() {
    let repo = long_history();
    let mut session = started_in(repo.root(), 120, 32);
    session.press("<Enter> j <Space>");
    session.press(&typing("/refund"));
    session.settle();

    assert_eq!(session.section(CHANGES)[0], "No commits match “refund”");
    session.press("<Enter>");
    assert_eq!(
        session.section(CHANGES)[..2],
        ["No commits match “refund”", "> Clear the filter"]
    );

    session.press("<Enter>");
    assert!(
        position(&session).starts_with("Commits 1–18"),
        "{}",
        session.text()
    );
    assert!(
        focused(&session).starts_with("(x) Domain"),
        "with no commit chosen, focus goes to the first area:\n{}",
        session.text()
    );
}

#[test]
fn key_help_on_the_filtered_picker_says_esc_clears_the_filter() {
    let repo = long_history();
    let mut session = started_in(repo.root(), 80, 24);
    session.press(&format!("{TO_THE_NEWEST} / 2 4 <Enter> ?"));

    let rows = session.overlay_rows();
    for (keys, does) in [
        ("/", "Change the filter"),
        ("Esc", "Clear the filter"),
        ("q", "Back"),
    ] {
        assert!(
            rows.iter()
                .any(|row| row.starts_with(keys) && row.ends_with(does)),
            "{does:?} is {keys:?}:\n{rows:#?}"
        );
    }
}

#[test]
fn the_filtered_commit_picker_at_120x32() {
    let repo = long_history();
    let mut session = started_in(repo.root(), 120, 32);
    session.press("<Enter>");
    session.press(&typing("/to 24"));
    session.settle();
    session.press("<Enter> j");

    insta::assert_debug_snapshot!("filtered_commit_picker_120x32", session.frame());
}

#[test]
fn the_filtered_commit_picker_at_80x24() {
    let repo = long_history();
    let mut session = started_in(repo.root(), 80, 24);
    session.press("<Enter>");
    session.press(&typing("/to 24"));
    session.settle();
    session.press("<Enter> j");

    insta::assert_debug_snapshot!("filtered_commit_picker_80x24", session.frame());
}
