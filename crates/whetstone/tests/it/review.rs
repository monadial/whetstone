//! The outbound review: exactly what a Practice on the learner's repository
//! will send and where, opened from the picker's "Review what will be sent".

use crate::repo::TestRepo;
use crate::repository::started_in;
use crate::session::{Session, columns};

/// From home: open the repository's picker, choose the newest commit, and
/// open its outbound review, with the five default areas selected.
const REVIEW_NEWEST: &str = "<Enter> j j j j j <Space> k k k k k <S-Tab> <Enter>";

/// A repository called `payments` whose newest commit adds `src/retry.rs`.
fn with_a_retry() -> TestRepo {
    let repo = TestRepo::new("payments");
    repo.write("README.md", "# Payments\n");
    repo.commit("Start the payments service");
    repo.write("src/retry.rs", "pub fn retry() {}\n");
    repo.commit("Retry failed payments\n\nA card declined for a network error is tried again.");
    repo
}

/// The rows of the screen, trimmed, from the title down to the first
/// heading, `until`.
fn rows_until(session: &Session, until: &str) -> Vec<String> {
    session
        .rows()
        .iter()
        .map(|row| row.trim().to_owned())
        .skip_while(|row| row != "Review what will be sent")
        .take_while(|row| row != until)
        .collect()
}

/// `repo`'s newest commit's short ID.
fn short_id(repo: &TestRepo) -> String {
    repo.git(&["log", "-1", "--format=%h"]).trim().to_owned()
}

#[test]
fn the_review_names_the_destination_and_the_change_and_what_of_it_is_not_sent() {
    let repo = with_a_retry();
    let mut session = started_in(repo.root(), 120, 60);

    session.press(REVIEW_NEWEST);

    assert_eq!(
        rows_until(&session, "Files"),
        [
            "Review what will be sent".to_owned(),
            String::new(),
            "Sends to OpenRouter · Jev: typesafe/jev-1.14 · Generator: anthropic/claude-sonnet-5.5"
                .to_owned(),
            "OpenRouter routes only to endpoints it lists as not keeping prompts.".to_owned(),
            "https://openrouter.ai/docs/guides/privacy/data-collection".to_owned(),
            String::new(),
            "Your answers in this session go to the same places. Nothing outside this list is sent."
                .to_owned(),
            String::new(),
            "Change".to_owned(),
            format!("> {}  Retry failed payments", short_id(&repo)),
            String::new(),
            "A card declined for a network error is tried again.".to_owned(),
            "Author, committer, dates and the commit ID are not sent.".to_owned(),
            String::new(),
        ]
    );
}

/// A repository called `payments` whose newest commit touches a file of
/// every kind the rules treat apart: plain text, a lockfile, a common
/// secret path, a file its config leaves out, a binary file, a symbolic
/// link and a submodule, whose `.gitmodules` comes with it. The config is
/// in the working tree, not committed.
fn with_every_kind_of_file() -> TestRepo {
    let repo = TestRepo::new("payments");
    repo.write("README.md", "# Payments\n");
    repo.commit("Start the payments service");
    repo.write("src/retry.rs", "pub fn retry() {}\n")
        .write("Cargo.lock", "version = 4\n")
        .write(".env.example", "API_URL=http://localhost\n")
        .write("fixtures/cards.json", "[]\n")
        .write("docs/logo.png", b"PNG\0\x01\x02binary")
        .symlink("docs/readme", "../README.md")
        .add_submodule("vendor/ledger");
    repo.commit("Retry failed payments");
    repo.write(".whetstone/config.toml", "exclude = [\"fixtures/\"]\n");
    repo
}

/// The Files section's rows, split into their columns, without the focus
/// marker.
fn files(session: &Session) -> Vec<Vec<String>> {
    columns(&session.section("Files"))
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|column| column.trim_start_matches("> ").to_owned())
                .collect()
        })
        .collect()
}

/// Each file's status in the Files section.
fn statuses(session: &Session) -> Vec<String> {
    files(session)
        .into_iter()
        .map(|row| row.last().cloned().unwrap_or_default())
        .collect()
}

#[test]
fn every_touched_file_is_listed_with_its_size_and_whether_it_is_sent_or_why_not() {
    let repo = with_every_kind_of_file();
    let mut session = started_in(repo.root(), 120, 60);

    session.press(REVIEW_NEWEST);

    assert_eq!(
        files(&session),
        [
            [".env.example", "1 KB", "left out: common secret path"].as_slice(),
            &[".gitmodules", "1 KB", "included"],
            &["Cargo.lock", "1 KB", "left out: lockfile"],
            &["docs/logo.png", "1 KB", "left out: binary file"],
            &["docs/readme", "left out: symlink"],
            &[
                "fixtures/cards.json",
                "1 KB",
                "left out: exclude pattern fixtures/"
            ],
            &["src/retry.rs", "1 KB", "included"],
            &["vendor/ledger", "left out: submodule"],
        ]
    );
}

#[test]
fn space_brings_back_a_file_left_out_by_default_or_by_config_and_leaves_it_out_again() {
    let repo = with_every_kind_of_file();
    let mut session = started_in(repo.root(), 120, 60);
    session.press(REVIEW_NEWEST);

    // The first file, the third and the sixth.
    session.press("j <Space> j j <Space> j j j <Space>");

    let brought_back = statuses(&session);
    assert_eq!(
        [&brought_back[0], &brought_back[2], &brought_back[5]],
        ["included by you"; 3]
    );
    session.press("<Space> k k k <Space> k k <Space>");
    assert_eq!(
        statuses(&session)[0..6],
        [
            "left out: common secret path",
            "included",
            "left out: lockfile",
            "left out: binary file",
            "left out: symlink",
            "left out: exclude pattern fixtures/",
        ]
    );
}

#[test]
fn space_leaves_out_an_included_file_and_brings_it_back() {
    let repo = with_every_kind_of_file();
    let mut session = started_in(repo.root(), 120, 60);
    session.press(REVIEW_NEWEST);
    session.press("j j j j j j j");
    assert_eq!(files(&session)[6], ["src/retry.rs", "1 KB", "included"]);

    session.press("<Space>");
    assert_eq!(
        files(&session)[6],
        ["src/retry.rs", "1 KB", "left out by you"]
    );

    session.press("<Space>");
    assert_eq!(files(&session)[6], ["src/retry.rs", "1 KB", "included"]);
}

#[test]
fn binary_files_symlinks_and_submodules_cannot_be_brought_back() {
    let repo = with_every_kind_of_file();
    let mut session = started_in(repo.root(), 120, 60);
    session.press(REVIEW_NEWEST);
    let before = files(&session);

    session.press("j j j j <Space> j <Space> j j j <Space>");

    assert_eq!(files(&session), before);
}
