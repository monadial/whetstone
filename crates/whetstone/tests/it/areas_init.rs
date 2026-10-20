//! Process seam: `whetstone areas init` copies Whetstone's default area
//! files into the repository it runs in, the only time Whetstone writes
//! into a repository.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

use assert_cmd::cargo::cargo_bin_cmd;
use tempfile::TempDir;
use whetstone_core::Area;

use crate::repo::TestRepo;
use crate::repository::started_in;
use crate::session::columns;

/// Runs `whetstone` with `args` in `dir`, in a clean environment whose home
/// is a fresh temporary directory, without a terminal.
fn whetstone_in(dir: &Path, args: &[&str]) -> Output {
    let home = TempDir::new().expect("create a temporary home directory");
    cargo_bin_cmd!("whetstone")
        .current_dir(dir)
        .env_clear()
        .env("HOME", home.path())
        .args(args)
        .output()
        .expect("run the whetstone binary")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout is UTF-8")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr is UTF-8")
}

/// `path` with every symlink in it resolved, so paths that name the same
/// file compare equal.
fn resolved(path: impl AsRef<Path>) -> PathBuf {
    fs::canonicalize(path).expect("the path exists")
}

/// The default area files' names, in file-name order.
fn default_file_names() -> Vec<&'static str> {
    Area::default_files()
        .iter()
        .map(|&(name, _)| name)
        .collect()
}

#[test]
fn areas_init_writes_the_five_default_area_files_and_prints_their_paths() {
    let repo = TestRepo::new("payments");

    let output = whetstone_in(repo.root(), &["areas", "init"]);

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stderr(&output), "");
    let printed: Vec<PathBuf> = stdout(&output).lines().map(resolved).collect();
    let areas = resolved(repo.path(".whetstone/areas"));
    assert_eq!(
        printed,
        default_file_names()
            .iter()
            .map(|name| areas.join(name))
            .collect::<Vec<_>>()
    );
    for &(name, contents) in Area::default_files() {
        assert_eq!(
            fs::read(areas.join(name)).expect("read a written area file"),
            contents,
            "{name} is the default file"
        );
    }
}

#[test]
fn areas_init_from_a_subdirectory_writes_at_the_git_root() {
    let repo = TestRepo::new("payments");
    repo.write("src/lib.rs", "");

    let output = whetstone_in(&repo.path("src"), &["areas", "init"]);

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(repo.path(".whetstone/areas/domain.md").is_file());
    assert!(!repo.path("src/.whetstone").exists());
}

#[test]
fn areas_init_refuses_and_writes_nothing_when_area_files_exist() {
    let repo = TestRepo::new("payments");
    repo.write(".whetstone/areas/security.md", "My own prompt\n");

    let output = whetstone_in(repo.root(), &["areas", "init"]);

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(stdout(&output), "");
    let error = stderr(&output);
    assert!(
        error.starts_with("error: ") && error.contains("already has area files"),
        "stderr says why:\n{error}"
    );
    assert!(error.contains("nothing was written"), "{error}");
    let left: Vec<String> = fs::read_dir(repo.path(".whetstone/areas"))
        .expect("read the areas directory")
        .map(|entry| {
            entry
                .expect("a directory entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(left, ["security.md"]);
    assert_eq!(
        fs::read_to_string(repo.path(".whetstone/areas/security.md")).expect("read it"),
        "My own prompt\n"
    );
}

#[test]
fn a_second_areas_init_refuses_and_leaves_the_first_ones_files() {
    let repo = TestRepo::new("payments");
    let first = whetstone_in(repo.root(), &["areas", "init"]);
    assert_eq!(first.status.code(), Some(0), "{}", stderr(&first));
    let before = repo.on_disk();

    let second = whetstone_in(repo.root(), &["areas", "init"]);

    assert_eq!(second.status.code(), Some(1));
    assert!(
        stderr(&second).contains("already has area files"),
        "{}",
        stderr(&second)
    );
    assert_eq!(repo.on_disk(), before, "nothing was written");
}

#[test]
fn areas_init_outside_a_git_repository_fails_with_a_clear_error_and_writes_nothing() {
    let outside = TempDir::new().expect("create a temporary directory");

    let output = whetstone_in(outside.path(), &["areas", "init"]);

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(stdout(&output), "");
    let error = stderr(&output);
    assert!(
        error.starts_with("error: not inside a Git repository"),
        "stderr explains:\n{error}"
    );
    assert!(
        fs::read_dir(outside.path())
            .expect("read the directory")
            .next()
            .is_none(),
        "nothing was created"
    );
}

#[test]
fn help_lists_the_areas_command_and_its_init_subcommand() {
    let outside = TempDir::new().expect("create a temporary directory");

    let help = stdout(&whetstone_in(outside.path(), &["--help"]));
    assert!(
        help.lines()
            .any(|line| line.trim_start().starts_with("areas ")),
        "help lists areas:\n{help}"
    );

    let areas_help = stdout(&whetstone_in(outside.path(), &["areas", "--help"]));
    assert!(
        areas_help
            .lines()
            .any(|line| line.trim_start().starts_with("init ")),
        "areas help lists init:\n{areas_help}"
    );

    let init = whetstone_in(outside.path(), &["areas", "init", "--help"]);
    assert_eq!(init.status.code(), Some(0), "{}", stderr(&init));
    let init_help = stdout(&init);
    assert!(
        init_help.starts_with(
            "Copy Whetstone's five default area files into `.whetstone/areas/` at the repository's root"
        ) && init_help.contains("Refuses if area files are already there"),
        "init help describes it:\n{init_help}"
    );
    assert!(
        init_help.contains("Usage: whetstone areas init"),
        "{init_help}"
    );
}

#[test]
fn the_files_areas_init_writes_load_as_the_repositorys_areas() {
    let repo = TestRepo::new("payments");
    repo.write("README.md", "# Payments\n");
    repo.commit("Start the payments service");
    let output = whetstone_in(repo.root(), &["areas", "init"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));

    let mut session = started_in(repo.root(), 120, 60);
    session.press("<Enter>");
    assert_eq!(
        columns(&session.section("Areas the questions focus on")),
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

    // Choose the commit and review it: the areas come from the repository.
    session.press("j j j j j <Space> k k k k k <S-Tab> <Enter>");
    assert_eq!(
        columns(&session.section("Areas")),
        [
            ["Domain", "this repository"],
            ["Infrastructure", "this repository"],
            ["Observability", "this repository"],
            ["Scalability", "this repository"],
            ["System design", "this repository"],
        ]
    );
}

#[test]
fn areas_init_in_a_bare_repository_fails_and_writes_nothing() {
    let repo = TestRepo::bare("payments.git");
    let before = repo.on_disk();

    let output = whetstone_in(repo.root(), &["areas", "init"]);

    assert_eq!(output.status.code(), Some(1));
    let error = stderr(&output);
    assert!(
        error.starts_with("error: ") && error.contains("is a bare repository"),
        "{error}"
    );
    assert_eq!(repo.on_disk(), before, "nothing was written");
}
