//! The time limit on finding connected code: past it the outbound review
//! carries on without connected code, while the analysis runs on and caches
//! its graph for the next review of that commit. Analyses run one at a
//! time, so switching quickly between commits analyses only the newest.

use std::fs;
use std::time::Duration;

use tempfile::TempDir;
use whetstone::Flow;

use crate::connected_code::{REVIEW_NEWEST, connected_code};
use crate::graph_cache::{graphs, started_with};
use crate::repo::TestRepo;
use crate::repository::started_in;
use crate::session::{Session, Setup, TIME_LIMIT};
use crate::stage_row::with_a_caller;

/// Starts the app without colour in `repo`, in a 120 by 60 terminal, with
/// `time_limit` on finding connected code.
fn started_with_a_limit(repo: &TestRepo, time_limit: Duration) -> Session {
    Session::start(
        Setup {
            variables: &[("NO_COLOR", "1")],
            start: Some(repo.root()),
            time_limit: Some(time_limit),
            ..Setup::default()
        },
        120,
        60,
    )
}

#[test]
fn with_a_time_limit_of_0_connected_code_is_not_available_and_reviewing_again_takes_it_from_the_cache()
 {
    let repo = with_a_caller();
    let mut session = started_with_a_limit(&repo, Duration::ZERO);
    session.press(REVIEW_NEWEST);

    session.wait(Duration::ZERO);
    let gave_up =
        ["Connected code isn't available for this change: finding it took longer than 0 s."];
    assert_eq!(connected_code(&session), gave_up);

    // The analysis runs on to its end, but the review stays without it.
    session.settle();
    assert_eq!(connected_code(&session), gave_up);
    assert_eq!(
        session.row_with("Related tests"),
        "Related tests and configuration aren't available for this change: finding them took \
         longer than 0 s."
    );

    // Back to the picker, and review the same commit again.
    session.press("q <Enter>");
    session.settle();
    assert_eq!(
        connected_code(&session),
        [
            "Connected code: 1 range · 1 KB of 100 KB limit · from cache",
            "report.py  lines 4–5  1 KB  included  calls `total`",
        ]
    );
}

/// The stage row's first line: the rows above the outbound review's title
/// start with it.
fn first_stages(session: &Session) -> String {
    session.row_with("Reading the change")
}

#[test]
fn connected_code_not_found_within_20_s_of_the_review_opening_is_not_available_and_skipped() {
    let repo = with_a_caller();
    let mut session = started_in(repo.root(), 120, 60);
    session.press(REVIEW_NEWEST);

    let just_short = TIME_LIMIT
        .checked_sub(Duration::from_millis(1))
        .expect("the time limit is longer than a millisecond");
    session.wait(just_short);
    assert_eq!(connected_code(&session), ["Connected code: finding…"]);
    session.wait(Duration::from_millis(1));

    assert_eq!(
        connected_code(&session),
        ["Connected code isn't available for this change: finding it took longer than 20 s."]
    );
    assert_eq!(
        first_stages(&session),
        "✓ Reading the change → – Finding connected code → ✓ Preparing evidence → ⠋ Outbound review →"
    );
}

#[test]
fn connected_code_found_in_time_stays_once_the_time_limit_runs_out() {
    let repo = with_a_caller();
    let mut session = started_in(repo.root(), 120, 60);
    session.press(REVIEW_NEWEST);
    session.settle();
    let found = connected_code(&session);

    session.wait(TIME_LIMIT);

    assert_eq!(connected_code(&session), found);
    assert_eq!(found[0], "Connected code: 1 range · 1 KB of 100 KB limit");
}

#[test]
fn the_time_limit_covers_finding_connected_code_in_the_cache() {
    let repo = with_a_caller();
    let mut session = started_in(repo.root(), 120, 60);
    session.press(REVIEW_NEWEST);
    session.settle();

    // Back to the picker, and review the same commit again, whose graph is
    // in the cache, but let the time limit run out before it loads.
    session.press("q <Enter>");
    session.wait(TIME_LIMIT);
    session.settle();

    assert_eq!(
        connected_code(&session),
        ["Connected code isn't available for this change: finding it took longer than 20 s."]
    );
}

/// [`with_a_caller`] with two more commits that change `total`, so its
/// commits, newest first, are "Count from three", "Count from two", "Count
/// from zero" and "Start the ledger", each with `report` as connected code.
fn with_three_changes() -> TestRepo {
    let repo = with_a_caller();
    for (start, name) in [("2", "two"), ("3", "three")] {
        repo.write(
            "ledger.py",
            format!("def total(items):\n    return sum(items, {start})\n"),
        );
        repo.commit(&format!("Count from {name}"));
    }
    repo
}

/// From the outbound review, the keys that go back to the picker and review
/// the commit `up` rows above the picker's Review control, which the oldest
/// commit is right above.
fn review_another(up: usize) -> String {
    format!("q {}<Space> {}<Enter>", "k ".repeat(up), "j ".repeat(up))
}

#[test]
fn switching_quickly_between_commits_analyses_only_the_newest() {
    let repo = with_three_changes();
    let cache_home = TempDir::new().expect("create a cache home");
    let temp = TempDir::new().expect("create a system temporary directory");
    let mut session = started_with(&repo, cache_home.path(), temp.path());
    // Review the newest commit, then, before its connected code is found,
    // the one before it, then the one before that.
    session.press(REVIEW_NEWEST);
    session.press(&review_another(3));
    session.row_with("Count from two");
    session.press(&review_another(2));
    session.row_with("Count from zero");

    session.settle();
    assert_eq!(
        connected_code(&session)[0],
        "Connected code: 1 range · 1 KB of 100 KB limit"
    );
    assert_eq!(
        graphs(cache_home.path()).len(),
        2,
        "the newest and the last"
    );

    // Reviewing the one in between again analyses it for the first time.
    session.press(&review_another(3));
    session.row_with("Count from two");
    session.settle();
    assert_eq!(
        connected_code(&session)[0],
        "Connected code: 1 range · 1 KB of 100 KB limit"
    );
    assert_eq!(graphs(cache_home.path()).len(), 3);
}

#[test]
fn a_review_waiting_for_another_commit_s_analysis_counts_its_time_limit_from_when_it_opened() {
    let repo = with_three_changes();
    let mut session = started_in(repo.root(), 120, 60);
    session.press(REVIEW_NEWEST);
    session.wait(Duration::from_secs(15));
    // The newest commit's analysis is still running as the one before it
    // is reviewed.
    session.press(&review_another(3));
    session.row_with("Count from two");

    // The newest commit's review would have given up now.
    session.wait(Duration::from_secs(5));
    assert_eq!(connected_code(&session), ["Connected code: finding…"]);
    session.wait(Duration::from_secs(15));
    assert_eq!(
        connected_code(&session),
        ["Connected code isn't available for this change: finding it took longer than 20 s."]
    );

    // Both analyses still run to their end, and the second is kept.
    session.settle();
    session.press("q <Enter>");
    session.settle();
    assert_eq!(
        connected_code(&session)[0],
        "Connected code: 1 range · 1 KB of 100 KB limit · from cache"
    );
}

#[test]
fn quitting_during_an_analysis_quits_at_once_and_analyses_nothing_that_waits() {
    let repo = with_three_changes();
    let cache_home = TempDir::new().expect("create a cache home");
    let temp = TempDir::new().expect("create a system temporary directory");
    let mut session = started_with(&repo, cache_home.path(), temp.path());
    // The newest commit is analysed, and the one before it waits its turn.
    session.press(REVIEW_NEWEST);
    session.press(&review_another(3));

    assert_eq!(session.press("<C-c>"), Flow::Quit);
    drop(session);

    assert_eq!(
        graphs(cache_home.path()).len(),
        1,
        "only the newest commit's"
    );
    let left: Vec<_> = fs::read_dir(temp.path())
        .expect("read the temporary directory")
        .collect();
    assert!(left.is_empty(), "left in the temporary directory: {left:?}");
}
