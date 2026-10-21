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
fn slash_searches_the_commits_by_message_and_n_and_capital_n_move_between_the_matches() {
    let repo = with_bumps(10);
    let log = git_log(&repo);
    let mut session = started_in(repo.root(), 80, 24);
    session.press("<Enter> /");
    assert_eq!(status(&session), "/", "{}", session.text());
    assert_eq!(footer(&session), "Enter search  Esc cancel");

    session.press(&typing("counter to 1"));
    session.press("<Enter>");

    // "to 10" and "to 1" match, newest first.
    assert_eq!(focused(&session), log[0]);
    assert_eq!(status(&session), "Match 1 of 2 for \"counter to 1\"");
    session.press("n");
    assert_eq!(focused(&session), log[9]);
    assert_eq!(status(&session), "Match 2 of 2 for \"counter to 1\"");
    session.press("n");
    assert_eq!(focused(&session), log[0]);
    session.press("N");
    assert_eq!(focused(&session), log[9]);
}

#[test]
fn a_search_matches_the_whole_message_and_starts_from_the_focused_commit() {
    let repo = with_bumps(3);
    repo.write("refund.rs", "");
    repo.commit("Add refunds\n\nA refund returns a payment.");
    let log = git_log(&repo);
    let mut session = started_in(repo.root(), 80, 24);
    // Onto the second commit.
    session.press("<Enter> j j j j j j");

    session.press(&typing("/payment"));
    session.press("<Enter>");

    // The first commit's subject, and the newest one's body, match: from
    // the second, the next match is the first commit, then round to the
    // newest.
    assert_eq!(focused(&session), format!("{}  first commit", log[4]));
    session.press("n");
    assert_eq!(focused(&session), log[0]);
}

#[test]
fn a_search_reads_further_when_nothing_read_so_far_matches() {
    let repo = with_bumps(74);
    let log = git_log(&repo);
    let mut session = started_in(repo.root(), 120, 32);

    session.press("<Enter>");
    session.press(&typing("/payments service"));
    session.press("<Enter>");

    assert_eq!(focused(&session), format!("{}  first commit", log[74]));
    assert_eq!(status(&session), "Match 1 of 1 for \"payments service\"");
}

#[test]
fn a_search_finds_a_commit_by_the_start_of_its_id() {
    let repo = with_bumps(74);
    let log = git_log(&repo);
    let full = repo.git(&["log", "--skip=60", "-1", "--format=%H"]);
    let mut session = started_in(repo.root(), 120, 32);

    session.press("<Enter>");
    let start = full.get(..9).expect("a commit ID has 40 digits");
    session.press(&typing(&format!("/{start}")));
    session.press("<Enter>");

    assert_eq!(focused(&session), log[60]);
}

#[test]
fn when_no_commit_matches_the_picker_says_so_and_focus_stays() {
    let repo = with_bumps(74);
    let mut session = started_in(repo.root(), 120, 32);
    session.press("<Enter> j");

    session.press(&typing("/refund"));
    session.press("<Enter>");

    assert_eq!(status(&session), "No match for \"refund\" in the commits");
    assert!(
        focused(&session).starts_with("(x) Infrastructure"),
        "{}",
        session.text()
    );
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
fn the_palette_on_the_repository_picker_offers_the_search_and_runs_it() {
    let repo = with_bumps(10);
    let log = git_log(&repo);
    let mut session = started_in(repo.root(), 120, 32);
    session.press("<Enter> :");

    let rows = session.box_rows("Actions on Practice");
    for (index, (label, does)) in [
        ("> Back", "Return to the screen before"),
        ("Search", "Search the commits by message or ID"),
        (
            "Next match",
            "Go to the next commit the last search matched",
        ),
        (
            "Previous match",
            "Go to the previous commit the last search matched",
        ),
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
    session.press(&typing("counter to"));
    session.press("<Enter> : j j <Enter>");
    assert_eq!(focused(&session), log[1]);
}
