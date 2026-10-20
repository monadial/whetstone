//! The graph cache in the real binary: a review's graph is cached in the
//! learner's cache folder, and practising on the same commit in a later run
//! takes its connected code from there.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;

use tempfile::TempDir;

use crate::pty::start;
use crate::pty_connected_code::REVIEW_NEWEST;
use crate::repo::TestRepo;

/// Runs `whetstone` in the foreground in a 120×32 terminal, where the
/// review's Connected code section is on the screen below the stage row.
const LAUNCH_AT_120_BY_32: &str = r#"stty rows 32 cols 120; "$WHETSTONE""#;

/// Runs Whetstone in `repo`, with its config, state and cache in `dirs`,
/// opens the newest commit's review, waits for `word` in its connected
/// code, and quits.
fn review_newest(repo: &TestRepo, dirs: &Path, word: &str) {
    let temp = TempDir::new().expect("create a system temporary directory");
    // Started in the repository, which is its home.
    let mut session = start(
        LAUNCH_AT_120_BY_32,
        &[
            ("HOME", repo.root()),
            ("XDG_CONFIG_HOME", &dirs.join("config")),
            ("XDG_STATE_HOME", &dirs.join("state")),
            ("XDG_CACHE_HOME", &dirs.join("cache")),
            ("TMPDIR", temp.path()),
        ],
    );
    session.exp_string("Quit").expect("home screen is drawn");
    session.send(REVIEW_NEWEST).expect("open the review");
    session.flush().expect("flush the keys");
    // Ratatui moves the cursor between words, so wait for a single word.
    session.exp_string(word).expect("connected code is found");
    session.send("\x03").expect("send Ctrl+C");
    session.flush().expect("flush Ctrl+C");
    session.exp_string("exit code: 0").expect("whetstone quits");
}

#[test]
fn a_reviews_graph_is_cached_in_the_cache_folder_and_found_there_by_a_later_run() {
    let repo = TestRepo::new("ops");
    repo.write("ops.py", "def sub(a, b):\n    return a - b\n")
        .write(
            "use_sub.py",
            "from ops import sub\n\n\ndef none_left(a):\n    return sub(a, a)\n",
        );
    repo.commit("Add sub");
    repo.write("ops.py", "def sub(a, b):\n    return b - a\n");
    repo.commit("Subtract the other way round");
    let dirs = TempDir::new().expect("create the config, state and cache directories");
    let graphs = dirs.path().join("cache/whetstone/graphs");

    // "Connected code: 1 range · 1 KB of 100 KB limit"
    review_newest(&repo, dirs.path(), "range");

    let cached = fs::read_dir(&graphs)
        .expect("read the graphs folder")
        .count();
    assert_eq!(cached, 1);
    let mode = fs::metadata(&graphs)
        .expect("the graphs folder's metadata")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700);

    // "Connected code: 1 range · 1 KB of 100 KB limit · from cache"
    review_newest(&repo, dirs.path(), "cache");

    let cached = fs::read_dir(&graphs)
        .expect("read the graphs folder")
        .count();
    assert_eq!(cached, 1);
}
