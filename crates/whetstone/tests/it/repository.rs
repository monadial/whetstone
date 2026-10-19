//! Practice on the learner's own repository: Whetstone started inside one
//! offers it on home, and its picker lists the repository's areas and
//! commits.

use std::path::Path;

use whetstone::Flow;

use crate::repo::TestRepo;
use crate::session::{NO_COLOR, Session, Setup, columns};

/// Starts the app without colour in `start`, in a `width` by `height`
/// terminal.
fn started_in(start: &Path, width: u16, height: u16) -> Session {
    Session::start(
        Setup {
            variables: NO_COLOR,
            start: Some(start),
            ..Setup::default()
        },
        width,
        height,
    )
}

/// Home's actions, and the lines between them, trimmed: from the first
/// "Practice on" row to the next blank one.
fn home_actions(session: &Session) -> Vec<String> {
    session
        .rows()
        .iter()
        .map(|row| row.trim().to_owned())
        .skip_while(|row| !row.contains("Practice on"))
        .take_while(|row| !row.is_empty())
        .collect()
}

/// A repository called `payments` with one commit.
fn payments() -> TestRepo {
    let repo = TestRepo::new("payments");
    repo.write("README.md", "# Payments\n");
    repo.commit("Start the payments service");
    repo
}

#[test]
fn inside_a_repository_home_offers_practice_on_it_first() {
    let repo = payments();

    let session = started_in(repo.root(), 80, 24);

    assert_eq!(
        home_actions(&session),
        [
            "> Practice on payments",
            "Nothing leaves this machine until you review what will be sent.",
            "Practice on the showcase project",
            "Settings",
            "Quit",
        ]
    );
}

/// The line on home that says why Practice on the learner's repository is
/// unavailable, which follows the actions.
fn unavailable_line(session: &Session) -> String {
    let rows: Vec<String> = session
        .rows()
        .iter()
        .map(|row| row.trim().to_owned())
        .collect();
    let quit = rows.iter().position(|row| row.ends_with("Quit"));
    assert!(quit.is_some(), "home lists Quit:\n{}", session.text());
    rows.into_iter()
        .skip(quit.unwrap_or_default() + 1)
        .find(|row| !row.is_empty())
        .unwrap_or_default()
}

/// The paragraph [`unavailable_line`] starts, its rows joined.
fn unavailable_paragraph(session: &Session) -> String {
    let rows: Vec<String> = session
        .rows()
        .iter()
        .map(|row| row.trim().to_owned())
        .collect();
    let first = unavailable_line(session);
    rows.iter()
        .skip_while(|row| **row != first)
        .take_while(|row| !row.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn outside_a_repository_home_offers_the_showcase_project_and_says_why_not_the_repository() {
    let outside = tempfile::TempDir::new().expect("create a temporary directory");

    let session = started_in(outside.path(), 80, 24);

    assert_eq!(
        home_actions(&session),
        ["> Practice on the showcase project", "Settings", "Quit"]
    );
    assert_eq!(
        unavailable_line(&session),
        "To practice on your own work, start Whetstone inside a Git repository."
    );
}

#[test]
fn with_an_invalid_config_home_names_the_file_and_the_problem_and_keeps_the_showcase_project() {
    for (config, problem) in [
        (
            "areas = [\"Domain\"]\n",
            "Practice on payments is unavailable because .whetstone/config.toml is invalid: the key `areas` is not allowed, only `exclude`.",
        ),
        (
            "exclude = [\"fixtures/\"]\n\n[jev]\nmodel = \"typesafe/jev-1.14\"\n",
            "Practice on payments is unavailable because .whetstone/config.toml is invalid: the key `jev` is not allowed, only `exclude`.",
        ),
        (
            "exclude = \"fixtures/\"\n",
            "Practice on payments is unavailable because .whetstone/config.toml is invalid: `exclude` must be a list of patterns.",
        ),
        (
            "exclude = [\"fixtures/\", 3]\n",
            "Practice on payments is unavailable because .whetstone/config.toml is invalid: `exclude` must be a list of patterns.",
        ),
        (
            "exclude = [\"fixtures/\", \"!fixtures/keep.txt\"]\n",
            "Practice on payments is unavailable because .whetstone/config.toml is invalid: invalid exclude pattern `!fixtures/keep.txt`: `!` can't bring files back, because exclude patterns only leave files out.",
        ),
        (
            "exclude = [\"src/[a\"]\n",
            "Practice on payments is unavailable because .whetstone/config.toml is invalid: invalid exclude pattern `src/[a`: a `[` has no closing `]`.",
        ),
        (
            "exclude = [\n",
            "Practice on payments is unavailable because .whetstone/config.toml is invalid: line 1 is not valid TOML: unclosed array, expected `]`.",
        ),
    ] {
        let repo = payments();
        repo.write(".whetstone/config.toml", config);

        let session = started_in(repo.root(), 120, 32);

        assert_eq!(
            home_actions(&session),
            ["> Practice on the showcase project", "Settings", "Quit"],
            "{config}"
        );
        assert_eq!(unavailable_paragraph(&session), problem, "{config}");
    }
}

#[test]
fn a_config_that_lists_exclude_patterns_is_valid() {
    let repo = payments();
    repo.write(
        ".whetstone/config.toml",
        "# Fixtures are generated.\nexclude = [\"fixtures/\", \"*.snap\"]\n",
    );

    let session = started_in(repo.root(), 80, 24);

    assert_eq!(home_actions(&session)[0], "> Practice on payments");
}

/// The heading above the areas.
const AREAS: &str = "Areas the questions focus on";

/// An area file named `name`, described by `description`.
fn area_file(name: &str, description: &str) -> String {
    format!("+++\nname = \"{name}\"\ndescription = \"{description}\"\n+++\n\nLook at {name}.\n")
}

#[test]
fn practice_on_the_repository_opens_the_picker_titled_with_its_name() {
    let repo = payments();
    let mut session = started_in(repo.root(), 80, 24);

    session.press("<Enter>");

    assert_eq!(session.rows()[1].trim(), "Practice on payments");
}

#[test]
fn a_repository_without_area_files_offers_the_five_defaults_all_selected() {
    let repo = payments();
    let mut session = started_in(repo.root(), 80, 24);

    session.press("<Enter>");

    assert_eq!(
        columns(&session.section(AREAS)),
        [
            vec![
                "> (x) Domain",
                "The rules, concepts and language the work is about"
            ],
            vec![
                "(x) Infrastructure",
                "Where the work runs and what it depends on"
            ],
            vec![
                "(x) Observability",
                "How you can tell what the work is doing and why"
            ],
            vec![
                "(x) Scalability",
                "How the work holds up as use and data grow"
            ],
            vec![
                "(x) System design",
                "How the parts fit together, and why this way"
            ],
        ]
    );
}

#[test]
fn a_repositorys_area_files_replace_the_defaults_in_file_name_order_all_selected() {
    let repo = payments();
    // Uncommitted: the working tree is what counts. Other files are not
    // area files.
    repo.write(
        ".whetstone/areas/security.md",
        area_file("Security", "Who can do what, and who is trusted"),
    )
    .write(
        ".whetstone/areas/accessibility.md",
        area_file("Accessibility", "Who can use it, and how"),
    )
    .write(
        ".whetstone/areas/README.txt",
        "Areas for the payments service.\n",
    );
    let mut session = started_in(repo.root(), 80, 24);

    session.press("<Enter>");

    assert_eq!(
        columns(&session.section(AREAS)),
        [
            ["> (x) Accessibility", "Who can use it, and how"],
            ["(x) Security", "Who can do what, and who is trusted"],
        ]
    );
}

/// A repository whose area files are two valid ones and an invalid one of
/// each kind: its file name, size or encoding, its front matter, its keys,
/// their values and its body.
fn with_invalid_area_files() -> TestRepo {
    let repo = payments();
    repo.write(
        ".whetstone/areas/security.md",
        area_file("Security", "Who can do what"),
    )
    .write(
        ".whetstone/areas/accessibility.md",
        area_file("Accessibility", "Who can use it"),
    )
    .write(
        ".whetstone/areas/Privacy.md",
        area_file("Privacy", "What is kept"),
    )
    .write(".whetstone/areas/big.md", "x".repeat(1_000_000))
    .write(".whetstone/areas/latin.md", b"+++\nname = \"Caf\xe9\"\n")
    .write(
        ".whetstone/areas/cost.md",
        "+++\nname = \"Cost\"\ndescription = \"What it costs\"\n",
    )
    .write(
        ".whetstone/areas/owner.md",
        "+++\nname = \"Owner\"\ndescription = \"Who owns it\"\nowner = \"ada\"\n+++\nLook.\n",
    )
    .write(".whetstone/areas/blank.md", area_file("Blank", " "))
    .write(
        ".whetstone/areas/empty.md",
        "+++\nname = \"Empty\"\ndescription = \"Nothing to say\"\n+++\n\n",
    );
    repo
}

#[test]
fn invalid_area_files_are_listed_in_file_name_order_disabled_with_their_reasons() {
    let repo = with_invalid_area_files();
    let mut session = started_in(repo.root(), 120, 32);

    session.press("<Enter>");

    assert_eq!(
        session.section(AREAS),
        [
            "> ( ) Privacy.md (invalid)  the file name must be an area ID followed by `.md`: lowercase letters, digits and",
            "hyphens",
            "(x) Accessibility         Who can use it",
            "( ) big.md (invalid)      the file is larger than 16 KB",
            "( ) blank.md (invalid)    `description` is empty",
            "( ) cost.md (invalid)     the front matter has no closing `+++` line",
            "( ) empty.md (invalid)    the area prompt after the front matter is empty",
            "( ) latin.md (invalid)    the file is not UTF-8 text",
            "( ) owner.md (invalid)    the front matter has the key `owner`, but only `name` and `description` are allowed",
            "(x) Security              Who can do what",
        ]
    );
    let start = |text: &str| {
        session
            .rows()
            .iter()
            .find_map(|row| row.find(text))
            .expect("the text is shown")
    };
    assert_eq!(
        start("hyphens"),
        start("the file name"),
        "a wrapped reason stays in its column:\n{}",
        session.text()
    );
}

#[test]
fn an_invalid_area_file_cannot_be_selected() {
    let repo = with_invalid_area_files();
    let mut session = started_in(repo.root(), 120, 32);
    session.press("<Enter>");

    session.press("<Space> <Enter>");

    assert!(
        session.section(AREAS)[0].starts_with("> ( ) Privacy.md (invalid)"),
        "{}",
        session.text()
    );
}

#[test]
fn a_long_area_description_is_cut_to_fit_an_80_column_picker() {
    let repo = payments();
    let description = "Whether the change keeps every payment's money accounted for, \
                       from the moment a customer pays to the moment the merchant is paid, \
                       across refunds, disputes, retries and currency conversions too";
    repo.write(
        ".whetstone/areas/ledger.md",
        area_file("Ledger", description),
    );
    let mut session = started_in(repo.root(), 80, 24);

    session.press("<Enter>");

    let row = session.row_with("Ledger");
    assert_eq!(
        row,
        "> (x) Ledger  Whether the change keeps every payment's money accounted for,…"
    );
    assert!(
        session.rows().iter().all(|row| !row.contains("currency")),
        "{}",
        session.text()
    );
}

/// The row of the control after the lists.
const REVIEW: &str = "Review what will be sent";

#[test]
fn review_what_will_be_sent_replaces_generate_questions_and_waits_for_a_change() {
    let repo = payments();
    let mut session = started_in(repo.root(), 80, 24);

    session.press("<Enter>");

    assert_eq!(
        session.row_with(REVIEW),
        "Review what will be sent (choose a change)"
    );
    assert!(
        !session.text().contains("Generate questions"),
        "{}",
        session.text()
    );
}

#[test]
fn review_what_will_be_sent_also_waits_for_an_area() {
    let repo = payments();
    let mut session = started_in(repo.root(), 80, 24);

    session.press("<Enter> <Space> j <Space> j <Space> j <Space> j <Space>");

    assert_eq!(
        session.row_with(REVIEW),
        "Review what will be sent (choose a change and an area)"
    );
}

#[test]
fn home_inside_a_repository_at_120x32() {
    let repo = payments();
    let session = started_in(repo.root(), 120, 32);

    insta::assert_debug_snapshot!("home_inside_a_repository_120x32", session.frame());
}

#[test]
fn home_inside_a_repository_at_80x24() {
    let repo = payments();
    let session = started_in(repo.root(), 80, 24);

    insta::assert_debug_snapshot!("home_inside_a_repository_80x24", session.frame());
}

#[test]
fn home_with_an_invalid_config_at_120x32() {
    let repo = payments();
    repo.write(".whetstone/config.toml", "areas = [\"Domain\"]\n");
    let session = started_in(repo.root(), 120, 32);

    insta::assert_debug_snapshot!("home_with_an_invalid_config_120x32", session.frame());
}

#[test]
fn home_with_an_invalid_config_at_80x24() {
    let repo = payments();
    repo.write(".whetstone/config.toml", "areas = [\"Domain\"]\n");
    let session = started_in(repo.root(), 80, 24);

    insta::assert_debug_snapshot!("home_with_an_invalid_config_80x24", session.frame());
}

#[test]
fn opening_the_repository_and_its_picker_writes_nothing() {
    for area_files in [false, true] {
        let repo = with_history();
        // A stale index, as an uncommitted edit leaves it: opening must not
        // refresh it.
        repo.write("README.md", "# Payments, edited\n");
        if area_files {
            repo.write(
                ".whetstone/areas/security.md",
                area_file("Security", "Who can do what"),
            );
        }
        let worktree = with_linked_worktree(&repo);
        let before = repo.on_disk();

        for start in [repo.path("src"), worktree.clone()] {
            let mut session = started_in(&start, 120, 32);
            session.press("<Enter> j j j j j <Space> j j j j j <Enter> q q");
            drop(session);
        }

        repo.assert_untouched(&before);
    }
}

#[test]
fn the_read_only_check_notices_git_refreshing_the_index() {
    let repo = payments();
    repo.write("README.md", "# Payments, edited\n");
    let before = repo.on_disk();

    repo.git(&["status", "--porcelain"]);

    assert_ne!(repo.on_disk(), before);
}

#[test]
fn from_a_subdirectory_home_finds_the_repository_root() {
    let repo = payments();
    repo.write("src/http/server.rs", "");

    let session = started_in(&repo.path("src/http"), 80, 24);

    assert_eq!(home_actions(&session)[0], "> Practice on payments");
}

/// `repo` with a linked worktree beside it, called `payments-review`, on a
/// branch of its own.
fn with_linked_worktree(repo: &TestRepo) -> std::path::PathBuf {
    let worktree = repo.beside("payments-review");
    repo.git(&[
        "worktree",
        "add",
        "--quiet",
        "-b",
        "review",
        worktree.to_str().expect("a UTF-8 path"),
    ]);
    worktree
}

#[test]
fn in_a_linked_worktree_home_offers_practice_on_the_worktree() {
    let repo = payments();
    repo.write("src/lib.rs", "");
    repo.commit("Add the library");
    let worktree = with_linked_worktree(&repo);

    for start in [worktree.clone(), worktree.join("src")] {
        let session = started_in(&start, 80, 24);

        assert_eq!(
            home_actions(&session)[0],
            "> Practice on payments-review",
            "from {}",
            start.display()
        );
    }
}

#[test]
fn an_invalid_config_is_found_from_a_subdirectory_and_in_a_linked_worktree() {
    let repo = payments();
    repo.write("src/lib.rs", "")
        .write(".whetstone/config.toml", "areas = [\"Domain\"]\n");
    repo.commit("Configure Whetstone the old way");
    let worktree = with_linked_worktree(&repo);

    for (start, name) in [
        (repo.path("src"), "payments"),
        (worktree.join("src"), "payments-review"),
    ] {
        let session = started_in(&start, 120, 32);

        assert_eq!(
            unavailable_paragraph(&session),
            format!(
                "Practice on {name} is unavailable because .whetstone/config.toml is invalid: \
                 the key `areas` is not allowed, only `exclude`."
            ),
            "from {}",
            start.display()
        );
    }
}

#[test]
fn in_a_bare_repository_home_says_it_has_no_working_tree() {
    let repo = TestRepo::bare("payments.git");

    let session = started_in(repo.root(), 120, 32);

    assert_eq!(
        home_actions(&session),
        ["> Practice on the showcase project", "Settings", "Quit"]
    );
    assert_eq!(
        unavailable_line(&session),
        "payments.git is a bare repository, so it has no working tree to practice on."
    );
}

#[test]
fn in_a_repository_with_no_commits_home_says_there_is_nothing_to_practice_on_yet() {
    let repo = TestRepo::new("payments");
    repo.write("README.md", "# Payments\n");

    let session = started_in(repo.root(), 80, 24);

    assert_eq!(
        home_actions(&session),
        ["> Practice on the showcase project", "Settings", "Quit"]
    );
    assert_eq!(
        unavailable_line(&session),
        "payments has no commits yet, so there is nothing to practice on."
    );
}

/// The heading above the changes.
const CHANGES: &str = "Change to practice on";

/// `payments` with three commits, a merge among them.
fn with_history() -> TestRepo {
    let repo = payments();
    repo.write("src/payment.rs", "pub struct Payment;\n");
    repo.commit("Model a payment\n\nA payment has an amount and a state.");
    repo.git(&["switch", "--quiet", "-c", "refunds"]);
    repo.write("src/refund.rs", "pub struct Refund;\n");
    repo.commit("Add refunds");
    repo.git(&["switch", "--quiet", "main"]);
    repo.write("README.md", "# Payments\n\nTake payments.\n");
    repo.commit("Describe the service");
    repo.merge("refunds", "Merge refunds");
    repo
}

#[test]
fn the_picker_lists_the_commits_reachable_from_head_newest_first_with_short_ids_and_summaries() {
    let repo = with_history();
    let mut session = started_in(repo.root(), 120, 32);

    session.press("<Enter>");

    // Git's own short IDs and subjects, in `git log` order.
    let expected: Vec<String> = repo
        .git(&["log", "--format=( ) %h  %s"])
        .lines()
        .map(str::to_owned)
        .collect();
    assert_eq!(expected.len(), 5);
    assert_eq!(session.section(CHANGES), expected);
    assert_eq!(
        columns(&session.section(CHANGES))
            .into_iter()
            .map(|row| row[1])
            .collect::<Vec<_>>(),
        [
            "Merge refunds",
            "Describe the service",
            "Add refunds",
            "Model a payment",
            "Start the payments service",
        ]
    );
}

/// From home: open the repository's picker and move past its five default
/// areas onto the newest commit.
const TO_THE_NEWEST_COMMIT: &str = "<Enter> j j j j j";

#[test]
fn choosing_a_commit_with_an_area_selected_enables_review_what_will_be_sent() {
    let repo = with_history();
    let mut session = started_in(repo.root(), 120, 32);

    session.press(TO_THE_NEWEST_COMMIT);
    session.press("j <Space>");

    let chosen: Vec<String> = session
        .section(CHANGES)
        .into_iter()
        .filter(|row| row.contains("(x)"))
        .collect();
    assert_eq!(chosen.len(), 1);
    assert!(chosen[0].ends_with("Describe the service"), "{chosen:?}");
    assert_eq!(session.row_with(REVIEW), REVIEW);
}

#[test]
fn with_a_commit_chosen_review_what_will_be_sent_still_waits_for_an_area() {
    let repo = with_history();
    let mut session = started_in(repo.root(), 120, 32);

    session.press(TO_THE_NEWEST_COMMIT);
    session.press("<Space> k <Space> k <Space> k <Space> k <Space> k <Space>");

    assert_eq!(
        session.row_with(REVIEW),
        "Review what will be sent (choose an area)"
    );
}

#[test]
fn enter_on_a_repository_commit_only_chooses_it() {
    let repo = with_history();
    let mut session = started_in(repo.root(), 120, 32);

    session.press(TO_THE_NEWEST_COMMIT);
    session.press("<Enter>");

    assert_eq!(session.rows()[1].trim(), "Practice on payments");
    assert!(
        session.section(CHANGES)[0].starts_with("> (x) "),
        "{}",
        session.text()
    );
}

#[test]
fn activating_review_what_will_be_sent_does_nothing_yet() {
    let repo = with_history();
    let mut session = started_in(repo.root(), 120, 32);
    session.press(TO_THE_NEWEST_COMMIT);
    session.press("<Space> j j j j j");
    assert_eq!(session.row_with(REVIEW), format!("> {REVIEW}"));
    let picker = session.text();

    assert_eq!(session.press("<Enter> <Space>"), Flow::Continue);

    assert_eq!(session.text(), picker);
}

/// `with_history`, with area files: a valid one, one whose description is
/// too long for a row at 80 columns, and an invalid one.
fn with_history_and_areas() -> TestRepo {
    let repo = with_history();
    repo.write(
        ".whetstone/areas/security.md",
        area_file("Security", "Who can do what, and who is trusted"),
    )
    .write(
        ".whetstone/areas/ledger.md",
        area_file(
            "Ledger",
            "Whether every payment's money stays accounted for, from the moment a customer pays to the moment the merchant is paid",
        ),
    )
    .write(
        ".whetstone/areas/owner.md",
        "+++\nname = \"Owner\"\ndescription = \"Who owns it\"\nowner = \"ada\"\n+++\nLook.\n",
    );
    repo
}

/// From home: open the repository's picker, choose the second commit and
/// move on to "Review what will be sent".
const REPOSITORY_CHOICES: &str = "<Enter> j j j j <Space> j j j j j";

#[test]
fn the_repository_picker_at_120x32() {
    let repo = with_history_and_areas();
    let mut session = started_in(repo.root(), 120, 32);
    session.press(REPOSITORY_CHOICES);

    insta::assert_debug_snapshot!("repository_picker_120x32", session.frame());
}

#[test]
fn the_repository_picker_at_80x24() {
    let repo = with_history_and_areas();
    let mut session = started_in(repo.root(), 80, 24);
    session.press(REPOSITORY_CHOICES);

    insta::assert_debug_snapshot!("repository_picker_80x24", session.frame());
}

#[test]
fn a_git_file_that_points_nowhere_is_named_rather_than_taken_for_no_repository() {
    let outside = tempfile::TempDir::new().expect("create a temporary directory");
    let project = outside.path().join("payments");
    std::fs::create_dir_all(project.join("src")).expect("create the project");
    std::fs::write(project.join(".git"), "gitdir: ../moved-away/.git\n").expect("write .git");

    let session = started_in(&project.join("src"), 120, 32);

    assert_eq!(
        home_actions(&session),
        ["> Practice on the showcase project", "Settings", "Quit"]
    );
    assert_eq!(
        unavailable_line(&session),
        "payments has a .git that Whetstone cannot open as a repository."
    );
}
