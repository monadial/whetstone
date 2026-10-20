//! Connected code in the real binary: at startup it removes analysis
//! directories a crash left behind, and a review's connected code is found
//! by a real background job without leaving its directory behind.
#![cfg(unix)]

use std::fs::{self, File};
use std::path::Path;
use std::time::{Duration, SystemTime};

use tempfile::TempDir;

use crate::pty::{LAUNCH, start};
use crate::repo::TestRepo;

/// The keys that open the outbound review of the newest commit from home,
/// with the five default areas selected, as a terminal sends them.
const REVIEW_NEWEST: &str = "\rjjjjj kkkkk\x1b[Z\r";

/// Makes the directory `name` in `dir`, last changed `age` ago.
fn directory(dir: &Path, name: &str, age: Duration) {
    let path = dir.join(name);
    fs::create_dir(&path).expect("make a directory");
    fs::write(path.join("main.rs"), "fn main() {}\n").expect("write a file in it");
    File::open(&path)
        .and_then(|file| file.set_modified(SystemTime::now() - age))
        .expect("date the directory");
}

#[test]
fn startup_removes_old_analysis_directories_and_a_review_leaves_none_of_its_own() {
    const DAY: Duration = Duration::from_hours(24);
    let repo = TestRepo::new("ops");
    repo.write("ops.py", "def sub(a, b):\n    return a - b\n")
        .write(
            "use_sub.py",
            "from ops import sub\n\n\ndef none_left(a):\n    return sub(a, a)\n",
        );
    repo.commit("Add sub");
    repo.write("ops.py", "def sub(a, b):\n    return b - a\n");
    repo.commit("Subtract the other way round");
    let temp = TempDir::new().expect("create a system temporary directory");
    directory(temp.path(), "whetstone-analysis-crashed", 2 * DAY);
    directory(temp.path(), "whetstone-analysis-running", Duration::ZERO);
    directory(temp.path(), "someone-elses", 2 * DAY);
    let dirs = TempDir::new().expect("create the config and state directories");

    // Started in the repository, which is its home, with its config and
    // state kept elsewhere.
    let mut session = start(
        LAUNCH,
        &[
            ("HOME", repo.root()),
            ("XDG_CONFIG_HOME", &dirs.path().join("config")),
            ("XDG_STATE_HOME", &dirs.path().join("state")),
            ("TMPDIR", temp.path()),
        ],
    );
    session.exp_string("Quit").expect("home screen is drawn");
    session.send(REVIEW_NEWEST).expect("open the review");
    session.flush().expect("flush the keys");
    // Ratatui moves the cursor between words, so wait for a single word of
    // "Connected code: 1 range · 1 KB".
    session
        .exp_string("range")
        .expect("connected code is found");
    session.send("\x03").expect("send Ctrl+C");
    session.flush().expect("flush Ctrl+C");
    session.exp_string("exit code: 0").expect("whetstone quits");

    let mut left: Vec<String> = fs::read_dir(temp.path())
        .expect("read the temporary directory")
        .map(|entry| {
            entry
                .expect("read an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    left.sort();
    assert_eq!(left, ["someone-elses", "whetstone-analysis-running"]);
}
