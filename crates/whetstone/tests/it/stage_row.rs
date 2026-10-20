//! The row of stages at the top of the outbound review: where Practice is
//! and what comes next, from reading the change to Jev reviewing the
//! questions. Finding connected code is current until its result arrives,
//! from an analysis or from the cache, and its spinner moves only on a tick
//! while it is found.

use tempfile::TempDir;

use crate::repo::TestRepo;
use crate::repository::started_in;
use crate::session::{Session, Setup};

/// From home: open the repository's picker, choose the newest commit, and
/// open its outbound review, with the five default areas selected.
const REVIEW_NEWEST: &str = "<Enter> j j j j j <Space> k k k k k <S-Tab> <Enter>";
/// The outbound review's title, which the row sits above.
const TITLE: &str = "Review what will be sent";

/// The stage row: the rows above the outbound review's title, trimmed, without the
/// blank ones.
fn stage_row(session: &Session) -> Vec<String> {
    let rows = session.rows();
    let title = rows.iter().position(|row| row.trim() == TITLE);
    assert!(title.is_some(), "no outbound review:\n{}", session.text());
    rows.iter()
        .take(title.unwrap_or_default())
        .map(|row| row.trim().to_owned())
        .filter(|row| !row.is_empty())
        .collect()
}

/// A Python repository in which `report` calls `total`, and a commit that
/// changes `total`.
fn with_a_caller() -> TestRepo {
    let repo = TestRepo::new("ledger");
    repo.write("ledger.py", "def total(items):\n    return sum(items)\n")
        .write(
            "report.py",
            "from ledger import total\n\n\ndef report(items):\n    return total(items)\n",
        );
    repo.commit("Start the ledger");
    repo.write("ledger.py", "def total(items):\n    return sum(items, 0)\n");
    repo.commit("Count from zero");
    repo
}

#[test]
fn the_outbound_review_opens_with_the_change_read_and_connected_code_being_found() {
    let repo = with_a_caller();
    let mut session = started_in(repo.root(), 120, 32);

    session.press(REVIEW_NEWEST);

    assert_eq!(
        stage_row(&session),
        [
            "✓ Reading the change → ⠋ Finding connected code → · Preparing evidence → · Outbound review →",
            "· Checking areas · Jev → · Writing questions · generator → · Reviewing questions · Jev",
        ]
    );
}

#[test]
fn once_connected_code_arrives_the_evidence_is_prepared_and_the_outbound_review_is_current() {
    let repo = with_a_caller();
    let mut session = started_in(repo.root(), 120, 32);
    session.press(REVIEW_NEWEST);

    session.settle();

    assert_eq!(
        session.row_with("Connected code"),
        "Connected code: 1 range · 1 KB of 100 KB limit"
    );
    assert_eq!(
        stage_row(&session),
        [
            "✓ Reading the change → ✓ Finding connected code → ✓ Preparing evidence → ⠋ Outbound review →",
            "· Checking areas · Jev → · Writing questions · generator → · Reviewing questions · Jev",
        ]
    );
}

#[test]
fn finding_no_connected_code_still_finishes_finding_it() {
    let repo = TestRepo::new("notes");
    repo.write("notes.py", "def note():\n    return 1\n");
    repo.commit("Start the notes");
    repo.write("notes.py", "def note():\n    return 2\n");
    repo.commit("Note two");
    let mut session = started_in(repo.root(), 120, 32);
    session.press(REVIEW_NEWEST);

    session.settle();

    assert_eq!(
        session.row_with("Connected code"),
        "Connected code: none found for this change."
    );
    assert_eq!(
        stage_row(&session)[0],
        "✓ Reading the change → ✓ Finding connected code → ✓ Preparing evidence → ⠋ Outbound review →"
    );
}

#[test]
fn at_80x24_the_row_wraps_between_stages_never_inside_one() {
    let repo = with_a_caller();
    let mut session = started_in(repo.root(), 80, 24);
    session.press(REVIEW_NEWEST);

    session.settle();

    assert_eq!(
        stage_row(&session),
        [
            "✓ Reading the change → ✓ Finding connected code → ✓ Preparing evidence →",
            "⠋ Outbound review → · Checking areas · Jev →",
            "· Writing questions · generator → · Reviewing questions · Jev",
        ]
    );
}

#[test]
fn connected_code_from_the_cache_still_finishes_finding_it() {
    let repo = with_a_caller();
    let mut session = started_in(repo.root(), 120, 32);
    session.press(REVIEW_NEWEST);
    session.settle();

    // Back to the picker, and review the same commit again.
    session.press("q <Enter>");
    session.settle();

    assert_eq!(
        session.row_with("Connected code"),
        "Connected code: 1 range · 1 KB of 100 KB limit · from cache"
    );
    assert_eq!(
        stage_row(&session),
        [
            "✓ Reading the change → ✓ Finding connected code → ✓ Preparing evidence → ⠋ Outbound review →",
            "· Checking areas · Jev → · Writing questions · generator → · Reviewing questions · Jev",
        ]
    );
}

/// A system temporary directory inside a `build` directory, where the
/// engine never reads, so nothing written there can be analysed: the
/// directory that holds it, and its path for `TMPDIR`.
fn where_the_analysis_fails() -> (TempDir, String) {
    let temp = TempDir::new().expect("create a temporary directory");
    let build = temp.path().join("build");
    std::fs::create_dir(&build).expect("create a build directory");
    let build = build.to_str().expect("a UTF-8 path").to_owned();
    (temp, build)
}

#[test]
fn when_the_analysis_fails_finding_connected_code_is_skipped() {
    let repo = with_a_caller();
    let (_temp, build) = where_the_analysis_fails();
    let mut session = Session::start(
        Setup {
            variables: &[("NO_COLOR", "1"), ("TMPDIR", &build)],
            start: Some(repo.root()),
            ..Setup::default()
        },
        120,
        32,
    );
    session.press(REVIEW_NEWEST);

    session.settle();

    assert_eq!(
        session.row_with("Connected code"),
        "Connected code isn't available for this change: the analysis failed."
    );
    assert_eq!(
        stage_row(&session)[0],
        "✓ Reading the change → – Finding connected code → ✓ Preparing evidence → ⠋ Outbound review →"
    );
}

#[test]
fn the_spinner_moves_on_each_tick_while_connected_code_is_found_and_stands_still_once_it_arrives() {
    let repo = with_a_caller();
    let mut session = started_in(repo.root(), 120, 32);
    session.press(REVIEW_NEWEST);
    let first = |session: &Session| stage_row(session).remove(0);
    assert_eq!(
        first(&session),
        "✓ Reading the change → ⠋ Finding connected code → · Preparing evidence → · Outbound review →"
    );

    session.tick();
    assert_eq!(
        first(&session),
        "✓ Reading the change → ⠙ Finding connected code → · Preparing evidence → · Outbound review →"
    );
    // Drawn again without a tick, it has not moved.
    assert_eq!(
        first(&session),
        "✓ Reading the change → ⠙ Finding connected code → · Preparing evidence → · Outbound review →"
    );
    session.tick();
    assert_eq!(
        first(&session),
        "✓ Reading the change → ⠹ Finding connected code → · Preparing evidence → · Outbound review →"
    );

    session.settle();
    let arrived = session.frame();
    assert_eq!(
        first(&session),
        "✓ Reading the change → ✓ Finding connected code → ✓ Preparing evidence → ⠋ Outbound review →"
    );
    session.tick();
    session.tick();
    assert_eq!(session.frame(), arrived, "the spinner stands still");
}

/// How far finding connected code has got in a snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Finding {
    /// Still being found.
    InProgress,
    /// Found.
    Done,
    /// Found in the cache, by reviewing the commit a second time.
    FromCache,
    /// Not available, because the analysis failed.
    AnalysisFailed,
}

/// Snapshots the outbound review of [`with_a_caller`]'s newest commit, once finding
/// its connected code has got as far as `finding`, at 120×32 and 80×24, in
/// colour and under `NO_COLOR`.
fn assert_snapshots(name: &str, finding: Finding) {
    let repo = with_a_caller();
    let (_temp, build) = where_the_analysis_fails();
    let failing = [("TMPDIR", build.as_str())];
    for (width, height) in [(120, 32), (80, 24)] {
        for (colour, colours) in [
            ("in_colour", ("COLORTERM", "truecolor")),
            ("no_color", ("NO_COLOR", "1")),
        ] {
            let variables: Vec<_> = [colours]
                .into_iter()
                .chain(
                    failing
                        .into_iter()
                        .filter(|_| finding == Finding::AnalysisFailed),
                )
                .collect();
            let mut session = Session::start(
                Setup {
                    variables: &variables,
                    start: Some(repo.root()),
                    ..Setup::default()
                },
                width,
                height,
            );
            session.press(REVIEW_NEWEST);
            if finding != Finding::InProgress {
                session.settle();
            }
            if finding == Finding::FromCache {
                session.press("q <Enter>");
                session.settle();
            }
            insta::assert_debug_snapshot!(
                format!("{name}_{colour}_{width}x{height}"),
                session.frame()
            );
        }
    }
}

#[test]
fn the_row_while_connected_code_is_found() {
    assert_snapshots("in_progress", Finding::InProgress);
}

#[test]
fn the_row_once_connected_code_is_found() {
    assert_snapshots("done", Finding::Done);
}

#[test]
fn the_row_once_connected_code_comes_from_the_cache() {
    assert_snapshots("from_cache", Finding::FromCache);
}

#[test]
fn the_row_when_the_analysis_fails() {
    assert_snapshots("analysis_failed", Finding::AnalysisFailed);
}
