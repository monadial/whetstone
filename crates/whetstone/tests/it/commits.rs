//! Choosing a commit from the learner's repository: the picker lists the
//! commits reachable from HEAD, newest first, reads more as the learner
//! scrolls, and finds them by message or ID.

use crate::repo::TestRepo;
use crate::repository::started_in;
use crate::session::Session;
use crate::viewer::{footer, status, typing};

/// A repository called `payments` whose first commit is followed by
/// `bumps` more, each bumping a counter: "Bump the counter to 1" and on.
fn with_bumps(bumps: usize) -> TestRepo {
    let repo = TestRepo::new("payments");
    repo.write("README.md", "# Payments\n");
    repo.commit("Start the payments service");
    for bump in 1..=bumps {
        repo.write("counter.txt", format!("{bump}\n"));
        repo.commit(&format!("Bump the counter to {bump}"));
    }
    repo
}

/// The rows of `repo`'s commits as `git log` lists them, newest first: the
/// short ID and subject, as the picker shows them unfocused.
pub(crate) fn git_log(repo: &TestRepo) -> Vec<String> {
    repo.git(&["log", "--format=( ) %h  %s"])
        .lines()
        .map(str::to_owned)
        .collect()
}

/// The focused row, trimmed, without its focus marker.
pub(crate) fn focused(session: &Session) -> String {
    let rows = session.rows();
    let row = rows
        .iter()
        .map(|row| row.trim())
        .find(|row| row.starts_with("> "));
    assert!(row.is_some(), "no row has focus:\n{}", session.text());
    row.unwrap_or_default().trim_start_matches("> ").to_owned()
}

#[test]
fn the_picker_reads_more_commits_as_the_learner_scrolls_past_the_first_fifty() {
    let repo = with_bumps(74);
    let log = git_log(&repo);
    assert_eq!(log.len(), 75);
    let mut session = started_in(repo.root(), 120, 32);

    // Past the five areas onto the newest commit, and down to the last of
    // the first fifty read.
    session.press("<Enter> j j j j j");
    session.press(&["j"; 49].join(" "));

    assert_eq!(focused(&session), log[49]);
    session.press("j");
    assert_eq!(focused(&session), log[50]);
    session.press(&vec!["j"; 24].join(" "));
    assert!(
        focused(&session).starts_with(&log[74]),
        "{}",
        session.text()
    );
    session.press("j");
    assert_eq!(
        focused(&session),
        "Review what will be sent (choose a change)"
    );
}

#[test]
fn slash_filters_the_commits_by_message_as_the_learner_types() {
    let repo = with_bumps(10);
    let log = git_log(&repo);
    let mut session = started_in(repo.root(), 80, 24);
    session.press("<Enter> /");
    assert_eq!(status(&session), "/", "{}", session.text());
    assert_eq!(footer(&session), "Enter keep  Esc clear");

    session.press(&typing("counter to 1"));

    // "to 10" and "to 1" match, newest first.
    assert_eq!(listed(&session), [log[0].clone(), log[9].clone()]);
    session.press("<Enter>");
    assert_eq!(focused(&session), log[0]);
    session.press("j");
    assert_eq!(focused(&session), log[9]);
    session.press("j");
    assert_eq!(
        focused(&session),
        "Review what will be sent (choose a change)"
    );
}

#[test]
fn a_filter_matches_the_whole_message() {
    let repo = with_bumps(3);
    repo.write("refund.rs", "");
    repo.commit("Add refunds\n\nA refund returns a payment.");
    let log = git_log(&repo);
    let mut session = started_in(repo.root(), 80, 24);
    // Onto the second commit.
    session.press("<Enter> j j j j j j");

    session.press(&typing("/payment"));
    session.press("<Enter>");

    // The newest one's body, and the first commit's subject, match, and
    // the second commit doesn't, so focus moves to the first match.
    let first = format!("{}  first commit", log[4]);
    assert_eq!(listed(&session), [log[0].clone(), first]);
    assert_eq!(focused(&session), log[0]);
}

#[test]
fn a_filter_finds_a_commit_by_the_start_of_its_id() {
    let repo = with_bumps(74);
    let log = git_log(&repo);
    let full = repo.git(&["log", "--skip=60", "-1", "--format=%H"]);
    let mut session = started_in(repo.root(), 120, 32);

    session.press("<Enter>");
    let start = full.get(..9).expect("a commit ID has 40 digits");
    session.press(&typing(&format!("/{start}")));
    // It is past the first fifty read, so older commits are read for it.
    session.settle();
    session.press("<Enter>");

    assert_eq!(focused(&session), log[60]);
}

/// The commit rows the change list shows, without the focus marker.
fn listed(session: &Session) -> Vec<String> {
    session
        .section(CHANGES)
        .into_iter()
        .filter(|row| row.contains("( ) ") || row.contains("(x) "))
        .map(|row| row.trim_start_matches("> ").to_owned())
        .collect()
}

#[test]
fn esc_closes_the_search_box_without_searching() {
    let repo = with_bumps(3);
    let mut session = started_in(repo.root(), 80, 24);
    session.press("<Enter>");

    session.press(&typing("/counter"));
    session.press("<Esc>");

    assert_eq!(status(&session), "");
    assert_eq!(
        footer(&session),
        "? keys  : actions  Space toggle  Enter open  PgDn/Up page  / search  q back"
    );
    assert!(
        focused(&session).starts_with("(x) Domain"),
        "{}",
        session.text()
    );
}

/// The heading above the commits.
const CHANGES: &str = "Change to practice on";

#[test]
fn in_a_shallow_clone_the_commit_whose_parent_is_missing_is_listed_but_cannot_be_chosen() {
    let repo = with_bumps(5);
    let log = git_log(&repo);
    let clone = repo.shallow_clone("payments-shallow", 3);
    let mut session = started_in(&clone, 120, 32);

    session.press("<Enter>");

    let boundary = format!("{} (parent not in this clone)", log[2]);
    assert_eq!(
        session
            .section(CHANGES)
            .into_iter()
            .take_while(|row| !row.starts_with("Previous page"))
            .collect::<Vec<_>>(),
        [log[0].clone(), log[1].clone(), boundary.clone()]
    );
    session.press("j j j j j j j <Space> <Enter>");
    assert_eq!(focused(&session), boundary);
    assert!(
        session
            .section(CHANGES)
            .iter()
            .all(|row| !row.contains("(x)")),
        "{}",
        session.text()
    );
}

/// A shallow clone of a history with a merge, holding the merge and the
/// three commits before it, the oldest of them without its parent.
fn shallow_with_a_merge() -> (TestRepo, std::path::PathBuf) {
    let repo = with_bumps(3);
    repo.git(&["switch", "--quiet", "-c", "refunds"]);
    repo.write("refund.rs", "");
    repo.commit("Add refunds");
    repo.git(&["switch", "--quiet", "main"]);
    repo.write("counter.txt", "4\n");
    repo.commit("Bump the counter to 4");
    repo.merge("refunds", "Merge refunds");
    let clone = repo.shallow_clone("payments-shallow", 3);
    (repo, clone)
}

#[test]
fn the_commit_picker_after_a_search_in_a_shallow_clone_at_120x32() {
    let (_repo, clone) = shallow_with_a_merge();
    let mut session = started_in(&clone, 120, 32);
    session.press("<Enter>");
    session.press(&typing("/counter"));
    session.press("<Enter>");

    insta::assert_debug_snapshot!("commit_picker_searched_120x32", session.frame());
}

#[test]
fn the_commit_picker_after_a_search_in_a_shallow_clone_at_80x24() {
    let (_repo, clone) = shallow_with_a_merge();
    let mut session = started_in(&clone, 80, 24);
    session.press("<Enter>");
    session.press(&typing("/counter"));
    session.press("<Enter>");

    insta::assert_debug_snapshot!("commit_picker_searched_80x24", session.frame());
}

#[test]
fn the_commit_picker_search_box_at_80x24() {
    let (_repo, clone) = shallow_with_a_merge();
    let mut session = started_in(&clone, 80, 24);
    session.press("<Enter>");
    session.press(&typing("/counter"));

    insta::assert_debug_snapshot!("commit_picker_search_box_80x24", session.frame());
}

#[test]
fn the_palette_on_the_repository_picker_offers_the_filter_and_runs_it() {
    let repo = with_bumps(10);
    let log = git_log(&repo);
    let mut session = started_in(repo.root(), 120, 32);
    session.press("<Enter> :");

    let rows = session.box_rows("Actions on Practice");
    for (index, (label, does)) in [
        ("> Back", "Return to the screen before"),
        ("Search", "Filter the commits by message or ID"),
    ]
    .into_iter()
    .enumerate()
    {
        assert!(
            rows[index].starts_with(label) && rows[index].ends_with(does),
            "{label:?}: {rows:#?}"
        );
    }

    session.press("j <Enter>");
    assert_eq!(status(&session), "/");
    session.press(&typing("counter to 1"));
    session.press("<Enter> j");
    assert_eq!(focused(&session), log[9]);

    // With the filter on, the palette offers to clear it, and focus stays.
    session.press(":");
    let rows = session.box_rows("Actions on Practice");
    assert!(
        rows[2].starts_with("Clear the filter") && rows[2].ends_with("Show every commit again"),
        "{rows:#?}"
    );
    session.press("j j <Enter>");
    assert_eq!(listed(&session).len(), 11, "{}", session.text());
    assert_eq!(focused(&session), log[9]);
}
