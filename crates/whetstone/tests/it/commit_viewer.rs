//! The change viewer on the learner's own commits: Enter on a commit in the
//! picker opens it, read from Git's object store at that commit.

use whetstone::Flow;

use crate::commits::{git_log, without_size};
use crate::repo::TestRepo;
use crate::repository::started_in;
use crate::session::Session;
use crate::viewer::code;

/// From home: open the repository's picker, move past its five default
/// areas onto the newest commit and open it.
const OPEN_NEWEST: &str = "<Enter> j j j j j <Enter>";

/// A repository called `payments` with two commits: the first adds a
/// readme and a payment, and the second edits both. The files hold lines
/// that do not start with a letter, so `git diff` puts no function name
/// after its hunk headers, as Whetstone puts none.
fn with_an_edit() -> TestRepo {
    let repo = TestRepo::new("payments");
    repo.write("README.md", "# Payments\n")
        .write("src/payment.rs", "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n");
    repo.commit("Start the payments service");
    repo.write("README.md", "# Payments\n\nTake payments.\n")
        .write("src/payment.rs", "1\n2\n3\n4\n5\n6\n7\n8\nnine\n10\n");
    repo.commit("Describe the service and rename nine");
    repo
}

/// The lines `git diff` prints for `path` in the commit `id`, compared with
/// its first parent, trimmed as the code pane's rows are.
fn git_diff(repo: &TestRepo, id: &str, path: &str) -> Vec<String> {
    repo.git(&["diff", "--no-color", &format!("{id}^"), id, "--", path])
        .lines()
        .map(|line| line.trim().to_owned())
        .collect()
}

/// The code pane's rows, without the empty ones under the code.
fn code_lines(session: &Session) -> Vec<String> {
    let mut rows = code(session);
    while rows.last().is_some_and(String::is_empty) {
        rows.pop();
    }
    rows
}

#[test]
fn enter_on_a_commit_opens_it_on_the_diff_of_its_first_file_as_git_diff_shows_it() {
    let repo = with_an_edit();
    let short = repo.git(&["log", "-1", "--format=%h"]).trim().to_owned();
    let mut session = started_in(repo.root(), 120, 32);

    session.press(OPEN_NEWEST);

    assert_eq!(
        session.rows()[1].trim(),
        format!("{short}  Describe the service and rename nine")
    );
    assert_eq!(
        session.box_rows("Files")[..2],
        ["(x) README.md", "( ) src/payment.rs"]
    );
    assert_eq!(code_lines(&session), git_diff(&repo, "HEAD", "README.md"));
    session.press("h j l");
    assert_eq!(
        code_lines(&session),
        git_diff(&repo, "HEAD", "src/payment.rs")
    );
    session.press("l");
    assert_eq!(code_lines(&session)[8], "9 9");
    session.press("l");
    assert_eq!(code_lines(&session)[8], "9 nine");
}

#[test]
fn a_deleted_file_shows_its_content_before_the_change() {
    let repo = with_an_edit();
    repo.git(&["rm", "--quiet", "src/payment.rs"]);
    repo.commit("Stop taking payments");
    let mut session = started_in(repo.root(), 120, 32);

    session.press(OPEN_NEWEST);

    assert_eq!(
        session
            .box_rows("Files")
            .into_iter()
            .filter(|row| !row.is_empty())
            .collect::<Vec<_>>(),
        ["(x) src/payment.rs"]
    );
    assert_eq!(
        code_lines(&session),
        git_diff(&repo, "HEAD", "src/payment.rs")
    );
    session.press("l");
    assert_eq!(
        code_lines(&session),
        [
            "1 1", "2 2", "3 3", "4 4", "5 5", "6 6", "7 7", "8 8", "9 nine", "10 10"
        ]
    );
    session.press("l");
    assert_eq!(
        code_lines(&session),
        ["This change deletes src/payment.rs, so it has no code after."]
    );
}

#[test]
fn a_subject_over_two_lines_reads_as_git_log_has_it_in_the_picker_and_the_viewer() {
    let repo = TestRepo::new("payments");
    repo.write("README.md", "# Payments\n");
    repo.commit("Start the payments service");
    repo.write("README.md", "# Payments\n\nTake payments.\n");
    repo.commit("Describe the service\nin the readme\n\nSay what it takes.");
    let subject = repo
        .git(&["log", "-1", "--format=%h  %s"])
        .trim()
        .to_owned();
    let mut session = started_in(repo.root(), 120, 32);

    session.press("<Enter>");
    assert_eq!(
        without_size(&session.row_with("Describe the service")),
        git_log(&repo)[0],
        "{}",
        session.text()
    );
    session.press("j j j j j <Enter>");

    assert_eq!(session.rows()[1].trim(), subject);
}

#[test]
fn q_and_esc_return_to_the_picker_with_the_opened_commit_chosen_and_focused() {
    for back in ["q", "<Esc>"] {
        let repo = with_an_edit();
        let mut session = started_in(repo.root(), 80, 24);
        session.press("<Enter> <Space>");
        session.press("j j j j j <Enter>");

        assert_eq!(session.press(back), Flow::Continue);

        let text = session.text();
        assert!(text.contains("  ( ) Domain"), "after {back}:\n{text}");
        assert!(
            text.contains("> (x) ") && text.contains("Describe the service"),
            "after {back}:\n{text}"
        );
    }
}

/// A repository called `payments` whose second commit adds a binary file,
/// a text file that is not UTF-8, a symbolic link and a submodule.
fn with_assets() -> TestRepo {
    let repo = TestRepo::new("payments");
    repo.write("README.md", "# Payments\n");
    repo.commit("Start the payments service");
    repo.write("docs/logo.png", b"PNG\0\x01\x02binary")
        .write("docs/latin1.txt", b"caf\xe9\n")
        .symlink("docs/readme", "../README.md")
        .add_submodule("vendor/ledger");
    repo.commit("Add assets, a link and the ledger");
    repo
}

#[test]
fn binary_files_submodules_and_symlinks_are_labelled_entries_without_content() {
    let repo = with_assets();
    let mut session = started_in(repo.root(), 120, 32);

    session.press(OPEN_NEWEST);

    assert_eq!(
        session.box_rows("Files")[..5],
        [
            "(x) .gitmodules",
            "( ) docs/latin1.txt  binary",
            "( ) docs/logo.png  binary",
            "( ) docs/readme  symlink",
            "( ) vendor/ledger  submodule",
        ]
    );
    session.press("h j j l");
    assert!(
        code_lines(&session)
            .contains(&"Binary files /dev/null and b/docs/logo.png differ".to_owned()),
        "{}",
        session.text()
    );
    session.press("l l");
    assert_eq!(
        code_lines(&session),
        ["docs/logo.png is a binary file, so it has no code to show."]
    );
    assert!(session.text().contains("> (x) After"), "{}", session.text());
    // From After to the next file's After, then from Diff to the next's.
    for (keys, path, kind, mode) in [
        ("h h h j l l l", "docs/readme", "a symbolic link", "120000"),
        ("h j l l l", "vendor/ledger", "a submodule", "160000"),
    ] {
        session.press(keys);
        assert_eq!(
            code_lines(&session),
            [format!("{path} is {kind}, so it has no code to show.")]
        );
        session.press("h h");
        assert_eq!(
            code_lines(&session),
            [
                format!("diff --git a/{path} b/{path}"),
                format!("new file mode {mode}"),
                code_lines(&session)[2].clone(),
            ],
            "only headers, no content"
        );
    }
}

/// Twenty numbered lines, `1` to `20`, with `edit` in place of line 10 if
/// given.
fn numbered(edit: Option<&str>) -> String {
    (1..=20)
        .map(|line| match (line, edit) {
            (10, Some(edit)) => format!("{edit}\n"),
            (line, _) => format!("{line}\n"),
        })
        .collect()
}

#[test]
fn a_renamed_file_is_one_entry_with_its_old_and_new_paths() {
    let repo = TestRepo::new("payments");
    repo.write("src/lib.txt", numbered(None));
    repo.commit("Start the library");
    repo.git(&["mv", "src/lib.txt", "src/core.txt"]);
    repo.write("src/core.txt", numbered(Some("ten")));
    repo.commit("Rename the library to core");
    let mut session = started_in(repo.root(), 120, 32);

    session.press(OPEN_NEWEST);

    assert_eq!(
        session.box_rows("Files")[0],
        "(x) src/core.txt  ← src/lib.txt"
    );
    // As `git diff -M` shows it, without the similarity index, which Git
    // estimates its own way.
    let expected: Vec<String> = repo
        .git(&["diff", "--no-color", "-M", "HEAD^", "HEAD"])
        .lines()
        .filter(|line| !line.starts_with("similarity index"))
        .map(|line| line.trim().to_owned())
        .collect();
    assert!(expected.contains(&"rename from src/lib.txt".to_owned()));
    assert_eq!(code_lines(&session), expected);
    session.press("l");
    assert_eq!(code_lines(&session)[9], "10 10");
    session.press("l");
    assert_eq!(code_lines(&session)[9], "10 ten");
}

#[test]
fn renames_are_found_among_more_files_than_gixs_default_limit_allows() {
    // 84 files renamed and edited at once give 84 × 84 pairs to compare,
    // past gix's default limit of 1,000 but within Git's 1,000 × 1,000.
    let repo = TestRepo::new("payments");
    let file = |index: usize, edit: &str| {
        (0..8)
            .map(|line| format!("{index} {line}\n"))
            .chain([format!("{index} {edit}\n")])
            .collect::<String>()
    };
    for index in 0..84 {
        repo.write(&format!("old/{index:02}.txt"), file(index, "first"));
    }
    repo.commit("Start with 84 files");
    repo.git(&["rm", "--quiet", "-r", "old"]);
    for index in 0..84 {
        repo.write(&format!("new/{index:02}.txt"), file(index, "second"));
    }
    repo.commit("Move and edit all 84");
    let mut session = started_in(repo.root(), 120, 32);

    session.press(OPEN_NEWEST);
    session.press("h G");

    assert_eq!(
        session
            .box_rows("Files")
            .into_iter()
            .find(|row| row.contains("(x)")),
        Some("> (x) new/83.txt  ← old/83.txt".to_owned()),
        "{}",
        session.text()
    );
}

/// The Files pane's rows that show a file.
fn files(session: &Session) -> Vec<String> {
    session
        .box_rows("Files")
        .into_iter()
        .filter(|row| !row.is_empty())
        .collect()
}

#[test]
fn a_merge_is_compared_with_its_first_parent_and_a_first_commit_with_an_empty_tree() {
    let repo = TestRepo::new("payments");
    repo.write("README.md", "# Payments\n")
        .write("src/payment.rs", "1\n");
    repo.commit("Start the payments service");
    repo.git(&["switch", "--quiet", "-c", "refunds"]);
    repo.write("src/refund.rs", "1\n");
    repo.commit("Add refunds");
    repo.git(&["switch", "--quiet", "main"]);
    repo.write("README.md", "# Payments\n\nTake payments.\n");
    repo.commit("Describe the service");
    repo.merge("refunds", "Merge refunds");
    let short = repo.git(&["log", "-1", "--format=%h"]).trim().to_owned();
    let mut session = started_in(repo.root(), 120, 32);

    session.press(OPEN_NEWEST);

    let title = session.rows()[1].trim().to_owned();
    assert!(
        title.starts_with(&format!("{short}  Merge refunds")),
        "{title}"
    );
    assert!(title.ends_with("  merge · vs first parent"), "{title}");
    // The whole merged work: what the merge brings to main.
    assert_eq!(files(&session), ["(x) src/refund.rs"]);

    session.press("q j j j <Enter>");

    let title = session.rows()[1].trim().to_owned();
    assert!(title.contains("  Start the payments service"), "{title}");
    assert!(title.ends_with("  first commit"), "{title}");
    assert_eq!(files(&session), ["(x) README.md", "( ) src/payment.rs"]);
    assert_eq!(code_lines(&session)[1], "new file mode 100644");
    session.press("l");
    assert_eq!(
        code_lines(&session),
        ["This change creates README.md, so it has no code before."]
    );
}

#[test]
fn uncommitted_edits_and_branch_switches_never_change_an_open_change() {
    let repo = with_an_edit();
    // An edit not yet committed when Whetstone starts.
    repo.write("README.md", "# Payments, edited\n");
    let mut session = started_in(repo.root(), 120, 32);
    session.press(OPEN_NEWEST);
    let diff = git_diff(&repo, "HEAD", "README.md");
    assert_eq!(code_lines(&session), diff);
    session.press("l l");
    let after = code_lines(&session);
    assert_eq!(after[2], "3 Take payments.");

    // While it is open, the learner commits on another branch, switches
    // to it and edits a file there.
    repo.git(&["switch", "--quiet", "-c", "elsewhere"]);
    repo.commit("Rewrite the readme elsewhere");
    repo.write("README.md", "# Elsewhere\n");
    session.press("h l");

    assert_eq!(code_lines(&session), after);
    session.press("h h");
    assert_eq!(code_lines(&session), diff);
    // Opened again, it is read again from the commit, and the picker still
    // lists the history from HEAD as it was at startup.
    session.press("q");
    assert!(
        !session.text().contains("Rewrite the readme elsewhere"),
        "{}",
        session.text()
    );
    session.press("<Enter>");
    assert_eq!(code_lines(&session), diff);
}

#[test]
fn browsing_searching_and_opening_commits_writes_nothing() {
    let repo = with_assets();
    repo.git(&["mv", "docs/latin1.txt", "docs/cafe.txt"]);
    repo.commit("Rename the latin-1 file");
    for bump in 1..=60 {
        repo.write("counter.txt", format!("{bump}\n"));
        repo.commit(&format!("Bump the counter to {bump}"));
    }
    let clone = repo.shallow_clone("payments-shallow", 3);
    // A stale index, as an uncommitted edit leaves it: nothing may refresh
    // it.
    repo.write("README.md", "# Payments, edited\n");
    let before = repo.on_disk();

    for start in [repo.root().to_path_buf(), clone] {
        let mut session = started_in(&start, 120, 32);
        // Page past the first fifty, search back to the first commit and
        // open it, then open the newest, and look at every pane.
        session.press("<Enter> <S-Tab> k j");
        session.press(&crate::viewer::typing("/payments service"));
        session.press("<Enter> <Enter> h j j l l l q");
        session.press("<Enter> j j j j j <Enter> h G l l l q q");
        drop(session);
    }

    repo.assert_untouched(&before);
}

#[test]
fn a_commit_whose_files_are_missing_from_the_repository_says_why_it_cannot_be_read() {
    let repo = with_a_missing_file();
    let short = repo.git(&["log", "-1", "--format=%h"]).trim().to_owned();
    let blob = repo.git(&["rev-parse", "HEAD:README.md"]).trim().to_owned();
    let mut session = started_in(repo.root(), 80, 24);

    session.press(OPEN_NEWEST);

    assert_eq!(
        session.rows()[1].trim(),
        format!("Whetstone cannot read commit {short}")
    );
    assert!(
        session.text().contains(&blob),
        "names the missing object:\n{}",
        session.text()
    );
    assert_eq!(session.press("q"), Flow::Continue);
    assert_eq!(session.rows()[1].trim(), "Practice on payments");
}

/// `with_assets` and a library, then a commit that renames the library and
/// edits it, and makes the symbolic link a file.
fn with_a_rename_and_assets() -> TestRepo {
    let repo = with_assets();
    repo.write("src/lib.txt", numbered(None));
    repo.commit("Start the library");
    repo.git(&["mv", "src/lib.txt", "src/core.txt"]);
    repo.write("src/core.txt", numbered(Some("ten")));
    std::fs::remove_file(repo.path("docs/readme")).expect("remove the link");
    repo.write("docs/readme", "See OVERVIEW.md.\n");
    repo.commit("Rename the library and make the readme's link a file");
    repo
}

#[test]
fn a_commit_with_assets_at_120x32() {
    let repo = with_assets();
    let mut session = started_in(repo.root(), 120, 32);
    session.press(OPEN_NEWEST);
    session.press("h j j");

    insta::assert_debug_snapshot!("commit_with_assets_120x32", session.frame());
}

#[test]
fn a_commit_with_assets_at_80x24() {
    let repo = with_assets();
    let mut session = started_in(repo.root(), 80, 24);
    session.press(OPEN_NEWEST);
    session.press("h j j l l l");

    insta::assert_debug_snapshot!("commit_with_assets_80x24", session.frame());
}

#[test]
fn a_rename_and_a_link_made_a_file_at_120x32() {
    let repo = with_a_rename_and_assets();
    let mut session = started_in(repo.root(), 120, 32);
    session.press(OPEN_NEWEST);

    insta::assert_debug_snapshot!("rename_and_link_made_a_file_120x32", session.frame());
}

#[test]
fn a_rename_and_a_link_made_a_file_at_80x24() {
    let repo = with_a_rename_and_assets();
    let mut session = started_in(repo.root(), 80, 24);
    session.press(OPEN_NEWEST);
    session.press("h j l");

    insta::assert_debug_snapshot!("rename_and_link_made_a_file_80x24", session.frame());
}

/// `with_an_edit`, with the newest commit's readme missing from the
/// repository, as in a partial clone, which fetches files only when they
/// are needed.
fn with_a_missing_file() -> TestRepo {
    let repo = with_an_edit();
    repo.remove_object(repo.git(&["rev-parse", "HEAD:README.md"]).trim());
    repo
}

#[test]
fn a_commit_that_cannot_be_read_at_120x32() {
    let repo = with_a_missing_file();
    let mut session = started_in(repo.root(), 120, 32);
    session.press(OPEN_NEWEST);

    insta::assert_debug_snapshot!("commit_that_cannot_be_read_120x32", session.frame());
}

#[test]
fn a_commit_that_cannot_be_read_at_80x24() {
    let repo = with_a_missing_file();
    let mut session = started_in(repo.root(), 80, 24);
    session.press(OPEN_NEWEST);

    insta::assert_debug_snapshot!("commit_that_cannot_be_read_80x24", session.frame());
}

#[test]
fn an_empty_file_renamed_is_one_entry_as_git_finds_it() {
    let repo = TestRepo::new("payments");
    repo.write("README.md", "# Payments\n")
        .write("src/empty.rs", "");
    repo.commit("Start the payments service");
    repo.git(&["mv", "src/empty.rs", "src/blank.rs"]);
    repo.commit("Rename the empty file");
    let mut session = started_in(repo.root(), 120, 32);

    session.press(OPEN_NEWEST);

    assert_eq!(files(&session), ["(x) src/blank.rs  ← src/empty.rs"]);
    let expected: Vec<String> = repo
        .git(&["diff", "--no-color", "-M", "HEAD^", "HEAD"])
        .lines()
        .filter(|line| !line.starts_with("similarity index"))
        .map(|line| line.trim().to_owned())
        .collect();
    assert!(expected.contains(&"rename from src/empty.rs".to_owned()));
    assert_eq!(code_lines(&session), expected);
}

#[test]
fn a_symbolic_link_made_a_file_is_a_deleted_link_and_a_new_file_as_git_shows_them() {
    let repo = TestRepo::new("payments");
    repo.write("README.md", "# Payments\n")
        .symlink("docs", "README.md");
    repo.commit("Start the payments service");
    std::fs::remove_file(repo.path("docs")).expect("remove the link");
    repo.write("docs", "See README.md.\n");
    repo.commit("Make the docs link a file");
    let mut session = started_in(repo.root(), 120, 32);

    session.press(OPEN_NEWEST);

    assert_eq!(files(&session), ["(x) docs  symlink", "( ) docs"]);
    assert_eq!(code_lines(&session)[1], "deleted file mode 120000");
    session.press("h j l");
    let expected: Vec<String> = repo
        .git(&["diff", "--no-color", "HEAD^", "HEAD"])
        .lines()
        .skip_while(|line| !line.starts_with("diff --git") || !line.ends_with("b/docs"))
        .skip(1)
        .skip_while(|line| !line.starts_with("diff --git"))
        .map(|line| line.trim().to_owned())
        .collect();
    assert!(
        expected.contains(&"+See README.md.".to_owned()),
        "{expected:?}"
    );
    assert_eq!(code_lines(&session), expected);
}

#[test]
fn a_merge_whose_second_parent_is_missing_still_opens_against_its_first() {
    let repo = TestRepo::new("payments");
    repo.write("README.md", "# Payments\n");
    repo.commit("Start the payments service");
    repo.git(&["switch", "--quiet", "-c", "refunds"]);
    repo.write("src/refund.rs", "1\n");
    let refunds = repo.commit("Add refunds");
    repo.git(&["switch", "--quiet", "main"]);
    repo.write("README.md", "# Payments\n\nTake payments.\n");
    repo.commit("Describe the service");
    repo.merge("refunds", "Merge refunds");
    // As a shallow clone cut by date can leave a merge's second parent out.
    repo.remove_object(&refunds);
    let mut session = started_in(repo.root(), 120, 32);

    session.press(OPEN_NEWEST);

    let title = session.rows()[1].trim().to_owned();
    assert!(title.ends_with("  merge · vs first parent"), "{title}");
    assert_eq!(files(&session), ["(x) src/refund.rs"]);
}
