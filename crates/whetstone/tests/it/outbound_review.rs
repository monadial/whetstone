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

#[test]
fn a_summary_too_long_for_the_row_wraps_so_the_whole_message_shows() {
    const SUMMARY: &str = "Retry payments that a card network declined for a temporary reason, \
                           at most three times";
    let repo = TestRepo::new("payments");
    repo.write("README.md", "# Payments\n");
    repo.commit("Start the payments service");
    repo.write("src/retry.rs", "pub fn retry() {}\n");
    repo.commit(SUMMARY);
    let mut session = started_in(repo.root(), 80, 40);

    session.press(REVIEW_NEWEST);

    let change = session.section("Change");
    let (not_sent, summary) = change.split_last().expect("the change has rows");
    assert_eq!(
        not_sent,
        "Author, committer, dates and the commit ID are not sent."
    );
    assert!(summary.len() > 1, "the summary wraps:\n{}", session.text());
    assert_eq!(
        summary.join(" "),
        format!("> {}  {SUMMARY}", short_id(&repo))
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
            [".env.example", "1 KB", "left out: possible secret path"].as_slice(),
            &[".gitmodules", "1 KB", "included"],
            &["Cargo.lock", "1 KB", "left out: lockfile"],
            &["docs/logo.png", "1 KB", "left out: binary file"],
            &["docs/readme", "–", "left out: symlink"],
            &[
                "fixtures/cards.json",
                "1 KB",
                "left out by repository config: fixtures/"
            ],
            &["src/retry.rs", "1 KB", "included"],
            &["vendor/ledger", "–", "left out: submodule"],
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
            "left out: possible secret path",
            "included",
            "left out: lockfile",
            "left out: binary file",
            "left out: symlink",
            "left out by repository config: fixtures/",
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

#[test]
fn a_binary_file_the_change_edits_shows_its_size_after_the_change() {
    let repo = TestRepo::new("payments");
    repo.write("docs/logo.png", b"PNG\0small");
    repo.commit("Add the logo");
    repo.write("docs/logo.png", [b"PNG\0".as_slice(), &[7; 4996]].concat());
    repo.commit("Draw the logo larger");
    let mut session = started_in(repo.root(), 120, 60);

    session.press(REVIEW_NEWEST);

    assert_eq!(
        files(&session),
        [["docs/logo.png", "5 KB", "left out: binary file"]]
    );
}

#[test]
fn a_file_renamed_away_from_a_path_left_out_is_left_out_for_its_old_path() {
    let repo = TestRepo::new("payments");
    repo.write(".env", "API_URL=http://localhost\nRETRIES=3\nTIMEOUT=30\n")
        .write("fixtures/cards.json", "[\"4242\", \"4000\", \"5555\"]\n");
    repo.commit("Start the payments service");
    repo.git(&["mv", ".env", "settings.txt"]);
    repo.git(&["mv", "fixtures/cards.json", "cards.json"]);
    repo.commit("Move the settings and the cards");
    repo.write(".whetstone/config.toml", "exclude = [\"fixtures/\"]\n");
    let mut session = started_in(repo.root(), 120, 60);

    session.press(REVIEW_NEWEST);

    assert_eq!(
        files(&session),
        [
            [
                "cards.json",
                "← fixtures/cards.json",
                "1 KB",
                "left out by repository config: fixtures/"
            ],
            [
                "settings.txt",
                "← .env",
                "1 KB",
                "left out: possible secret path"
            ],
        ]
    );
}

#[test]
fn a_config_pattern_never_replaces_the_default_reason_a_file_is_left_out() {
    let repo = TestRepo::new("payments");
    repo.write("README.md", "# Payments\n");
    repo.commit("Start the payments service");
    repo.write(".env.example", "API_URL=http://localhost\n");
    repo.commit("Document the settings");
    repo.write(".whetstone/config.toml", "exclude = [\".env*\"]\n");
    let mut session = started_in(repo.root(), 120, 60);
    session.press(REVIEW_NEWEST);
    assert_eq!(statuses(&session), ["left out: possible secret path"]);

    session.press("j <Space>");

    assert_eq!(statuses(&session), ["included by you"]);
}

/// A fake AWS access key ID, built from pieces so that no secret scanner
/// finds a whole one in the source.
const AWS_KEY: &str = concat!("AKIA", "FAKEFAKEFAKEFAKE");
/// A fake GitHub token, built the same way.
const GITHUB_TOKEN: &str = concat!("ghp_", "FAKEfakeFAKEfakeFAKEfakeFAKEfakeFAKE");

/// A repository called `payments` whose newest commit adds an AWS key to
/// `src/settings.py` and a GitHub token to `.env`, names the token in its
/// message, and edits the readme.
fn with_secrets() -> TestRepo {
    let repo = TestRepo::new("payments");
    repo.write("README.md", "# Payments\n")
        .write("src/settings.py", "REGION = \"eu-west-1\"\n");
    repo.commit("Start the payments service");
    repo.write("README.md", "# Payments\n\nTake payments.\n")
        .write(
            "src/settings.py",
            format!("REGION = \"eu-west-1\"\nKEY = \"{AWS_KEY}\"\n"),
        )
        .write(".env", format!("GITHUB_TOKEN={GITHUB_TOKEN}\n"));
    repo.commit(&format!(
        "Configure the payments client\n\nIt uses the token {GITHUB_TOKEN} for now."
    ));
    repo
}

/// The row of the Send control, trimmed, without its focus marker.
fn send(session: &Session) -> String {
    let rows = session.rows();
    let row = rows
        .iter()
        .map(|row| row.trim().trim_start_matches("> "))
        .find(|row| row.starts_with("Send") && !row.starts_with("Sends"));
    assert!(row.is_some(), "no Send row:\n{}", session.text());
    row.unwrap_or_default().to_owned()
}

#[test]
fn the_commit_message_and_every_included_file_are_checked_for_secrets() {
    let repo = with_secrets();
    let mut session = started_in(repo.root(), 120, 60);

    session.press(REVIEW_NEWEST);

    assert_eq!(
        session.section("Secret check"),
        [
            "Possible secret in the commit message, line 3: GitHub token, ghp_********",
            "( ) Not a secret",
            "Possible secret in src/settings.py, line 2: AWS access key ID, AKIA********",
            "Leave out the file",
            "( ) Not a secret",
            "Whetstone checks for common secret formats and can't guarantee there are none.",
        ]
    );
    assert_eq!(send(&session), "Send (resolve the possible secrets)");
}

#[test]
fn leaving_out_the_file_or_marking_the_match_as_not_a_secret_resolves_a_finding() {
    let repo = with_secrets();
    let mut session = started_in(repo.root(), 120, 60);
    session.press(REVIEW_NEWEST);

    // From the change round to Back, then up to the file's first action.
    session.press("<S-Tab> k k k");
    assert_eq!(
        session.row_with("Leave out the file"),
        "> Leave out the file"
    );
    session.press("<Space>");

    assert_eq!(statuses(&session)[2], "left out by you");
    assert_eq!(
        session.section("Secret check"),
        [
            "Possible secret in the commit message, line 3: GitHub token, ghp_********",
            "( ) Not a secret",
            "Whetstone checks for common secret formats and can't guarantee there are none.",
        ]
    );
    assert_eq!(send(&session), "Send (resolve the possible secrets)");

    session.press("k <Space>");
    assert_eq!(session.section("Secret check")[1], "> (x) Not a secret");
    assert_eq!(send(&session), "Send (arrives with Jev)");

    session.press("<Space>");
    assert_eq!(session.section("Secret check")[1], "> ( ) Not a secret");
    assert_eq!(send(&session), "Send (resolve the possible secrets)");
}

#[test]
fn not_a_secret_dismisses_only_the_match_it_marks() {
    let repo = with_secrets();
    let mut session = started_in(repo.root(), 120, 60);
    session.press(REVIEW_NEWEST);

    // From the change round to Back, then up to the commit message's
    // finding.
    session.press("<S-Tab> k k k k <Space>");

    assert_eq!(
        session.section("Secret check"),
        [
            "Possible secret in the commit message, line 3: GitHub token, ghp_********",
            "> (x) Not a secret",
            "Possible secret in src/settings.py, line 2: AWS access key ID, AKIA********",
            "Leave out the file",
            "( ) Not a secret",
            "Whetstone checks for common secret formats and can't guarantee there are none.",
        ]
    );
    assert_eq!(send(&session), "Send (resolve the possible secrets)");
}

#[test]
fn two_matches_of_one_rule_on_one_line_are_marked_as_not_secrets_apart() {
    let other_key = concat!("AKIA", "EKAFEKAFEKAFEKAF");
    let repo = TestRepo::new("payments");
    repo.write("README.md", "# Payments\n");
    repo.commit("Start the payments service");
    repo.write(
        "src/settings.py",
        format!("KEYS = [\"{AWS_KEY}\", \"{other_key}\"]\n"),
    );
    repo.commit("Configure the payments client");
    let mut session = started_in(repo.root(), 120, 60);
    session.press(REVIEW_NEWEST);
    let finding = "Possible secret in src/settings.py, line 1: AWS access key ID, AKIA********";

    // From the change round to Back, then up past Send and the second
    // match's actions to the first match's Not a secret.
    session.press("<S-Tab> k k k k <Space>");

    assert_eq!(
        session.section("Secret check"),
        [
            finding,
            "Leave out the file",
            "> (x) Not a secret",
            finding,
            "Leave out the file",
            "( ) Not a secret",
            "Whetstone checks for common secret formats and can't guarantee there are none.",
        ]
    );
    assert_eq!(send(&session), "Send (resolve the possible secrets)");
}

#[test]
fn a_finding_in_the_commit_message_can_only_be_marked_as_not_a_secret() {
    let repo = TestRepo::new("payments");
    repo.write("README.md", "# Payments\n");
    repo.commit(&format!("Start the payments service with {AWS_KEY}"));
    let mut session = started_in(repo.root(), 120, 60);
    session.press("<Enter> j j j j j <Space> k k k k k <S-Tab> <Enter>");

    assert_eq!(
        session.section("Secret check"),
        [
            "Possible secret in the commit message, line 1: AWS access key ID, AKIA********",
            "( ) Not a secret",
            "Whetstone checks for common secret formats and can't guarantee there are none.",
        ]
    );
}

#[test]
fn a_file_brought_back_is_checked_for_secrets_as_it_returns() {
    let repo = with_secrets();
    let mut session = started_in(repo.root(), 120, 60);
    session.press(REVIEW_NEWEST);
    assert_eq!(
        files(&session)[0],
        [".env", "1 KB", "left out: possible secret path"]
    );

    session.press("j <Space>");

    assert_eq!(statuses(&session)[0], "included by you");
    assert_eq!(
        session.section("Secret check")[2..5],
        [
            "Possible secret in .env, line 1: GitHub token, ghp_********",
            "Leave out the file",
            "( ) Not a secret",
        ]
    );
}

#[test]
fn a_secret_the_change_removes_is_found_in_the_diff_it_sends() {
    let repo = with_secrets();
    repo.write("src/settings.py", "REGION = \"eu-west-1\"\n");
    repo.commit("Read the key from the environment");
    let mut session = started_in(repo.root(), 120, 60);

    session.press(REVIEW_NEWEST);

    assert_eq!(
        session.section("Secret check")[0],
        "Possible secret in src/settings.py, diff line 7: AWS access key ID, AKIA********"
    );
}

/// `count` presses of `key`, separated by spaces.
fn times(key: &str, count: usize) -> String {
    vec![key; count].join(" ")
}

/// A repository called `payments` whose newest commit adds a text file of
/// more than 400 KB and an AWS key to `src/settings.py`.
fn with_a_large_file_and_a_secret() -> TestRepo {
    let repo = TestRepo::new("payments");
    repo.write("README.md", "# Payments\n");
    repo.commit("Start the payments service");
    repo.write("data/cards.txt", "card\n".repeat(90_000))
        .write("src/settings.py", format!("KEY = \"{AWS_KEY}\"\n"));
    repo.commit("Load the test cards");
    repo
}

#[test]
fn over_the_limit_the_review_says_the_change_is_too_large_until_files_are_left_out() {
    let repo = with_a_large_file_and_a_secret();
    let mut session = started_in(repo.root(), 120, 60);
    session.press(REVIEW_NEWEST);
    let size = session.section("Size");
    assert_eq!(size[1], "This change is too large to practise on.");
    let kilobytes: usize = size[0]
        .strip_suffix(" KB of 400 KB")
        .and_then(|kilobytes| kilobytes.parse().ok())
        .unwrap_or_default();
    assert!(kilobytes > 900, "{size:?}");

    session.press("j <Space>");

    let size = session.section("Size");
    assert_eq!(size.len(), 1, "{size:?}");
    assert!(size[0].ends_with(" KB of 400 KB"), "{size:?}");
    assert_eq!(files(&session)[0][2], "left out by you");
}

#[test]
fn a_file_brought_back_counts_towards_the_size() {
    // A file left out by default, and one left out by the repository's
    // config, each of 300 KB.
    for (path, config) in [
        ("Cargo.lock", None),
        ("fixtures/cards.txt", Some("exclude = [\"fixtures/\"]\n")),
    ] {
        let repo = TestRepo::new("payments");
        repo.write("README.md", "# Payments\n");
        repo.commit("Start the payments service");
        repo.write("src/retry.rs", "pub fn retry() {}\n")
            .write(path, "[[package]]\n".repeat(25_000));
        repo.commit("Retry failed payments");
        if let Some(config) = config {
            repo.write(".whetstone/config.toml", config);
        }
        let mut session = started_in(repo.root(), 120, 60);
        session.press(REVIEW_NEWEST);
        // Only the source file, the message and the area prompts count.
        assert_eq!(session.section("Size"), ["6 KB of 400 KB"], "{path}");

        session.press("j <Space>");

        assert_eq!(files(&session)[0][0], path);
        assert_eq!(statuses(&session)[0], "included by you", "{path}");
        let size = session.section("Size");
        let kilobytes: usize = size[0]
            .strip_suffix(" KB of 400 KB")
            .and_then(|kilobytes| kilobytes.parse().ok())
            .unwrap_or_default();
        assert!(kilobytes > 600, "its diff and content count: {size:?}");
        assert_eq!(
            size[1], "This change is too large to practise on.",
            "{size:?}"
        );
    }
}

#[test]
fn send_says_why_it_is_disabled_secrets_first_then_size_then_nothing_to_send_then_jev() {
    let repo = with_a_large_file_and_a_secret();
    let mut session = started_in(repo.root(), 120, 60);
    session.press(REVIEW_NEWEST);
    assert_eq!(send(&session), "Send (resolve the possible secrets)");

    // Back, Send, then the finding's Not a secret.
    session.press("<S-Tab> k k <Space>");
    assert_eq!(send(&session), "Send (over the size limit)");

    // Up past the finding's actions and the five areas to the large file.
    session.press(&times("k", 8));
    session.press("<Space> j <Space>");
    assert_eq!(statuses(&session), ["left out by you"; 2]);
    assert_eq!(send(&session), "Send (nothing left to send)");

    session.press("<Space>");
    assert_eq!(send(&session), "Send (arrives with Jev)");
}

/// The rows of the reader's text, trimmed, under its title.
fn reader(session: &Session) -> Vec<String> {
    session
        .rows()
        .iter()
        .skip(3)
        .map(|row| row.trim().to_owned())
        .collect()
}

#[test]
fn enter_on_a_file_reads_exactly_what_is_sent_for_it_its_diff_and_full_content() {
    let repo = with_a_retry();
    let mut session = started_in(repo.root(), 120, 40);
    session.press(REVIEW_NEWEST);

    session.press("j <Enter>");

    assert_eq!(session.rows()[1].trim(), "What is sent for src/retry.rs");
    let index = repo.git(&["log", "-1", "--format=", "--raw", "--abbrev=7"]);
    let blob = index.split_whitespace().nth(3).unwrap_or_default();
    assert_eq!(
        reader(&session)[..13],
        [
            "Diff".to_owned(),
            "1 diff --git a/src/retry.rs b/src/retry.rs".to_owned(),
            "2 new file mode 100644".to_owned(),
            format!("3 index 0000000..{blob}"),
            "4 --- /dev/null".to_owned(),
            "5 +++ b/src/retry.rs".to_owned(),
            "6 @@ -0,0 +1 @@".to_owned(),
            "7 +pub fn retry() {}".to_owned(),
            String::new(),
            "Full content".to_owned(),
            "1 pub fn retry() {}".to_owned(),
            String::new(),
            String::new(),
        ]
    );

    session.press("q");
    assert_eq!(session.rows()[1].trim(), "Review what will be sent");
    assert_eq!(files(&session)[0], ["src/retry.rs", "1 KB", "included"]);
    assert!(session.row_with("src/retry.rs").starts_with("> "));
}

#[test]
fn a_deleted_file_sends_its_content_before_the_change_and_a_left_out_file_sends_nothing() {
    let repo = with_a_retry();
    std::fs::remove_file(repo.path("src/retry.rs")).expect("remove the file");
    repo.write("Cargo.lock", "version = 4\n");
    repo.commit("Stop retrying");
    let mut session = started_in(repo.root(), 120, 40);
    session.press(REVIEW_NEWEST);

    session.press("j j <Enter>");
    assert_eq!(session.rows()[1].trim(), "What is sent for src/retry.rs");
    let rows = reader(&session);
    assert_eq!(
        rows[9..11],
        [
            "Full content before the change, which deletes it",
            "1 pub fn retry() {}",
        ],
        "{rows:#?}"
    );

    session.press("q k <Enter>");
    assert_eq!(session.rows()[1].trim(), "What is sent for Cargo.lock");
    assert_eq!(
        reader(&session)[..2],
        ["Nothing is sent", "Cargo.lock is left out: lockfile."]
    );
}

#[test]
fn enter_on_an_area_reads_its_prompt_and_where_it_comes_from() {
    let repo = with_a_retry();
    let mut session = started_in(repo.root(), 120, 40);
    session.press(REVIEW_NEWEST);

    session.press("j j <Enter>");

    assert_eq!(session.rows()[1].trim(), "Area prompt: Domain");
    assert_eq!(
        reader(&session)[..5],
        [
            "Prompt source",
            "Whetstone default",
            "",
            "Area prompt, as it is sent",
            "Focus on the subject the change is about: the concepts it names, the rules",
        ]
    );
}

#[test]
fn enter_on_the_change_reads_its_full_id_and_its_message_as_it_is_sent() {
    let repo = with_a_retry();
    let id = repo.head();
    let mut session = started_in(repo.root(), 120, 40);
    session.press(REVIEW_NEWEST);

    session.press("<Enter>");

    assert_eq!(
        session.rows()[1].trim(),
        format!("Change {}", short_id(&repo))
    );
    assert_eq!(
        reader(&session)[..7],
        [
            "Commit ID, which is not sent".to_owned(),
            id,
            String::new(),
            "Message, as it is sent".to_owned(),
            "Retry failed payments".to_owned(),
            String::new(),
            "A card declined for a network error is tried again.".to_owned(),
        ]
    );
}

#[test]
fn areas_from_the_repository_say_their_prompt_comes_from_this_repository() {
    let repo = with_a_retry();
    repo.write(
        ".whetstone/areas/security.md",
        "+++\nname = \"Security\"\ndescription = \"Who can do what\"\n+++\nLook at who is trusted.\n",
    );
    let mut session = started_in(repo.root(), 120, 60);

    session.press("<Enter> j <Space> k <S-Tab> <Enter>");
    assert_eq!(session.section("Areas"), ["Security  this repository"]);

    session.press("j j <Enter>");

    assert_eq!(session.rows()[1].trim(), "Area prompt: Security");
    assert_eq!(
        reader(&session)[..5],
        [
            "Prompt source",
            "this repository",
            "",
            "Area prompt, as it is sent",
            "Look at who is trusted.",
        ]
    );
}

#[test]
fn only_the_selected_areas_are_sent() {
    let repo = with_a_retry();
    let mut session = started_in(repo.root(), 120, 60);

    // Deselect Infrastructure and Scalability.
    session.press("<Enter> j <Space> j j <Space> j j <Space> k k k k k <S-Tab> <Enter>");

    assert_eq!(
        columns(&session.section("Areas")),
        [
            ["Domain", "Whetstone default"],
            ["Observability", "Whetstone default"],
            ["System design", "Whetstone default"],
        ]
    );
}

#[test]
fn the_destination_names_the_models_the_learners_settings_choose() {
    let repo = with_a_retry();
    let mut session = Session::start(
        crate::session::Setup {
            config: Some(
                "[jev]\nmodel = \"typesafe/jev-1.13\"\n\n[generator]\nmodel = \"openai/gpt-6\"\n",
            ),
            variables: crate::session::NO_COLOR,
            start: Some(repo.root()),
            ..crate::session::Setup::default()
        },
        120,
        40,
    );

    session.press(REVIEW_NEWEST);

    assert_eq!(
        session.rows()[3].trim(),
        "Sends to OpenRouter · Jev: typesafe/jev-1.13 · Generator: openai/gpt-6"
    );
}

#[test]
fn review_choices_survive_going_back_to_the_picker_and_reset_for_another_commit() {
    let repo = with_every_kind_of_file();
    let mut session = started_in(repo.root(), 120, 60);
    session.press(REVIEW_NEWEST);
    // Bring back the first file, and leave out the last text file.
    session.press("j <Space> j j j j j j <Space>");
    let chosen = statuses(&session);
    assert_eq!(chosen[0], "included by you");
    assert_eq!(chosen[6], "left out by you");

    session.press("q <Enter>");
    assert_eq!(statuses(&session), chosen);

    // Choose the first commit, then the newest again, and review it.
    session.press("q k <Space> k <Space> j j <Enter>");
    assert_eq!(session.rows()[1].trim(), "Review what will be sent");
    assert_eq!(statuses(&session)[0], "left out: possible secret path");
    assert_eq!(statuses(&session)[6], "included");
}

#[test]
fn back_returns_to_the_picker_and_keeps_the_choices_for_the_same_commit() {
    let repo = with_every_kind_of_file();
    let mut session = started_in(repo.root(), 120, 60);
    session.press("<Enter> j j j j j <Space> k k k k k <S-Tab>");
    let picker = session.text();
    session.press("<Enter> j <Space>");
    let chosen = statuses(&session);
    assert_eq!(chosen[0], "included by you");

    // From the first file, round to Back.
    session.press("k <S-Tab>");
    assert_eq!(session.row_with("Back"), "> Back");
    session.press("<Enter>");

    assert_eq!(session.text(), picker);
    session.press("<Enter>");
    assert_eq!(statuses(&session), chosen);
}

#[test]
fn the_next_run_reviews_the_same_commit_afresh() {
    let repo = with_secrets();
    let before = repo.on_disk();
    let mut first = started_in(repo.root(), 120, 60);
    first.press(REVIEW_NEWEST);
    let fresh = first.text();
    // Bring back `.env`, and mark the commit message's token as not a
    // secret.
    first.press("j <Space> j j j j j j j j j <Space>");
    assert_ne!(first.text(), fresh);
    drop(first);

    let mut second = started_in(repo.root(), 120, 60);
    second.press(REVIEW_NEWEST);

    assert_eq!(second.text(), fresh);
    repo.assert_untouched(&before);
}

#[test]
fn browsing_commits_and_reviewing_one_writes_nothing() {
    let repo = with_secrets();
    for bump in 1..=55 {
        repo.write("counter.txt", format!("{bump}\n"));
        repo.commit(&format!("Bump the counter to {bump}"));
    }
    // A stale index, as an uncommitted edit leaves it: nothing may refresh
    // it.
    repo.write("README.md", "# Payments, edited\n");
    let before = repo.on_disk();
    let mut session = started_in(repo.root(), 80, 24);

    // Page past the first fifty commits, search back to the one with
    // secrets and open it in the viewer.
    session.press("<Enter> <S-Tab> k j");
    session.press(&crate::viewer::typing("/Configure the payments"));
    session.press("<Enter> <Enter> h j l l q");
    // Review it: bring back `.env` and read it, read an area, mark a
    // match, leave a file out, then go back and review it again.
    session.press("j j <Enter>");
    assert_eq!(session.rows()[1].trim(), "Review what will be sent");
    session.press("j <Space> <Enter> G q j j j <Enter> q k k k k <S-Tab> k k <Space> k <Space>");
    session.press("q <Enter> <Enter> q q q q");
    drop(session);

    repo.assert_untouched(&before);
}

/// Everything recorded in the log while `run` runs, at every level.
fn logged(run: impl FnOnce()) -> String {
    use std::io;
    use std::sync::{Arc, Mutex};

    #[derive(Debug, Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl io::Write for Captured {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().expect("log lock").extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_max_level(tracing::Level::TRACE)
        .finish();
    tracing::subscriber::with_default(subscriber, run);
    let bytes = captured.0.lock().expect("log lock").clone();
    String::from_utf8(bytes).expect("the log is UTF-8")
}

#[test]
fn a_session_with_secrets_leaves_no_trace_of_them_file_contents_or_prompts_in_the_log() {
    let repo = with_secrets();

    let log = logged(|| {
        let mut session = started_in(repo.root(), 80, 24);
        session.press(REVIEW_NEWEST);
        session.press("j <Space> <Enter> q j j <Enter> q <S-Tab> k k <Space>");
    });

    assert!(log.contains("opened the outbound review"), "{log}");
    assert!(
        log.contains(&repo.head()),
        "commit IDs may be logged: {log}"
    );
    for unlogged in [
        AWS_KEY,
        GITHUB_TOKEN,
        "eu-west-1",
        "Take payments.",
        "Focus on the subject the change is about",
    ] {
        assert!(!log.contains(unlogged), "{unlogged:?} in the log:\n{log}");
    }
}

/// `with_every_kind_of_file`, whose newest commit also adds an AWS key to
/// `src/settings.py`. Snapshots never hold a whole fake secret, so that no
/// secret scanner finds one in them: the review shows only its masked
/// excerpt, and no snapshot reads the file.
fn for_snapshots() -> TestRepo {
    let repo = TestRepo::new("payments");
    repo.write("README.md", "# Payments\n");
    repo.commit("Start the payments service");
    repo.write("src/retry.rs", "pub fn retry() {}\n")
        .write("src/settings.py", format!("KEY = \"{AWS_KEY}\"\n"))
        .write("Cargo.lock", "version = 4\n")
        .write(".env.example", "API_URL=http://localhost\n")
        .write("fixtures/cards.json", "[]\n")
        .write("docs/logo.png", b"PNG\0\x01\x02binary")
        .symlink("docs/readme", "../README.md")
        .add_submodule("vendor/ledger");
    repo.commit("Retry failed payments\n\nA card declined for a network error is tried again.");
    repo.write(".whetstone/config.toml", "exclude = [\"fixtures/\"]\n");
    repo
}

#[test]
fn the_review_at_120x32() {
    let repo = for_snapshots();
    let mut session = started_in(repo.root(), 120, 32);
    session.press(REVIEW_NEWEST);

    insta::assert_debug_snapshot!("outbound_review_120x32", session.frame());
}

#[test]
fn the_review_at_80x24() {
    let repo = for_snapshots();
    let mut session = started_in(repo.root(), 80, 24);
    session.press(REVIEW_NEWEST);

    insta::assert_debug_snapshot!("outbound_review_80x24", session.frame());
}

#[test]
fn the_reviews_secret_check_at_120x32() {
    let repo = for_snapshots();
    let mut session = started_in(repo.root(), 120, 32);
    session.press(REVIEW_NEWEST);
    session.press("<S-Tab> k k <Space>");

    insta::assert_debug_snapshot!("outbound_review_secret_check_120x32", session.frame());
}

#[test]
fn the_reviews_secret_check_at_80x24() {
    let repo = for_snapshots();
    let mut session = started_in(repo.root(), 80, 24);
    session.press(REVIEW_NEWEST);
    session.press("<S-Tab> k k <Space>");

    insta::assert_debug_snapshot!("outbound_review_secret_check_80x24", session.frame());
}

#[test]
fn reading_what_a_file_sends_at_120x32() {
    let repo = for_snapshots();
    let mut session = started_in(repo.root(), 120, 32);
    session.press(REVIEW_NEWEST);
    session.press(&times("j", 7));
    session.press("<Enter>");

    insta::assert_debug_snapshot!("outbound_review_reading_a_file_120x32", session.frame());
}

#[test]
fn reading_what_a_file_sends_at_80x24() {
    let repo = for_snapshots();
    let mut session = started_in(repo.root(), 80, 24);
    session.press(REVIEW_NEWEST);
    session.press(&times("j", 7));
    session.press("<Enter>");

    insta::assert_debug_snapshot!("outbound_review_reading_a_file_80x24", session.frame());
}

#[test]
fn at_80x24_the_review_scrolls_to_keep_the_focused_control_and_its_heading_in_view() {
    let repo = for_snapshots();
    let mut session = started_in(repo.root(), 80, 24);
    session.press(REVIEW_NEWEST);
    assert_eq!(session.rows()[1].trim(), "Review what will be sent");

    session.press("<S-Tab>");
    let rows: Vec<String> = session
        .rows()
        .iter()
        .map(|row| row.trim().to_owned())
        .collect();
    assert_eq!(
        rows[20..23],
        [
            "Send (resolve the possible secrets)",
            "> Back",
            "? keys  : actions  Space toggle  Enter read  q back"
        ]
    );
    assert!(rows.contains(&"Size".to_owned()), "{rows:#?}");

    // Up past Send, the secret check's actions, the five areas and the
    // nine files but the first.
    session.press(&times("k", 17));
    assert_eq!(session.rows()[1].trim(), "Files", "{}", session.text());
    assert!(session.rows()[2].trim().starts_with("> .env.example"));

    session.press("k");
    assert_eq!(session.rows()[1].trim(), "Review what will be sent");
}

#[test]
fn statuses_and_findings_read_the_same_without_colour_and_in_16_colours() {
    let repo = for_snapshots();
    let mut plain = started_in(repo.root(), 120, 60);
    let mut colours = Session::start(
        crate::session::Setup {
            variables: &[("TERM", "xterm")],
            start: Some(repo.root()),
            ..crate::session::Setup::default()
        },
        120,
        60,
    );
    for session in [&mut plain, &mut colours] {
        session.press(REVIEW_NEWEST);
        session.press("j <Space> k <S-Tab> k k <Space>");
    }

    assert_eq!(colours.rows(), plain.rows());
    assert_eq!(statuses(&plain)[0], "included by you");
    assert!(
        plain
            .section("Secret check")
            .contains(&"> (x) Not a secret".to_owned()),
        "{}",
        plain.text()
    );
}

#[test]
fn the_reviews_secret_check_in_16_colours() {
    let repo = for_snapshots();
    let mut session = Session::start(
        crate::session::Setup {
            variables: &[("TERM", "xterm")],
            start: Some(repo.root()),
            ..crate::session::Setup::default()
        },
        80,
        24,
    );
    session.press(REVIEW_NEWEST);
    session.press("<S-Tab> k k <Space>");

    insta::assert_debug_snapshot!(
        "outbound_review_secret_check_16_colours_80x24",
        session.frame()
    );
}
