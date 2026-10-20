//! The graph cache: connected code for a commit whose graph was found
//! before comes from the cache, marked "from cache", without writing or
//! analysing anything, and a damaged or unwritable cache never stops
//! connected code from being found.

use std::fs::{self, File};
use std::io::Read as _;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use crate::connected_code::{
    REVIEW_NEWEST, connected_code, connected_rows, logged, rust_pay, rust_payments,
};
use crate::repo::TestRepo;
use crate::repository::started_in;
use crate::session::{Session, Setup};
use whetstone::Flow;

/// [`rust_payments`] with a commit that changes `Payments::charge`.
fn double_the_charge() -> TestRepo {
    let repo = rust_payments();
    repo.write("src/pay.rs", rust_pay("validate(amount * 2)"));
    repo.commit("Double the charge");
    repo
}

/// The connected code of [`double_the_charge`]'s commit.
const CHARGE_CONNECTED: [[&str; 5]; 2] = [
    [
        "src/check.rs",
        "lines 1–3",
        "1 KB",
        "included",
        "called by `charge`",
    ],
    [
        "src/shop.rs",
        "lines 3–5",
        "1 KB",
        "included",
        "calls `charge`",
    ],
];

/// Starts the app without colour in `repo`, in a 120 by 60 terminal, with
/// `cache_home` as `XDG_CACHE_HOME` and `temp` as the system temporary
/// directory.
fn started_with(repo: &TestRepo, cache_home: &Path, temp: &Path) -> Session {
    Session::start(
        Setup {
            variables: &[
                ("NO_COLOR", "1"),
                ("XDG_CACHE_HOME", cache_home.to_str().expect("a UTF-8 path")),
                ("TMPDIR", temp.to_str().expect("a UTF-8 path")),
            ],
            start: Some(repo.root()),
            ..Setup::default()
        },
        120,
        60,
    )
}

/// Reviews `repo`'s newest commit in a session of its own, with
/// `cache_home` as `XDG_CACHE_HOME`, until its connected code is found.
fn review_newest(repo: &TestRepo, cache_home: &Path) {
    let temp = TempDir::new().expect("create a system temporary directory");
    let mut session = started_with(repo, cache_home, temp.path());
    session.press(REVIEW_NEWEST);
    session.settle();
}

/// The files in the graph cache under `cache_home`: its `graphs` folder in
/// Whetstone's cache folder.
fn graphs(cache_home: &Path) -> Vec<PathBuf> {
    let mut graphs: Vec<PathBuf> = fs::read_dir(cache_home.join("whetstone/graphs"))
        .expect("read the graphs folder")
        .map(|entry| entry.expect("a graphs folder entry").path())
        .collect();
    graphs.sort();
    graphs
}

#[test]
fn a_commit_reviewed_again_takes_its_connected_code_from_the_cache() {
    let repo = double_the_charge();
    let mut session = started_in(repo.root(), 120, 60);
    session.press(REVIEW_NEWEST);
    session.settle();
    assert_eq!(
        connected_code(&session)[0],
        "Connected code: 2 ranges · 1 KB of 100 KB limit"
    );

    // Back to the picker, and review the same commit again.
    session.press("q <Enter>");
    assert_eq!(connected_code(&session), ["Connected code: finding…"]);
    session.settle();

    assert_eq!(
        connected_code(&session)[0],
        "Connected code: 2 ranges · 1 KB of 100 KB limit · from cache"
    );
    assert_eq!(connected_rows(&session), CHARGE_CONNECTED);
}

#[test]
fn practising_on_a_commit_again_writes_nothing_for_analysis_and_analyses_nothing() {
    let repo = double_the_charge();
    let cache_home = TempDir::new().expect("create a cache home");
    review_newest(&repo, cache_home.path());
    // A system temporary directory that does not exist, so writing the
    // commit's files there for analysis would fail.
    let temp = TempDir::new().expect("create a system temporary directory");
    let nowhere = temp.path().join("nowhere");

    let mut session = started_with(&repo, cache_home.path(), &nowhere);
    session.press(REVIEW_NEWEST);
    session.settle();

    assert_eq!(
        connected_code(&session)[0],
        "Connected code: 2 ranges · 1 KB of 100 KB limit · from cache"
    );
    assert_eq!(connected_rows(&session), CHARGE_CONNECTED);
    assert!(!nowhere.exists());
}

#[test]
fn a_damaged_cache_file_is_analysed_again_and_rebuilt() {
    let repo = double_the_charge();
    let cache_home = TempDir::new().expect("create a cache home");
    review_newest(&repo, cache_home.path());
    let [graph] = graphs(cache_home.path())
        .try_into()
        .expect("one cached graph");
    fs::write(&graph, "not a graph").expect("damage the cached graph");
    let temp = TempDir::new().expect("create a system temporary directory");
    let mut session = started_with(&repo, cache_home.path(), temp.path());

    session.press(REVIEW_NEWEST);
    session.settle();

    assert_eq!(
        connected_code(&session)[0],
        "Connected code: 2 ranges · 1 KB of 100 KB limit"
    );
    assert_eq!(connected_rows(&session), CHARGE_CONNECTED);
    assert_eq!(graphs(cache_home.path()), std::slice::from_ref(&graph));
    assert_ne!(
        fs::read(&graph).expect("read the rebuilt graph"),
        b"not a graph"
    );
    session.press("q <Enter>");
    session.settle();
    assert_eq!(
        connected_code(&session)[0],
        "Connected code: 2 ranges · 1 KB of 100 KB limit · from cache"
    );
}

#[test]
fn an_unwritable_cache_folder_still_gives_connected_code_and_is_logged_once() {
    let repo = double_the_charge();
    let cache_home = TempDir::new().expect("create a cache home");
    // A file where Whetstone's cache folder should be, which stops even a
    // privileged user from writing there.
    fs::write(cache_home.path().join("whetstone"), "").expect("write a file there");
    let temp = TempDir::new().expect("create a system temporary directory");

    let log = logged(|| {
        let mut session = started_with(&repo, cache_home.path(), temp.path());
        session.press(REVIEW_NEWEST);
        session.settle();
        assert_eq!(
            connected_code(&session)[0],
            "Connected code: 2 ranges · 1 KB of 100 KB limit"
        );
        assert_eq!(connected_rows(&session), CHARGE_CONNECTED);

        // Reviewed again, it is analysed again.
        session.press("q <Enter>");
        session.settle();
        assert_eq!(
            connected_code(&session)[0],
            "Connected code: 2 ranges · 1 KB of 100 KB limit"
        );
        assert_eq!(connected_rows(&session), CHARGE_CONNECTED);
    });

    assert_eq!(
        log.matches("cannot write to the cache folder").count(),
        1,
        "{log}"
    );
}

#[test]
fn after_a_session_the_cache_folder_holds_one_graph_and_nothing_else_is_left_or_logged() {
    let repo = double_the_charge();
    let before = repo.on_disk();
    let cache_home = TempDir::new().expect("create a cache home");
    let temp = TempDir::new().expect("create a system temporary directory");

    let log = logged(|| {
        let mut session = started_with(&repo, cache_home.path(), temp.path());
        session.press(REVIEW_NEWEST);
        session.settle();
        // Read the first piece of connected code, then review the commit
        // again, from the cache, leave that piece out, and quit while the
        // commit's connected code is found a third time.
        session.press("j j <Enter> q q <Enter>");
        session.settle();
        assert_eq!(
            connected_code(&session)[0],
            "Connected code: 2 ranges · 1 KB of 100 KB limit · from cache"
        );
        session.press("j j <Space> q <Enter>");
        assert_eq!(session.press("<C-c>"), Flow::Quit);
    });

    repo.assert_untouched(&before);
    assert_eq!(graphs(cache_home.path()).len(), 1);
    let left: Vec<_> = fs::read_dir(temp.path())
        .expect("read the temporary directory")
        .collect();
    assert!(left.is_empty(), "left in the temporary directory: {left:?}");
    assert_eq!(
        log.matches("loaded a cached graph").count(),
        2,
        "a hit each time the review opens again: {log}"
    );
    for unlogged in [
        "charge",
        "validate",
        "checkout",
        "src/check.rs",
        "src/shop.rs",
        "graphs",
        ".json",
    ] {
        assert!(!log.contains(unlogged), "{unlogged:?} in the log:\n{log}");
    }
}

#[test]
fn what_a_change_links_to_and_names_is_read_from_the_commit_on_a_cache_hit_too() {
    let repo = rust_payments();
    repo.write("docs/payments.md", "# Payments\n")
        .write("config/limits.toml", "max = 100\n");
    repo.commit("Document the payments");
    repo.write("src/pay.rs", rust_pay("validate(amount * 2)"))
        .write(
            "README.md",
            "# Shop\n\nSee [the payments](docs/payments.md) and config/limits.toml.\n",
        );
    repo.commit("Double the charge and say where it is documented");
    let mut session = started_in(repo.root(), 120, 60);
    session.press(REVIEW_NEWEST);
    session.settle();

    session.press("q <Enter>");
    session.settle();

    assert_eq!(
        connected_code(&session)[0],
        "Connected code: 2 ranges and 2 files · 1 KB of 100 KB limit · from cache"
    );
    assert_eq!(
        connected_rows(&session),
        [
            CHARGE_CONNECTED[0],
            CHARGE_CONNECTED[1],
            [
                "docs/payments.md",
                "whole file",
                "1 KB",
                "included",
                "linked from `README.md`"
            ],
            [
                "config/limits.toml",
                "whole file",
                "1 KB",
                "included",
                "named in `README.md`"
            ],
        ]
    );
}

/// The outbound review of [`double_the_charge`]'s commit, reviewed a second
/// time in a `width` by `height` terminal, with its connected code from
/// the cache.
fn reviewed_again(width: u16, height: u16) -> Session {
    let repo = double_the_charge();
    let mut session = started_in(repo.root(), width, height);
    session.press(REVIEW_NEWEST);
    session.settle();
    session.press("q <Enter>");
    session.settle();
    session
}

#[test]
fn connected_code_from_the_cache_in_the_review_at_120x32() {
    let session = reviewed_again(120, 32);

    insta::assert_debug_snapshot!("connected_code_from_cache_120x32", session.frame());
}

#[test]
fn connected_code_from_the_cache_in_the_review_at_80x24() {
    let mut session = reviewed_again(80, 24);
    // Down to the first piece of connected code, past the touched file, so
    // that the summary is on the screen below the stage row.
    session.press("j j");

    insta::assert_debug_snapshot!("connected_code_from_cache_80x24", session.frame());
}

#[test]
fn a_cached_graph_is_compressed_json_without_the_commits_file_contents() {
    let repo = double_the_charge();
    let cache_home = TempDir::new().expect("create a cache home");
    review_newest(&repo, cache_home.path());
    let [graph] = graphs(cache_home.path())
        .try_into()
        .expect("one cached graph");

    let mut json = String::new();
    flate2::read::GzDecoder::new(File::open(&graph).expect("open the cached graph"))
        .read_to_string(&mut json)
        .expect("a compressed graph");

    serde_json::from_str::<serde_json::Value>(&json).expect("the graph is JSON");
    for contents in [
        "amount: u32",
        "-> bool",
        "amount > 0",
        "amount * 2",
        "payments.charge(10)",
        "pub struct Payments",
        "use crate::",
    ] {
        assert!(!json.contains(contents), "{contents:?} is in the graph");
    }
}
